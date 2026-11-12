//! Cumulative string-keyed VM property reservations, independent of GC.
//! A reservation covers a value slot plus four persistent copies of its key:
//! value storage, insertion order, descriptors and non-enumerable metadata.
use crate::{Heap, JsError, Runtime, Value};
use std::collections::BTreeMap;

pub(super) const MAX_NAMED_PROPERTIES: usize = 4096;
const MAX_PROPERTY_RESERVATIONS: usize = 65_536;
const MAX_PROPERTY_KEY_BYTES: usize = 8 * 1024 * 1024;
const MAX_PROPERTY_WORK: usize = 4 * 1024 * 1024;
const KEY_COPIES: usize = 4;

#[derive(Clone, Debug, Default)]
pub(super) struct PropertyBudget {
    reservations: usize,
    key_bytes: usize,
    work: usize,
}
impl PropertyBudget {
    pub(super) fn preserve(&mut self, consumed: &Self) {
        self.reservations = self.reservations.max(consumed.reservations);
        self.key_bytes = self.key_bytes.max(consumed.key_bytes);
        self.work = self.work.max(consumed.work);
    }
}

impl Runtime {
    pub(super) fn property_work(&mut self, work: usize) -> Result<(), JsError> {
        let total = self
            .property_budget
            .work
            .checked_add(work)
            .filter(|total| *total <= MAX_PROPERTY_WORK)
            .ok_or_else(|| {
                JsError::resource("JavaScript aggregate property work budget exceeded")
            })?;
        let remaining = self
            .remaining
            .checked_sub(work as u64)
            .ok_or_else(|| JsError::resource("Property operation instruction budget exceeded"))?;
        self.property_budget.work = total;
        self.remaining = remaining;
        Ok(())
    }
    fn reserve_property_keys(&mut self, count: usize, bytes: usize) -> Result<(), JsError> {
        let reservations = self
            .property_budget
            .reservations
            .checked_add(count)
            .filter(|total| *total <= MAX_PROPERTY_RESERVATIONS)
            .ok_or_else(|| {
                JsError::resource("JavaScript aggregate property count budget exceeded")
            })?;
        let key_bytes = bytes
            .checked_mul(KEY_COPIES)
            .and_then(|bytes| self.property_budget.key_bytes.checked_add(bytes))
            .filter(|total| *total <= MAX_PROPERTY_KEY_BYTES)
            .ok_or_else(|| {
                JsError::resource("JavaScript aggregate property key byte budget exceeded")
            })?;
        self.property_budget.reservations = reservations;
        self.property_budget.key_bytes = key_bytes;
        Ok(())
    }
    pub(super) fn check_named_property_count(count: usize) -> Result<(), JsError> {
        if count > MAX_NAMED_PROPERTIES {
            return Err(JsError::resource("Object property limit exceeded"));
        }
        Ok(())
    }
    pub(super) fn reserve_property_builder_key(
        &mut self,
        count: usize,
        key: &str,
    ) -> Result<(), JsError> {
        if key.len() > 1024 {
            return Err(JsError::resource("Property name exceeds limit"));
        }
        Self::check_named_property_count(count.saturating_add(1))?;
        self.property_work(
            key.len()
                .saturating_add(1 + count.checked_ilog2().unwrap_or(0) as usize),
        )?;
        self.reserve_property_keys(1, key.len())
    }
    pub(super) fn reserve_property_map(
        &mut self,
        properties: &BTreeMap<String, Value>,
    ) -> Result<(), JsError> {
        Self::check_named_property_count(properties.len())?;
        let mut bytes = 0usize;
        for key in properties.keys() {
            if key.len() > 1024 {
                return Err(JsError::resource("Property name exceeds limit"));
            }
            bytes = bytes
                .checked_add(key.len())
                .ok_or_else(|| JsError::resource("Property key byte budget overflow"))?;
        }
        self.property_work(
            bytes.saturating_add(
                properties
                    .len()
                    .saturating_mul(1 + properties.len().checked_ilog2().unwrap_or(0) as usize),
            ),
        )?;
        self.reserve_property_keys(properties.len(), bytes)
    }
    pub(super) fn reserve_named_property(&mut self, id: usize, key: &str) -> Result<bool, JsError> {
        if key.len() > 1024 {
            return Err(JsError::resource("Property name exceeds limit"));
        }
        let count = match self.heap.get(id) {
            Some(Heap::Object(object)) => object.properties.len(),
            Some(Heap::Function(function)) => function.properties.len(),
            Some(Heap::Array(_)) => self.array_properties.get(&id).map_or(0, BTreeMap::len),
            _ => return Err(JsError::runtime("Invalid property owner")),
        };
        self.property_work(
            key.len()
                .saturating_add(1 + count.checked_ilog2().unwrap_or(0) as usize),
        )?;
        let exists = match &self.heap[id] {
            Heap::Object(object) => object.properties.contains_key(key),
            Heap::Function(function) => function.properties.contains_key(key),
            Heap::Array(_) => self
                .array_properties
                .get(&id)
                .is_some_and(|map| map.contains_key(key)),
            Heap::Vacant => unreachable!(),
        };
        if exists {
            return Ok(false);
        }
        let restricted = self.reflection.number_constructor == Some(id)
            || self.reflection.symbol_constructor == Some(id)
            || self.stdlib_property_owner(id)
            || matches!(&self.heap[id], Heap::Object(object) if object.regexp.is_some());
        let limit = if restricted {
            128
        } else {
            MAX_NAMED_PROPERTIES
        };
        if count >= limit {
            return Err(JsError::resource("Object property limit exceeded"));
        }
        self.reserve_property_keys(1, key.len())?;
        Ok(true)
    }
    // for-in prepares several key snapshots. Refuse their size and work before
    // cloning names, and account for the temporary key copies as string storage.
    pub(super) fn preflight_property_enumeration(
        &mut self,
        id: usize,
        prior_bytes: usize,
    ) -> Result<(), JsError> {
        let scan = match self.heap.get(id) {
            Some(Heap::Object(object)) => object.properties.len(),
            Some(Heap::Function(function)) => function.properties.len() + 2,
            Some(Heap::Array(values)) => {
                values.len() + self.array_properties.get(&id).map_or(0, BTreeMap::len)
            }
            _ => return Err(JsError::runtime("Invalid property owner")),
        };
        self.property_work(scan)?;
        let (count, bytes) = match &self.heap[id] {
            Heap::Object(object) => (
                object.properties.len(),
                object.properties.keys().map(String::len).sum::<usize>(),
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
                let mut count = 0;
                let mut bytes = 0;
                for index in 0..values.len() {
                    if self.array_has_index(id, index, values.len()) {
                        count += 1;
                        bytes += 1 + index.checked_ilog10().unwrap_or(0) as usize;
                    }
                }
                if let Some(properties) = self.array_properties.get(&id) {
                    count += properties.len();
                    bytes += properties.keys().map(String::len).sum::<usize>();
                }
                (count, bytes)
            }
            Heap::Vacant => unreachable!(),
        };
        if count > 4096 || prior_bytes.saturating_add(bytes) > 65_536 {
            return Err(JsError::resource("Enumeration key budget exceeded"));
        }
        self.property_work(
            bytes
                .saturating_add(count)
                .saturating_mul(1 + count.checked_ilog2().unwrap_or(0) as usize),
        )?;
        self.charge_string(bytes.saturating_mul(3))
    }

    // Ordinary names include descriptor storage in their reservation. Array
    // indices and length have no stored string key and reserve separately.
    pub(super) fn reserve_property_metadata(
        &mut self,
        id: usize,
        key: &str,
    ) -> Result<(), JsError> {
        if self
            .property_metadata
            .get(&id)
            .is_some_and(|map| map.contains_key(key))
        {
            return Ok(());
        }
        let named = match self.heap.get(id) {
            Some(Heap::Object(object)) => object.properties.contains_key(key),
            Some(Heap::Function(function)) => function.properties.contains_key(key),
            Some(Heap::Array(_)) => self
                .array_properties
                .get(&id)
                .is_some_and(|map| map.contains_key(key)),
            _ => false,
        };
        if !named
            && matches!(self.heap.get(id), Some(Heap::Array(_)))
            && (key == "length" || crate::properties::array_index(key).is_some())
        {
            let count = self.property_metadata.get(&id).map_or(0, BTreeMap::len);
            // Indexed slots/length use the array storage ceiling, not the named
            // property cap. Their metadata still spends the aggregate quotas.
            self.property_work(
                key.len()
                    .saturating_add(1 + count.checked_ilog2().unwrap_or(0) as usize),
            )?;
            self.reserve_property_keys(1, key.len())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
