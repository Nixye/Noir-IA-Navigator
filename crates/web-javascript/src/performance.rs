//! Document-scoped, host-supplied monotonic timing. No OS access in the VM.
use crate::{Host, JsError, Runtime, Value, descriptors::PropertyMetadata};
use std::collections::BTreeMap;

impl Runtime {
    pub(super) fn performance_namespace(&mut self, host: &impl Host) -> Result<Value, JsError> {
        if let Some(value) = self.reflection.prototypes.get("Performance namespace") {
            return Ok(value.clone());
        }
        let origin = host.performance_time_origin()?;
        if !origin.is_finite() || origin < 0.0 {
            return Err(JsError::runtime("Invalid host performance time origin"));
        }
        // This bounded subset stores timeOrigin as a readonly own data property.
        // A full WebIDL Performance prototype/EventTarget is not implemented.
        let value = self.ordinary_object(BTreeMap::from([
            ("now".into(), Value::Native("Performance.now")),
            ("timeOrigin".into(), Value::Number(origin)),
        ]))?;
        let id = self.heap_owner(&value)?;
        for (key, writable, configurable) in [("now", true, true), ("timeOrigin", false, false)] {
            self.property_metadata.entry(id).or_default().insert(
                key.into(),
                PropertyMetadata {
                    writable,
                    enumerable: false,
                    configurable,
                    accessor: None,
                },
            );
        }
        self.reflection
            .prototypes
            .insert("Performance namespace", value.clone());
        Ok(value)
    }

    pub(super) fn performance_now(
        &mut self,
        this: &Value,
        host: &impl Host,
    ) -> Result<Value, JsError> {
        if self.reflection.prototypes.get("Performance namespace") != Some(this) {
            return Err(self.property_type_error("Invalid performance.now receiver"));
        }
        let millis = host.performance_now()?;
        if !millis.is_finite() || millis < 0.0 {
            return Err(JsError::runtime("Invalid host monotonic time"));
        }
        Ok(Value::Number(millis))
    }
}
