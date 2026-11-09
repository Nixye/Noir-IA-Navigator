//! Bounded own properties of the Number intrinsic function.
//! Only Number and Symbol opt in; arbitrary native/Host handles stay opaque.
use crate::{Heap, JsError, Runtime, Value, descriptors::PropertyMetadata};

impl Runtime {
    pub(super) fn intrinsic_constructor_owner(&mut self, name: &str) -> Result<usize, JsError> {
        match name {
            "Symbol" => self.symbol_constructor_owner(),
            "Number" => self.number_constructor_owner(),
            _ => Err(JsError::runtime("Unsupported intrinsic property storage")),
        }
    }

    pub(super) fn intrinsic_constructor_storage(&mut self, name: &str) -> Result<Value, JsError> {
        let id = self.intrinsic_constructor_owner(name)?;
        debug_assert!(matches!(self.heap.get(id), Some(Heap::Object(_))));
        Ok(Value::Object(id))
    }

    // Keep the callable identity Native("Number"). Never expose the backing
    // object to page code. Seed once, so deleted properties cannot reappear.
    fn number_constructor_owner(&mut self) -> Result<usize, JsError> {
        if let Some(id) = self.reflection.number_constructor {
            return Ok(id);
        }
        let prototype = self.reflection_prototype("Number")?;
        let name = self.string("Number".into())?;
        let mut entries = vec![
            ("length", Value::Number(1.), false, true),
            ("name", name, false, true),
            ("prototype", prototype, false, false),
        ];
        for (name, value) in [
            ("EPSILON", f64::EPSILON),
            ("MAX_SAFE_INTEGER", 9_007_199_254_740_991.),
            ("MAX_VALUE", f64::MAX),
            ("MIN_SAFE_INTEGER", -9_007_199_254_740_991.),
            ("MIN_VALUE", f64::from_bits(1)),
            ("NaN", f64::NAN),
            ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
            ("POSITIVE_INFINITY", f64::INFINITY),
        ] {
            entries.push((name, Value::Number(value), false, false));
        }
        for name in [
            "isFinite",
            "isInteger",
            "isNaN",
            "isSafeInteger",
            "parseFloat",
            "parseInt",
        ] {
            entries.push((
                name,
                crate::numeric_globals::number_property(name).expect("known Number method"),
                true,
                true,
            ));
        }
        self.charge_string(entries.iter().map(|(key, _, _, _)| key.len() * 3).sum())?;
        let object = self.object(
            entries
                .iter()
                .map(|(key, value, _, _)| ((*key).into(), value.clone()))
                .collect(),
            Some(Value::Native("Function.prototype")),
        )?;
        let Value::Object(id) = object else {
            unreachable!()
        };
        self.remember_property_order(
            id,
            entries.iter().map(|(key, _, _, _)| (*key).into()).collect(),
        );
        self.property_metadata.insert(
            id,
            entries
                .into_iter()
                .map(|(key, _, writable, configurable)| {
                    (
                        key.into(),
                        PropertyMetadata {
                            writable,
                            enumerable: false,
                            configurable,
                            accessor: None,
                        },
                    )
                })
                .collect(),
        );
        self.reflection.number_constructor = Some(id);
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
