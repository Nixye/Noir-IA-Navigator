//! Bounded property existence, deletion and enumeration for the VM-owned heap.
use crate::{
    Expr, Heap, Host, JsError, Place, Runtime, Value, intrinsic_global, intrinsic_property,
    strict_equal,
};
use std::collections::BTreeSet;

pub(super) fn array_index(property: &str) -> Option<usize> {
    let index = property.parse::<usize>().ok()?;
    (index.to_string() == property).then_some(index)
}

impl Runtime {
    pub(super) fn array_prototype_value(&mut self) -> Result<Value, JsError> {
        if let Some(id) = self.array_prototype {
            return Ok(Value::Array(id));
        }
        let id = self.allocate(Heap::Array(Vec::new()))?;
        self.array_prototype = Some(id);
        let properties = std::collections::BTreeMap::from([
            ("constructor".into(), Value::Native("Array")),
            ("push".into(), Value::Native("Array.push")),
            ("pop".into(), Value::Native("Array.pop")),
            ("join".into(), Value::Native("Array.join")),
            ("forEach".into(), Value::Native("Array.forEach")),
            ("map".into(), Value::Native("Array.map")),
            ("slice".into(), Value::Native("Array.slice")),
            ("concat".into(), Value::Native("Array.concat")),
            ("filter".into(), Value::Native("Array.filter")),
            ("reduce".into(), Value::Native("Array.reduce")),
            ("reduceRight".into(), Value::Native("Array.reduceRight")),
            ("find".into(), Value::Native("Array.find")),
            ("findIndex".into(), Value::Native("Array.findIndex")),
            ("findLast".into(), Value::Native("Array.findLast")),
            ("findLastIndex".into(), Value::Native("Array.findLastIndex")),
            ("every".into(), Value::Native("Array.every")),
            ("some".into(), Value::Native("Array.some")),
            ("indexOf".into(), Value::Native("Array.indexOf")),
            ("lastIndexOf".into(), Value::Native("Array.lastIndexOf")),
            ("includes".into(), Value::Native("Array.includes")),
            ("at".into(), Value::Native("Array.at")),
            ("reverse".into(), Value::Native("Array.reverse")),
            ("toReversed".into(), Value::Native("Array.toReversed")),
            ("shift".into(), Value::Native("Array.shift")),
            ("unshift".into(), Value::Native("Array.unshift")),
            ("splice".into(), Value::Native("Array.splice")),
            ("toSpliced".into(), Value::Native("Array.toSpliced")),
            ("sort".into(), Value::Native("Array.sort")),
            ("toSorted".into(), Value::Native("Array.toSorted")),
            ("fill".into(), Value::Native("Array.fill")),
            ("copyWithin".into(), Value::Native("Array.copyWithin")),
            ("flat".into(), Value::Native("Array.flat")),
            ("flatMap".into(), Value::Native("Array.flatMap")),
            ("toString".into(), Value::Native("Array.toString")),
        ]);
        self.array_non_enumerable
            .insert(id, properties.keys().cloned().collect());
        self.array_properties.insert(id, properties);
        Ok(Value::Array(id))
    }
    pub(super) fn property_type_error(&mut self, message: &str) -> JsError {
        match self.native("TypeError", &[Value::string(message)]) {
            Ok(value) => {
                let mut error = JsError::runtime(message);
                error.thrown = Some(value);
                error
            }
            Err(error) => error,
        }
    }
    pub(super) fn hide_properties(&mut self, value: &Value, properties: &[&str]) {
        if let Value::Object(id) = value
            && let Some(Heap::Object(object)) = self.heap.get_mut(*id)
        {
            object
                .non_enumerable
                .extend(properties.iter().map(|key| (*key).to_owned()));
        }
    }
    pub(super) fn array_has_index(&self, id: usize, index: usize, length: usize) -> bool {
        index < length
            && !self
                .array_holes
                .get(&id)
                .is_some_and(|holes| holes.contains(&index))
    }
    pub(super) fn has_own_property(
        &mut self,
        object: &Value,
        property: &str,
    ) -> Result<bool, JsError> {
        self.tick()?;
        if self.is_global_object(object) {
            return Ok(self.global_own_property(property).is_some());
        }
        if let Value::Native(name @ ("Symbol" | "Number")) = object {
            let storage = self.intrinsic_constructor_storage(name)?;
            return self.has_own_property(&storage, property);
        }
        if let Some(present) = self.stdlib_own_index(object, property) {
            return Ok(present);
        }
        Ok(match object {
            Value::Object(id) => {
                let Some(Heap::Object(object)) = self.heap.get(*id) else {
                    return Err(JsError::runtime("Invalid object reference"));
                };
                object.properties.contains_key(property)
            }
            Value::Function(id) => {
                let Some(Heap::Function(function)) = self.heap.get(*id) else {
                    return Err(JsError::runtime("Invalid function reference"));
                };
                function.properties.contains_key(property)
                    || (property == "length" && !self.synthetic_deleted(*id, property))
                    || (property == "prototype" && function.has_prototype())
            }
            Value::Array(id) => {
                let Some(Heap::Array(values)) = self.heap.get(*id) else {
                    return Err(JsError::runtime("Invalid array reference"));
                };
                property == "length"
                    || array_index(property)
                        .is_some_and(|index| self.array_has_index(*id, index, values.len()))
                    || self
                        .array_properties
                        .get(id)
                        .is_some_and(|properties| properties.contains_key(property))
            }
            _ => {
                return Err(JsError::runtime(
                    "Own property inspection supports VM objects, functions and arrays only",
                ));
            }
        })
    }
    pub(super) fn has_property(
        &mut self,
        object: &Value,
        property: &str,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        if property.len() > 1024 {
            return Err(JsError::resource("Property name exceeds limit"));
        }
        if let Value::Native(name @ ("Symbol" | "Number")) = object {
            let storage = self.intrinsic_constructor_storage(name)?;
            return self.has_property(&storage, property, host);
        }
        if let Some(present) = self.stdlib_has(object, property) {
            return Ok(present);
        }
        let mut current = object.clone();
        for _ in 0..64 {
            self.tick()?;
            match current {
                Value::Object(id) => {
                    let Some(Heap::Object(object)) = self.heap.get(id) else {
                        return Err(JsError::runtime("Invalid object reference"));
                    };
                    if object.properties.contains_key(property) {
                        return Ok(true);
                    }
                    match &object.prototype {
                        Some(prototype) => current = prototype.clone(),
                        None => return Ok(false),
                    }
                }
                Value::Array(_) => {
                    if self.has_own_property(&current, property)? {
                        return Ok(true);
                    }
                    current = if matches!(current, Value::Array(id) if Some(id) == self.array_prototype)
                    {
                        self.object_prototype_value()?
                    } else {
                        self.array_prototype_value()?
                    };
                }
                Value::Function(id) => {
                    if self.has_own_property(&current, property)? {
                        return Ok(true);
                    }
                    if let Some(base) = self.class_static_parent(id) {
                        current = base;
                    } else {
                        return Ok(crate::callables::callable_method(property).is_some());
                    }
                }
                Value::Native(name) => {
                    if matches!(name, "Symbol" | "Number") {
                        current = self.intrinsic_constructor_storage(name)?;
                        continue;
                    }
                    if name == "Function.prototype" {
                        if crate::native_symbol::FUNCTION_PROTOTYPE_NAMES.contains(&property) {
                            return Ok(true);
                        }
                        current = self.object_prototype_value()?;
                        continue;
                    }
                    if property == "prototype" && crate::wasm::constructor(name) {
                        return Ok(true);
                    }
                    if property == "prototype" {
                        if matches!(
                            name,
                            "Array"
                                | "Function"
                                | "Object"
                                | "String"
                                | "Number"
                                | "Boolean"
                                | "BigInt"
                                | "WeakRef"
                                | "FinalizationRegistry"
                                | "WeakMap"
                                | "WeakSet"
                        ) {
                            return Ok(true);
                        }
                        return Err(JsError::runtime(
                            "Native constructor prototype inspection is not implemented",
                        ));
                    }
                    return Ok(crate::reflection::property(name, property).is_some()
                        || intrinsic_property(name, property).is_some()
                        || matches!(property, "name" | "length")
                        || crate::callables::callable_method(property).is_some());
                }
                Value::Host(id) if Some(id) == host.global_object() => {
                    if self.global_deleted(property) {
                        return Ok(false);
                    }
                    if self.global_own_property(property).is_some()
                        || intrinsic_global(property).is_some()
                        || host.global(property).is_some()
                    {
                        return Ok(true);
                    }
                    return host.has_property(id, property);
                }
                Value::Host(id) => {
                    if host.is_callable(id) && crate::callables::callable_method(property).is_some()
                    {
                        return Ok(true);
                    }
                    return host.has_property(id, property);
                }
                _ => return Err(self.property_type_error("Right operand of in must be an object")),
            }
        }
        Err(JsError::resource("Prototype existence depth exceeded"))
    }
    pub(super) fn delete_expression(
        &mut self,
        expression: &Expr,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let result = match expression {
            Expr::Name(name) => {
                if let Some(scope) = self.lookup_binding(scope, name) {
                    if scope == 0
                        && self.scopes[0].bindings[name].global_property
                        && let Some(id) = host.global_object()
                    {
                        self.delete_property(Value::Host(id), name, host)?
                    } else {
                        false
                    }
                } else if let Some(id) = host.global_object() {
                    self.delete_property(Value::Host(id), name, host)?
                } else {
                    true
                }
            }
            Expr::Member(object, key) => {
                let object = self.eval(object, scope, host)?;
                let key = self.eval(key, scope, host)?;
                let place = self.ordinary_member_place_values(object, key, host)?;
                return self.delete_ordinary_place(place, host);
            }
            Expr::PrivateMember(..) => {
                return Err(JsError::syntax(0, "Cannot delete private class fields"));
            }
            _ => {
                self.eval(expression, scope, host)?;
                true
            }
        };
        Ok(Value::Bool(result))
    }
    // Operator semantics only: Reflect.deleteProperty retains the low-level
    // boolean result even when its caller is strict.
    pub(super) fn delete_ordinary_place(
        &mut self,
        place: Place,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let deleted = match place {
            Place::Member(object, property) => self.delete_property(object, &property, host)?,
            Place::SymbolMember(object, key) => match object {
                Value::Number(_)
                | Value::Bool(_)
                | Value::BigInt(_)
                | Value::Symbol(..)
                | Value::String(_)
                | Value::Utf16(_) => true,
                _ => self.delete_symbol(object, key, host)?,
            },
            _ => return Err(JsError::runtime("Expected ordinary delete reference")),
        };
        if !deleted && self.strict {
            return Err(self.property_type_error("Cannot delete non-configurable property"));
        }
        Ok(Value::Bool(deleted))
    }
    pub(super) fn delete_property(
        &mut self,
        object: Value,
        property: &str,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        self.tick()?;
        if let Value::Native(name @ ("Symbol" | "Number")) = object {
            let storage = self.intrinsic_constructor_storage(name)?;
            return self.delete_property(storage, property, host);
        }
        if let Some(result) = self.stdlib_delete(&object, property)? {
            return Ok(result);
        }
        if property.len() > 1024 {
            return Err(JsError::resource("Property name exceeds limit"));
        }
        if let Value::Object(id) | Value::Array(id) | Value::Function(id) = &object
            && !self.configurable_property(*id, property)
        {
            return Ok(false);
        }
        if let Value::Function(id) = &object
            && property == "length"
            && !self.synthetic_deleted(*id, property)
        {
            self.charge_string(property.len())?;
        }
        match object {
            Value::Object(id) => {
                let Some(Heap::Object(object)) = self.heap.get_mut(id) else {
                    return Err(JsError::runtime("Invalid object reference"));
                };
                if object.regexp.is_some() && object.non_enumerable.contains(property) {
                    if property == "lastIndex" {
                        return Ok(false);
                    }
                    return Err(JsError::runtime(
                        "Deletion of synthetic RegExp builtins is not implemented",
                    ));
                }
                object.properties.remove(property);
                object.non_enumerable.remove(property);
                self.forget_descriptor(id, property);
                Ok(true)
            }
            Value::Array(id) => {
                if property == "length" {
                    return Ok(false);
                }
                let Some(Heap::Array(values)) = self.heap.get_mut(id) else {
                    return Err(JsError::runtime("Invalid array reference"));
                };
                if let Some(index) = array_index(property) {
                    if let Some(value) = values.get_mut(index) {
                        *value = Value::Undefined;
                        self.array_holes.entry(id).or_default().insert(index);
                    }
                } else if let Some(properties) = self.array_properties.get_mut(&id) {
                    properties.remove(property);
                    if let Some(hidden) = self.array_non_enumerable.get_mut(&id) {
                        hidden.remove(property);
                    }
                }
                self.forget_descriptor(id, property);
                Ok(true)
            }
            Value::Function(id) => {
                let Some(Heap::Function(function)) = self.heap.get_mut(id) else {
                    return Err(JsError::runtime("Invalid function reference"));
                };
                if property == "prototype" && function.has_prototype() {
                    return Ok(false);
                }
                function.properties.remove(property);
                if property == "length" && !self.synthetic_deleted(id, property) {
                    self.deleted_synthetics
                        .entry(id)
                        .or_default()
                        .insert(property.to_owned());
                }
                self.forget_descriptor(id, property);
                Ok(true)
            }
            Value::Host(id) if Some(id) == host.global_object() => {
                self.delete_global_property(property, host)
            }
            Value::Host(_) => Err(JsError::runtime(
                "Deletion of host properties is not implemented",
            )),
            Value::Native(_) => Err(self.property_type_error(
                "Deletion of this native function's properties is not implemented",
            )),
            Value::Null | Value::Undefined => {
                Err(self.property_type_error("Cannot delete property of null or undefined"))
            }
            Value::String(text) => {
                if property == "length" {
                    return Ok(false);
                }
                if let Some(index) = array_index(property) {
                    self.remaining =
                        self.remaining
                            .checked_sub(text.len() as u64)
                            .ok_or_else(|| {
                                JsError::resource("String property inspection work budget exceeded")
                            })?;
                    return Ok(index >= text.encode_utf16().count());
                }
                Ok(true)
            }
            Value::Utf16(units) => Ok(property != "length"
                && !array_index(property).is_some_and(|index| index < units.len())),
            _ => Ok(true),
        }
    }
    pub(super) fn instance_of(
        &mut self,
        value: Value,
        constructor: Value,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        let mut constructor = constructor;
        let mut depth = 0;
        while let Value::Function(id) = &constructor
            && let Some(bound) = self.bound_functions.get(id)
        {
            if depth >= 32 {
                return Err(JsError::resource("Bound instanceof depth exceeded"));
            }
            constructor = bound.target.clone();
            depth += 1;
            self.tick()?;
        }
        if !matches!(
            constructor,
            Value::Function(_)
                | Value::Native(
                    "Array" | "Function" | "Object" | "String" | "Number" | "Boolean" | "BigInt"
                )
        ) && !matches!(constructor,Value::Native(name) if crate::stdlib::global(name).is_some() || crate::wasm::constructor(name) || crate::reflection::error_constructor(name).is_some() || matches!(name,"Promise"|"WeakRef"|"FinalizationRegistry"|"WeakMap"|"WeakSet"))
        {
            return Err(self.property_type_error("instanceof requires a supported function constructor; other native/host constructors are not implemented"));
        }
        if !matches!(
            value,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Host(_)
                | Value::Native(_)
        ) {
            return Ok(false);
        }
        let prototype = self.get(constructor, "prototype", host)?;
        if !matches!(
            prototype,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Host(_)
                | Value::Native("Function.prototype")
        ) {
            return Err(
                self.property_type_error("instanceof constructor prototype must be an object")
            );
        }
        let mut current = value;
        for _ in 0..64 {
            self.tick()?;
            match current {
                Value::Object(id) => {
                    let Some(Heap::Object(object)) = self.heap.get(id) else {
                        return Err(JsError::runtime("Invalid object reference"));
                    };
                    let Some(parent) = object.prototype.clone() else {
                        return Ok(false);
                    };
                    if strict_equal(&parent, &prototype) {
                        return Ok(true);
                    }
                    current = parent;
                }
                Value::Host(_) => {
                    return Err(JsError::runtime(
                        "Host prototype traversal is not implemented",
                    ));
                }
                Value::Array(id) => {
                    let parent = if Some(id) == self.array_prototype {
                        self.object_prototype_value()?
                    } else {
                        self.array_prototype_value()?
                    };
                    if strict_equal(&parent, &prototype) {
                        return Ok(true);
                    }
                    current = parent;
                }
                Value::Function(_) => {
                    return Ok(strict_equal(
                        &Value::Native("Function.prototype"),
                        &prototype,
                    ));
                }
                Value::Native(name) => {
                    if matches!(name, "Symbol" | "Number" | "Function.prototype") {
                        let parent = if matches!(name, "Symbol" | "Number") {
                            Value::Native("Function.prototype")
                        } else {
                            self.object_prototype_value()?
                        };
                        if strict_equal(&parent, &prototype) {
                            return Ok(true);
                        }
                        current = parent;
                        continue;
                    }
                    return Ok(name != "Function.prototype"
                        && strict_equal(&Value::Native("Function.prototype"), &prototype));
                }
                _ => return Ok(false),
            }
        }
        Err(JsError::resource("instanceof prototype depth exceeded"))
    }
    /// Lexical ordering is the documented subset, with a bounded key snapshot.
    pub(super) fn enumerable_keys(&mut self, value: &Value) -> Result<Vec<String>, JsError> {
        if matches!(
            value,
            Value::Null | Value::Undefined | Value::Bool(_) | Value::Number(_)
        ) {
            return Ok(Vec::new());
        }
        let string_length = match value {
            Value::String(text) => Some(text.encode_utf16().count()),
            Value::Utf16(units) => Some(units.len()),
            _ => None,
        };
        if let Some(length) = string_length {
            if length > 4096 {
                return Err(JsError::resource("Enumeration key count exceeds 4096"));
            }
            let mut keys = Vec::new();
            for index in 0..length {
                self.tick()?;
                let key = index.to_string();
                self.charge_string(key.len())?;
                keys.push(key);
            }
            return Ok(keys);
        }
        let mut current = value.clone();
        let mut keys = Vec::new();
        let mut seen = BTreeSet::new();
        let mut bytes = 0usize;
        for _ in 0..64 {
            self.tick()?;
            let mut own = Vec::new();
            if let Value::Native(name @ ("Symbol" | "Number")) = current {
                current = self.intrinsic_constructor_storage(name)?;
            }
            let next = match current {
                Value::Native("Function.prototype") => {
                    own.extend(
                        crate::native_symbol::FUNCTION_PROTOTYPE_NAMES
                            .iter()
                            .map(|key| ((*key).into(), false)),
                    );
                    Some(self.object_prototype_value()?)
                }
                Value::Object(id) => {
                    let Some(Heap::Object(object)) = self.heap.get(id) else {
                        return Err(JsError::runtime("Invalid object reference"));
                    };
                    own.extend(
                        object
                            .properties
                            .keys()
                            .map(|key| (key.clone(), self.own_enumerable(id, key))),
                    );
                    object.prototype.clone()
                }
                Value::Function(id) => {
                    let Some(Heap::Function(function)) = self.heap.get(id) else {
                        return Err(JsError::runtime("Invalid function reference"));
                    };
                    own.extend(
                        function
                            .properties
                            .keys()
                            .map(|key| (key.clone(), self.own_enumerable(id, key))),
                    );
                    if !self.synthetic_deleted(id, "length")
                        && !function.properties.contains_key("length")
                    {
                        own.push(("length".into(), false));
                    }
                    if function.has_prototype() && !function.properties.contains_key("prototype") {
                        own.push(("prototype".into(), false));
                    }
                    self.class_static_parent(id)
                }
                Value::Array(id) => {
                    let Some(Heap::Array(values)) = self.heap.get(id) else {
                        return Err(JsError::runtime("Invalid array reference"));
                    };
                    own.extend(
                        (0..values.len())
                            .filter(|index| self.array_has_index(id, *index, values.len()))
                            .map(|index| {
                                (
                                    index.to_string(),
                                    self.own_enumerable(id, &index.to_string()),
                                )
                            }),
                    );
                    if let Some(properties) = self.array_properties.get(&id) {
                        own.extend(
                            properties
                                .keys()
                                .map(|key| (key.clone(), self.own_enumerable(id, key))),
                        );
                    }
                    if Some(id) == self.array_prototype {
                        Some(self.object_prototype_value()?)
                    } else {
                        Some(self.array_prototype_value()?)
                    }
                }
                Value::Host(_) | Value::Native(_) => {
                    return Err(JsError::runtime(
                        "Host/native property enumeration is not implemented",
                    ));
                }
                _ => None,
            };
            if let Some(id) = match current {
                Value::Object(id) | Value::Array(id) | Value::Function(id) => Some(id),
                _ => None,
            } {
                if let Some(extra) = self.stdlib_own_keys(&current) {
                    own.extend(extra.into_iter().map(|key| (key, true)));
                }
                let mut names = own.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>();
                self.sort_property_keys(id, &mut names);
                let ranks = names
                    .iter()
                    .enumerate()
                    .map(|(index, key)| (key.as_str(), index))
                    .collect::<std::collections::BTreeMap<_, _>>();
                own.sort_by_key(|(key, _)| ranks.get(key.as_str()).copied().unwrap_or(usize::MAX));
            }
            for (key, enumerable) in own {
                self.tick()?;
                bytes = bytes.saturating_add(key.len());
                if bytes > 65_536 || seen.len() >= 4096 {
                    return Err(JsError::resource("Enumeration key budget exceeded"));
                }
                self.charge_string(key.len())?;
                if seen.insert(key.clone()) && enumerable {
                    keys.push(key);
                }
            }
            match next {
                Some(parent) => current = parent,
                None => return Ok(keys),
            }
        }
        Err(JsError::resource("Enumeration prototype depth exceeded"))
    }
}

