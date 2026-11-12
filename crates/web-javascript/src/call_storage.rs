//! Argument buffers share the cumulative Value-slot budget with arrays.
//! Moving a buffer is free; each newly materialized slot is reserved before
//! allocation/copy, and neither collection nor rollback refunds reservations.
use crate::{Function, JsError, Runtime, Value};

pub(super) const MAX_FUNCTION_PARAMETERS: usize = 64;
pub(super) const MAX_CALL_ARGUMENTS: usize = 64;

pub(super) fn check_arguments(length: usize) -> Result<(), JsError> {
    if length > MAX_CALL_ARGUMENTS {
        Err(JsError::resource("Call argument limit exceeded"))
    } else {
        Ok(())
    }
}

impl Runtime {
    pub(super) fn value_copy_work(&mut self, cells: usize) -> Result<(), JsError> {
        self.remaining = self
            .remaining
            .checked_sub(cells as u64)
            .ok_or_else(|| JsError::resource("Argument copy work budget exceeded"))?;
        Ok(())
    }

    pub(super) fn value_buffer(&mut self, length: usize) -> Result<Vec<Value>, JsError> {
        self.charge_array_slots(length)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(length)
            .map_err(|_| JsError::resource("Argument storage unavailable"))?;
        Ok(values)
    }

    pub(super) fn reserve_argument_slot(&mut self, values: &mut Vec<Value>) -> Result<(), JsError> {
        check_arguments(values.len().saturating_add(1))?;
        self.reserve_value_slot(values, MAX_CALL_ARGUMENTS)
    }

    pub(super) fn reserve_value_slot(
        &mut self,
        values: &mut Vec<Value>,
        maximum: usize,
    ) -> Result<(), JsError> {
        if values.len() >= maximum {
            return Err(JsError::resource("Value buffer width exceeded"));
        }
        if values.len() < values.capacity() {
            return Ok(());
        }
        // Unknown-width spread grows geometrically, never beyond the call cap.
        // Charge requested capacity, not pushes into an already reserved buffer.
        let capacity = values.capacity();
        let next = capacity.saturating_mul(2).max(4).min(maximum);
        self.check_array_slots(next - capacity)?;
        self.value_copy_work(capacity)?;
        self.charge_array_slots(next - capacity)?;
        values
            .try_reserve_exact(next - values.len())
            .map_err(|_| JsError::resource("Argument storage unavailable"))
    }

    pub(super) fn copy_argument_values(&mut self, values: &[Value]) -> Result<Vec<Value>, JsError> {
        check_arguments(values.len())?;
        let mut copy = self.value_buffer(values.len())?;
        copy.extend_from_slice(values);
        Ok(copy)
    }

    pub(super) fn preflight_binding_storage(
        &self,
        count: usize,
        bytes: usize,
    ) -> Result<(), JsError> {
        if self.binding_count.saturating_add(count) > 16_384 {
            return Err(JsError::resource(
                "JavaScript total binding budget exceeded",
            ));
        }
        if self.string_bytes.saturating_add(bytes) > self.limits.string_bytes {
            return Err(JsError::resource(
                "JavaScript string allocation budget exceeded",
            ));
        }
        Ok(())
    }

    /// Inspect borrowed patterns before allocating an activation or copying names.
    pub(super) fn preflight_function_bindings(
        &self,
        function: &Function,
        id: usize,
    ) -> Result<bool, JsError> {
        let parameter_count = function
            .parameters
            .as_ref()
            .map_or(function.params.len(), |p| p.len());
        if parameter_count > MAX_FUNCTION_PARAMETERS {
            return Err(JsError::resource("Function parameter limit exceeded"));
        }
        let mut count = 1usize;
        let mut bytes = "this".len();
        let mut shadows_arguments = false;
        let mut visit = |name: &str| {
            count = count
                .checked_add(1)
                .ok_or_else(|| JsError::resource("Binding count overflow"))?;
            if count > 1024 {
                return Err(JsError::resource("Bindings per scope exceed 1024"));
            }
            bytes = bytes
                .checked_add(name.len())
                .ok_or_else(|| JsError::resource("Binding byte budget overflow"))?;
            shadows_arguments |= name == "arguments";
            Ok(())
        };
        if let Some(parameters) = &function.parameters {
            for parameter in parameters.iter() {
                let mut pattern_count = 0usize;
                parameter.pattern.visit_names(&mut |name| {
                    pattern_count += 1;
                    if pattern_count > 128 {
                        return Err(JsError::resource("Pattern binding limit exceeded"));
                    }
                    visit(name)
                })?;
            }
        } else {
            for name in function.params.iter() {
                visit(name)?;
            }
        }
        if !function.arrow && !shadows_arguments {
            count += 1;
            bytes += "arguments".len();
        }
        if self.language.methods.contains_key(&id) {
            count += 1;
            bytes += "[[home]]".len();
        }
        if count > 1024 {
            return Err(JsError::resource("Bindings per scope exceed 1024"));
        }
        self.preflight_binding_storage(count, bytes)?;
        Ok(shadows_arguments)
    }
}

#[cfg(test)]
mod tests;
