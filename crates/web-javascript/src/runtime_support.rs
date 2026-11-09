//! Bounded property identity, insertion order and array exotic length semantics.
use crate::{Heap, Host, JsError, Runtime, Value};
use std::collections::BTreeSet;

pub(super) fn number_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == f64::INFINITY {
        return "Infinity".into();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".into();
    }
    if value == 0. {
        return "0".into();
    }
    let raw = value.to_string();
    if (1e-6..1e21).contains(&value.abs()) {
        return raw;
    }
    let unsigned = raw.trim_start_matches('-');
    let (mantissa, power) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, 0), |(mantissa, power)| {
            (mantissa, power.parse::<i32>().unwrap_or(0))
        });
    let decimal = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits = mantissa.chars().filter(|ch| *ch != '.').collect::<String>();
    let leading = digits.bytes().take_while(|ch| *ch == b'0').count();
    let significant = digits[leading..].trim_end_matches('0');
    let exponent = decimal - leading as i32 - 1 + power;
    let mut out = String::new();
    if value < 0. {
        out.push('-');
    }
    out.push_str(&significant[..1]);
    if significant.len() > 1 {
        out.push('.');
        out.push_str(&significant[1..]);
    }
    out.push('e');
    if exponent >= 0 {
        out.push('+');
    }
    out.push_str(&exponent.to_string());
    out
}