#[cfg(test)]
mod tests {
    use crate::{ErrorKind, Host, JsError, Limits, Runtime, Value};
    struct Empty;
    impl Host for Empty {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            panic!("Property operators must not probe host getters")
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            panic!("No privileged writes")
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            panic!("No host calls")
        }
    }
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty)
    }
    #[test]
    fn existence_distinguishes_undefined_and_inherited_properties() {
        for source in [
            "const p={inherited:undefined};const o=Object.create(p);o.own=undefined;'inherited' in o && 'own' in o && !('missing' in o) && !Object.hasOwn(o,'inherited') && Object.hasOwn(o,'own');",
            "const a=[undefined,2];a.extra=undefined;'length' in a && 0 in a && 'extra' in a && 'push' in a && !('01' in a) && !('3' in a);",
            "function F(){}; 'prototype' in F && 'length' in F && 'call' in F && Object.hasOwn(F,'prototype');",
            "try{'x' in undefined;}catch(e){e.name==='TypeError';}",
            "try{'x' in 'xyz';}catch(e){e.name==='TypeError';}",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn delete_preserves_references_bindings_and_real_array_holes() {
        for source in [
            "let n=0;const o={x:undefined};function base(){n++;return o;}function key(){n++;return 'x';}delete base()[key()] && n===2 && !('x' in o);",
            "let n=0;function effect(){n++;return 42;}delete effect() && n===1 && !(delete n) && delete missing;",
            "const p={x:1};const o=Object.create(p);delete o.x && 'x' in o && p.x===1;",
            "const a=[undefined,2,3];delete a[1] && a.length===3 && !(1 in a) && 0 in a && !Object.hasOwn(a,'1') && Object.keys(a).join(',')==='0,2' && !(delete a.length);",
            "const a=Array(3);!(0 in a) && a.length===3 && Object.keys(a).length===0 && a.join(',')===',,';",
            "const a=[];a[3]=undefined;!(0 in a) && 3 in a && a.length===4;",
            "const a=[1,2];delete a[1];a.pop();a.push(7);1 in a && a[1]===7 && a.length===2;",
            "const a=[1,2,3];delete a[1];let seen='';const b=a.map((v,i)=>{seen+=i;return v*2;});seen==='02' && !(1 in b) && b.length===3 && b[2]===6;",
            "const a=[1,2,3];let seen='';a.forEach((v,i)=>{seen+=i;if(i===0)delete a[1];});seen==='02';",
            "const a=[1,2];delete a[1];a[1]=undefined;1 in a && Object.hasOwn(a,'1');",
            "function F(){};F.custom=1;delete F.custom && !('custom' in F) && !(delete F.prototype);",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn ordinary_delete_orders_key_expression_before_nullish_and_key_coercion() {
        for source in [
            "let order='';const key={toString(){order+='c';return 'x';}};function base(){order+='b';return null;}function keyExpr(){order+='k';return key;}try{delete base()[keyExpr()];}catch(e){e.name==='TypeError' && order==='bk';}",
            "let order='';function key(){order+='k';throw 7;}try{delete undefined[key()];}catch(e){e===7 && order==='k';}",
            "let order='';const o={x:1};function base(){order+='b';return o;}function key(){order+='k';return {toString(){order+='c';return 'x';}};}delete base()[key()] && order==='bkc' && !Object.hasOwn(o,'x');",
            "let n=0;const o={x:1};const key={toString(){n++;throw 9;}};try{delete o[key];}catch(e){e===9 && n===1 && o.x===1;}",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn ordinary_delete_does_not_get_values_and_strict_failure_is_operator_only() {
        for source in [
            "let gets=0;const o={};Object.defineProperty(o,'x',{get(){gets++;throw 9;},configurable:true});delete o.x && delete o.absent && gets===0 && !Object.hasOwn(o,'x');",
            "let gets=0;const o={};Object.defineProperty(o,'x',{get(){gets++;throw 9;}});!(delete o.x) && gets===0 && Object.hasOwn(o,'x');",
            "'use strict';let gets=0;const o={};Object.defineProperty(o,'x',{get(){gets++;throw 9;}});try{delete o.x;}catch(e){e.name==='TypeError' && gets===0 && Reflect.deleteProperty(o,'x')===false;}",
            "'use strict';const a=[1];try{delete a.length;}catch(e){e.name==='TypeError' && Reflect.deleteProperty(a,'length')===false && delete a[0] && !(0 in a);}",
            "const s=Symbol('x');const o={};o[s]=1;delete o[s] && delete o[s] && o[s]===undefined;",
            "const s=Symbol('x');const o={};Object.defineProperty(o,s,{get(){throw 9;}});!(delete o[s]) && Reflect.deleteProperty(o,s)===false;",
            "'use strict';const s=Symbol('x');const o={};Object.defineProperty(o,s,{value:1});try{delete o[s];}catch(e){e.name==='TypeError' && Reflect.deleteProperty(o,s)===false;}",
            "const s=Symbol('x');delete (1)[s] && delete true[s] && delete (1n)[s] && delete Symbol('v')[s] && delete 'abc'[s];",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn primitive_string_delete_uses_virtual_utf16_properties_without_boxing() {
        // Construct code units without extending the lexer's surrogate-escape support.
        for source in [
            "const s=String.fromCharCode(0xd83d,0xde00);!(delete s[0]) && !(delete s[1]) && delete s[2] && !(delete s.length);",
            "const s=String.fromCharCode(0xd800,0x61,0xdfff);!(delete s[0]) && !(delete s[2]) && delete s[3] && !(delete s.length) && delete s['01'] && delete s['-0'] && delete s['1e0'] && delete s['1.0'];",
            "'use strict';const s=String.fromCharCode(0xd800);try{delete s[0];}catch(e){e.name==='TypeError';}",
            "'use strict';try{delete 'abc'.length;}catch(e){e.name==='TypeError';}",
            "const s='x'.repeat(256);!(delete s[255]) && delete s[256];",
            "const s=Object('abc');!(delete s[0]) && !(delete s.length);",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        let mut runtime = Runtime::default();
        for value in [
            Value::string("\u{1f600}"),
            Value::Utf16(vec![0xd800, 0xdfff].into()),
        ] {
            for (key, expected) in [
                ("0", false),
                ("1", false),
                ("2", true),
                ("length", false),
                ("01", true),
                ("-0", true),
                ("1e0", true),
                ("1.0", true),
            ] {
                assert_eq!(
                    runtime
                        .delete_property(value.clone(), key, &mut Empty)
                        .unwrap(),
                    expected,
                    "{key}"
                );
            }
        }
    }
    #[test]
    fn ordinary_delete_respects_work_budget_without_allocating_primitive_wrappers() {
        let mut runtime = Runtime {
            remaining: 4,
            ..Runtime::default()
        };
        assert_eq!(
            runtime
                .delete_property(Value::string("\u{1f600}"), "0", &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        runtime.remaining = 1;
        assert!(
            !runtime
                .delete_property(Value::Utf16(vec![0xd800; 256].into()), "255", &mut Empty)
                .unwrap()
        );
        assert_eq!(runtime.remaining, 0);
        let heap_before = runtime.heap.len();
        let bytes_before = runtime.string_bytes;
        for value in [
            Value::Number(1.),
            Value::Bool(true),
            Value::BigInt(std::sync::Arc::new(crate::BigInt::parse("1").unwrap())),
            Value::Symbol(12, "value".into()),
            Value::string("x".repeat(256)),
            Value::Utf16(vec![0xd800; 256].into()),
        ] {
            assert_eq!(
                runtime
                    .delete_ordinary_place(crate::Place::SymbolMember(value, 13), &mut Empty)
                    .unwrap(),
                Value::Bool(true)
            );
        }
        assert_eq!(runtime.heap.len(), heap_before);
        assert_eq!(runtime.string_bytes, bytes_before);
    }
    #[test]
    fn implemented_array_methods_are_reachable_through_the_prototype() {
        for method in [
            "push",
            "pop",
            "join",
            "forEach",
            "map",
            "slice",
            "concat",
            "filter",
            "reduce",
            "reduceRight",
            "find",
            "findIndex",
            "findLast",
            "findLastIndex",
            "every",
            "some",
            "indexOf",
            "lastIndexOf",
            "includes",
            "at",
            "reverse",
            "toReversed",
            "shift",
            "unshift",
            "splice",
            "toSpliced",
            "sort",
            "toSorted",
            "fill",
            "copyWithin",
            "flat",
            "flatMap",
            "toString",
        ] {
            let source = format!(
                "const a=[];typeof a.{method}==='function'&&a.{method}===Array.prototype.{method}&&!Object.hasOwn(a,'{method}')&&!Array.prototype.propertyIsEnumerable('{method}');"
            );
            assert_eq!(run(&source).unwrap(), Value::Bool(true), "{method}");
        }
        for source in [
            "const includes=Array.prototype.includes;includes.call([1,2],2)&&!includes.call([1],2);",
            "const a=[2,1];Array.prototype.sort.call(a);a.join()==='1,2';",
            "const a=[];Array.prototype.includes=function(){return 7};a.includes()===7;",
            "delete Array.prototype.sort;typeof [].sort==='undefined';",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn instanceof_walks_real_own_and_shared_array_prototypes() {
        for source in [
            "function F(){};function G(){};const o=new F();o instanceof F && !(o instanceof G) && !(1 instanceof F);",
            "function F(){};const p=F.prototype;const o=Object.create(Object.create(p));o instanceof F;",
            "function F(){};const old=new F();F.prototype={};!(old instanceof F) && new F() instanceof F;",
            "[] instanceof Array && new Array(2) instanceof Array && Object.getPrototypeOf([])===Array.prototype && Array.isArray(Array.prototype) && !(Array.prototype instanceof Array);",
            "Object.create(Array.prototype) instanceof Array && !(Object.create(null) instanceof Array);",
            "Array.prototype.shared=undefined;const a=[]; 'shared' in a && !Object.hasOwn(a,'shared') && Object.keys(a).length===0 && a.constructor===Array;",
            "Array.prototype[1]=9;const a=Array(2);1 in a && !Object.hasOwn(a,'1') && a[1]===9 && a.join(',')===',9';",
            "const a=[1];const push=Array.prototype.push;push.call(a,2);a.join(',')==='1,2';",
            "const a=[];Array.prototype.push=function(v){this.seen=v;return 9;};a.push(7)===9 && a.seen===7;",
            "const a=[];delete Array.prototype.push;!('push' in a) && typeof a.push==='undefined';",
            "function F(){};F.prototype=Array.prototype;new F() instanceof F && new F() instanceof Array;",
            "try{({}) instanceof 4;}catch(e){e.name==='TypeError';}",
            "try{({}) instanceof (()=>{});}catch(e){e.name==='TypeError';}",
            "function F(){};F.prototype=4;try{({}) instanceof F;}catch(e){e.name==='TypeError';}",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        assert_eq!(run("({}) instanceof Object;").unwrap(), Value::Bool(true));
    }
    #[test]
    fn for_in_scopes_prototypes_control_flow_and_deletions_are_bounded() {
        for source in [
            "const p={a:1,z:2};const o=Object.create(p);o.a=3;o.b=4;let keys='';for(const k in o){keys+=k;}keys==='abz';",
            "let f=[];for(let k in {a:1,b:2})f.push(()=>k);f[0]()==='a' && f[1]()==='b' && typeof k==='undefined';",
            "let s='';for(var k in {a:1,b:2})s+=k;k==='b' && s==='ab';",
            "let n=0;const result={};function key(){n++;return 'current';}for(result[key()] in {a:1,b:2}){}n===2 && result.current==='b';",
            "let s='';const o={a:1,b:2,c:3};for(let k in o){s+=k;if(k==='a')delete o.b;}s==='ac';",
            "let s='';outer:for(let k in {a:1,b:2,c:3}){if(k==='b')continue outer;s+=k;if(k==='c')break;}s==='ac';",
            "let n=0;for(const key in null)n++;for(const key in undefined)n++;for(let key in 42)n++;n===0;",
            "let s='';for(const key in 'abc')s+=key;s==='012';",
            "let s='';for(const key in String.fromCharCode(0xd800,0x61,0xdfff))s+=key;s==='012';",
            "class A{};A.base=1;class B extends A{};B.child=2;let s='';for(const key in B)s+=key;s==='childbase';",
            "class A{};A.hidden=1;A.visible=2;class B extends A{};Object.defineProperty(B,'hidden',{value:3,enumerable:false});let s='';for(const key in B)s+=key;s==='visible';",
            "let s='';const a=[1,2];delete a[0];a.extra=true;for(const k in a)s+=k;s==='1extra';",
            "let s='';for(const k in /x/g)s+=k;for(const k in JSON)s+=k;function F(){};for(const k in F.prototype)s+=k;s==='';",
            "let x={a:1};try{for(let x in x){}}catch(e){e.message.includes('before initialization');}",
            "let count=0;for(let i=0;i<2;i++){if('x' in {x:1})count++;}count===2;",
            "for(let k in {a:1}){function nested(){var k;return true;}nested();}true;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        let mut runtime = Runtime::default();
        runtime.execute("let effects=0;", &mut Empty).unwrap();
        for source in [
            "effects++;for(1+2 in {}){}",
            "effects++;for(const a=1 in {}){}",
            "effects++;for(var a,b in {}){}",
            "effects++;for(let k in {}){var k;}",
            "effects++;for(const k in {}){if(false){var k;}}",
            "effects++;for(let k in {}){for(var k in {}){}}",
            "effects++;for(let k in {}){try{}catch(e){var k;}}",
            "effects++;for(let k in {}){switch(0){case 0:var k;}}",
        ] {
            assert_eq!(
                runtime.execute(source, &mut Empty).unwrap_err().kind,
                ErrorKind::Syntax
            );
            assert_eq!(runtime.global_value("effects"), Some(Value::Number(0.)));
        }
    }
    #[test]
    fn resource_limits_stop_enumeration_prototype_walk_and_array_storage() {
        let mut runtime = Runtime::new(Limits {
            instructions: 1_000_000,
            ..Limits::default()
        });
        assert_eq!(
            runtime
                .execute(
                    "let a=[];for(let i=0;i<17;i++)a.push(Array(4096));",
                    &mut Empty
                )
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            run("let o={};for(let i=0;i<65;i++)o=Object.create(o);'missing' in o;")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            run("let o={};for(let i=0;i<65;i++)o=Object.create(o);for(let k in o){};")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let mut runtime = Runtime::new(Limits {
            instructions: 50,
            ..Limits::default()
        });
        assert_eq!(runtime.execute("try{for(let key in 'abcdefghijklmnopqrstuvwxyz'){while(true){}}}catch(e){true;}",&mut Empty).unwrap_err().kind,ErrorKind::Resource);
    }
    #[test]
    fn host_existence_uses_metadata_without_getters_or_privileged_operations() {
        struct Metadata;
        impl Host for Metadata {
            fn global(&mut self, name: &str) -> Option<Value> {
                match name {
                    "window" | "this" => Some(Value::Host(2)),
                    "document" => Some(Value::Host(1)),
                    _ => None,
                }
            }
            fn global_object(&self) -> Option<u64> {
                Some(2)
            }
            fn has_property(&mut self, object: u64, property: &str) -> Result<bool, JsError> {
                Ok((object == 1 && property == "empty") || (object == 2 && property == "known"))
            }
            fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
                panic!("Existence cannot probe a getter")
            }
            fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
                panic!("Cannot write host")
            }
            fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
                panic!("Cannot call host")
            }
        }
        assert_eq!(Runtime::default().execute("window.own=undefined; 'own' in window && 'known' in window && !('missing' in window) && 'empty' in document && !('missing' in document) && delete window.own && !('own' in window);",&mut Metadata).unwrap(),Value::Bool(true));
        assert!(
            Runtime::default()
                .execute("delete window.known;", &mut Metadata)
                .unwrap_err()
                .message
                .contains("host properties")
        );
    }
    #[test]
    fn persistent_new_property_keys_consume_string_budget_without_recharging_replacements() {
        for kind in 0..3 {
            let mut runtime = Runtime::new(Limits {
                string_bytes: 32,
                ..Limits::default()
            });
            let create = |runtime: &mut Runtime| match kind {
                0 => runtime.object(Default::default(), None),
                1 => runtime
                    .allocate(crate::Heap::Array(Vec::new()))
                    .map(Value::Array),
                _ => runtime.make_function(
                    std::sync::Arc::new(Vec::new()),
                    std::sync::Arc::new(Vec::new()),
                    0,
                    false,
                ),
            };
            let first = create(&mut runtime).unwrap();
            let second = create(&mut runtime).unwrap();
            let key = "abcdefghijklmnopqrstuvwx";
            runtime
                .set(first.clone(), key, Value::Number(1.), &mut Empty)
                .unwrap();
            runtime
                .set(first, key, Value::Number(2.), &mut Empty)
                .unwrap();
            assert_eq!(
                runtime
                    .set(second, key, Value::Number(3.), &mut Empty)
                    .unwrap_err()
                    .kind,
                ErrorKind::Resource
            );
        }
    }
}
