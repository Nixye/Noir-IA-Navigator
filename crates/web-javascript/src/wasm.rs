//! Synchronous, numeric, stateless WebAssembly bridge. No ambient authority.
//! Cloning the VM shares immutable compiled modules, never a mutable Store.
use crate::{Function, Heap, Host, JsError, Runtime, Value, descriptors::PropertyMetadata};
use std::{collections::BTreeMap, sync::Arc};
use web_wasm::{CompiledModule, ValueType, WasmError, WasmErrorKind, WasmValue};

const MAX_MODULES: usize = 64;
const MAX_INSTANCES: usize = 128;
const MAX_BINARY_TOTAL: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) struct ExportFunction {
    instance: usize,
    module: usize,
    name: String,
    params: Vec<ValueType>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct WasmState {
    namespace: Option<usize>,
    prototypes: BTreeMap<&'static str, Value>,
    modules: BTreeMap<usize, CompiledModule>,
    instances: BTreeMap<usize, (usize, usize)>,
    pub functions: BTreeMap<usize, ExportFunction>,
    // Cumulative session quotas are intentionally not replenished by GC.
    module_count: usize,
    instance_count: usize,
    binary_bytes: usize,
}

pub(super) fn constructor(name: &str) -> bool {
    matches!(
        name,
        "WebAssembly.Module"
            | "WebAssembly.Instance"
            | "WebAssembly.CompileError"
            | "WebAssembly.LinkError"
            | "WebAssembly.RuntimeError"
    )
}

impl Runtime {
    pub(super) fn wasm_preserve_budgets(&mut self, consumed: &Self) {
        self.wasm.module_count = self.wasm.module_count.max(consumed.wasm.module_count);
        self.wasm.instance_count = self.wasm.instance_count.max(consumed.wasm.instance_count);
        self.wasm.binary_bytes = self.wasm.binary_bytes.max(consumed.wasm.binary_bytes);
    }
    pub(super) fn wasm_namespace(&mut self) -> Result<Value, JsError> {
        if let Some(id) = self.wasm.namespace {
            return Ok(Value::Object(id));
        }
        let names = [
            ("validate", "WebAssembly.validate"),
            ("Module", "WebAssembly.Module"),
            ("Instance", "WebAssembly.Instance"),
            ("CompileError", "WebAssembly.CompileError"),
            ("LinkError", "WebAssembly.LinkError"),
            ("RuntimeError", "WebAssembly.RuntimeError"),
        ];
        let object = self.ordinary_object(
            names
                .into_iter()
                .map(|(key, value)| (key.into(), Value::Native(value)))
                .collect(),
        )?;
        self.hide_properties(
            &object,
            &[
                "validate",
                "Module",
                "Instance",
                "CompileError",
                "LinkError",
                "RuntimeError",
            ],
        );
        if let Value::Object(id) = object {
            self.wasm.namespace = Some(id);
        }
        Ok(object)
    }

    fn wasm_prototype(&mut self, name: &'static str) -> Result<Value, JsError> {
        if let Some(value) = self.wasm.prototypes.get(name) {
            return Ok(value.clone());
        }
        let prototype = self.ordinary_object(BTreeMap::from([(
            "constructor".into(),
            Value::Native(name),
        )]))?;
        self.hide_properties(&prototype, &["constructor"]);
        self.wasm.prototypes.insert(name, prototype.clone());
        Ok(prototype)
    }

    pub(super) fn wasm_get(
        &mut self,
        object: &Value,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if let Value::Native(name) = object {
            if constructor(name) && property == "prototype" {
                return self.wasm_prototype(name).map(Some);
            }
            if let Some(name) = name.strip_prefix("WebAssembly.") {
                if property == "name" {
                    return self.string(name.into()).map(Some);
                }
                if property == "length" {
                    return Ok(Some(Value::Number(1.)));
                }
            }
        }
        Ok(None)
    }

    fn wasm_error_value(&mut self, name: &'static str, message: String) -> Result<Value, JsError> {
        let prototype = self.wasm_prototype(name)?;
        let message = self.string(message)?;
        let short = name.strip_prefix("WebAssembly.").unwrap_or(name);
        let error_name = self.string(short.into())?;
        let value = self.object(
            BTreeMap::from([("name".into(), error_name), ("message".into(), message)]),
            Some(prototype),
        )?;
        self.hide_properties(&value, &["name", "message"]);
        Ok(value)
    }

    fn wasm_error(&mut self, error: WasmError) -> JsError {
        let name = match error.kind {
            WasmErrorKind::Resource => return JsError::resource(error.message),
            WasmErrorKind::Compile => "WebAssembly.CompileError",
            WasmErrorKind::Unsupported | WasmErrorKind::Link => "WebAssembly.LinkError",
            WasmErrorKind::Runtime => "WebAssembly.RuntimeError",
        };
        match self.wasm_error_value(name, error.message.clone()) {
            Ok(value) => {
                let mut result = JsError::runtime(error.message);
                result.thrown = Some(value);
                result
            }
            Err(error) => error,
        }
    }

    pub(super) fn wasm_native_call(
        &mut self,
        name: &str,
        args: &[Value],
        construct: bool,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        match name {
            "WebAssembly.validate" | "WebAssembly.Module" => {
                if construct != (name == "WebAssembly.Module") {
                    return Err(self.property_type_error(
                        "WebAssembly.Module requires new; validate is not a constructor",
                    ));
                }
                if name == "WebAssembly.Module" && self.wasm.module_count >= MAX_MODULES {
                    return Err(JsError::resource(
                        "WebAssembly cumulative module budget exceeded",
                    ));
                }
                let bytes = self.wasm_buffer_source(
                    &first,
                    MAX_BINARY_TOTAL.saturating_sub(self.wasm.binary_bytes),
                )?;
                self.wasm.binary_bytes += bytes.len();
                if name == "WebAssembly.validate" {
                    return match web_wasm::validate(&bytes) {
                        Ok(()) => Ok(Some(Value::Bool(true))),
                        Err(error) if error.kind == WasmErrorKind::Resource => {
                            Err(self.wasm_error(error))
                        }
                        Err(_) => Ok(Some(Value::Bool(false))),
                    };
                }
                self.wasm.module_count += 1;
                // Allocation and compilation are both bounded before entering the backend.
                self.wasm_heap_capacity(2)?;
                let module = web_wasm::compile(&bytes).map_err(|error| self.wasm_error(error))?;
                let prototype = self.wasm_prototype("WebAssembly.Module")?;
                let value = self.object(BTreeMap::new(), Some(prototype))?;
                let Value::Object(id) = value else {
                    unreachable!()
                };
                self.wasm.modules.insert(id, module);
                Ok(Some(value))
            }
            "WebAssembly.Instance" => {
                if !construct {
                    return Err(self.property_type_error("WebAssembly.Instance requires new"));
                }
                let Value::Object(module_id) = first else {
                    return Err(self.property_type_error("WebAssembly.Instance requires a Module"));
                };
                let Some(module) = self.wasm.modules.get(&module_id).cloned() else {
                    return Err(self.property_type_error("WebAssembly.Instance requires a Module"));
                };
                // Type-check imports, but never inspect unused properties or getters.
                if let Some(imports) = args.get(1)
                    && !matches!(
                        imports,
                        Value::Undefined
                            | Value::Object(_)
                            | Value::Array(_)
                            | Value::Function(_)
                            | Value::Native(_)
                            | Value::Host(_)
                    )
                {
                    return Err(self.property_type_error(
                        "WebAssembly imports must be an object or undefined",
                    ));
                }
                module
                    .ensure_instantiable()
                    .map_err(|error| self.wasm_error(error))?;
                if self.wasm.instance_count >= MAX_INSTANCES {
                    return Err(JsError::resource(
                        "WebAssembly cumulative instance budget exceeded",
                    ));
                }
                self.wasm_heap_capacity(module.exports().len() + 3)?;
                self.wasm.instance_count += 1;
                let prototype = self.wasm_prototype("WebAssembly.Instance")?;
                let instance = self.object(BTreeMap::new(), Some(prototype))?;
                let Value::Object(instance_id) = instance else {
                    unreachable!()
                };
                let mut exports = BTreeMap::new();
                let mut functions: BTreeMap<u32, Value> = BTreeMap::new();
                for export in module.exports() {
                    self.tick()?;
                    let index = export
                        .function_index
                        .ok_or_else(|| JsError::runtime("Missing WebAssembly function index"))?;
                    let function = if let Some(function) = functions.get(&index) {
                        function.clone()
                    } else {
                        let function = self.wasm_function(
                            instance_id,
                            module_id,
                            &export.name,
                            index,
                            &export.params,
                        )?;
                        functions.insert(index, function.clone());
                        function
                    };
                    exports.insert(export.name.clone(), function);
                }
                let exported = self.object(exports, None)?;
                let Value::Object(exports_id) = exported else {
                    unreachable!()
                };
                self.remember_property_order(
                    exports_id,
                    module
                        .exports()
                        .iter()
                        .map(|export| export.name.clone())
                        .collect(),
                );
                for export in module.exports() {
                    self.wasm_descriptor(exports_id, &export.name, false, true, false);
                }
                self.reflection.non_extensible.insert(exports_id);
                self.reserve_named_property(instance_id, "exports")?;
                if let Heap::Object(object) = &mut self.heap[instance_id] {
                    object.properties.insert("exports".into(), exported);
                }
                self.wasm_descriptor(instance_id, "exports", false, false, false);
                self.wasm
                    .instances
                    .insert(instance_id, (module_id, exports_id));
                Ok(Some(instance))
            }
            "WebAssembly.CompileError" | "WebAssembly.LinkError" | "WebAssembly.RuntimeError" => {
                let name = match name {
                    "WebAssembly.CompileError" => "WebAssembly.CompileError",
                    "WebAssembly.LinkError" => "WebAssembly.LinkError",
                    _ => "WebAssembly.RuntimeError",
                };
                let message = if matches!(first, Value::Undefined) {
                    String::new()
                } else {
                    let value = self.language_string(first, host)?;
                    self.text(&value, 0)?
                };
                self.wasm_error_value(name, message).map(Some)
            }
            _ => Ok(None),
        }
    }

    fn wasm_heap_capacity(&self, additional: usize) -> Result<(), JsError> {
        if self
            .heap
            .len()
            .saturating_sub(self.free_heap.len())
            .saturating_add(additional)
            > self.limits.heap_objects
        {
            return Err(JsError::resource("WebAssembly heap budget exceeded"));
        }
        Ok(())
    }

    fn wasm_descriptor(
        &mut self,
        id: usize,
        key: &str,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) {
        self.property_metadata.entry(id).or_default().insert(
            key.into(),
            PropertyMetadata {
                writable,
                enumerable,
                configurable,
                accessor: None,
            },
        );
    }

    fn wasm_function(
        &mut self,
        instance: usize,
        module: usize,
        name: &str,
        index: u32,
        params: &[ValueType],
    ) -> Result<Value, JsError> {
        self.charge_string(name.len() + "name".len() + "length".len())?;
        let function_name = self.string(index.to_string())?;
        let id = self.allocate(Heap::Function(Function {
            params: Arc::new(Vec::new()),
            body: Arc::new(Vec::new()),
            scope: 0,
            strict: true,
            parameters: None,
            constructible: false,
            kind: crate::parser::FunctionKind::Ordinary,
            method: false,
            arrow: true,
            lexical_this: Value::Undefined,
            internal_prototype: None,
            properties: BTreeMap::from([
                ("name".into(), function_name),
                ("length".into(), Value::Number(params.len() as f64)),
            ]),
        }))?;
        self.wasm_descriptor(id, "name", false, false, true);
        self.wasm_descriptor(id, "length", false, false, true);
        self.wasm.functions.insert(
            id,
            ExportFunction {
                instance,
                module,
                name: name.into(),
                params: params.to_vec(),
            },
        );
        Ok(Value::Function(id))
    }

    pub(super) fn wasm_invoke(
        &mut self,
        id: usize,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let function = self
            .wasm
            .functions
            .get(&id)
            .cloned()
            .ok_or_else(|| JsError::runtime("Invalid WebAssembly function"))?;
        let module = self
            .wasm
            .modules
            .get(&function.module)
            .cloned()
            .ok_or_else(|| JsError::runtime("Invalid WebAssembly module"))?;
        let mut converted = Vec::with_capacity(function.params.len());
        for (index, ty) in function.params.iter().enumerate() {
            let value = args.get(index).cloned().unwrap_or(Value::Undefined);
            converted.push(match ty {
                ValueType::I64 => {
                    let primitive =
                        self.operator_primitive(value, crate::coercion::Hint::Number, host)?;
                    // ToBigInt rejects Number, unlike BigInt(). Reduce only after
                    // observable primitive conversion; never round through f64.
                    let Value::BigInt(number) =
                        self.bigint_as_uint_n(Value::Number(64.), primitive)?
                    else {
                        unreachable!()
                    };
                    WasmValue::I64(i64::from_le_bytes(number.low_64_le()))
                }
                ValueType::I32 | ValueType::F32 | ValueType::F64 => {
                    let Value::Number(number) = self.operator_number(value, host)? else {
                        unreachable!()
                    };
                    match ty {
                        ValueType::I32 => WasmValue::I32(to_int32(number)),
                        ValueType::F32 => WasmValue::F32(number as f32),
                        ValueType::F64 => WasmValue::F64(number),
                        _ => unreachable!(),
                    }
                }
                _ => {
                    return Err(
                        self.property_type_error("Unsupported WebAssembly JavaScript signature")
                    );
                }
            });
        }
        let outcome = web_wasm::execute(&module, &function.name, &converted, self.remaining);
        // Fuel is debited on success AND traps, including fatal resource errors.
        self.remaining = self.remaining.min(outcome.fuel_remaining);
        match outcome.result.map_err(|error| self.wasm_error(error))? {
            None => Ok(Value::Undefined),
            Some(WasmValue::I32(value)) => Ok(Value::Number(value as f64)),
            Some(WasmValue::I64(value)) => {
                self.tick()?;
                self.charge_string(8)?;
                Ok(Value::BigInt(Arc::new(crate::BigInt::from_64_le(
                    value.to_le_bytes(),
                    true,
                ))))
            }
            Some(WasmValue::F32(value)) => Ok(Value::Number(value as f64)),
            Some(WasmValue::F64(value)) => Ok(Value::Number(value)),
        }
    }

    pub(super) fn wasm_roots(&self) -> Vec<Value> {
        self.wasm
            .namespace
            .map(Value::Object)
            .into_iter()
            .chain(self.wasm.prototypes.values().cloned())
            .collect()
    }
    pub(super) fn wasm_trace(&self, id: usize) -> Vec<Value> {
        let mut roots = Vec::new();
        if let Some(function) = self.wasm.functions.get(&id) {
            roots.extend([
                Value::Object(function.module),
                Value::Object(function.instance),
            ]);
        }
        if let Some((module, exports)) = self.wasm.instances.get(&id) {
            roots.extend([Value::Object(*module), Value::Object(*exports)]);
        }
        roots
    }
    pub(super) fn wasm_sweep(&mut self, live: &[bool]) {
        self.wasm
            .modules
            .retain(|id, _| live.get(*id) == Some(&true));
        self.wasm
            .instances
            .retain(|id, _| live.get(*id) == Some(&true));
        self.wasm
            .functions
            .retain(|id, _| live.get(*id) == Some(&true));
    }
}

fn to_int32(number: f64) -> i32 {
    if !number.is_finite() || number == 0. {
        return 0;
    }
    let unsigned = number.trunc().rem_euclid(4_294_967_296.);
    if unsigned >= 2_147_483_648. {
        (unsigned - 4_294_967_296.) as i32
    } else {
        unsigned as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;

    struct Empty;
    impl Host for Empty {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Ok(())
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
    }

    // Tiny binary fixtures are assembled as data, never by an external compiler.
    fn binary(params: &[u8], result: Option<u8>, body: &[u8], aliases: bool) -> Vec<u8> {
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        let mut types = vec![1, 0x60, params.len() as u8];
        types.extend_from_slice(params);
        types.push(u8::from(result.is_some()));
        types.extend(result);
        bytes.extend([1, types.len() as u8]);
        bytes.extend(types);
        bytes.extend([3, 2, 1, 0]);
        let exports = if aliases {
            vec![2, 1, b'f', 0, 0, 1, b'g', 0, 0]
        } else {
            vec![1, 1, b'f', 0, 0]
        };
        bytes.extend([7, exports.len() as u8]);
        bytes.extend(exports);
        let mut code = vec![1, (body.len() + 2) as u8, 0];
        code.extend_from_slice(body);
        code.push(0x0b);
        bytes.extend([10, code.len() as u8]);
        bytes.extend(code);
        bytes
    }
    fn array(bytes: &[u8]) -> String {
        format!(
            "new Uint8Array([{}])",
            bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
    fn setup(bytes: &[u8]) -> String {
        format!(
            "const bytes={};const m=new WebAssembly.Module(bytes);const i=new WebAssembly.Instance(m);const f=i.exports.f;",
            array(bytes)
        )
    }
    fn check(source: &str) {
        assert_eq!(
            Runtime::default().execute(source, &mut Empty).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }

    #[test]
    fn exports_are_real_callable_identities_with_aliases_and_prototypes() {
        let prefix = setup(&binary(
            &[0x7f, 0x7f],
            Some(0x7f),
            &[0x20, 0, 0x20, 1, 0x6a],
            true,
        ));
        for suffix in [
            "typeof WebAssembly==='object'&&typeof f==='function'&&f(20,22)===42;",
            "f===i.exports.f&&f===i.exports.g&&f!==new WebAssembly.Instance(m).exports.f;",
            "f.name==='0'&&f.length===2&&f.call(null,20,22)===42&&f.apply(null,[20,22])===42&&f.bind(null,20)(22)===42;",
            "m instanceof WebAssembly.Module&&i instanceof WebAssembly.Instance&&f instanceof Function;",
            "let ok=false;try{new f();}catch(e){ok=e.name==='TypeError';}ok&&!Object.hasOwn(f,'prototype');",
            "let ok=false;try{new (f.bind(null))();}catch(e){ok=e.name==='TypeError';}ok;",
            "Object.getPrototypeOf(i.exports)===null&&Object.isFrozen(i.exports)&&Object.keys(i.exports).join(',')==='f,g';",
            "let n=0;const imports={};Object.defineProperty(imports,'unused',{get:function(){n++;throw 1;},enumerable:true,configurable:true});const v=new WebAssembly.Instance(m,imports);n===0&&v.exports.f(1,2)===3;",
        ] {
            check(&format!("{prefix}{suffix}"));
        }
    }

    #[test]
    fn buffer_sources_snapshot_exact_ranges_without_getters() {
        let bytes = binary(&[0x7f], Some(0x7f), &[0x20, 0], false);
        let mut padded = vec![255, 255];
        padded.extend_from_slice(&bytes);
        padded.push(255);
        for view in [
            format!("new Uint8Array(b.buffer,2,{})", bytes.len()),
            format!("new DataView(b.buffer,2,{})", bytes.len()),
        ] {
            check(&format!(
                "const b={};const v={view};const m=new WebAssembly.Module(v);b[2]=255;new WebAssembly.Instance(m).exports.f(42)===42;",
                array(&padded)
            ));
        }
        check(&format!(
            "const b={};WebAssembly.validate(b.buffer)&&WebAssembly.validate(b);",
            array(&bytes)
        ));
        for invalid in [
            "[]",
            "'bytes'",
            "{}",
            "null",
            "undefined",
            "42",
            "Object.defineProperty({},'buffer',{get:function(){throw 9;},enumerable:true,configurable:true})",
        ] {
            check(&format!(
                "let ok=false;try{{WebAssembly.validate({invalid});}}catch(e){{ok=e.name==='TypeError';}}ok;"
            ));
        }
    }

    #[test]
    fn malformed_compile_and_unsupported_link_and_runtime_traps_are_distinct() {
        check(
            "let ok=false;try{new WebAssembly.Module(new Uint8Array([0]));}catch(e){ok=e.name==='CompileError'&&e instanceof WebAssembly.CompileError;}ok&&!WebAssembly.validate(new Uint8Array([0]));",
        );
        // Valid core module with a memory is valid, but outside the adapter subset.
        check(
            "const b=new Uint8Array([0,97,115,109,1,0,0,0,5,3,1,0,1]);let ok=false;const m=new WebAssembly.Module(b);try{new WebAssembly.Instance(m);}catch(e){ok=e.name==='LinkError';}ok&&WebAssembly.validate(b);",
        );
        let prefix = setup(&binary(&[], None, &[0], false));
        check(&format!(
            "{prefix}let ok=false;try{{f();}}catch(e){{ok=e.name==='RuntimeError'&&e instanceof WebAssembly.RuntimeError;}}ok;"
        ));
    }

    #[test]
    fn numeric_conversion_order_wrap_missing_extra_and_floating_point() {
        let add = setup(&binary(
            &[0x7f, 0x7f],
            Some(0x7f),
            &[0x20, 0, 0x20, 1, 0x6a],
            false,
        ));
        check(&format!(
            "{add}let order='';const a={{valueOf(){{order+='a';return 4294967295;}}}};const b={{valueOf(){{order+='b';return 2;}}}};f(a,b)===1&&order==='ab'&&f(NaN,Infinity)===0&&f()===0&&f(-4294967297,0)===-1;"
        ));
        check(&format!("{add}let n=0;f(20,22,++n)===42&&n===1;"));
        check(&format!(
            "{add}let ok=false;try{{f(1n,2);}}catch(e){{ok=e.name==='TypeError';}}ok;"
        ));
        for ty in [0x7d, 0x7c] {
            let prefix = setup(&binary(&[ty], Some(ty), &[0x20, 0], false));
            check(&format!(
                "{prefix}Object.is(f(-0),-0)&&Number.isNaN(f())&&f(Infinity)===Infinity;"
            ));
            if ty == 0x7d {
                check(&format!("{prefix}f(16777217)===16777216;"));
            }
        }
        let prefix = setup(&binary(&[], None, &[], false));
        check(&format!("{prefix}f()===undefined;"));
    }

    #[test]
    fn i64_bigint_conversion_is_exact_signed_and_modular() {
        let prefix = setup(&binary(&[0x7e], Some(0x7e), &[0x20, 0], false));
        for suffix in [
            "typeof f(1n)==='bigint'&&f(9007199254740993n)===9007199254740993n;",
            "f(9223372036854775807n)===9223372036854775807n&&f(-9223372036854775808n)===-9223372036854775808n;",
            "f(9223372036854775808n)===-9223372036854775808n&&f(18446744073709551615n)===-1n&&f(18446744073709551616n)===0n;",
            "f(-18446744073709551617n)===-1n&&f(-9223372036854775809n)===9223372036854775807n;",
            "f(true)===1n&&f(false)===0n&&f('9007199254740993')===9007199254740993n&&f('0xffffffffffffffff')===-1n&&f('')===0n;",
            "let n=0;f(1n,{valueOf(){n++;throw 1;}})===1n&&n===0;",
            "let hint='';const v={};v[Symbol.toPrimitive]=function(h){hint=h;return '9007199254740993';};f(v)===9007199254740993n&&hint==='number';",
            "let order='';const v={toString(){order+='s';return '42';}};Object.defineProperty(v,'valueOf',{get:function(){order+='g';return function(){order+='v';return {};};},enumerable:true,configurable:true});f(v)===42n&&order==='gvs';",
        ] {
            check(&format!("{prefix}{suffix}"));
        }
        for invalid in [
            "1",
            "NaN",
            "Infinity",
            "Symbol()",
            "null",
            "undefined",
            "{valueOf(){return 1;}}",
        ] {
            check(&format!(
                "{prefix}let ok=false;try{{f({invalid});}}catch(e){{ok=e.name==='TypeError';}}ok;"
            ));
        }
        check(&format!(
            "{prefix}let ok=false;try{{f();}}catch(e){{ok=e.name==='TypeError';}}ok;"
        ));
        check(&format!(
            "{prefix}let ok=false;try{{f('1.5');}}catch(e){{ok=e.name==='SyntaxError';}}ok;"
        ));
    }

    #[test]
    fn mixed_i64_signatures_preserve_coercion_order_and_abrupt_completion() {
        // Every argument is converted, even when the body returns only parameter 1.
        let prefix = setup(&binary(
            &[0x7f, 0x7e, 0x7d, 0x7c],
            Some(0x7e),
            &[0x20, 1],
            false,
        ));
        check(&format!(
            "{prefix}let order='';const a={{valueOf(){{order+='a';return 1;}}}};const b={{valueOf(){{order+='b';return 9007199254740993n;}}}};const c={{valueOf(){{order+='c';return 2;}}}};const d={{valueOf(){{order+='d';return 3;}}}};f(a,b,c,d)===9007199254740993n&&order==='abcd';"
        ));
        check(&format!(
            "{prefix}let order='';const sentinel={{}};const b={{}};Object.defineProperty(b,Symbol.toPrimitive,{{get:function(){{order+='b';throw sentinel;}},enumerable:true,configurable:true}});let caught=false;try{{f({{valueOf(){{order+='a';return 1;}}}},b,{{valueOf(){{order+='c';return 2;}}}},0);}}catch(e){{caught=e===sentinel;}}caught&&order==='ab';"
        ));
        for args in ["1n,2n,3,4", "1,2,3,4", "1,2n,3n,4", "1,2n,3,4n"] {
            check(&format!(
                "{prefix}let ok=false;try{{f({args});}}catch(e){{ok=e.name==='TypeError';}}ok;"
            ));
        }
    }

    #[test]
    fn i64_traps_preserve_prior_coercion_effects_and_debit_fuel() {
        let prefix = setup(&binary(&[0x7e], Some(0x7e), &[0], false));
        let mut vm = Runtime::default();
        vm.execute(&format!("{prefix}let effects=0;"), &mut Empty)
            .unwrap();
        let before = vm.remaining;
        assert_eq!(vm.execute("let caught=false;try{f({valueOf(){effects++;return 1n;}});}catch(e){caught=e instanceof WebAssembly.RuntimeError;}caught&&effects===1;", &mut Empty).unwrap(), Value::Bool(true));
        assert!(vm.remaining < before);
    }

    #[test]
    fn i64_loop_exhaustion_is_fatal_and_rollback_cannot_refund_fuel() {
        let mut vm = Runtime::default();
        let bytes = binary(&[0x7e], None, &[0x03, 0x40, 0x0c, 0, 0x0b], false);
        vm.execute(&setup(&bytes), &mut Empty).unwrap();
        let previous = vm.clone();
        vm.remaining = 1_000;
        let error = vm
            .execute("try{f(1n);}catch(e){42;}finally{99;}", &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(vm.remaining < 1_000);
        let remaining = vm.remaining;
        vm.rollback_to(previous);
        assert_eq!(vm.remaining, remaining);
        assert_eq!(vm.wasm.functions.len(), 1);
    }

    #[test]
    fn i64_result_allocation_is_charged_after_wasm_fuel() {
        let mut vm = Runtime::default();
        let bytes = binary(&[], Some(0x7e), &[0x42, 1], false);
        vm.execute(&setup(&bytes), &mut Empty).unwrap();
        let Value::Function(id) = vm.global_value("f").unwrap() else {
            unreachable!()
        };
        let allocated = vm.string_bytes;
        let before = vm.remaining;
        vm.wasm_invoke(id, &[], &mut Empty).unwrap();
        assert_eq!(vm.string_bytes, allocated + 8);
        assert!(vm.remaining < before);
        vm.limits.string_bytes = vm.string_bytes + 7;
        let before = vm.remaining;
        let error = vm.wasm_invoke(id, &[], &mut Empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(vm.remaining < before);
    }

    #[test]
    fn i64_function_survives_gc_and_rollback_without_mutable_wasm_state() {
        let mut vm = Runtime::default();
        let bytes = binary(&[0x7e], Some(0x7e), &[0x20, 0], false);
        let function = vm
            .execute(
                &format!("(()=>{{{}return f;}})();", setup(&bytes)),
                &mut Empty,
            )
            .unwrap();
        vm.collect_garbage(std::slice::from_ref(&function), &Empty)
            .unwrap();
        let previous = vm.clone();
        let value = Value::BigInt(Arc::new(crate::BigInt::from_64_le(
            9_007_199_254_740_993i64.to_le_bytes(),
            true,
        )));
        assert_eq!(
            vm.invoke(function.clone(), vec![value.clone()], &mut Empty)
                .unwrap(),
            value
        );
        let remaining = vm.remaining;
        vm.rollback_to(previous);
        assert_eq!(vm.remaining, remaining);
        assert_eq!(
            vm.invoke(function, vec![value.clone()], &mut Empty)
                .unwrap(),
            value
        );
        vm.collect_garbage(&[], &Empty).unwrap();
        assert!(
            vm.wasm.modules.is_empty()
                && vm.wasm.instances.is_empty()
                && vm.wasm.functions.is_empty()
        );
    }

    #[test]
    fn function_roots_keep_instances_alive_and_sweep_precedes_id_reuse() {
        let mut vm = Runtime::default();
        let bytes = binary(&[0x7f], Some(0x7f), &[0x20, 0], false);
        let function = vm
            .execute(
                &format!("(()=>{{{}return f;}})();", setup(&bytes)),
                &mut Empty,
            )
            .unwrap();
        vm.collect_garbage(std::slice::from_ref(&function), &Empty)
            .unwrap();
        assert_eq!(
            vm.invoke(function.clone(), vec![Value::Number(7.)], &mut Empty)
                .unwrap(),
            Value::Number(7.)
        );
        assert_eq!(vm.wasm.modules.len(), 1);
        assert_eq!(vm.wasm.instances.len(), 1);
        vm.collect_garbage(&[], &Empty).unwrap();
        assert!(
            vm.wasm.modules.is_empty()
                && vm.wasm.instances.is_empty()
                && vm.wasm.functions.is_empty()
        );
        let _replacement = vm.object(BTreeMap::new(), None).unwrap();
        assert!(vm.wasm.functions.is_empty());
    }

    #[test]
    fn fuel_is_shared_fatal_and_rollback_clones_only_immutable_modules() {
        let mut vm = Runtime::default();
        // loop { br 0 }, no state or return value.
        let bytes = binary(&[], None, &[0x03, 0x40, 0x0c, 0, 0x0b], false);
        vm.execute(&setup(&bytes), &mut Empty).unwrap();
        let previous = vm.clone();
        vm.remaining = 100;
        let error = vm
            .execute("try{f();}catch(e){42;}finally{99;}", &mut Empty)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(vm.remaining < 100);
        let remaining = vm.remaining;
        vm.rollback_to(previous);
        assert_eq!(vm.remaining, remaining);
        assert_eq!(vm.wasm.modules.len(), 1);
        assert_eq!(vm.wasm.functions.len(), 1);
    }
}
