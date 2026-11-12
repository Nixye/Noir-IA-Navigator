//! Callable adapters retain VM values and reuse the existing call/heap budgets.
use crate::{Function, Heap, Host, JsError, Runtime, Value};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone)]
pub(super) struct BoundFunction {
    pub target: Value,
    pub this: Value,
    pub arguments: Arc<Vec<Value>>,
}

pub(super) fn callable_method(property: &str) -> Option<Value> {
    Some(Value::Native(match property {
        "call" => "Function.call",
        "apply" => "Function.apply",
        "bind" => "Function.bind",
        "constructor" => "Function",
        _ => return None,
    }))
}

pub(super) fn native_length(name: &str) -> usize {
    if let Some(length) = crate::numeric_globals::native_length(name) {
        return length;
    }
    if let Some(length) = crate::math::native_length(name) {
        return length;
    }
    match name {
        "SubtleCrypto.digest" => 2,
        "WebAssembly.Module"
        | "WebAssembly.Instance"
        | "WebAssembly.validate"
        | "WebAssembly.CompileError"
        | "WebAssembly.LinkError"
        | "WebAssembly.RuntimeError" => 1,
        "Function.prototype" | "Array.pop" | "Performance.now" => 0,
        "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array" | "BigInt64Array"
        | "BigUint64Array" => 3,
        "DataView.getBigInt64" | "DataView.getBigUint64" => 1,
        "DataView.setBigInt64" | "DataView.setBigUint64" => 2,
        "Function.call"
        | "Symbol.for"
        | "Symbol.keyFor"
        | "Function.bind"
        | "Function"
        | "Array"
        | "Object"
        | "String"
        | "WeakRef"
        | "FinalizationRegistry"
        | "FinalizationRegistry.unregister"
        | "WeakMap.get"
        | "WeakMap.has"
        | "WeakMap.delete"
        | "WeakSet.add"
        | "WeakSet.has"
        | "WeakSet.delete"
        | "BigInt"
        | "BigInt.toString"
        | "Generator.next"
        | "Generator.throw"
        | "Generator.return"
        | "AsyncGenerator.next"
        | "AsyncGenerator.throw"
        | "AsyncGenerator.return"
        | "Reflect.ownKeys"
        | "Reflect.getPrototypeOf"
        | "Reflect.isExtensible"
        | "Reflect.preventExtensions"
        | "Number"
        | "Boolean"
        | "Array.isArray"
        | "Array.from"
        | "Array.push"
        | "Array.join"
        | "Array.forEach"
        | "Array.map"
        | "String.fromCharCode"
        | "Object.getPrototypeOf"
        | "Object.keys"
        | "Object.values"
        | "Object.entries"
        | "Object.getOwnPropertyNames"
        | "Object.getOwnPropertyDescriptors"
        | "RegExp.test"
        | "RegExp.exec"
        | "Error"
        | "TypeError"
        | "RangeError"
        | "ReferenceError"
        | "SyntaxError"
        | "parseFloat"
        | "isNaN"
        | "eval"
        | "queueMicrotask"
        | "Promise"
        | "Promise.resolve"
        | "Promise.reject"
        | "Promise.all"
        | "Promise.race"
        | "Promise.allSettled"
        | "Promise.any"
        | "Promise.catch"
        | "Promise.finally"
        | "Object.preventExtensions"
        | "Object.isExtensible"
        | "Object.seal"
        | "Object.freeze"
        | "Object.isSealed"
        | "Object.isFrozen"
        | "Object.getOwnPropertySymbols"
        | "Object.hasOwnProperty"
        | "Object.propertyIsEnumerable"
        | "Object.isPrototypeOf"
        | "String.fromCodePoint" => 1,
        "FinalizationRegistry.register"
        | "WeakMap.set"
        | "Function.apply"
        | "RegExp"
        | "Object.create"
        | "Object.hasOwn"
        | "Object.getOwnPropertyDescriptor"
        | "Object.defineProperties"
        | "JSON.parse"
        | "parseInt"
        | "BigInt.asIntN"
        | "BigInt.asUintN"
        | "Reflect.construct"
        | "Reflect.get"
        | "Reflect.has"
        | "Reflect.deleteProperty"
        | "Reflect.getOwnPropertyDescriptor"
        | "Reflect.setPrototypeOf"
        | "Promise.then"
        | "AggregateError"
        | "Object.assign"
        | "Object.is"
        | "Object.setPrototypeOf" => 2,
        "Object.defineProperty" | "Reflect.apply" | "Reflect.set" | "Reflect.defineProperty" => 3,
        _ => 0,
    }
}

