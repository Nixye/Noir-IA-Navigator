//! Read-only ancestry of the explicitly supported Error constructors.
//! This inventory is not an owner, a public descriptor API or mutation authority.
use crate::{
    JsError, Runtime, Value,
    descriptors::{Descriptor, PropertyMetadata},
};

const ERROR_CONSTRUCTOR_NAMES: &[&str] = &["length", "name", "prototype"];

pub(crate) fn error_ancestor(value: &Value) -> Option<&'static str> {
    match value {
        Value::Native(name) => super::error_constructor(name),
        _ => None,
    }
}

pub(crate) fn readonly_ancestor_names(value: &Value) -> Option<&'static [&'static str]> {
    if error_ancestor(value).is_some() {
        Some(ERROR_CONSTRUCTOR_NAMES)
    } else if *value == Value::Native("Function.prototype") {
        Some(crate::native_symbol::FUNCTION_PROTOTYPE_NAMES)
    } else {
        None
    }
}

impl Runtime {
    pub(crate) fn readonly_ancestor_parent(
        &mut self,
        value: &Value,
    ) -> Result<Option<Value>, JsError> {
        if let Some(name) = error_ancestor(value) {
            return Ok(Some(Value::Native(if name == "Error" {
                "Function.prototype"
            } else {
                "Error"
            })));
        }
        if *value == Value::Native("Function.prototype") {
            return self.object_prototype_value().map(Some);
        }
        Ok(None)
    }

    /// The metadata-only path never allocates a name or materializes a public
    /// Error.prototype. Existence, assignment checks and enumeration use it.
    pub(crate) fn readonly_ancestor_descriptor(
        &mut self,
        object: &Value,
        key: &str,
        read_value: bool,
    ) -> Result<Option<Descriptor>, JsError> {
        let Some(names) = readonly_ancestor_names(object) else {
            return Ok(None);
        };
        if !names.contains(&key) {
            return Ok(None);
        }
        let (value, writable, configurable) = if let Some(name) = error_ancestor(object) {
            let value = if read_value {
                match key {
                    "length" => Value::Number(1.),
                    "name" => self.string(name.into())?,
                    "prototype" => self.reflection_prototype(name)?,
                    _ => unreachable!(),
                }
            } else {
                Value::Undefined
            };
            (value, false, key != "prototype")
        } else {
            let value = if read_value {
                self.function_prototype_descriptor(key)?.unwrap().value
            } else {
                Value::Undefined
            };
            (value, !matches!(key, "length" | "name"), true)
        };
        Ok(Some(Descriptor {
            value,
            metadata: PropertyMetadata {
                writable,
                enumerable: false,
                configurable,
                accessor: None,
            },
        }))
    }
}
