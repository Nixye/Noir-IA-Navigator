//! Own-property descriptors for the bounded VM heap and explicit intrinsic constructors.
//! Other host/native exotics remain errors; callbacks gain no host authority.
use crate::{Heap, Host, JsError, Runtime, Value, strict_equal};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(super) struct PropertyMetadata {
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
    pub accessor: Option<(Value, Value)>,
}

#[derive(Clone)]
pub(super) struct Descriptor {
    pub(super) value: Value,
    pub(super) metadata: PropertyMetadata,
}

#[derive(Default)]
pub(super) struct Definition {
    pub(super) value: Option<Value>,
    pub(super) writable: Option<bool>,
    pub(super) enumerable: Option<bool>,
    pub(super) configurable: Option<bool>,
    pub(super) getter: Option<Value>,
    pub(super) setter: Option<Value>,
}

pub(super) enum Assignment {
    Store,
    Handled,
    Blocked,
}

fn same_value(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => {
            (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
        }
        _ => strict_equal(left, right),
    }
}

impl Runtime {
    pub(super) fn symbol_descriptor(
        &mut self,
        object: &Value,
        key: u64,
    ) -> Result<Option<Descriptor>, JsError> {
        let id = self.heap_owner_for(object, "symbol-descriptor")?;
        Ok(self
            .symbol_properties
            .get(&id)
            .and_then(|properties| properties.get(&key))
            .map(|value| Descriptor {
                value: value.clone(),
                metadata: self
                    .symbol_metadata
                    .get(&id)
                    .and_then(|properties| properties.get(&key))
                    .cloned()
                    .unwrap_or(PropertyMetadata {
                        writable: true,
                        enumerable: true,
                        configurable: true,
                        accessor: None,
                    }),
            }))
    }
    fn define_symbol_property(
        &mut self,
        object: &Value,
        key: u64,
        definition: Definition,
    ) -> Result<(), JsError> {
        let id = self.heap_owner_for(object, "define-symbol-property")?;
        let current = self.symbol_descriptor(object, key)?;
        let changes_kind = current.as_ref().is_some_and(|current| {
            if definition.getter.is_some() || definition.setter.is_some() {
                current.metadata.accessor.is_none()
            } else if definition.value.is_some() || definition.writable.is_some() {
                current.metadata.accessor.is_some()
            } else {
                false
            }
        });
        if let Some(current) = &current
            && !current.metadata.configurable
        {
            let invalid = definition.configurable == Some(true)
                || definition
                    .enumerable
                    .is_some_and(|value| value != current.metadata.enumerable)
                || changes_kind
                || if let Some((getter, setter)) = &current.metadata.accessor {
                    definition
                        .getter
                        .as_ref()
                        .is_some_and(|value| !same_value(value, getter))
                        || definition
                            .setter
                            .as_ref()
                            .is_some_and(|value| !same_value(value, setter))
                } else {
                    !current.metadata.writable
                        && (definition.writable == Some(true)
                            || definition
                                .value
                                .as_ref()
                                .is_some_and(|value| !same_value(value, &current.value)))
                };
            if invalid {
                return Err(
                    self.property_type_error("Cannot redefine a non-configurable symbol property")
                );
            }
        }
        let fresh = current.is_none();
        if fresh && self.reflection.non_extensible.contains(&id) {
            return Err(self.property_type_error("Object is not extensible"));
        }
        let mut descriptor = current.unwrap_or(Descriptor {
            value: Value::Undefined,
            metadata: PropertyMetadata {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: None,
            },
        });
        if changes_kind {
            descriptor.value = Value::Undefined;
            descriptor.metadata.writable = false;
            descriptor.metadata.accessor = None;
        }
        if definition.getter.is_some() || definition.setter.is_some() {
            let (getter, setter) = descriptor
                .metadata
                .accessor
                .take()
                .unwrap_or((Value::Undefined, Value::Undefined));
            descriptor.metadata.accessor = Some((
                definition.getter.unwrap_or(getter),
                definition.setter.unwrap_or(setter),
            ));
            descriptor.value = Value::Undefined;
            descriptor.metadata.writable = false;
        }
        if let Some(value) = definition.value {
            descriptor.value = value;
        }
        if let Some(value) = definition.writable {
            descriptor.metadata.writable = value;
        }
        if let Some(value) = definition.enumerable {
            descriptor.metadata.enumerable = value;
        }
        if let Some(value) = definition.configurable {
            descriptor.metadata.configurable = value;
        }
        if fresh {
            if self
                .symbol_properties
                .get(&id)
                .is_some_and(|properties| properties.len() >= 128)
            {
                return Err(JsError::resource(
                    "Symbol descriptor property count exceeded",
                ));
            }
            self.symbol_order.entry(id).or_default().push(key);
        }
        self.symbol_properties
            .entry(id)
            .or_default()
            .insert(key, descriptor.value);
        self.symbol_metadata
            .entry(id)
            .or_default()
            .insert(key, descriptor.metadata);
        Ok(())
    }
    pub(super) fn lock_object(
        &mut self,
        object: &Value,
        freeze: bool,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let id = self.descriptor_owner(object, if freeze { "freeze" } else { "seal" })?;
        if self.stdlib_own_keys(object).is_some() {
            return Err(
                self.property_type_error("Integrity levels for typed arrays are not implemented")
            );
        }
        let names = self.own_names(object)?;
        for name in names {
            if let Some(descriptor) = self.own_descriptor(object, &name, host)? {
                self.define_property(
                    object,
                    &name,
                    Definition {
                        configurable: Some(false),
                        writable: if freeze && descriptor.metadata.accessor.is_none() {
                            Some(false)
                        } else {
                            None
                        },
                        ..Definition::default()
                    },
                    host,
                )?;
            }
        }
        let keys = self
            .symbol_properties
            .get(&id)
            .map(|map| map.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for key in keys {
            if let Some(descriptor) = self.symbol_descriptor(object, key)? {
                self.define_symbol_property(
                    object,
                    key,
                    Definition {
                        configurable: Some(false),
                        writable: if freeze && descriptor.metadata.accessor.is_none() {
                            Some(false)
                        } else {
                            None
                        },
                        ..Definition::default()
                    },
                )?;
            }
        }
        Ok(())
    }
    pub(super) fn object_locked(
        &mut self,
        object: &Value,
        freeze: bool,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        if matches!(object, Value::Host(_) | Value::Native(_))
            && !matches!(object, Value::Native("Symbol" | "Number"))
        {
            return Err(self.exotic_descriptor_error(
                object,
                if freeze { "isFrozen" } else { "isSealed" },
                "Integrity inspection of Host/native objects is not implemented",
            ));
        }
        if !matches!(
            object,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Native("Symbol" | "Number")
        ) {
            return Ok(true);
        }
        let id = self.descriptor_owner(object, if freeze { "isFrozen" } else { "isSealed" })?;
        if !self.reflection.non_extensible.contains(&id) {
            return Ok(false);
        }
        for name in self.own_names(object)? {
            if let Some(descriptor) = self.own_descriptor(object, &name, host)?
                && (descriptor.metadata.configurable
                    || (freeze
                        && descriptor.metadata.accessor.is_none()
                        && descriptor.metadata.writable))
            {
                return Ok(false);
            }
        }
        let keys = self
            .symbol_properties
            .get(&id)
            .map(|map| map.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for key in keys {
            if let Some(descriptor) = self.symbol_descriptor(object, key)?
                && (descriptor.metadata.configurable
                    || (freeze
                        && descriptor.metadata.accessor.is_none()
                        && descriptor.metadata.writable))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(super) fn create_data_property(
        &mut self,
        object: &Value,
        key: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        if let Some(existing) = self.own_descriptor(object, key, host)?
            && !existing.metadata.configurable
        {
            return Ok(false);
        }
        self.define_property(
            object,
            key,
            Definition {
                value: Some(value),
                writable: Some(true),
                enumerable: Some(true),
                configurable: Some(true),
                ..Definition::default()
            },
            host,
        )?;
        Ok(true)
    }
    pub(super) fn heap_owner(&mut self, value: &Value) -> Result<usize, JsError> {
        self.heap_owner_for(value, "own-properties")
    }

    // Only VM tags and trusted static native names enter this diagnostic. Never
    // inspect target properties or format a page value (including symbol labels).
    // JsError applies its existing per-context/count/total byte bounds.
    fn exotic_descriptor_error(
        &mut self,
        value: &Value,
        operation: &str,
        message: &str,
    ) -> JsError {
        let mut error = self.property_type_error(message);
        let target = match value {
            Value::Native(name) => format!("native={}", name.chars().take(64).collect::<String>()),
            Value::Host(handle) => format!("host={handle}"),
            _ => return error,
        };
        error.add_diagnostic_context(&format!("descriptor operation={operation} {target}"));
        error
    }

    fn heap_owner_for(&mut self, value: &Value, operation: &str) -> Result<usize, JsError> {
        if self.is_global_object(value) {
            return self.global_storage();
        }
        match value {
            Value::Native(name @ ("Symbol" | "Number")) => self.intrinsic_constructor_owner(name),
            Value::Native(_) | Value::Host(_) => Err(self.exotic_descriptor_error(
                value,
                operation,
                "Own properties of this Host/native object are not implemented",
            )),
            Value::Object(id) if matches!(self.heap.get(*id), Some(Heap::Object(_))) => Ok(*id),
            Value::Array(id) if matches!(self.heap.get(*id), Some(Heap::Array(_))) => Ok(*id),
            Value::Function(id) if matches!(self.heap.get(*id), Some(Heap::Function(_))) => Ok(*id),
            _ => Err(JsError::runtime(
                "Own properties require a VM object, array or function; Host/native exotics are not implemented",
            )),
        }
    }
    fn descriptor_owner(&mut self, value: &Value, operation: &str) -> Result<usize, JsError> {
        if self.is_global_object(value) {
            return self.global_storage();
        }
        match value {
            Value::Native(name @ ("Symbol" | "Number")) => self.intrinsic_constructor_owner(name),
            Value::Object(id) => match self.heap.get(*id) {
                Some(Heap::Object(object)) if object.regexp.is_none() => Ok(*id),
                Some(Heap::Object(_)) => Err(JsError::runtime(
                    "RegExp exotic descriptors are not implemented",
                )),
                _ => Err(JsError::runtime("Invalid descriptor object")),
            },
            Value::Array(id) if matches!(self.heap.get(*id), Some(Heap::Array(_))) => Ok(*id),
            Value::Function(id) if matches!(self.heap.get(*id), Some(Heap::Function(_))) => Ok(*id),
            Value::Host(_) | Value::Native(_) => Err(self.exotic_descriptor_error(
                value,
                operation,
                "Host/native exotic descriptors are not implemented (except Number and Symbol)",
            )),
            _ => Err(self.property_type_error("Descriptors require an object, array or function")),
        }
    }

    pub(super) fn own_enumerable(&self, id: usize, property: &str) -> bool {
        if let Some(present) = self.stdlib_own_index(&Value::Object(id), property) {
            return present;
        }
        if let Some(metadata) = self
            .property_metadata
            .get(&id)
            .and_then(|properties| properties.get(property))
        {
            return metadata.enumerable;
        }
        match self.heap.get(id) {
            Some(Heap::Object(object)) => !object.non_enumerable.contains(property),
            Some(Heap::Function(function)) => {
                if property == "length" {
                    function.properties.contains_key(property)
                        && self.synthetic_deleted(id, property)
                } else {
                    property != "prototype" || !function.has_prototype()
                }
            }
            Some(Heap::Array(_)) => {
                property != "length"
                    && !self
                        .array_non_enumerable
                        .get(&id)
                        .is_some_and(|hidden| hidden.contains(property))
            }
            _ => false,
        }
    }

    pub(super) fn synthetic_deleted(&self, id: usize, property: &str) -> bool {
        self.deleted_synthetics
            .get(&id)
            .is_some_and(|properties| properties.contains(property))
    }

    pub(super) fn read_accessor(
        &mut self,
        id: usize,
        property: &str,
        receiver: Value,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        let accessor = self
            .property_metadata
            .get(&id)
            .and_then(|properties| properties.get(property))
            .and_then(|metadata| metadata.accessor.clone());
        match accessor {
            Some((Value::Undefined, _)) => Ok(Some(Value::Undefined)),
            Some((getter, _)) => self.call(getter, vec![], receiver, host).map(Some),
            None => Ok(None),
        }
    }

    pub(super) fn own_descriptor(
        &mut self,
        object: &Value,
        property: &str,
        host: &mut impl Host,
    ) -> Result<Option<Descriptor>, JsError> {
        if *object == Value::Native("Function.prototype") {
            return self.function_prototype_descriptor(property);
        }
        if self.is_global_object(object) {
            return self.global_descriptor(property, host);
        }
        let id = self.descriptor_owner(object, "getOwnPropertyDescriptor")?;
        if let Some(present) = self.stdlib_own_index(object, property) {
            return if present {
                Ok(Some(Descriptor {
                    value: self.get(object.clone(), property, host)?,
                    metadata: PropertyMetadata {
                        writable: true,
                        enumerable: true,
                        configurable: true,
                        accessor: None,
                    },
                }))
            } else {
                Ok(None)
            };
        }
        if property.len() > 1024 {
            return Err(JsError::resource(
                "Descriptor property name exceeds 1024 bytes",
            ));
        }
        if !self.has_own_property(object, property)? {
            return Ok(None);
        }
        let value = match self.heap.get(id) {
            Some(Heap::Object(object)) => object
                .properties
                .get(property)
                .cloned()
                .unwrap_or(Value::Undefined),
            Some(Heap::Array(values)) if property == "length" => Value::Number(values.len() as f64),
            Some(Heap::Array(values)) => crate::properties::array_index(property)
                .and_then(|index| values.get(index).cloned())
                .or_else(|| {
                    self.array_properties
                        .get(&id)
                        .and_then(|properties| properties.get(property))
                        .cloned()
                })
                .unwrap_or(Value::Undefined),
            Some(Heap::Function(function)) => {
                if let Some(value) = function.properties.get(property) {
                    value.clone()
                } else if property == "length" {
                    Value::Number(function.params.len() as f64)
                } else {
                    self.get(object.clone(), property, host)?
                }
            }
            _ => return Err(JsError::runtime("Invalid descriptor owner")),
        };
        let metadata = self
            .property_metadata
            .get(&id)
            .and_then(|properties| properties.get(property))
            .cloned()
            .unwrap_or_else(|| {
                let (writable, configurable) = match self.heap.get(id) {
                    Some(Heap::Array(_)) if property == "length" => (true, false),
                    Some(Heap::Function(function))
                        if property == "prototype" && function.has_prototype() =>
                    {
                        (true, false)
                    }
                    Some(Heap::Function(function))
                        if property == "length" && !function.properties.contains_key(property) =>
                    {
                        (false, true)
                    }
                    _ => (true, true),
                };
                PropertyMetadata {
                    writable,
                    enumerable: self.own_enumerable(id, property),
                    configurable,
                    accessor: None,
                }
            });
        Ok(Some(Descriptor { value, metadata }))
    }

    pub(super) fn assignment_action(
        &mut self,
        object: &Value,
        property: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Assignment, JsError> {
        if self.is_global_object(object) {
            return self.global_assignment(property, value, object.clone(), host);
        }
        if !matches!(
            object,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Native("Symbol" | "Number")
        ) {
            return Ok(Assignment::Store);
        }
        let owner = self.heap_owner(object)?;
        let may_extend_array = match self.heap.get(owner) {
            Some(Heap::Array(values)) => {
                crate::properties::array_index(property).is_none_or(|index| {
                    index < values.len()
                        || self
                            .property_metadata
                            .get(&owner)
                            .and_then(|map| map.get("length"))
                            .is_none_or(|metadata| metadata.writable)
                })
            }
            _ => true,
        };
        let may_store = may_extend_array
            && (!self.reflection.non_extensible.contains(&owner)
                || self.has_own_property(object, property)?);
        let mut current = object.clone();
        for _ in 0..64 {
            self.tick()?;
            // Existing RegExp builtins retain their documented specialized path.
            if matches!(current, Value::Object(id) if matches!(self.heap.get(id),Some(Heap::Object(object)) if object.regexp.is_some()))
            {
                return Ok(Assignment::Store);
            }
            if let Some(descriptor) = self.own_descriptor(&current, property, host)? {
                if let Some((_, setter)) = descriptor.metadata.accessor {
                    if matches!(setter, Value::Undefined) {
                        return Ok(Assignment::Blocked);
                    }
                    self.call(setter, vec![value], object.clone(), host)?;
                    return Ok(Assignment::Handled);
                }
                return Ok(if descriptor.metadata.writable && may_store {
                    Assignment::Store
                } else {
                    Assignment::Blocked
                });
            }
            current = match current {
                Value::Native("Symbol" | "Number") => Value::Native("Function.prototype"),
                Value::Native("Function.prototype") => self.object_prototype_value()?,
                Value::Object(id) => match &self.heap[id] {
                    Heap::Object(object) => object.prototype.clone().unwrap_or(Value::Null),
                    _ => unreachable!(),
                },
                Value::Array(id) => {
                    if Some(id) == self.array_prototype {
                        self.object_prototype_value()?
                    } else {
                        self.array_prototype_value()?
                    }
                }
                _ => Value::Null,
            };
            if matches!(current, Value::Host(_) | Value::Native(_))
                && !matches!(
                    current,
                    Value::Native("Symbol" | "Number" | "Function.prototype")
                )
            {
                return Err(JsError::runtime(
                    "Assignments through Host/native prototype descriptors are not implemented",
                ));
            }
            if matches!(current, Value::Null) {
                return Ok(if may_store {
                    Assignment::Store
                } else {
                    Assignment::Blocked
                });
            }
        }
        Err(JsError::resource(
            "Descriptor assignment prototype depth exceeded",
        ))
    }

    pub(super) fn set_or_throw(
        &mut self,
        object: Value,
        property: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        match self.assignment_action(&object, property, value.clone(), host)? {
            Assignment::Store => self.set_unchecked(object, property, value, host),
            Assignment::Handled => Ok(()),
            Assignment::Blocked => Err(self.property_type_error("Property is not writable")),
        }
    }

    pub(super) fn configurable_property(&self, id: usize, property: &str) -> bool {
        self.property_metadata
            .get(&id)
            .and_then(|properties| properties.get(property))
            .is_none_or(|metadata| metadata.configurable)
    }

    pub(super) fn forget_descriptor(&mut self, id: usize, property: &str) {
        if let Some(order) = self.property_order.get_mut(&id) {
            order.retain(|key| key != property);
        }
        if let Some(properties) = self.property_metadata.get_mut(&id) {
            properties.remove(property);
        }
    }

    pub(super) fn definition(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<Definition, JsError> {
        self.descriptor_owner(&value, "read-descriptor-definition")?;
        let mut result = Definition::default();
        for field in [
            "enumerable",
            "configurable",
            "value",
            "writable",
            "get",
            "set",
        ] {
            if !self.has_property(&value, field, host)? {
                continue;
            }
            let entry = self.get(value.clone(), field, host)?;
            match field {
                "enumerable" => result.enumerable = Some(entry.truthy()),
                "configurable" => result.configurable = Some(entry.truthy()),
                "writable" => result.writable = Some(entry.truthy()),
                "value" => result.value = Some(entry),
                "get" | "set" => {
                    if !matches!(
                        entry,
                        Value::Undefined | Value::Function(_) | Value::Native(_)
                    ) && !matches!(entry, Value::Host(id) if host.is_callable(id))
                    {
                        return Err(self.property_type_error(
                            "Descriptor getter/setter must be callable or undefined",
                        ));
                    }
                    if field == "get" {
                        result.getter = Some(entry);
                    } else {
                        result.setter = Some(entry);
                    }
                }
                _ => unreachable!(),
            }
        }
        if (result.getter.is_some() || result.setter.is_some())
            && (result.value.is_some() || result.writable.is_some())
        {
            return Err(self
                .property_type_error("A property descriptor cannot mix data and accessor fields"));
        }
        Ok(result)
    }

    pub(super) fn define_property(
        &mut self,
        object: &Value,
        property: &str,
        definition: Definition,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if self.is_global_object(object) {
            return self.define_global_property(property, definition, host);
        }
        let id = self.descriptor_owner(object, "defineProperty")?;
        if let Some(result) = self.stdlib_define_index(object, property, &definition, host)? {
            return if result {
                Ok(())
            } else {
                Err(self.property_type_error("Invalid typed array index descriptor"))
            };
        }
        if property.len() > 1024 {
            return Err(JsError::resource(
                "Descriptor property name exceeds 1024 bytes",
            ));
        }
        if matches!(object, Value::Array(_)) && property == "length" {
            if definition.getter.is_some()
                || definition.setter.is_some()
                || definition.configurable == Some(true)
                || definition.enumerable == Some(true)
            {
                return Err(self.property_type_error("Invalid array length descriptor"));
            }
            let old_writable = self
                .property_metadata
                .get(&id)
                .and_then(|map| map.get("length"))
                .is_none_or(|meta| meta.writable);
            if !old_writable && definition.writable == Some(true) {
                return Err(self.property_type_error("Cannot make array length writable"));
            }
            let result = if let Some(value) = definition.value {
                self.set_array_length(id, value, host)
            } else {
                Ok(())
            };
            if definition.writable == Some(false) {
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
            return result;
        }
        let existing = self.own_descriptor(object, property, host)?;
        if existing.is_none() && self.reflection.non_extensible.contains(&id) {
            return Err(self.property_type_error("Object is not extensible"));
        }
        let changes_kind = existing.as_ref().is_some_and(|current| {
            if definition.getter.is_some() || definition.setter.is_some() {
                current.metadata.accessor.is_none()
            } else if definition.value.is_some() || definition.writable.is_some() {
                current.metadata.accessor.is_some()
            } else {
                false
            }
        });
        if let Some(current) = &existing
            && !current.metadata.configurable
        {
            let invalid = definition.configurable == Some(true)
                || definition
                    .enumerable
                    .is_some_and(|enumerable| enumerable != current.metadata.enumerable)
                || changes_kind
                || if let Some((getter, setter)) = &current.metadata.accessor {
                    definition
                        .getter
                        .as_ref()
                        .is_some_and(|next| !same_value(next, getter))
                        || definition
                            .setter
                            .as_ref()
                            .is_some_and(|next| !same_value(next, setter))
                } else {
                    !current.metadata.writable
                        && (definition.writable == Some(true)
                            || definition
                                .value
                                .as_ref()
                                .is_some_and(|next| !same_value(next, &current.value)))
                };
            if invalid {
                return Err(self.property_type_error("Cannot redefine a non-configurable property"));
            }
        }
        let mut descriptor = existing.unwrap_or(Descriptor {
            value: Value::Undefined,
            metadata: PropertyMetadata {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: None,
            },
        });
        if changes_kind {
            descriptor.value = Value::Undefined;
            descriptor.metadata.writable = false;
            descriptor.metadata.accessor = None;
        }
        if definition.getter.is_some() || definition.setter.is_some() {
            let (getter, setter) = descriptor
                .metadata
                .accessor
                .take()
                .unwrap_or((Value::Undefined, Value::Undefined));
            descriptor.metadata.accessor = Some((
                definition.getter.unwrap_or(getter),
                definition.setter.unwrap_or(setter),
            ));
            descriptor.value = Value::Undefined;
            descriptor.metadata.writable = false;
        }
        if let Some(value) = definition.value {
            descriptor.value = value;
        }
        if let Some(writable) = definition.writable {
            descriptor.metadata.writable = writable;
        }
        if let Some(enumerable) = definition.enumerable {
            descriptor.metadata.enumerable = enumerable;
        }
        if let Some(configurable) = definition.configurable {
            descriptor.metadata.configurable = configurable;
        }
        if !self
            .property_metadata
            .get(&id)
            .is_some_and(|properties| properties.contains_key(property))
        {
            self.charge_string(property.len())?;
        }
        self.set_unchecked(object.clone(), property, descriptor.value, host)?;
        if let Some(deleted) = self.deleted_synthetics.get_mut(&id) {
            deleted.remove(property);
        }
        self.property_metadata
            .entry(id)
            .or_default()
            .insert(property.to_owned(), descriptor.metadata);
        Ok(())
    }

    /// Boolean [[DefineOwnProperty]] for Reflect's validated VM targets. No
    /// exception is caught here: conversion, getter and resource errors escape.
    pub(super) fn reflect_define_ordinary(
        &mut self,
        object: &Value,
        key: &Value,
        definition: Definition,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        let owner = self.descriptor_owner(object, "Reflect.defineProperty")?;
        let name = if matches!(key, Value::Symbol(..)) {
            None
        } else {
            Some(self.text(key, 0)?)
        };
        if matches!(object, Value::Array(_)) && name.as_deref() == Some("length") {
            return Err(
                self.property_type_error("Reflect array length definition is not implemented")
            );
        }
        let current = if let Value::Symbol(symbol, _) = key {
            self.symbol_descriptor(object, *symbol)?
        } else {
            self.own_descriptor(object, name.as_deref().unwrap(), host)?
        };
        if let Some(current) = &current {
            if !current.metadata.configurable {
                let accessor = definition.getter.is_some() || definition.setter.is_some();
                let data = definition.value.is_some() || definition.writable.is_some();
                let changes_kind = (accessor && current.metadata.accessor.is_none())
                    || (data && current.metadata.accessor.is_some());
                let forbidden = definition.configurable == Some(true)
                    || definition
                        .enumerable
                        .is_some_and(|value| value != current.metadata.enumerable)
                    || changes_kind
                    || if let Some((getter, setter)) = &current.metadata.accessor {
                        definition
                            .getter
                            .as_ref()
                            .is_some_and(|value| !same_value(value, getter))
                            || definition
                                .setter
                                .as_ref()
                                .is_some_and(|value| !same_value(value, setter))
                    } else {
                        !current.metadata.writable
                            && (definition.writable == Some(true)
                                || definition
                                    .value
                                    .as_ref()
                                    .is_some_and(|value| !same_value(value, &current.value)))
                    };
                if forbidden {
                    return Ok(false);
                }
            }
        } else if self.reflection.non_extensible.contains(&owner) {
            return Ok(false);
        }
        if let Value::Array(id) = object
            && let Some(index) = name.as_deref().and_then(crate::properties::array_index)
        {
            let Some(Heap::Array(values)) = self.heap.get(*id) else {
                return Err(JsError::runtime("Invalid array reference"));
            };
            if index >= values.len()
                && self
                    .property_metadata
                    .get(id)
                    .and_then(|map| map.get("length"))
                    .is_some_and(|metadata| !metadata.writable)
            {
                return Ok(false);
            }
        }
        if let Value::Symbol(symbol, _) = key {
            self.define_symbol_property(object, *symbol, definition)?;
        } else {
            self.define_property(object, name.as_deref().unwrap(), definition, host)?;
        }
        Ok(true)
    }
    pub(super) fn own_names(&mut self, object: &Value) -> Result<Vec<String>, JsError> {
        if self.is_global_object(object) {
            return Err(
                self.property_type_error("Complete global own-key enumeration is not implemented")
            );
        }
        let id = self.heap_owner_for(object, "own-property-names")?;
        let scan_work = match &self.heap[id] {
            Heap::Vacant => return Err(JsError::runtime("Collected property owner")),
            Heap::Object(object) => object.properties.len(),
            Heap::Function(function) => function.properties.len() + 2,
            Heap::Array(values) => {
                values.len() + self.array_properties.get(&id).map_or(0, BTreeMap::len) + 1
            }
        };
        self.remaining = self
            .remaining
            .checked_sub((scan_work as u64).saturating_mul(2))
            .ok_or_else(|| JsError::resource("Own property scan instruction budget exceeded"))?;
        let (count, bytes) = match &self.heap[id] {
            Heap::Vacant => return Err(JsError::runtime("Collected property owner")),
            Heap::Object(object) => (
                object.properties.len(),
                object.properties.keys().map(String::len).sum(),
            ),
            Heap::Function(function) => {
                let length = usize::from(
                    !self.synthetic_deleted(id, "length")
                        && !function.properties.contains_key("length"),
                );
                let prototype = usize::from(
                    function.has_prototype() && !function.properties.contains_key("prototype"),
                );
                (
                    function.properties.len() + length + prototype,
                    function.properties.keys().map(String::len).sum::<usize>()
                        + 6 * length
                        + 9 * prototype,
                )
            }
            Heap::Array(values) => {
                let mut count = 1;
                let mut bytes = 6;
                for index in 0..values.len() {
                    if self.array_has_index(id, index, values.len()) {
                        count += 1;
                        bytes += if index == 0 {
                            1
                        } else {
                            index.ilog10() as usize + 1
                        };
                    }
                }
                if let Some(extra) = self.array_properties.get(&id) {
                    count += extra.len();
                    bytes += extra.keys().map(String::len).sum::<usize>();
                }
                (count, bytes)
            }
        };
        if count > 4096 {
            return Err(JsError::resource("Own property name count exceeds 4096"));
        }
        self.charge_string(bytes)?;
        self.remaining = self.remaining.checked_sub(count as u64).ok_or_else(|| {
            JsError::resource("Own property enumeration instruction budget exceeded")
        })?;
        let mut names = match &self.heap[id] {
            Heap::Vacant => return Err(JsError::runtime("Collected property owner")),
            Heap::Object(object) => object.properties.keys().cloned().collect::<Vec<_>>(),
            Heap::Function(function) => {
                let mut names = function.properties.keys().cloned().collect::<Vec<_>>();
                if !self.synthetic_deleted(id, "length")
                    && !function.properties.contains_key("length")
                {
                    names.push("length".into());
                }
                if function.has_prototype() && !function.properties.contains_key("prototype") {
                    names.push("prototype".into());
                }
                names
            }
            Heap::Array(values) => {
                let mut names = (0..values.len())
                    .filter(|index| self.array_has_index(id, *index, values.len()))
                    .map(|index| index.to_string())
                    .collect::<Vec<_>>();
                if let Some(extra) = self.array_properties.get(&id) {
                    names.extend(extra.keys().cloned());
                }
                names.push("length".into());
                names
            }
        };
        if let Some(extra) = self.stdlib_own_keys(object) {
            self.remaining = self
                .remaining
                .checked_sub(extra.len() as u64)
                .ok_or_else(|| JsError::resource("Native key enumeration budget exceeded"))?;
            names.extend(extra);
            let mut seen = std::collections::BTreeSet::new();
            names.retain(|key| seen.insert(key.clone()));
            if names.len() > 4096 {
                return Err(JsError::resource("Own property key count exceeded"));
            }
        }
        self.sort_property_keys(id, &mut names);
        Ok(names)
    }

    fn define_properties(
        &mut self,
        object: &Value,
        descriptors: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        self.descriptor_owner(object, "defineProperties")?;
        let names = self.own_names(&descriptors)?;
        let owner = self.descriptor_owner(&descriptors, "read-descriptor-map")?;
        let mut definitions = Vec::new();
        for key in names {
            if !self.has_own_property(&descriptors, &key)? || !self.own_enumerable(owner, &key) {
                continue;
            }
            if definitions.len() >= 128 {
                return Err(JsError::resource("Descriptor batch exceeds 128 properties"));
            }
            let value = self.get(descriptors.clone(), &key, host)?;
            definitions.push((key, self.definition(value, host)?));
        }
        let mut symbol_definitions = Vec::new();
        let symbol_keys = self.symbol_order.get(&owner).cloned().unwrap_or_default();
        for key in symbol_keys {
            if self
                .symbol_metadata
                .get(&owner)
                .and_then(|map| map.get(&key))
                .is_some_and(|metadata| !metadata.enumerable)
            {
                continue;
            }
            let value = self.get_symbol(descriptors.clone(), key, host)?;
            symbol_definitions.push((key, self.definition(value, host)?));
        }
        if definitions.len() + symbol_definitions.len() > 128 {
            return Err(JsError::resource("Descriptor batch exceeds 128 properties"));
        }
        for (key, definition) in definitions {
            self.define_property(object, &key, definition, host)?;
        }
        for (key, definition) in symbol_definitions {
            self.define_symbol_property(object, key, definition)?;
        }
        Ok(())
    }

    pub(super) fn descriptor_native(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        self.capture_global_object(host)?;
        let object = args.first().cloned().unwrap_or(Value::Undefined);
        if name == "Object.getOwnPropertySymbols" {
            let owner = self.heap_owner_for(&object, name)?;
            let mut keys = self.symbol_order.get(&owner).cloned().unwrap_or_default();
            if let Some(properties) = self.symbol_properties.get(&owner) {
                for key in properties.keys() {
                    if !keys.contains(key) {
                        keys.push(*key);
                    }
                }
            }
            let values = keys
                .into_iter()
                .map(|key| self.symbol_value(key))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Value::Array(self.allocate(Heap::Array(values))?));
        }
        if name == "Object.create" {
            let prototype = match object {
                Value::Null => None,
                Value::Object(_)
                | Value::Function(_)
                | Value::Array(_)
                | Value::Host(_)
                | Value::Native("Symbol" | "Number") => Some(object),
                _ => {
                    return Err(
                        self.property_type_error("Object prototype must be an object or null")
                    );
                }
            };
            let result = self.object(BTreeMap::new(), prototype)?;
            if let Some(properties) = args.get(1)
                && !matches!(properties, Value::Undefined)
            {
                self.define_properties(&result, properties.clone(), host)?;
            }
            return Ok(result);
        }
        if name == "Object.defineProperty" {
            self.descriptor_owner(&object, name)?;
            let key =
                self.coerce_property_key(args.get(1).cloned().unwrap_or(Value::Undefined), host)?;
            if let Value::Symbol(key, _) = key {
                let definition =
                    self.definition(args.get(2).cloned().unwrap_or(Value::Undefined), host)?;
                self.define_symbol_property(&object, key, definition)?;
                return Ok(object);
            }
            let key = self.text(&key, 0)?;
            let definition =
                self.definition(args.get(2).cloned().unwrap_or(Value::Undefined), host)?;
            self.define_property(&object, &key, definition, host)?;
            return Ok(object);
        }
        if name == "Object.defineProperties" {
            self.define_properties(
                &object,
                args.get(1).cloned().unwrap_or(Value::Undefined),
                host,
            )?;
            return Ok(object);
        }
        if name == "Object.getOwnPropertyDescriptor" {
            if matches!(object, Value::Null | Value::Undefined) {
                return Err(self.property_type_error("Cannot inspect null or undefined"));
            }
            let object = self.box_value(object)?;
            let key =
                self.coerce_property_key(args.get(1).cloned().unwrap_or(Value::Undefined), host)?;
            if let Value::Symbol(key, _) = key {
                return match self.symbol_descriptor(&object, key)? {
                    Some(descriptor) => self.descriptor_object(descriptor),
                    None => Ok(Value::Undefined),
                };
            }
            let key = self.text(&key, 0)?;
            return match self.own_descriptor(&object, &key, host)? {
                Some(descriptor) => self.descriptor_object(descriptor),
                None => Ok(Value::Undefined),
            };
        }
        let names = self.own_names(&object)?;
        if name == "Object.getOwnPropertyDescriptors" {
            let owner = self.heap_owner_for(&object, name)?;
            let mut symbols = self.symbol_order.get(&owner).cloned().unwrap_or_default();
            if let Some(properties) = self.symbol_properties.get(&owner) {
                for key in properties.keys() {
                    if !symbols.contains(key) {
                        symbols.push(*key);
                    }
                }
            }
            if names.len() + symbols.len() > 128 {
                return Err(JsError::resource(
                    "Descriptor result exceeds object property limit",
                ));
            }
            let mut result = BTreeMap::new();
            for key in &names {
                if let Some(descriptor) = self.own_descriptor(&object, key, host)? {
                    result.insert(key.clone(), self.descriptor_object(descriptor)?);
                }
            }
            let result = self.object(result, None)?;
            let result_owner = self.heap_owner(&result)?;
            self.remember_property_order(result_owner, names);
            for key in symbols {
                if let Some(descriptor) = self.symbol_descriptor(&object, key)? {
                    let value = self.descriptor_object(descriptor)?;
                    self.define_symbol_property(
                        &result,
                        key,
                        Definition {
                            value: Some(value),
                            writable: Some(true),
                            enumerable: Some(true),
                            configurable: Some(true),
                            ..Definition::default()
                        },
                    )?;
                }
            }
            return Ok(result);
        }
        let owner = self.heap_owner_for(&object, name)?;
        let mut result = Vec::new();
        for key in names {
            if !self.has_own_property(&object, &key)? {
                continue;
            }
            if name != "Object.getOwnPropertyNames" && !self.own_enumerable(owner, &key) {
                continue;
            }
            result.push(match name {
                "Object.getOwnPropertyNames" | "Object.keys" => self.string(key)?,
                "Object.values" => self.get(object.clone(), &key, host)?,
                "Object.entries" => {
                    let value = self.get(object.clone(), &key, host)?;
                    let key = self.string(key)?;
                    Value::Array(self.allocate(Heap::Array(vec![key, value]))?)
                }
                _ => return Err(JsError::runtime("Unknown property descriptor operation")),
            });
        }
        Ok(Value::Array(self.allocate(Heap::Array(result))?))
    }

    fn descriptor_object(&mut self, descriptor: Descriptor) -> Result<Value, JsError> {
        self.charge_string(if descriptor.metadata.accessor.is_some() {
            28
        } else {
            35
        })?;
        let mut properties = BTreeMap::from([
            (
                "enumerable".into(),
                Value::Bool(descriptor.metadata.enumerable),
            ),
            (
                "configurable".into(),
                Value::Bool(descriptor.metadata.configurable),
            ),
        ]);
        if let Some((getter, setter)) = descriptor.metadata.accessor {
            properties.insert("get".into(), getter);
            properties.insert("set".into(), setter);
        } else {
            properties.insert("value".into(), descriptor.value);
            properties.insert("writable".into(), Value::Bool(descriptor.metadata.writable));
        }
        self.object(properties, None)
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[derive(Default)]
    struct InertHost {
        accesses: usize,
    }
    impl Host for InertHost {
        fn global(&mut self, name: &str) -> Option<Value> {
            (name == "opaque").then_some(Value::Host(42))
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            self.accesses += 1;
            Ok(Value::Undefined)
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            self.accesses += 1;
            Ok(())
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            self.accesses += 1;
            Ok(Value::Undefined)
        }
    }

    #[test]
    fn native_descriptor_rejection_survives_secondary_reporter_failure() {
        let mut host = InertHost::default();
        let error = Runtime::default()
            .execute(
                "const reporter={};try{Object.defineProperty(Object,'private-key',{value:'private-value'});}catch(e){reporter._DumpException(e);}",
                &mut host,
            )
            .unwrap_err();
        assert_eq!(error.message, "Value is not callable (_DumpException)");
        let previous = &error.previous_exceptions()[0];
        assert_eq!(
            previous.message,
            "Host/native exotic descriptors are not implemented (except Number and Symbol)"
        );
        assert_eq!(
            previous.contexts,
            ["descriptor operation=Object.defineProperty native=Object"]
        );
        assert!(!error.to_string().contains("private-"));
        assert_eq!(host.accesses, 0);
    }

    #[test]
    fn host_and_other_descriptor_operations_identify_the_actual_receiver() {
        for (source, operation, target) in [
            (
                "Object.getOwnPropertyDescriptor(opaque,'secret');",
                "getOwnPropertyDescriptor",
                "host=42",
            ),
            (
                "Object.defineProperties(Object,{});",
                "defineProperties",
                "native=Object",
            ),
            (
                "Object.defineProperty(Function.prototype,'x',{});",
                "Object.defineProperty",
                "native=Function.prototype",
            ),
            ("Object.isFrozen(opaque);", "isFrozen", "host=42"),
            (
                "Object.getOwnPropertySymbols(Object);",
                "Object.getOwnPropertySymbols",
                "native=Object",
            ),
            (
                "Object.getOwnPropertyNames(opaque);",
                "own-property-names",
                "host=42",
            ),
            // The descriptor definition, not the valid first argument, is rejected.
            (
                "Object.defineProperty({},'x',Object);",
                "read-descriptor-definition",
                "native=Object",
            ),
        ] {
            let mut host = InertHost::default();
            let error = Runtime::default().execute(source, &mut host).unwrap_err();
            assert_eq!(
                error.diagnostic_contexts(),
                [format!("descriptor operation={operation} {target}")],
                "{source}"
            );
            assert_eq!(host.accesses, 0, "{source}");
        }
    }

    #[test]
    fn rejection_diagnostics_do_not_coerce_keys_or_read_descriptor_getters() {
        let mut runtime = Runtime::default();
        let mut host = InertHost::default();
        let result = runtime.execute(
            "let touched=0;const key={toString:function(){touched++;return 'private-key';}};const d={};Object.defineProperty(d,'value',{get:function(){touched++;return 'private-value';}});let caught;try{Object.defineProperty(opaque,key,d);}catch(e){caught=e;}touched===0&&caught.name==='TypeError'&&caught.message==='Host/native exotic descriptors are not implemented (except Number and Symbol)'&&caught.cause===undefined;",
            &mut host,
        ).unwrap();
        assert_eq!(result, Value::Bool(true));
        assert_eq!(host.accesses, 0);
    }

    #[test]
    fn symbol_key_descriptions_and_payloads_are_not_diagnostic_content() {
        let error = Runtime::default().execute(
            "const reporter={};try{Object.getOwnPropertyDescriptor(opaque,Symbol('private-label'));}catch(e){reporter._DumpException(e);}",
            &mut InertHost::default(),
        ).unwrap_err();
        assert_eq!(
            error.previous_exceptions()[0].contexts,
            ["descriptor operation=symbol-descriptor host=42"]
        );
        assert!(!error.to_string().contains("private-label"));
    }

    #[test]
    fn diagnostic_allocation_is_bounded_and_does_not_change_vm_budget() {
        let mut baseline = Runtime::default();
        let mut diagnostic = baseline.clone();
        let message =
            "Host/native exotic descriptors are not implemented (except Number and Symbol)";
        let original = baseline.property_type_error(message);
        let annotated = diagnostic.exotic_descriptor_error(
            &Value::Host(u64::MAX),
            "Object.defineProperty",
            message,
        );
        assert_eq!(annotated.message, original.message);
        assert_eq!(annotated.thrown, original.thrown);
        assert_eq!(diagnostic.remaining, baseline.remaining);
        assert_eq!(diagnostic.heap.len(), baseline.heap.len());
        assert!(annotated.diagnostic_contexts()[0].len() <= 128);
        assert!(annotated.diagnostic_contexts()[0].ends_with("host=18446744073709551615"));
    }
}
