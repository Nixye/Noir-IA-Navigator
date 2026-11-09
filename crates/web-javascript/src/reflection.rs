//! Object prototypes, native-function metadata and primitive wrappers.
use crate::{Heap, Host, JsError, Runtime, Value, descriptors::PropertyMetadata};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Default)]
pub(super) struct ReflectionState {
    pub symbol_constructor: Option<usize>,
    pub prototypes: BTreeMap<&'static str, Value>,
    pub boxed: BTreeMap<usize, Value>,
    pub non_extensible: BTreeSet<usize>,
}
pub(super) fn error_constructor(name: &str) -> Option<&'static str> {
    Some(match name {
        "Error" => "Error",
        "TypeError" => "TypeError",
        "RangeError" => "RangeError",
        "ReferenceError" => "ReferenceError",
        "SyntaxError" => "SyntaxError",
        _ => return None,
    })
}
pub(super) fn property(name: &str, property: &str) -> Option<Value> {
    Some(Value::Native(match (name, property) {
        ("Object", "assign") => "Object.assign",
        ("Object", "is") => "Object.is",
        ("Object", "setPrototypeOf") => "Object.setPrototypeOf",
        ("Object", "preventExtensions") => "Object.preventExtensions",
        ("Object", "isExtensible") => "Object.isExtensible",
        ("Object", "seal") => "Object.seal",
        ("Object", "freeze") => "Object.freeze",
        ("Object", "isSealed") => "Object.isSealed",
        ("Object", "isFrozen") => "Object.isFrozen",
        ("Object", "getOwnPropertySymbols") => "Object.getOwnPropertySymbols",
        _ => return None,
    }))
}
impl Runtime {
    pub(super) fn object_prototype_value(&mut self) -> Result<Value, JsError> {
        self.reflection_prototype("Object")
    }
    pub(super) fn ordinary_object(
        &mut self,
        properties: BTreeMap<String, Value>,
    ) -> Result<Value, JsError> {
        let prototype = self.object_prototype_value()?;
        self.object(properties, Some(prototype))
    }
    pub(super) fn reflection_prototype(&mut self, kind: &'static str) -> Result<Value, JsError> {
        if let Some(value) = self.reflection.prototypes.get(kind) {
            return Ok(value.clone());
        }
        let parent = if kind == "Object" {
            None
        } else if error_constructor(kind).is_some() && kind != "Error" {
            Some(self.reflection_prototype("Error")?)
        } else {
            Some(self.object_prototype_value()?)
        };
        let mut methods = if kind == "Object" {
            vec![
                ("constructor", Value::Native("Object")),
                ("hasOwnProperty", Value::Native("Object.hasOwnProperty")),
                (
                    "propertyIsEnumerable",
                    Value::Native("Object.propertyIsEnumerable"),
                ),
                ("isPrototypeOf", Value::Native("Object.isPrototypeOf")),
                ("toString", Value::Native("Object.toString")),
                ("toLocaleString", Value::Native("Object.toLocaleString")),
                ("valueOf", Value::Native("Object.valueOf")),
            ]
        } else if error_constructor(kind).is_some() {
            vec![
                ("constructor", Value::Native(kind)),
                ("name", self.string(kind.into())?),
                ("message", self.string(String::new())?),
            ]
        } else {
            vec![
                ("constructor", Value::Native(kind)),
                (
                    "valueOf",
                    Value::Native(match kind {
                        "String" => "String.valueOf",
                        "Number" => "Number.valueOf",
                        "BigInt" => "BigInt.valueOf",
                        _ => "Boolean.valueOf",
                    }),
                ),
                (
                    "toString",
                    Value::Native(match kind {
                        "String" => "String.toString",
                        "Number" => "Number.toString",
                        "BigInt" => "BigInt.toString",
                        _ => "Boolean.toString",
                    }),
                ),
            ]
        };
        if kind == "String" {
            for name in [
                "at",
                "charAt",
                "charCodeAt",
                "codePointAt",
                "slice",
                "substring",
                "substr",
                "includes",
                "indexOf",
                "lastIndexOf",
                "startsWith",
                "endsWith",
                "concat",
                "repeat",
                "padStart",
                "padEnd",
                "trim",
                "trimStart",
                "trimEnd",
                "trimLeft",
                "trimRight",
                "toLowerCase",
                "toUpperCase",
                "normalize",
                "isWellFormed",
                "toWellFormed",
                "replace",
                "replaceAll",
                "split",
                "match",
                "search",
            ] {
                if let Some(method) = crate::strings::method(name) {
                    methods.push((name, method));
                }
            }
            methods.push(("length", Value::Number(0.)));
        }
        if kind == "BigInt" {
            methods.push(("toLocaleString", Value::Native("BigInt.toLocaleString")));
        }
        if kind == "Number" {
            methods.push(("toFixed", Value::Native("Number.toFixed")));
        }
        let order = methods
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect::<Vec<_>>();
        let prototype = self.object(
            methods
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
            parent,
        )?;
        self.reflection.prototypes.insert(kind, prototype.clone());
        let Value::Object(id) = prototype else {
            unreachable!()
        };
        self.remember_property_order(id, order.clone());
        for name in order {
            self.property_metadata.entry(id).or_default().insert(
                name,
                PropertyMetadata {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        if kind == "String" {
            self.property_metadata.entry(id).or_default().insert(
                "length".into(),
                PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: None,
                },
            );
        }
        if kind == "BigInt" {
            let Value::Symbol(symbol, _) = self.well_known_symbol("toStringTag")? else {
                unreachable!()
            };
            let tag = self.string("BigInt".into())?;
            self.symbol_properties
                .entry(id)
                .or_default()
                .insert(symbol, tag);
            self.symbol_order.entry(id).or_default().push(symbol);
            self.symbol_metadata.entry(id).or_default().insert(
                symbol,
                PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        // ECMA-262 21.2.3: BigInt.prototype has no [[BigIntData]] slot.
        if matches!(kind, "String" | "Number" | "Boolean") {
            self.reflection.boxed.insert(
                id,
                match kind {
                    "String" => Value::string(""),
                    "Number" => Value::Number(0.),
                    _ => Value::Bool(false),
                },
            );
        }
        Ok(prototype)
    }
    pub(super) fn box_value(&mut self, value: Value) -> Result<Value, JsError> {
        let kind = match &value {
            Value::String(_) | Value::Utf16(_) => "String",
            Value::Number(_) => "Number",
            Value::Bool(_) => "Boolean",
            Value::BigInt(_) => "BigInt",
            Value::Symbol(..) => {
                return Err(self.property_type_error("Boxed Symbol objects are not implemented"));
            }
            Value::Null | Value::Undefined => return self.ordinary_object(BTreeMap::new()),
            _ => return Ok(value),
        };
        let prototype = self.reflection_prototype(kind)?;
        let mut properties = BTreeMap::new();
        if let Some(units) = crate::strings::units(&value) {
            if units.len() > 127 {
                return Err(JsError::resource(
                    "String wrapper exceeds 127 UTF-16 indices",
                ));
            }
            properties.insert("length".into(), Value::Number(units.len() as f64));
            for (index, unit) in units.into_iter().enumerate() {
                properties.insert(index.to_string(), self.utf16_string(vec![unit])?);
            }
        }
        let keys = properties.keys().cloned().collect::<Vec<_>>();
        let object = self.object(properties, Some(prototype))?;
        if let Value::Object(id) = object {
            for key in keys {
                let enumerable = key != "length";
                self.property_metadata.entry(id).or_default().insert(
                    key,
                    PropertyMetadata {
                        writable: false,
                        enumerable,
                        configurable: false,
                        accessor: None,
                    },
                );
            }
        }
        if let Value::Object(id) = object {
            self.reflection.boxed.insert(id, value);
        }
        Ok(object)
    }
    pub(super) fn reflection_get(
        &mut self,
        object: &Value,
        property: &str,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        if let Value::Native(name) = object {
            if let Some(value) = property_value(name, property) {
                return Ok(Some(value));
            }
            if property == "prototype"
                && (matches!(*name, "Object" | "String" | "Number" | "Boolean" | "BigInt")
                    || error_constructor(name).is_some())
            {
                return self.reflection_prototype(name).map(Some);
            }
        }
        let kind = match object {
            Value::Number(_) => Some("Number"),
            Value::BigInt(_) => Some("BigInt"),
            Value::Bool(_) => Some("Boolean"),
            _ => None,
        };
        if let Some(kind) = kind {
            let prototype = self.reflection_prototype(kind)?;
            return self
                .get_with_receiver(prototype, property, object.clone(), host)
                .map(Some);
        }
        Ok(None)
    }
    pub(super) fn reflection_call(
        &mut self,
        name: &str,
        args: &[Value],
        this: Value,
        construct: bool,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        if construct
            && (name.starts_with("Object.")
                || name.starts_with("String.")
                || name.starts_with("Number.")
                || name.starts_with("Boolean.")
                || name.starts_with("BigInt."))
        {
            return Err(self.property_type_error("Method is not a constructor"));
        }
        if let Some(method) = name
            .strip_prefix("String.")
            .and_then(crate::strings::method)
            && let Value::Object(id) = &this
            && let Some(primitive) = self.reflection.boxed.get(id).cloned()
        {
            if !matches!(primitive, Value::String(_) | Value::Utf16(_)) {
                return Err(self.property_type_error("String wrapper receiver required"));
            }
            return self.call(method, args.to_vec(), primitive, host).map(Some);
        }
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        let result = match name {
            "Object" => self.box_value(first)?,
            "Boolean" if construct => {
                let primitive = self.native(name, args)?;
                self.box_value(primitive)?
            }
            "Object.setPrototypeOf" => {
                return Err(self.property_type_error("Object.setPrototypeOf is not implemented"));
            }
            "Number.toFixed" => {
                let primitive = if let Value::Object(id) = &this {
                    self.reflection
                        .boxed
                        .get(id)
                        .cloned()
                        .unwrap_or(Value::Undefined)
                } else {
                    this
                };
                let Value::Number(number) = primitive else {
                    return Err(
                        self.property_type_error("Number.toFixed requires a Number receiver")
                    );
                };
                if matches!(first, Value::BigInt(_) | Value::Symbol(..))
                    || self.is_bigint_wrapper(&first)
                {
                    return Err(self.property_type_error(
                        "Cannot implicitly convert BigInt or Symbol to a number",
                    ));
                }
                let digits = if first == Value::Undefined {
                    0.
                } else {
                    self.number(&first)
                };
                let digits = if digits.is_nan() { 0. } else { digits.trunc() };
                if !(0. ..=20.).contains(&digits) {
                    return Err(self.property_type_error("toFixed supports 0 to 20 digits"));
                }
                let text = if !number.is_finite() || number.abs() >= 1e21 {
                    crate::runtime_support::number_text(number)
                } else {
                    format!("{number:.precision$}", precision = digits as usize)
                };
                self.string(text)?
            }
            "Object.getOwnPropertySymbols" => {
                if matches!(first, Value::Null | Value::Undefined) {
                    return Err(self.property_type_error("Cannot inspect null or undefined"));
                }
                let object = self.box_value(first)?;
                self.descriptor_native(name, &[object], host)?
            }
            "Object.isPrototypeOf" => {
                if matches!(this, Value::Null | Value::Undefined) {
                    return Err(
                        self.property_type_error("Prototype receiver cannot be null or undefined")
                    );
                }
                let mut current = first;
                let mut found = false;
                for depth in 0..64 {
                    self.tick()?;
                    current = match current {
                        Value::Native("Symbol") => Value::Native("Function.prototype"),
                        Value::Native("Function.prototype") => self.object_prototype_value()?,
                        Value::Object(id) => match self.heap.get(id) {
                            Some(Heap::Object(object)) => {
                                object.prototype.clone().unwrap_or(Value::Null)
                            }
                            _ => Value::Null,
                        },
                        Value::Array(id) => {
                            if Some(id) == self.array_prototype {
                                self.object_prototype_value()?
                            } else {
                                self.array_prototype_value()?
                            }
                        }
                        Value::Host(_) | Value::Native(_) | Value::Function(_) => {
                            return Err(self.property_type_error(
                                "Prototype traversal for Host/native/functions is not implemented",
                            ));
                        }
                        _ => Value::Null,
                    };
                    if current == Value::Null {
                        break;
                    }
                    if crate::strict_equal(&current, &this) {
                        found = true;
                        break;
                    }
                    if depth == 63 {
                        return Err(JsError::resource("Prototype traversal depth exceeded"));
                    }
                }
                Value::Bool(found)
            }
            "Object.valueOf" => {
                if matches!(this, Value::Null | Value::Undefined) {
                    return Err(self.property_type_error("Object.valueOf requires a receiver"));
                }
                self.box_value(this)?
            }
            "String.valueOf" | "String.toString" | "Number.valueOf" | "Number.toString"
            | "Boolean.valueOf" | "Boolean.toString" => {
                if name == "Number.toString"
                    && !matches!(first, Value::Undefined)
                    && self.number(&first) != 10.
                {
                    return Err(self.property_type_error(
                        "Number.toString radices other than 10 are not implemented",
                    ));
                }
                let primitive =
                    if let Value::Object(id) = &this {
                        self.reflection.boxed.get(id).cloned().ok_or_else(|| {
                            JsError::runtime("Primitive wrapper receiver required")
                        })?
                    } else {
                        this
                    };
                let valid = match name.split('.').next().unwrap_or("") {
                    "String" => matches!(primitive, Value::String(_) | Value::Utf16(_)),
                    "Number" => matches!(primitive, Value::Number(_)),
                    "Boolean" => matches!(primitive, Value::Bool(_)),
                    _ => false,
                };
                if !valid {
                    return Err(self.property_type_error("Incompatible primitive method receiver"));
                }
                if name.ends_with("valueOf") || name.starts_with("String.") {
                    primitive
                } else {
                    self.string(primitive.primitive_text()?)?
                }
            }
            "Object.hasOwnProperty" | "Object.propertyIsEnumerable" => {
                // These prototype methods convert the key before ToObject(this).
                let first = self.coerce_property_key(first, host)?;
                if matches!(this, Value::Null | Value::Undefined) {
                    return Err(self.property_type_error("Object method requires a receiver"));
                }
                let object = self.box_value(this)?;
                let present = if let Value::Symbol(key, _) = first {
                    let id = self.heap_owner(&object)?;
                    self.symbol_properties
                        .get(&id)
                        .is_some_and(|map| map.contains_key(&key))
                        && (name == "Object.hasOwnProperty"
                            || self
                                .symbol_metadata
                                .get(&id)
                                .and_then(|map| map.get(&key))
                                .is_none_or(|metadata| metadata.enumerable))
                } else {
                    let key = self.text(&first, 0)?;
                    if self.is_global_object(&object) {
                        if name == "Object.hasOwnProperty" {
                            self.global_has_own(&key, host)?
                        } else {
                            self.global_descriptor(&key, host)?
                                .is_some_and(|d| d.metadata.enumerable)
                        }
                    } else {
                        self.has_own_property(&object, &key)?
                            && (name == "Object.hasOwnProperty" || {
                                let owner = self.heap_owner(&object)?;
                                self.own_enumerable(owner, &key)
                            })
                    }
                };
                Value::Bool(present)
            }
            "Object.toString" => {
                let tagged = if let Value::Object(id) = &this {
                    self.reflection.boxed.get(id).unwrap_or(&this)
                } else {
                    &this
                };
                let tag = match tagged {
                    Value::Undefined => "Undefined",
                    Value::Null => "Null",
                    Value::Bool(_) => "Boolean",
                    Value::Number(_) => "Number",
                    Value::BigInt(_) => "Object",
                    Value::String(_) | Value::Utf16(_) => "String",
                    Value::Symbol(..) => "Symbol",
                    Value::Array(_) => "Array",
                    Value::Function(_) | Value::Native(_) => "Function",
                    _ => "Object",
                };
                let tag = tag.to_owned();
                let custom = if matches!(
                    this,
                    Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::BigInt(_)
                ) {
                    let Value::Symbol(key, _) = self.well_known_symbol("toStringTag")? else {
                        unreachable!()
                    };
                    let value = self.get_symbol(this.clone(), key, host)?;
                    if matches!(value, Value::String(_) | Value::Utf16(_)) {
                        Some(value)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(custom) = custom {
                    let length = match &custom {
                        Value::String(text) => text.encode_utf16().count(),
                        Value::Utf16(units) => units.len(),
                        _ => unreachable!(),
                    };
                    if length > 65_536 - 9 {
                        return Err(JsError::resource("Object tag exceeds string limit"));
                    }
                    self.remaining = self
                        .remaining
                        .checked_sub(length as u64)
                        .ok_or_else(|| JsError::resource("Object tag work budget exceeded"))?;
                    let mut output = Vec::with_capacity(length + 9);
                    output.extend("[object ".encode_utf16());
                    output.extend(crate::strings::units(&custom).unwrap());
                    output.push(b']' as u16);
                    self.utf16_string(output)?
                } else {
                    self.string(format!("[object {tag}]"))?
                }
            }
            "Object.toLocaleString" => {
                let callable = self.get(this.clone(), "toString", host)?;
                self.call(callable, vec![], this, host)?
            }
            "Object.is" => {
                let other = args.get(1).unwrap_or(&Value::Undefined);
                Value::Bool(match (&first, other) {
                    (Value::Number(a), Value::Number(b)) => {
                        (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
                    }
                    _ => crate::strict_equal(&first, other),
                })
            }
            "Object.assign" => {
                if matches!(first, Value::Null | Value::Undefined) {
                    return Err(self
                        .property_type_error("Object.assign target cannot be null or undefined"));
                }
                let target = self.box_value(first)?;
                for source in args.iter().skip(1) {
                    if matches!(source, Value::Null | Value::Undefined) {
                        continue;
                    }
                    let source = self.box_value(source.clone())?;
                    let owner = self.heap_owner(&source)?;
                    for key in self.own_names(&source)? {
                        if self.has_own_property(&source, &key)? && self.own_enumerable(owner, &key)
                        {
                            let value = self.get(source.clone(), &key, host)?;
                            self.set_or_throw(target.clone(), &key, value, host)?;
                        }
                    }
                    for key in self.symbol_order.get(&owner).cloned().unwrap_or_default() {
                        if self
                            .symbol_properties
                            .get(&owner)
                            .is_some_and(|map| map.contains_key(&key))
                            && self
                                .symbol_metadata
                                .get(&owner)
                                .and_then(|map| map.get(&key))
                                .is_none_or(|metadata| metadata.enumerable)
                        {
                            let value = self.get_symbol(source.clone(), key, host)?;
                            let strict = self.strict;
                            self.strict = true;
                            let result = self.set_symbol(target.clone(), key, value, host);
                            self.strict = strict;
                            result?;
                        }
                    }
                }
                target
            }
            "Object.preventExtensions" | "Object.seal" | "Object.freeze" => {
                if (matches!(first, Value::Host(_) | Value::Native(_))
                    && first != Value::Native("Symbol"))
                    || self.stdlib_own_keys(&first).is_some()
                    || matches!(&first,Value::Object(id) if matches!(self.heap.get(*id),Some(Heap::Object(object)) if object.regexp.is_some()))
                {
                    return Err(self.property_type_error(
                        "Integrity levels for this exotic object are not implemented",
                    ));
                }
                let owner = match &first {
                    Value::Native("Symbol") => Some(self.symbol_constructor_owner()?),
                    Value::Array(id) | Value::Object(id) | Value::Function(id) => Some(*id),
                    _ => None,
                };
                if let Some(owner) = owner {
                    if name != "Object.preventExtensions" {
                        self.lock_object(&first, name == "Object.freeze", host)?;
                    }
                    self.reflection.non_extensible.insert(owner);
                }
                first
            }
            "Object.isExtensible" => {
                if matches!(first, Value::Host(_) | Value::Native(_))
                    && first != Value::Native("Symbol")
                {
                    return Err(self.property_type_error(
                        "Host/native extensibility inspection is not implemented",
                    ));
                }
                Value::Bool(if first == Value::Native("Symbol") {
                    let owner = self.symbol_constructor_owner()?;
                    !self.reflection.non_extensible.contains(&owner)
                } else {
                    matches!(&first,Value::Object(id)|Value::Array(id)|Value::Function(id) if !self.reflection.non_extensible.contains(id))
                })
            }
            "Object.isSealed" | "Object.isFrozen" => {
                Value::Bool(self.object_locked(&first, name == "Object.isFrozen", host)?)
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }
    pub(super) fn reflection_cached_roots(&self) -> Vec<Value> {
        self.reflection
            .prototypes
            .values()
            .cloned()
            .chain(self.reflection.symbol_constructor.map(Value::Object))
            .collect()
    }
    pub(super) fn reflection_trace(&self, id: usize) -> Vec<Value> {
        self.reflection
            .boxed
            .get(&id)
            .cloned()
            .into_iter()
            .collect()
    }
    pub(super) fn reflection_sweep(&mut self, live: &BTreeSet<usize>) {
        self.reflection.boxed.retain(|id, _| live.contains(id));
        self.reflection
            .non_extensible
            .retain(|id| live.contains(id));
    }
}
fn property_value(name: &str, key: &str) -> Option<Value> {
    property(name, key)
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
    #[test]
    fn errors_have_real_prototype_chains_without_name_branding() {
        let mut runtime = Runtime::default();
        runtime.execute("TypeError.prototype;", &mut Empty).unwrap();
        runtime.collect_garbage(&[], &Empty).unwrap();
        assert_eq!(
            runtime
                .execute(
                    "Object.getPrototypeOf(TypeError.prototype)===Error.prototype;",
                    &mut Empty
                )
                .unwrap(),
            Value::Bool(true)
        );
        for kind in [
            "Error",
            "TypeError",
            "RangeError",
            "ReferenceError",
            "SyntaxError",
        ] {
            let source = format!(
                "const e=new {kind}('failure');Object.getPrototypeOf(e)==={kind}.prototype&&e instanceof {kind}&&e instanceof Error&&e instanceof Object&&e.message==='failure'&&{kind}.prototype.constructor==={kind}&&{kind}.prototype.name==='{kind}'&&Object.keys({kind}.prototype).length===0;"
            );
            let mut runtime = Runtime::default();
            assert_eq!(
                runtime.execute(&source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
            runtime.collect_garbage(&[], &Empty).unwrap();
            assert_eq!(
                runtime.execute("e instanceof Error;", &mut Empty).unwrap(),
                Value::Bool(true)
            );
        }
        for source in [
            "Object.getPrototypeOf(TypeError.prototype)===Error.prototype&&Object.getPrototypeOf(Error.prototype)===Object.prototype;",
            "const e=new TypeError();e.name='RangeError';e instanceof TypeError&&!(e instanceof RangeError)&&!({name:'TypeError'} instanceof TypeError);",
            "Object.create(TypeError.prototype) instanceof TypeError&&!(TypeError.prototype instanceof TypeError)&&TypeError.prototype instanceof Error;",
            "let ok=false;try{Array.from(null)}catch(e){ok=e instanceof TypeError&&e instanceof Error}ok;",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn object_tag_preserves_utf16_and_observes_getter_once() {
        // This tests runtime UTF-16 preservation, not unsupported surrogate literal escapes.
        for source in [
            "let count=0;const o={};Object.defineProperty(o,Symbol.toStringTag,{get:function(){count++;return String.fromCharCode(0xd800)}});const text=Object.prototype.toString.call(o);text.length===10&&text.charCodeAt(8)===55296&&count===1;",
            "const o={};o[Symbol.toStringTag]={toString:function(){throw 9}};Object.prototype.toString.call(o)==='[object Object]';",
            "const o={};Object.defineProperty(o,Symbol.toStringTag,{get:function(){throw 17}});try{Object.prototype.toString.call(o);false}catch(e){e===17;}",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn ordinary_integrity_levels_and_symbols() {
        for source in [
            "const o={a:1};Object.preventExtensions(o);o.b=2;o.a=3;!Object.isExtensible(o)&&o.a===3&&!Object.hasOwn(o,'b');",
            "const s=Symbol('s');const o={a:1};o[s]=2;Object.freeze(o);o[s]=3;o.a=4;Object.isFrozen(o)&&o[s]===2&&o.a===1&&!(delete o[s]);",
            "const o={a:1};Object.seal(o);o.a=4;Object.isSealed(o)&&!Object.isFrozen(o)&&o.a===4&&!(delete o.a);",
            "const o=Object.preventExtensions({});try{Object.defineProperty(o,'x',{value:1});}catch(e){e.name==='TypeError';}",
            "const s=Symbol();const o=Object.preventExtensions({});try{Object.defineProperty(o,s,{value:1});}catch(e){e.name==='TypeError';}",
            "const s=Symbol();const a={};a[s]=2;const b=Object.assign({},a);b[s]===2&&Object.getOwnPropertySymbols(b)[0]===s;",
            "try{Object.assign(Object.freeze({x:1}),{x:2});}catch(e){e.name==='TypeError';}",
            "Object.is(NaN,NaN)&&!Object.is(0,-0)&&Object.is(0,0);",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn primitive_wrappers_preserve_identity_and_string_descriptors() {
        for source in [
            "const n=new Number(4);n.valueOf()===4&&n.toString()==='4'&&n instanceof Number;",
            "const b=new Boolean(false);b.valueOf()===false&&b.toString()==='false';",
            "const s=new String('abc');s.length===3&&s[1]==='b'&&s.slice(1)==='bc'&&s.valueOf()==='abc';",
            "const s=Object('ab');s[0]='z';s[0]==='a'&&Object.keys(s).join(',')==='0,1'&&!Object.getOwnPropertyDescriptor(s,'length').writable;",
            "Object.getPrototypeOf({})===Object.prototype&&Object.getPrototypeOf(Object.prototype)===null;",
        ] {
            assert_eq!(
                Runtime::default().execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
}
