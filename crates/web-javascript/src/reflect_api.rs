//! Conservative Reflect operations over the VM's bounded object model.
//! No Proxy, exotic reflection, or alternate
//! newTarget yet. Unsupported operations are absent, not success-shaped stubs.
use crate::{
    Heap, Host, JsError, Runtime, Value,
    descriptors::{Definition, PropertyMetadata},
};
use std::collections::BTreeMap;

impl Runtime {
    pub(super) fn reflect_namespace(&mut self) -> Result<Value, JsError> {
        if let Some(value) = self.reflection.prototypes.get("Reflect") {
            return Ok(value.clone());
        }
        let methods = [
            ("apply", "Reflect.apply"),
            ("construct", "Reflect.construct"),
            ("get", "Reflect.get"),
            ("set", "Reflect.set"),
            ("defineProperty", "Reflect.defineProperty"),
            ("setPrototypeOf", "Reflect.setPrototypeOf"),
            ("has", "Reflect.has"),
            ("deleteProperty", "Reflect.deleteProperty"),
            ("ownKeys", "Reflect.ownKeys"),
            (
                "getOwnPropertyDescriptor",
                "Reflect.getOwnPropertyDescriptor",
            ),
            ("getPrototypeOf", "Reflect.getPrototypeOf"),
            ("isExtensible", "Reflect.isExtensible"),
            ("preventExtensions", "Reflect.preventExtensions"),
        ];
        let value = self.ordinary_object(
            methods
                .iter()
                .map(|(key, name)| ((*key).to_owned(), Value::Native(name)))
                .collect::<BTreeMap<_, _>>(),
        )?;
        let id = self.heap_owner(&value)?;
        self.remember_property_order(
            id,
            methods.iter().map(|(key, _)| (*key).to_owned()).collect(),
        );
        for (key, _) in methods {
            self.property_metadata.entry(id).or_default().insert(
                key.into(),
                PropertyMetadata {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        let tag = self.well_known_symbol("toStringTag")?;
        if let Value::Symbol(key, _) = tag {
            let label = self.string("Reflect".into())?;
            self.symbol_properties
                .entry(id)
                .or_default()
                .insert(key, label);
            self.symbol_order.entry(id).or_default().push(key);
            self.symbol_metadata.entry(id).or_default().insert(
                key,
                PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        self.reflection.prototypes.insert("Reflect", value.clone());
        Ok(value)
    }

    fn reflect_target(&mut self, value: &Value) -> Result<usize, JsError> {
        if !matches!(
            value,
            Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Native("Symbol")
        ) {
            return Err(self.property_type_error(
                "Reflect requires a VM object; Host/native exotic targets are unsupported",
            ));
        }
        let id = self.heap_owner(value)?;
        if self.stdlib_is_exotic(value)
            || matches!(self.heap.get(id), Some(Heap::Object(object)) if object.regexp.is_some())
        {
            return Err(self.property_type_error(
                "Reflect on native-backed exotic objects is not implemented",
            ));
        }
        Ok(id)
    }

    fn reflect_key(&mut self, value: &Value, host: &mut impl Host) -> Result<Value, JsError> {
        self.coerce_property_key(value.clone(), host)
    }

    fn reflect_prototype(&mut self, value: &Value) -> Result<Value, JsError> {
        self.reflect_target(value)?;
        match value {
            Value::Native("Symbol") => Ok(Value::Native("Function.prototype")),
            Value::Object(id) => match &self.heap[*id] {
                Heap::Object(object) => Ok(object.prototype.clone().unwrap_or(Value::Null)),
                _ => unreachable!(),
            },
            Value::Array(id) if Some(*id) == self.array_prototype => self.object_prototype_value(),
            Value::Array(_) => self.array_prototype_value(),
            Value::Function(id) => Ok(self
                .class_static_parent(*id)
                .unwrap_or(Value::Native("Function.prototype"))),
            _ => unreachable!(),
        }
    }

    // Traverse ourselves so both string and symbol accessors keep the explicitly
    // supplied receiver, including class static inheritance. Existence never
    // evaluates getters. The native Function prototype is a read-only boundary.
    fn reflect_lookup(
        &mut self,
        target: Value,
        key: Value,
        receiver: Value,
        existence: bool,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let mut current = target;
        for _ in 0..64 {
            self.tick()?;
            if current == Value::Null {
                return Ok(if existence {
                    Value::Bool(false)
                } else {
                    Value::Undefined
                });
            }
            if current == Value::Native("Function.prototype") {
                if !matches!(key, Value::Symbol(..)) {
                    let name = self.text(&key, 0)?;
                    if let Some(descriptor) = self.function_prototype_descriptor(&name)? {
                        return Ok(if existence {
                            Value::Bool(true)
                        } else {
                            descriptor.value
                        });
                    }
                }
                current = self.object_prototype_value()?;
                continue;
            }
            self.reflect_target(&current)?;
            let descriptor = if let Value::Symbol(symbol, _) = key {
                self.symbol_descriptor(&current, symbol)?
            } else {
                let name = self.text(&key, 0)?;
                self.own_descriptor(&current, &name, host)?
            };
            if let Some(descriptor) = descriptor {
                if existence {
                    return Ok(Value::Bool(true));
                }
                return match descriptor.metadata.accessor {
                    Some((Value::Undefined, _)) => Ok(Value::Undefined),
                    Some((getter, _)) => self.call(getter, vec![], receiver, host),
                    None => Ok(descriptor.value),
                };
            }
            current = self.reflect_prototype(&current)?;
        }
        Err(JsError::resource(
            "Reflect prototype traversal exceeds 64 levels",
        ))
    }

    fn reflect_arguments(
        &mut self,
        value: &Value,
        host: &mut impl Host,
    ) -> Result<Vec<Value>, JsError> {
        self.reflect_target(value)?;
        let length = self.get(value.clone(), "length", host)?;
        if matches!(
            length,
            Value::Symbol(..)
                | Value::BigInt(_)
                | Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Native(_)
                | Value::Host(_)
        ) {
            return Err(self.property_type_error(
                "Reflect argument length requires a primitive numeric conversion",
            ));
        }
        let length = self.number(&length);
        let length = if length.is_nan() || length <= 0. {
            0.
        } else {
            length.trunc()
        };
        if length > 32. {
            return Err(JsError::resource(
                "Reflect argument list exceeds 32 entries",
            ));
        }
        let mut result = Vec::with_capacity(length as usize);
        for index in 0..length as usize {
            self.tick()?;
            result.push(self.get(value.clone(), &index.to_string(), host)?);
        }
        Ok(result)
    }

    fn reflect_constructor(&mut self, target: &Value, host: &impl Host) -> Result<(), JsError> {
        let mut current = target.clone();
        for _ in 0..64 {
            self.tick()?;
            if let Value::Host(id) = &current {
                return if host.is_constructor(*id) {
                    Ok(())
                } else {
                    Err(self.property_type_error(
                        "Reflect.construct target is not a supported constructor",
                    ))
                };
            }
            if let Value::Native(name) = &current {
                if crate::wasm::constructor(name) {
                    return Ok(());
                }
                return if matches!(
                    *name,
                    "Object"
                        | "Array"
                        | "String"
                        | "Number"
                        | "Boolean"
                        | "Function"
                        | "RegExp"
                        | "Error"
                        | "TypeError"
                        | "RangeError"
                        | "ReferenceError"
                        | "SyntaxError"
                        | "AggregateError"
                        | "Promise"
                        | "WeakRef"
                        | "FinalizationRegistry"
                        | "WeakMap"
                        | "WeakSet"
                        | "Date"
                        | "Map"
                        | "Set"
                        | "ArrayBuffer"
                        | "DataView"
                        | "Int8Array"
                        | "Uint8Array"
                        | "Uint8ClampedArray"
                        | "Int16Array"
                        | "Uint16Array"
                        | "Int32Array"
                        | "Uint32Array"
                        | "Float32Array"
                        | "Float64Array"
                        | "BigInt64Array"
                        | "BigUint64Array"
                ) {
                    Ok(())
                } else {
                    Err(self.property_type_error(
                        "Reflect.construct target is not a supported constructor",
                    ))
                };
            }
            let Value::Function(id) = current else {
                return Err(self.property_type_error(
                    "Reflect.construct target is not a supported constructor",
                ));
            };
            if let Some(bound) = self.bound_functions.get(&id) {
                current = bound.target.clone();
                continue;
            }
            return if matches!(self.heap.get(id), Some(Heap::Function(function)) if function.constructible)
            {
                Ok(())
            } else {
                Err(self.property_type_error("Reflect.construct target is not a constructor"))
            };
        }
        Err(JsError::resource(
            "Reflect bound constructor depth exceeded",
        ))
    }

    fn reflect_write_receiver(
        &mut self,
        receiver: Value,
        key: &Value,
        value: Value,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        if !matches!(
            receiver,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Native(_)
                | Value::Host(_)
        ) {
            return Ok(false);
        }
        self.reflect_target(&receiver)?;
        let current = if let Value::Symbol(symbol, _) = key {
            self.symbol_descriptor(&receiver, *symbol)?
        } else {
            let name = self.text(key, 0)?;
            self.own_descriptor(&receiver, &name, host)?
        };
        let definition = if let Some(current) = current {
            if current.metadata.accessor.is_some() || !current.metadata.writable {
                return Ok(false);
            }
            Definition {
                value: Some(value),
                ..Definition::default()
            }
        } else {
            Definition {
                value: Some(value),
                writable: Some(true),
                enumerable: Some(true),
                configurable: Some(true),
                ..Definition::default()
            }
        };
        self.reflect_define_ordinary(&receiver, key, definition, host)
    }

    fn reflect_set(
        &mut self,
        target: Value,
        key: Value,
        value: Value,
        receiver: Value,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        let mut current = target;
        for _ in 0..64 {
            self.tick()?;
            if current == Value::Null {
                return self.reflect_write_receiver(receiver, &key, value, host);
            }
            let descriptor = if current == Value::Native("Function.prototype") {
                if matches!(key, Value::Symbol(..)) {
                    None
                } else {
                    let name = self.text(&key, 0)?;
                    self.function_prototype_descriptor(&name)?
                }
            } else if let Value::Symbol(symbol, _) = &key {
                self.reflect_target(&current)?;
                self.symbol_descriptor(&current, *symbol)?
            } else {
                self.reflect_target(&current)?;
                let name = self.text(&key, 0)?;
                self.own_descriptor(&current, &name, host)?
            };
            if let Some(descriptor) = descriptor {
                if let Some((_, setter)) = descriptor.metadata.accessor {
                    if setter == Value::Undefined {
                        return Ok(false);
                    }
                    self.call(setter, vec![value], receiver, host)?;
                    return Ok(true);
                }
                if !descriptor.metadata.writable {
                    return Ok(false);
                }
                return self.reflect_write_receiver(receiver, &key, value, host);
            }
            current = if current == Value::Native("Function.prototype") {
                self.object_prototype_value()?
            } else {
                self.reflect_prototype(&current)?
            };
        }
        Err(JsError::resource(
            "Reflect set prototype traversal exceeds 64 levels",
        ))
    }

    fn reflect_set_prototype(&mut self, target: &Value, prototype: Value) -> Result<bool, JsError> {
        if !matches!(
            prototype,
            Value::Null
                | Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Host(_)
                | Value::Native(_)
        ) {
            return Err(self.property_type_error(
                "Reflect.setPrototypeOf prototype must be an object or null",
            ));
        }
        let current = self.reflect_prototype(target)?;
        if crate::strict_equal(&current, &prototype) {
            return Ok(true);
        }
        let owner = self.reflect_target(target)?;
        if *target == Value::Native("Symbol") && self.reflection.non_extensible.contains(&owner) {
            return Ok(false);
        }
        if !matches!(target, Value::Object(_)) {
            return Err(self.property_type_error(
                "Reflect prototype mutation of arrays/functions is not implemented",
            ));
        }
        if self.reflection.non_extensible.contains(&owner)
            || self
                .reflection
                .prototypes
                .get("Object")
                .is_some_and(|value| crate::strict_equal(value, target))
        {
            return Ok(false);
        }
        let mut ancestor = prototype.clone();
        let mut finished = false;
        for _ in 0..64 {
            self.tick()?;
            if ancestor == Value::Null {
                finished = true;
                break;
            }
            if crate::strict_equal(&ancestor, target) {
                return Ok(false);
            }
            ancestor = if ancestor == Value::Native("Function.prototype") {
                self.object_prototype_value()?
            } else {
                self.reflect_prototype(&ancestor)?
            };
        }
        if !finished {
            return Err(JsError::resource(
                "Reflect prototype cycle check exceeds 64 levels",
            ));
        }
        // No callback or mutation occurs before the full cycle check completes.
        let Heap::Object(object) = &mut self.heap[owner] else {
            unreachable!()
        };
        object.prototype = if prototype == Value::Null {
            None
        } else {
            Some(prototype)
        };
        Ok(true)
    }
    pub(super) fn reflect_call(
        &mut self,
        name: &str,
        args: &[Value],
        construct: bool,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        if !name.starts_with("Reflect.") {
            return Ok(None);
        }
        if construct {
            return Err(self.property_type_error("Reflect methods are not constructors"));
        }
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        if name == "Reflect.apply" {
            if !self.is_callable_value(&target, host) {
                return Err(self.property_type_error("Reflect.apply target must be callable"));
            }
            let arguments =
                self.reflect_arguments(args.get(2).unwrap_or(&Value::Undefined), host)?;
            return self
                .call(
                    target,
                    arguments,
                    args.get(1).cloned().unwrap_or(Value::Undefined),
                    host,
                )
                .map(Some);
        }
        if name == "Reflect.construct" {
            self.reflect_constructor(&target, host)?;
            if let Some(new_target) = args.get(2) {
                self.reflect_constructor(new_target, host)?;
                if !crate::strict_equal(new_target, &target) {
                    return Err(self.property_type_error(
                        "Reflect.construct with a different newTarget is not implemented",
                    ));
                }
            }
            let arguments =
                self.reflect_arguments(args.get(1).unwrap_or(&Value::Undefined), host)?;
            return self.construct(target, arguments, host).map(Some);
        }
        let owner = self.reflect_target(&target)?;
        let result = match name {
            "Reflect.getPrototypeOf" => self.reflect_prototype(&target)?,
            "Reflect.setPrototypeOf" => Value::Bool(self.reflect_set_prototype(
                &target,
                args.get(1).cloned().unwrap_or(Value::Undefined),
            )?),
            "Reflect.defineProperty" => {
                let key = self.reflect_key(args.get(1).unwrap_or(&Value::Undefined), host)?;
                let definition =
                    self.definition(args.get(2).cloned().unwrap_or(Value::Undefined), host)?;
                Value::Bool(self.reflect_define_ordinary(&target, &key, definition, host)?)
            }
            "Reflect.set" => {
                let key = self.reflect_key(args.get(1).unwrap_or(&Value::Undefined), host)?;
                let value = args.get(2).cloned().unwrap_or(Value::Undefined);
                let receiver = args.get(3).cloned().unwrap_or_else(|| target.clone());
                Value::Bool(self.reflect_set(target, key, value, receiver, host)?)
            }
            "Reflect.isExtensible" => Value::Bool(!self.reflection.non_extensible.contains(&owner)),
            "Reflect.preventExtensions" => {
                self.reflection.non_extensible.insert(owner);
                Value::Bool(true)
            }
            "Reflect.ownKeys" => {
                let names = self.own_names(&target)?;
                let mut symbols = self.symbol_order.get(&owner).cloned().unwrap_or_default();
                if let Some(properties) = self.symbol_properties.get(&owner) {
                    for key in properties.keys() {
                        if !symbols.contains(key) {
                            symbols.push(*key);
                        }
                    }
                }
                if names.len().saturating_add(symbols.len()) > 4096 {
                    return Err(JsError::resource("Reflect own key count exceeds 4096"));
                }
                let mut keys = Vec::with_capacity(names.len() + symbols.len());
                for name in names {
                    self.tick()?;
                    keys.push(self.string(name)?);
                }
                for key in symbols {
                    self.tick()?;
                    keys.push(self.symbol_value(key)?);
                }
                Value::Array(self.allocate(Heap::Array(keys))?)
            }
            "Reflect.get"
            | "Reflect.has"
            | "Reflect.deleteProperty"
            | "Reflect.getOwnPropertyDescriptor" => {
                let key = self.reflect_key(args.get(1).unwrap_or(&Value::Undefined), host)?;
                match name {
                    "Reflect.get" | "Reflect.has" => {
                        let receiver = args.get(2).cloned().unwrap_or_else(|| target.clone());
                        self.reflect_lookup(target, key, receiver, name == "Reflect.has", host)?
                    }
                    "Reflect.getOwnPropertyDescriptor" => self.descriptor_native(
                        "Object.getOwnPropertyDescriptor",
                        &[target, key],
                        host,
                    )?,
                    _ => Value::Bool(if let Value::Symbol(symbol, _) = key {
                        self.delete_symbol(target, symbol, host)?
                    } else {
                        let key = self.text(&key, 0)?;
                        self.delete_property(target, &key, host)?
                    }),
                }
            }
            _ => return Err(self.property_type_error("Reflect operation is not implemented")),
        };
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Host, JsError, Runtime, Value};
    struct Empty;
    impl Host for Empty {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            Err(JsError::runtime("No host"))
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Err(JsError::runtime("No host"))
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            Err(JsError::runtime("No host"))
        }
    }
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty)
    }
    #[test]
    fn namespace_is_an_object_with_hidden_detachable_methods() {
        for source in [
            "typeof Reflect==='object' && Reflect===Reflect && Object.keys(Reflect).length===0;",
            "Object.getPrototypeOf(Reflect)===Object.prototype && !Object.getOwnPropertyDescriptor(Reflect,'get').enumerable;",
            "const get=Reflect.get;get({x:4},'x')===4 && typeof Reflect.set==='function' && typeof Reflect.defineProperty==='function' && typeof Reflect.setPrototypeOf==='function';",
            "try{new Reflect.get({},'x');false;}catch(e){e.name==='TypeError';}",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn property_reads_preserve_receivers_and_existence_skips_getters() {
        for source in [
            "const p={};Object.defineProperty(p,'x',{get:function(){return this.n;}});const o=Object.create(p);Reflect.get(o,'x',{n:7})===7;",
            "const s=Symbol();const p={};Object.defineProperty(p,s,{get:function(){return this.n;}});Reflect.get(Object.create(p),s,{n:8})===8;",
            "const o={};Object.defineProperty(o,'x',{get:function(){throw 17;}});Reflect.has(o,'x') && Reflect.getOwnPropertyDescriptor(o,'x').get!==undefined;",
            "const o={};Object.defineProperty(o,'x',{get:function(){throw 17;}});try{Reflect.get(o,'x');false;}catch(e){e===17;}",
            "const o=Object.create({x:undefined});Reflect.has(o,'x') && !Reflect.has(o,'y');",
            "const s=Symbol();const o={};o[s]=undefined;Reflect.has(Object.create(o),s);",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn keys_deletion_and_integrity_have_reflect_results() {
        for source in [
            "const s=Symbol();const o={b:1,2:2,a:3};Object.defineProperty(o,'hidden',{value:4});o[s]=5;const k=Reflect.ownKeys(o);k.length===5&&k[0]==='2'&&k[1]==='b'&&k[2]==='a'&&k[3]==='hidden'&&k[4]===s;",
            "const o={a:1};Object.defineProperty(o,'x',{value:2});Reflect.deleteProperty(o,'a')&&!Reflect.deleteProperty(o,'x')&&Reflect.deleteProperty(o,'missing');",
            "const s=Symbol();const o={};Object.defineProperty(o,s,{value:2});!Reflect.deleteProperty(o,s)&&Reflect.getOwnPropertyDescriptor(o,s).value===2;",
            "const a=[1,2];Reflect.deleteProperty(a,'0')&&!Reflect.has(a,'0')&&a.length===2&&!Reflect.deleteProperty(a,'length');",
            "const o={};Reflect.isExtensible(o)&&Reflect.preventExtensions(o)===true&&!Reflect.isExtensible(o)&&Reflect.preventExtensions(o)===true;",
            "Reflect.getPrototypeOf(Object.create(null))===null&&Reflect.getPrototypeOf([])===Array.prototype;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn call_construct_and_argument_getters_obey_existing_bounds() {
        for source in [
            "function f(x,y){return this.n+x+y;}Reflect.apply(f,{n:2},{0:3,1:4,length:2})===9;",
            "Reflect.apply(Math.max,null,[3,9,2])===9;",
            "function F(x){this.x=x;}const o=Reflect.construct(F,[5]);o.x===5&&o instanceof F;",
            "Reflect.construct(Array,[2,3]).join(',')==='2,3';",
            "function F(x){this.x=x;}const B=F.bind(null,7);Reflect.construct(B,[]).x===7;",
            "function F(){}Reflect.construct(F,[],F) instanceof F;",
            "const a={};Object.defineProperty(a,'length',{get:function(){throw 31;}});try{Reflect.apply(function(){},null,a);false;}catch(e){e===31;}",
            "let n=0;const a={};Object.defineProperty(a,'length',{get:function(){n++;return 0;}});try{Reflect.construct(()=>1,a);}catch(e){}n===0;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        assert!(run("Reflect.apply(function(){},null,{length:33});").is_err());
    }
    #[test]
    fn define_property_returns_false_only_for_internal_rejection() {
        for source in [
            "class Base{};Base.value=7;class Desc extends Base{};const o={};Reflect.defineProperty(o,'x',Desc)&&o.x===7;",
            "let calls=0;class Base{};Object.defineProperty(Base,'value',{get:function(){calls++;return 9}});class Desc extends Base{};const o={};Reflect.defineProperty(o,'x',Desc)&&o.x===9&&calls===1;",
            "class Base{};Object.defineProperty(Base,'value',{get:function(){throw 42}});class Desc extends Base{};try{Reflect.defineProperty({},'x',Desc);false}catch(e){e===42;}",
            "const o={};Reflect.defineProperty(o,'x',{value:3}) && o.x===3 && !Reflect.defineProperty(o,'x',{value:4});",
            "const o={};Reflect.preventExtensions(o);!Reflect.defineProperty(o,'x',{value:2})&&!Reflect.has(o,'x');",
            "const o={};Reflect.preventExtensions(o);const d={};Object.defineProperty(d,'value',{get:function(){throw 23;}});try{Reflect.defineProperty(o,'x',d);false;}catch(e){e===23;}",
            "try{Reflect.defineProperty({},'x',{get:3});false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.defineProperty({},'x',{value:1,get:function(){}});false;}catch(e){e.name==='TypeError';}",
            "const s=Symbol();const o={};Reflect.defineProperty(o,s,{value:NaN})&&Reflect.defineProperty(o,s,{value:NaN})&&!Reflect.defineProperty(o,s,{value:2});",
            "const o={};Reflect.defineProperty(o,'x',{value:0});!Reflect.defineProperty(o,'x',{value:-0});",
            "const o={};Reflect.defineProperty(o,'x',{value:1});try{Object.defineProperty(o,'x',{value:2});false;}catch(e){e.name==='TypeError';}",
            "const a=[];Object.defineProperty(a,'length',{writable:false});!Reflect.defineProperty(a,'0',{value:2})&&a.length===0;",
            "const a=[];Reflect.defineProperty(a,'2',{value:7,writable:true})&&a.length===3&&a[2]===7;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn set_obeys_receiver_descriptors_and_propagates_setters() {
        for source in [
            "const p={x:1};const r={};Reflect.set(p,'x',4,r)&&p.x===1&&r.x===4;",
            "const p={};Object.defineProperty(p,'x',{value:1});!Reflect.set(p,'x',2,{})&&!Reflect.set(p,'x',2);",
            "const p={x:1};const r={};Object.defineProperty(r,'x',{set:function(){throw 99;}});!Reflect.set(p,'x',2,r);",
            "const p={};Object.defineProperty(p,'x',{set:function(v){this.n=v;}});const r={};Reflect.set(Object.create(p),'x',8,r)&&r.n===8&&!Reflect.has(p,'n');",
            "const s=Symbol();const p={};Object.defineProperty(p,s,{set:function(v){this.n=v;}});const r={};Reflect.set(Object.create(p),s,9,r)&&r.n===9;",
            "const p={};Object.defineProperty(p,'x',{set:function(){throw 37;}});try{Reflect.set(p,'x',1);false;}catch(e){e===37;}",
            "const p={};Object.defineProperty(p,'x',{get:function(){throw 41;}});!Reflect.set(p,'x',1);",
            "!Reflect.set({},'x',2,3)&&!Reflect.set({},'x',2,null);",
            "const r={};Reflect.preventExtensions(r);!Reflect.set({x:1},'x',2,r);",
            "const r={x:0};Object.preventExtensions(r);Reflect.set({x:1},'x',2,r)&&r.x===2;",
            "const a=[];Object.defineProperty(a,'length',{writable:false});!Reflect.set(a,'0',1)&&a.length===0;",
            "const a=[];Reflect.set(a,'2',5)&&a.length===3&&a[2]===5;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn prototype_mutation_checks_identity_cycles_and_extensibility() {
        for source in [
            "const o={};const p={x:3};Reflect.setPrototypeOf(o,p)&&Reflect.getPrototypeOf(o)===p&&o.x===3;",
            "const o={};Reflect.setPrototypeOf(o,null)&&Reflect.getPrototypeOf(o)===null;",
            "const o={};!Reflect.setPrototypeOf(o,o)&&Reflect.getPrototypeOf(o)===Object.prototype;",
            "const a={};const b=Object.create(a);!Reflect.setPrototypeOf(a,b)&&Reflect.getPrototypeOf(a)===Object.prototype;",
            "const o={};Object.preventExtensions(o);Reflect.setPrototypeOf(o,Object.prototype)&&!Reflect.setPrototypeOf(o,null);",
            "!Reflect.setPrototypeOf(Object.prototype,{})&&Reflect.setPrototypeOf(Object.prototype,null);",
            "Reflect.setPrototypeOf([],Array.prototype);",
            "try{Reflect.setPrototypeOf([],null);false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.setPrototypeOf({},3);false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.setPrototypeOf({},new Map());false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.defineProperty([],'length',{value:0});false;}catch(e){e.name==='TypeError';}",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn unsupported_targets_and_coercions_fail_explicitly() {
        for source in [
            "try{Reflect.get(3,'x');false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.ownKeys('abc');false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.preventExtensions(new Uint8Array(2));false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.get(/x/,'source');false;}catch(e){e.name==='TypeError';}",
            "Reflect.get({x:7}, {toString:function(){return 'x';}})===7;",
            "try{Reflect.apply(function(){},null,null);false;}catch(e){e.name==='TypeError';}",
            "try{Reflect.construct(function(){},[],function(){});false;}catch(e){e.name==='TypeError';}",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
}