impl Runtime {
    pub fn read_property(
        &mut self,
        object: Value,
        property: &str,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if property.len() > 1024 {
            return Err(JsError::resource("Property name exceeds limit"));
        }
        self.tick()?;
        self.get(object, property, host)
    }

    pub fn is_callable_value(&self, value: &Value, host: &impl Host) -> bool {
        match value {
            Value::Function(id) => matches!(self.heap.get(*id), Some(Heap::Function(_))),
            Value::Native(_) => true,
            Value::Host(id) => host.is_callable(*id),
            _ => false,
        }
    }

    pub(super) fn callable_adapter(
        &mut self,
        method: &str,
        target: Value,
        mut args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        crate::call_storage::check_arguments(args.len())?;
        if !self.is_callable_value(&target, host) {
            return Err(self.property_type_error("Function adapter requires a callable receiver"));
        }
        let this = args.first().cloned().unwrap_or(Value::Undefined);
        match method {
            "call" => {
                if !args.is_empty() {
                    self.value_copy_work(args.len() - 1)?;
                    args.remove(0);
                }
                self.call(target, args, this, host)
            }
            "apply" => {
                let arguments = self.apply_arguments(args.get(1), host)?;
                self.call(target, arguments, this, host)
            }
            "bind" => {
                if !args.is_empty() {
                    self.value_copy_work(args.len() - 1)?;
                    args.remove(0);
                }
                self.bind_function(target, this, args, host)
            }
            _ => Err(JsError::runtime("Unknown function adapter")),
        }
    }

    fn apply_arguments(
        &mut self,
        value: Option<&Value>,
        host: &mut impl Host,
    ) -> Result<Vec<Value>, JsError> {
        let value = match value {
            None | Some(Value::Undefined | Value::Null) => return Ok(Vec::new()),
            Some(
                value @ (Value::Array(_)
                | Value::Object(_)
                | Value::Function(_)
                | Value::Host(_)
                | Value::Native(_)),
            ) => value.clone(),
            Some(_) => {
                return Err(self.property_type_error("Function.apply arguments must be an object"));
            }
        };
        let length = self.get(value.clone(), "length", host)?;
        if matches!(
            length,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Host(_)
                | Value::Native(_)
        ) {
            return Err(JsError::runtime(
                "Object coercion of apply length is not implemented",
            ));
        }
        let length = self.number(&length);
        // ToLength, with the existing much smaller call argument cap enforced
        // before allocating or evaluating indexed getters.
        let length = if length.is_nan() || length <= 0. {
            0.
        } else {
            length.trunc()
        };
        if length > crate::call_storage::MAX_CALL_ARGUMENTS as f64 {
            return Err(JsError::resource("Apply argument limit exceeded"));
        }
        let mut arguments = self.value_buffer(length as usize)?;
        for index in 0..length as usize {
            self.tick()?;
            arguments.push(self.get(value.clone(), &index.to_string(), host)?);
        }
        Ok(arguments)
    }

