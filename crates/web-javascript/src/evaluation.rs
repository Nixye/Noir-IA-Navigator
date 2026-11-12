//! Borrowed synchronous expression continuations and shared callable leaves.
//! Recursive literal helpers below exist only as differential test oracles.
pub(crate) mod storage;
pub(crate) mod sync_arguments;
pub(crate) mod sync_calls;
pub(crate) mod sync_frames;
pub(crate) mod sync_literals;
pub(crate) mod sync_machine;
pub(crate) mod sync_references;
#[cfg(test)]
use crate::{ArrayHoles, descriptors};
use crate::{Expr, Host, JsError, Runtime, Value, regexp_source};
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};

impl Runtime {
    #[cfg(test)]
    #[inline(never)]
    pub(super) fn eval_array_expression(
        &mut self,
        node: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let Expr::Array(values) = node else {
            unreachable!()
        };

        if values.len() > 4096 {
            return Err(JsError::resource("Array limit exceeded"));
        }
        let mut output = Vec::new();
        let mut holes = ArrayHoles::new();
        for expr in values {
            match expr {
                Expr::Spread(expr) => {
                    let source = self.eval(expr, scope, host)?;
                    self.literal_array_spread(&mut output, source, host)?;
                }
                Expr::Hole => {
                    self.literal_array_append(&mut output, &mut holes, Value::Undefined, true)?
                }
                expr => {
                    let value = self.eval(expr, scope, host)?;
                    self.literal_array_append(&mut output, &mut holes, value, false)?;
                }
            }
        }
        self.literal_array_finish(output, holes)
    }
    #[cfg(test)]
    #[inline(never)]
    pub(super) fn eval_object_expression(
        &mut self,
        node: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let Expr::Object(pairs) = node else {
            unreachable!()
        };

        Self::check_named_property_count(pairs.len())?;
        let object = self.ordinary_object(BTreeMap::new())?;
        for (name, expr) in pairs {
            if let Expr::Spread(expr) = expr {
                let source = self.eval(expr, scope, host)?;
                self.copy_properties(object.clone(), source, &BTreeSet::new(), host)?;
            } else {
                let value = self.eval(expr, scope, host)?;
                if matches!(expr, Expr::Function { method: true, .. }) {
                    let function_name = self.string(name.clone())?;
                    self.set_unchecked(value.clone(), "name", function_name, host)?;
                    if let Value::Function(id) = &value {
                        self.property_metadata.entry(*id).or_default().insert(
                            "name".into(),
                            descriptors::PropertyMetadata {
                                writable: false,
                                enumerable: false,
                                configurable: true,
                                accessor: None,
                            },
                        );
                    }
                }
                self.create_data_property(&object, name, value, host)?;
            }
        }
        Ok(object)
    }
    #[inline(never)]
    pub(super) fn eval_function_expression(
        &mut self,
        node: &Expr,
        scope: usize,
        _host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let Expr::Function {
            name,
            params,
            body,
            arrow,
            parameters,
            kind,
            method,
        } = node
        else {
            unreachable!()
        };

        if let Some(name) = name {
            let child = self.scope(scope, false)?;
            self.declare(child, name, false, true)?;
            let function = self.make_function(params.clone(), body.clone(), child, *arrow)?;
            self.attach_parameters(&function, parameters.clone())?;
            self.configure_resumable_function(&function, *kind, *method)?;
            self.scopes[child].bindings.get_mut(name).unwrap().value = function.clone();
            Ok(function)
        } else {
            let function = self.make_function(params.clone(), body.clone(), scope, *arrow)?;
            self.attach_parameters(&function, parameters.clone())?;
            self.configure_resumable_function(&function, *kind, *method)?;
            Ok(function)
        }
    }
    #[cfg(test)]
    #[inline(never)]
    pub(super) fn eval_template_expression(
        &mut self,
        node: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let Expr::Template(parts) = node else {
            unreachable!()
        };

        let mut units = Vec::new();
        for part in parts {
            let value = self.eval(part, scope, host)?;
            self.literal_template_append(&mut units, value, host)?;
        }
        self.literal_template_finish(units)
    }
    #[inline(never)]
    pub(super) fn call_native(
        &mut self,
        name: &'static str,
        args: Vec<Value>,
        this: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if name == "Array.from" {
            return self.array_from(&args, this, host);
        }
        if name == "SubtleCrypto.digest" {
            return self.crypto_digest(&args, &this, host);
        }
        if name == "Performance.now" {
            return self.performance_now(&this, host);
        }
        if let Some(value) = self.wasm_native_call(name, &args, false, host)? {
            return Ok(value);
        }
        if let Some(value) = self.numeric_global_call(name, &args, false, host)? {
            return Ok(value);
        }
        if let Some(value) = self.conversion_builtin(name, &args, false, host)? {
            return Ok(value);
        }
        if matches!(
            name,
            "BigInt.toString" | "BigInt.valueOf" | "BigInt.toLocaleString"
        ) {
            return self.bigint_method(name, &this, &args);
        }
        if let Some(value) = self.resumable_native_call(name, &args, this.clone(), host)? {
            return Ok(value);
        }
        if let Some(value) = self.reflect_call(name, &args, false, host)? {
            return Ok(value);
        }
        if let Some(value) = self.math_call(name, &args, false, host)? {
            return Ok(value);
        }
        if let Some(value) = self.reflection_call(name, &args, this.clone(), false, host)? {
            return Ok(value);
        }
        if let Some(value) = self.promise_call(name, &args, this.clone(), false, host)? {
            return Ok(value);
        }
        if let Some(value) = self.weak_ref_call(name, &args, this.clone(), false)? {
            return Ok(value);
        }
        if let Some(value) =
            self.finalization_registry_call(name, &args, this.clone(), false, host)?
        {
            return Ok(value);
        }
        if let Some(value) = self.weak_collection_call(name, &args, this.clone(), false, host)? {
            return Ok(value);
        }
        if let Some(value) = self.stdlib_call(name, &args, this.clone(), false, host)? {
            return Ok(value);
        }
        if name == "JSON.parse" {
            return self.json_parse_with_host(&args, host);
        }
        if name == "JSON.stringify" {
            return self.json_stringify(&args, host);
        }
        if name == "String.fromCharCode" {
            return self.string_from_char_code(&args);
        }
        if name == "String.fromCodePoint" {
            return self.string_from_code_point(&args);
        }
        if let Some(method) = name.strip_prefix("String.") {
            return self.string_method(&this, method, args, host);
        }
        if let Some(method) = name.strip_prefix("Function.")
            && matches!(method, "call" | "apply" | "bind")
        {
            return self.callable_adapter(method, this, args, host);
        }
        if name == "Function.prototype" {
            return Ok(Value::Undefined);
        }
        if name == "Object.hasOwn" {
            let target = args.first().cloned().unwrap_or(Value::Undefined);
            if matches!(target, Value::Null | Value::Undefined) {
                return Err(self.property_type_error("Object.hasOwn requires a non-null target"));
            }
            let object = self.box_value(target)?;
            let key =
                self.coerce_property_key(args.get(1).cloned().unwrap_or(Value::Undefined), host)?;
            let present = if let Value::Symbol(symbol, _) = key {
                self.symbol_descriptor(&object, symbol)?.is_some()
            } else {
                let name = self.text(&key, 0)?;
                if self.is_global_object(&object) {
                    self.global_has_own(&name, host)?
                } else {
                    self.has_own_property(&object, &name)?
                }
            };
            return Ok(Value::Bool(present));
        }
        if name == "eval" {
            return self.dynamic_eval(&args, 0, false, host);
        }
        if name == "Function" {
            return self.dynamic_function(&args, host);
        }
        if matches!(
            name,
            "Object.create"
                | "Object.getOwnPropertyDescriptor"
                | "Object.getOwnPropertyDescriptors"
                | "Object.getOwnPropertyNames"
                | "Object.defineProperty"
                | "Object.defineProperties"
                | "Object.keys"
                | "Object.values"
                | "Object.entries"
        ) {
            return self.descriptor_native(name, &args, host);
        }
        if name == "Array.forEach" {
            return self.array_for_each(this, &args, host);
        }
        if let Some(property) = name.strip_prefix("Array.")
            && matches!(
                property,
                "push"
                    | "pop"
                    | "join"
                    | "forEach"
                    | "map"
                    | "slice"
                    | "concat"
                    | "filter"
                    | "reduce"
                    | "reduceRight"
                    | "find"
                    | "findIndex"
                    | "findLast"
                    | "findLastIndex"
                    | "every"
                    | "some"
                    | "indexOf"
                    | "lastIndexOf"
                    | "includes"
                    | "at"
                    | "reverse"
                    | "toReversed"
                    | "shift"
                    | "unshift"
                    | "splice"
                    | "toSpliced"
                    | "sort"
                    | "toSorted"
                    | "fill"
                    | "copyWithin"
                    | "flat"
                    | "flatMap"
                    | "toString"
            )
        {
            let Value::Array(id) = this else {
                return Err(JsError::runtime("Array method requires an array receiver"));
            };
            return self.array_method(id, property, args, host);
        }
        if matches!(name, "RegExp.test" | "RegExp.exec" | "RegExp.toString") {
            let Value::Object(id) = this else {
                return Err(JsError::runtime("RegExp method requires a RegExp receiver"));
            };
            let compiled = self.regexp(id)?;
            if name == "RegExp.toString" {
                return self.string(format!(
                    "/{}/{}",
                    regexp_source(&compiled.source),
                    compiled.flags
                ));
            }
            let text = self.text(args.first().unwrap_or(&Value::Undefined), 0)?;
            let result = self.regexp_exec(id, &text)?;
            return if name == "RegExp.test" {
                Ok(Value::Bool(result.is_some()))
            } else {
                match result {
                    Some(result) => self.regexp_array(result, &text),
                    None => Ok(Value::Null),
                }
            };
        }
        self.native(name, &args)
    }
}

#[cfg(test)]
mod stack_tests;

#[cfg(test)]
mod sync_guard_tests;

#[cfg(test)]
#[path = "evaluation/sequence_tests.rs"]
mod sequence_tests;

#[cfg(test)]
#[path = "evaluation/conditional_chain_tests.rs"]
mod conditional_chain_tests;
