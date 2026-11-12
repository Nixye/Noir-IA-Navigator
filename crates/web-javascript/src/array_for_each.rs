//! Generic forEach, shared by Array and host NodeLists. No collection snapshot.
use crate::{Host, JsError, Runtime, Value};

impl Runtime {
    pub(super) fn array_for_each(
        &mut self,
        receiver: Value,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
        let mut roots = Vec::new();
        let result = (|| {
            for value in [&receiver, &callback, &this_arg] {
                roots.push(self.retain_value(value.clone())?);
            }
            if matches!(receiver, Value::Null | Value::Undefined) {
                return Err(self.property_type_error("forEach requires a non-null receiver"));
            }
            // Preserve the VM's existing primitive boxing limits (including
            // bounded String wrappers and unsupported boxed Symbols).
            let object = self.box_value(receiver)?;
            roots.push(self.retain_value(object.clone())?);
            let length = self.get(object.clone(), "length", host)?;
            let length_root = self.retain_value(length.clone())?;
            let converted = self.coerce_to_number(length, host);
            self.release_value(length_root);
            let length = converted?;
            let length = if length.is_nan() || length <= 0. {
                0
            } else {
                length.trunc().min(9_007_199_254_740_991.) as u64
            };
            // Length getters/conversion are observable even for a bad callback.
            if !self.is_callable_value(&callback, host) {
                return Err(self.property_type_error("forEach callback must be callable"));
            }
            for index in 0..length {
                // Sparse and huge array-likes cannot bypass the existing VM
                // instruction allowance. No length-proportional allocation.
                self.tick()?;
                let key = index.to_string();
                if !self.has_property(&object, &key, host)? {
                    continue;
                }
                let value = self.get(object.clone(), &key, host)?;
                self.call(
                    callback.clone(),
                    vec![value, Value::Number(index as f64), object.clone()],
                    this_arg.clone(),
                    host,
                )?;
            }
            Ok(Value::Undefined)
        })();
        for root in roots {
            self.release_value(root);
        }
        result
    }
}

#[cfg(test)]
mod tests;