    pub(super) fn bind_function(
        &mut self,
        target: Value,
        this: Value,
        arguments: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if arguments.len() > crate::call_storage::MAX_CALL_ARGUMENTS {
            return Err(JsError::resource("Bound argument limit exceeded"));
        }
        // BoundFunctionCreate captures [[Prototype]] before observable length
        // and name reads. Later changes to the target do not affect the bound.
        let internal_prototype = match &target {
            Value::Function(id) => Some(self.function_internal_prototype(*id)?),
            value if crate::reflection::intrinsic_ancestors::error_ancestor(value).is_some() => {
                self.readonly_ancestor_parent(value)?
            }
            _ => None,
        };
        // Bound length depends only on an own length property. A deleted VM
        // length must not read a native ancestor's length or invoke an inherited
        // getter. The existing native metadata inventory is own, without
        // granting native public descriptor access.
        let own_length = match &target {
            Value::Function(_) | Value::Native("Number" | "Symbol") => {
                self.has_own_property(&target, "length")?
            }
            Value::Native(_) => true,
            Value::Host(id) => host.is_constructor(*id),
            _ => false,
        };
        let length = if own_length {
            self.get(target.clone(), "length", host)?
        } else {
            Value::Undefined
        };
        let length = match length {
            Value::Number(number) if !number.is_nan() && number > 0. => {
                (number.trunc() - arguments.len() as f64).max(0.)
            }
            _ => 0.,
        };
        let name = match &target {
            Value::Host(id) if !host.is_constructor(*id) => Value::Undefined,
            _ => self.get(target.clone(), "name", host)?,
        };
        let name = if let Value::String(name) = name {
            name
        } else {
            Arc::from("")
        };
        if name.len() > 65_536 - "bound ".len() {
            return Err(JsError::resource("Bound function name exceeds limit"));
        }
        let bound_name = self.string(format!("bound {name}"))?;
        // The argument builder already reserved this buffer. Arc adopts it
        // without cloning its Value slots.
        self.charge_string("length".len() + "name".len())?;
        let id = self.allocate(Heap::Function(Function {
            params: Arc::new(Vec::new()),
            body: Arc::new(Vec::new()),
            scope: 0,
            strict: false,
            parameters: None,
            constructible: false,
            kind: crate::parser::FunctionKind::Ordinary,
            method: false,
            // Bound call/construct are intercepted before ordinary/arrow paths.
            // This flag also suppresses an ordinary own prototype property.
            arrow: true,
            lexical_this: Value::Undefined,
            internal_prototype,
            properties: BTreeMap::from([
                ("length".into(), Value::Number(length)),
                ("name".into(), bound_name),
            ]),
        }))?;
        for property in ["length", "name"] {
            self.property_metadata.entry(id).or_default().insert(
                property.into(),
                crate::descriptors::PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        self.bound_functions.insert(
            id,
            BoundFunction {
                target,
                this,
                arguments: Arc::new(arguments),
            },
        );
        Ok(Value::Function(id))
    }

    pub(super) fn prepend_bound(
        &mut self,
        bound: &BoundFunction,
        args: Vec<Value>,
    ) -> Result<Vec<Value>, JsError> {
        let length = bound.arguments.len().saturating_add(args.len());
        if length > crate::call_storage::MAX_CALL_ARGUMENTS {
            return Err(JsError::resource("Bound call argument limit exceeded"));
        }
        let mut combined = self.value_buffer(length)?;
        combined.extend(bound.arguments.iter().cloned());
        combined.extend(args);
        Ok(combined)
    }

    pub(super) fn construct_bound(
        &mut self,
        bound: BoundFunction,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if self.call_depth >= self.limits.call_depth {
            return Err(JsError::resource("JavaScript call stack budget exceeded"));
        }
        let args = self.prepend_bound(&bound, args)?;
        self.call_depth += 1;
        let result = self.construct(bound.target, args, host);
        self.call_depth -= 1;
        result
    }
}

pub(super) fn error_context(mut error: JsError, context: &str) -> JsError {
    if error.kind == crate::ErrorKind::Runtime && error.message == "Value is not callable" {
        error.message = format!(
            "Value is not callable ({})",
            context.chars().take(96).collect::<String>()
        );
    }
    error
}