pub(super) fn ordered_index(key: &str) -> Option<u32> {
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

impl Runtime {
    pub(super) fn remember_property_order(&mut self, id: usize, keys: Vec<String>) {
        let mut seen = BTreeSet::new();
        self.property_order.insert(
            id,
            keys.into_iter()
                .filter(|key| seen.insert(key.clone()))
                .collect(),
        );
    }
    pub(super) fn note_property_insertion(&mut self, id: usize, key: &str) {
        let order = self.property_order.entry(id).or_default();
        if !order.iter().any(|existing| existing == key) {
            order.push(key.to_owned());
        }
    }
    pub(super) fn sort_property_keys(&self, id: usize, keys: &mut [String]) {
        let order = self.property_order.get(&id);
        keys.sort_by(
            |left, right| match (ordered_index(left), ordered_index(right)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => {
                    let rank = |key: &str| {
                        order
                            .and_then(|order| order.iter().position(|candidate| candidate == key))
                            .unwrap_or(usize::MAX)
                    };
                    rank(left).cmp(&rank(right))
                }
            },
        );
    }
    pub(super) fn set_array_length(
        &mut self,
        id: usize,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        if matches!(value, Value::BigInt(_) | Value::Symbol(..)) || self.is_bigint_wrapper(&value) {
            return Err(self.property_type_error(
                "Cannot implicitly convert BigInt or Symbol to array length",
            ));
        }
        if matches!(
            value,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Host(_)
                | Value::Native(_)
        ) {
            return Err(JsError::runtime(
                "Object coercion for array length is not implemented",
            ));
        }
        if matches!(value, Value::BigInt(_) | Value::Symbol(..)) {
            return Err(self.property_type_error(
                "Cannot implicitly convert BigInt or Symbol to array length",
            ));
        }
        let number = self.number(&value);
        if !number.is_finite() || number < 0. || number > u32::MAX as f64 || number.fract() != 0. {
            let mut error = JsError::runtime("Invalid array length");
            error.thrown =
                Some(self.native("RangeError", &[Value::string("Invalid array length")])?);
            return Err(error);
        }
        if number > 4096. {
            return Err(JsError::resource("Array length exceeds 4096"));
        }
        let next = number as usize;
        let old = match self.heap.get(id) {
            Some(Heap::Array(values)) => values.len(),
            _ => return Err(JsError::runtime("Invalid array length receiver")),
        };
        if next == old {
            return Ok(());
        }
        if self
            .property_metadata
            .get(&id)
            .and_then(|map| map.get("length"))
            .is_some_and(|meta| !meta.writable)
        {
            return Err(self.property_type_error("Array length is not writable"));
        }
        if next > old {
            self.charge_array_slots(next - old)?;
            let Heap::Array(values) = &mut self.heap[id] else {
                unreachable!()
            };
            values.resize(next, Value::Undefined);
            self.array_holes.entry(id).or_default().extend(old..next);
        } else {
            for index in (next..old).rev() {
                self.tick()?;
                if !self.delete_property(Value::Array(id), &index.to_string(), host)? {
                    let Heap::Array(values) = &mut self.heap[id] else {
                        unreachable!()
                    };
                    values.truncate(index + 1);
                    if let Some(holes) = self.array_holes.get_mut(&id) {
                        holes.retain(|hole| *hole <= index);
                    }
                    return Err(
                        self.property_type_error("Cannot truncate a non-configurable array index")
                    );
                }
            }
            let Heap::Array(values) = &mut self.heap[id] else {
                unreachable!()
            };
            values.truncate(next);
            if let Some(holes) = self.array_holes.get_mut(&id) {
                holes.retain(|index| *index < next);
            }
        }
        Ok(())
    }
    pub(super) fn symbol_owner(&mut self, object: &Value) -> Result<usize, JsError> {
        self.heap_owner(object)
    }
    pub(super) fn get_symbol(
        &mut self,
        object: Value,
        key: u64,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let receiver = object.clone();
        let mut current = if matches!(object, Value::BigInt(_)) {
            self.reflection_prototype("BigInt")?
        } else {
            object
        };
        for _ in 0..64 {
            self.tick()?;
            if current == Value::Native("Function.prototype") {
                current = self.object_prototype_value()?;
            }
            let id = self.symbol_owner(&current)?;
            if let Some(metadata) = self.symbol_metadata.get(&id).and_then(|map| map.get(&key))
                && let Some((getter, _)) = metadata.accessor.clone()
            {
                return if getter == Value::Undefined {
                    Ok(Value::Undefined)
                } else {
                    self.call(getter, vec![], receiver, host)
                };
            }
            if let Some(value) = self
                .symbol_properties
                .get(&id)
                .and_then(|map| map.get(&key))
            {
                return Ok(value.clone());
            }
            current = match self.heap.get(id) {
                Some(Heap::Object(object)) => object.prototype.clone().unwrap_or(Value::Null),
                Some(Heap::Array(_)) => {
                    if Some(id) == self.array_prototype {
                        self.object_prototype_value()?
                    } else {
                        self.array_prototype_value()?
                    }
                }
                _ => Value::Null,
            };
            if current == Value::Null {
                return Ok(Value::Undefined);
            }
        }
        Err(JsError::resource("Symbol prototype depth exceeded"))
    }
    pub(super) fn has_symbol(
        &mut self,
        object: &Value,
        key: u64,
        _host: &mut impl Host,
    ) -> Result<bool, JsError> {
        let mut current = object.clone();
        for _ in 0..64 {
            self.tick()?;
            if current == Value::Native("Function.prototype") {
                current = self.object_prototype_value()?;
            }
            let id = self.symbol_owner(&current)?;
            if self
                .symbol_properties
                .get(&id)
                .is_some_and(|map| map.contains_key(&key))
            {
                return Ok(true);
            }
            current = match self.heap.get(id) {
                Some(Heap::Object(object)) => object.prototype.clone().unwrap_or(Value::Null),
                Some(Heap::Array(_)) => {
                    if Some(id) == self.array_prototype {
                        self.object_prototype_value()?
                    } else {
                        self.array_prototype_value()?
                    }
                }
                _ => Value::Null,
            };
            if current == Value::Null {
                return Ok(false);
            }
        }
        Err(JsError::resource("Symbol existence depth exceeded"))
    }
    pub(super) fn set_symbol(
        &mut self,
        object: Value,
        key: u64,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let owner = self.symbol_owner(&object)?;
        let mut current = object.clone();
        for _ in 0..64 {
            self.tick()?;
            if current == Value::Native("Function.prototype") {
                current = self.object_prototype_value()?;
            }
            let id = self.symbol_owner(&current)?;
            if let Some(metadata) = self
                .symbol_metadata
                .get(&id)
                .and_then(|map| map.get(&key))
                .cloned()
            {
                if let Some((_, setter)) = metadata.accessor {
                    if setter == Value::Undefined {
                        return if self.strict {
                            Err(self.property_type_error("Symbol property has no setter"))
                        } else {
                            Ok(())
                        };
                    }
                    self.call(setter, vec![value], object, host)?;
                    return Ok(());
                }
                if !metadata.writable {
                    return if self.strict {
                        Err(self.property_type_error("Symbol property is not writable"))
                    } else {
                        Ok(())
                    };
                }
                break;
            }
            if self
                .symbol_properties
                .get(&id)
                .is_some_and(|map| map.contains_key(&key))
            {
                break;
            }
            current = match self.heap.get(id) {
                Some(Heap::Object(object)) => object.prototype.clone().unwrap_or(Value::Null),
                Some(Heap::Array(_)) => {
                    if Some(id) == self.array_prototype {
                        self.object_prototype_value()?
                    } else {
                        self.array_prototype_value()?
                    }
                }
                _ => Value::Null,
            };
            if current == Value::Null {
                break;
            }
        }
        if self.reflection.non_extensible.contains(&owner)
            && !self
                .symbol_properties
                .get(&owner)
                .is_some_and(|map| map.contains_key(&key))
        {
            return if self.strict {
                Err(self.property_type_error("Object is not extensible"))
            } else {
                Ok(())
            };
        }
        let properties = self.symbol_properties.entry(owner).or_default();
        if properties.len() >= 128 && !properties.contains_key(&key) {
            return Err(JsError::resource("Symbol property limit exceeded"));
        }
        if !properties.contains_key(&key) {
            self.symbol_order.entry(owner).or_default().push(key);
        }
        properties.insert(key, value);
        Ok(())
    }
    pub(super) fn delete_symbol(
        &mut self,
        object: Value,
        key: u64,
        _host: &mut impl Host,
    ) -> Result<bool, JsError> {
        let id = self.symbol_owner(&object)?;
        if self
            .symbol_metadata
            .get(&id)
            .and_then(|map| map.get(&key))
            .is_some_and(|metadata| !metadata.configurable)
        {
            return Ok(false);
        }
        if let Some(properties) = self.symbol_properties.get_mut(&id) {
            properties.remove(&key);
        }
        if let Some(metadata) = self.symbol_metadata.get_mut(&id) {
            metadata.remove(&key);
        }
        if let Some(order) = self.symbol_order.get_mut(&id) {
            order.retain(|symbol| *symbol != key);
        }
        Ok(true)
    }
}
