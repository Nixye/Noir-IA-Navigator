//! Descriptor-backed properties of the embedder's one global object.
//! Storage is VM-owned; other Host objects never acquire descriptor authority.
use crate::descriptors::{Assignment, Definition, Descriptor, PropertyMetadata};
use crate::{Heap, Host, JsError, Object, Runtime, Value, intrinsic_global};
use std::collections::{BTreeMap, BTreeSet};

impl Runtime {
    pub(super) fn capture_global_object(&mut self, host: &impl Host) -> Result<(), JsError> {
        let next = host.global_object();
        if self.global_object_handle.is_some() && self.global_object_handle != next {
            return Err(JsError::runtime(
                "Global object identity changed within a runtime",
            ));
        }
        self.global_object_handle = next;
        Ok(())
    }
    pub(super) fn is_global_object(&self, value: &Value) -> bool {
        matches!(value, Value::Host(id) if Some(*id) == self.global_object_handle)
    }
    pub(super) fn global_storage(&mut self) -> Result<usize, JsError> {
        if let Some(id) = self.global_properties {
            return Ok(id);
        }
        let id = self.allocate(Heap::Object(Object {
            properties: BTreeMap::new(),
            non_enumerable: BTreeSet::new(),
            prototype: None,
            regexp: None,
        }))?;
        self.global_properties = Some(id);
        Ok(id)
    }
    pub(super) fn global_stored(&self, name: &str) -> Option<Value> {
        self.global_properties.and_then(|id| match &self.heap[id] {
            Heap::Object(object) => object.properties.get(name).cloned(),
            _ => None,
        })
    }
    pub(super) fn global_deleted(&self, name: &str) -> bool {
        self.global_properties
            .is_some_and(|id| self.synthetic_deleted(id, name))
    }
    pub(super) fn read_global_stored(
        &mut self,
        name: &str,
        receiver: Value,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        let Some(value) = self.global_stored(name) else {
            return Ok(None);
        };
        let id = self.global_properties.unwrap();
        Ok(Some(
            self.read_accessor(id, name, receiver, host)?
                .unwrap_or(value),
        ))
    }
    // Host-backed accessors, methods and readonly DOM aliases retain their
    // specialized contract. Reading descriptor metadata must not call them.
    pub(super) fn materialize_global_property(
        &mut self,
        name: &str,
        host: &mut impl Host,
    ) -> Result<usize, JsError> {
        if name.len() > 1024 {
            return Err(JsError::resource(
                "Descriptor property name exceeds 1024 bytes",
            ));
        }
        let id = self.global_storage()?;
        let global = self
            .global_object_handle
            .ok_or_else(|| JsError::runtime("No global object"))?;
        if host.global_accessor(name)
            || host.has_property(global, name)?
            || host.global(name).is_some()
        {
            // Intrinsics are supplied by the VM, not privileged host services.
            if intrinsic_global(name).is_none() {
                return Err(self.property_type_error(
                    "Descriptors of host-backed global properties are not implemented",
                ));
            }
        }
        if self.global_stored(name).is_some() || self.global_deleted(name) {
            return Ok(id);
        }
        let binding = self.scopes[0]
            .bindings
            .get(name)
            .filter(|b| b.global_property && b.initialized)
            .cloned();
        let constant = matches!(name, "undefined" | "NaN" | "Infinity");
        let value = if let Some(binding) = &binding {
            Some(binding.value.clone())
        } else if intrinsic_global(name).is_some() {
            Some(self.get(Value::Host(global), name, host)?)
        } else {
            None
        };
        if let Some(value) = value {
            self.set_unchecked(Value::Object(id), name, value, host)?;
            self.property_metadata.entry(id).or_default().insert(
                name.to_owned(),
                PropertyMetadata {
                    writable: !constant,
                    enumerable: binding.is_some() && !constant,
                    configurable: !constant
                        && binding.as_ref().is_none_or(|b| b.global_configurable),
                    accessor: None,
                },
            );
        }
        Ok(id)
    }
    pub(super) fn global_descriptor(
        &mut self,
        name: &str,
        host: &mut impl Host,
    ) -> Result<Option<Descriptor>, JsError> {
        let id = self.materialize_global_property(name, host)?;
        self.own_descriptor(&Value::Object(id), name, host)
    }
    pub(super) fn define_global_property(
        &mut self,
        name: &str,
        definition: Definition,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let id = self.materialize_global_property(name, host)?;
        self.define_property(&Value::Object(id), name, definition, host)?;
        let value = self.global_stored(name).unwrap_or(Value::Undefined);
        if let Some(binding) = self.scopes[0]
            .bindings
            .get_mut(name)
            .filter(|b| b.global_property)
        {
            binding.value = value;
        }
        Ok(())
    }
    pub(super) fn global_assignment(
        &mut self,
        name: &str,
        value: Value,
        receiver: Value,
        host: &mut impl Host,
    ) -> Result<Assignment, JsError> {
        if matches!(name, "undefined" | "NaN" | "Infinity") {
            return Ok(Assignment::Blocked);
        }
        let Some(id) = self.global_properties else {
            return Ok(Assignment::Store);
        };
        if self.global_stored(name).is_none() {
            return Ok(Assignment::Store);
        }
        let Some(descriptor) = self.own_descriptor(&Value::Object(id), name, host)? else {
            return Ok(Assignment::Store);
        };
        if let Some((_, setter)) = descriptor.metadata.accessor {
            if setter == Value::Undefined {
                return Ok(Assignment::Blocked);
            }
            self.call(setter, vec![value], receiver, host)?;
            return Ok(Assignment::Handled);
        }
        Ok(if descriptor.metadata.writable {
            Assignment::Store
        } else {
            Assignment::Blocked
        })
    }
    pub(super) fn store_global_property(
        &mut self,
        name: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if intrinsic_global(name).is_some()
            && !self.global_deleted(name)
            && self.global_stored(name).is_none()
        {
            self.materialize_global_property(name, host)?;
        }
        let binding = self.scopes[0]
            .bindings
            .get(name)
            .is_some_and(|b| b.global_property);
        if !binding || self.global_stored(name).is_some() {
            let id = self.global_storage()?;
            self.set_unchecked(Value::Object(id), name, value.clone(), host)?;
            if let Some(deleted) = self.deleted_synthetics.get_mut(&id) {
                deleted.remove(name);
            }
        }
        if binding {
            self.scopes[0].bindings.get_mut(name).unwrap().value = value;
        }
        Ok(())
    }
    pub(super) fn delete_global_property(
        &mut self,
        name: &str,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        if matches!(name, "undefined" | "NaN" | "Infinity") {
            return Ok(false);
        }
        if intrinsic_global(name).is_none()
            && let Some(global) = self.global_object_handle
            && (host.global_accessor(name)
                || host.has_property(global, name)?
                || host.global(name).is_some())
        {
            return Err(JsError::runtime(
                "Deletion of host properties is not implemented",
            ));
        }
        let id = self.materialize_global_property(name, host)?;
        if !self.delete_property(Value::Object(id), name, host)? {
            return Ok(false);
        }
        if self.scopes[0]
            .bindings
            .get(name)
            .is_some_and(|b| b.global_property)
        {
            self.scopes[0].bindings.remove(name);
            self.binding_count -= 1;
        }
        if intrinsic_global(name).is_some() {
            self.deleted_synthetics
                .entry(id)
                .or_default()
                .insert(name.to_owned());
        }
        Ok(true)
    }
    pub(super) fn global_has_own(
        &mut self,
        name: &str,
        host: &mut impl Host,
    ) -> Result<bool, JsError> {
        if self.global_deleted(name) {
            return Ok(false);
        }
        if self.global_own_property(name).is_some() || intrinsic_global(name).is_some() {
            return Ok(true);
        }
        let id = self
            .global_object_handle
            .ok_or_else(|| JsError::runtime("No global object"))?;
        host.has_property(id, name)
    }
    pub(super) fn initialize_binding_value(
        &mut self,
        scope: usize,
        name: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        // Host accessors keep their existing declaration contract. Only VM
        // properties participate in this descriptor-backed binding path.
        if scope == 0
            && !host.global_accessor(name)
            && self.scopes[0]
                .bindings
                .get(name)
                .is_some_and(|b| b.global_property)
            && let Some(id) = host.global_object()
        {
            self.set(Value::Host(id), name, value, host)?;
        } else {
            self.scopes[scope]
                .bindings
                .get_mut(name)
                .ok_or_else(|| JsError::runtime("Missing binding"))?
                .value = value;
        }
        self.scopes[scope]
            .bindings
            .get_mut(name)
            .ok_or_else(|| JsError::runtime("Missing binding"))?
            .initialized = true;
        Ok(())
    }
    pub(super) fn define_global_function(
        &mut self,
        name: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if host.global_object().is_some()
            && !self.global_deleted(name)
            && (intrinsic_global(name).is_some()
                || self.scopes[0]
                    .bindings
                    .get(name)
                    .is_some_and(|b| b.global_property))
        {
            self.materialize_global_property(name, host)?;
        }
        if self.global_stored(name).is_none() {
            return Ok(());
        }
        let id = self.global_properties.unwrap();
        let descriptor = self
            .own_descriptor(&Value::Object(id), name, host)?
            .unwrap();
        if !descriptor.metadata.configurable
            && (descriptor.metadata.accessor.is_some()
                || !descriptor.metadata.writable
                || !descriptor.metadata.enumerable)
        {
            return Err(self.property_type_error(
                "Cannot declare a function over a restricted global property",
            ));
        }
        self.define_property(
            &Value::Object(id),
            name,
            Definition {
                value: Some(value),
                writable: Some(true),
                enumerable: Some(true),
                configurable: Some(
                    descriptor.metadata.configurable && self.global_declaration_deletable,
                ),
                ..Definition::default()
            },
            host,
        )
    }
}

#[cfg(test)]
mod tests;
