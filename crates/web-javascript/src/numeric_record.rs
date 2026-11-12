//! Narrow embedder bridge for fresh numeric geometry snapshots.
use crate::{JsError, Runtime, Value};
use std::collections::BTreeMap;
const FIELDS: [&str; 8] = [
    "x", "y", "width", "height", "left", "top", "right", "bottom",
];
impl Runtime {
    /// Eight own writable data properties. Never invokes page setters or JS.
    /// Uses ordinary VM object/property quotas, not a separate host handle pool.
    pub fn numeric_record(&mut self, fields: &[(&str, f64)]) -> Result<Value, JsError> {
        if fields.len() != FIELDS.len()
            || fields
                .iter()
                .zip(FIELDS)
                .any(|((key, value), expected)| *key != expected || !value.is_finite())
        {
            return Err(JsError::runtime("Invalid numeric geometry record"));
        }
        let prototype = self.object_prototype_value()?;
        let mut properties = BTreeMap::new();
        for (key, value) in fields {
            self.reserve_property_builder_key(properties.len(), key)?;
            properties.insert((*key).to_owned(), Value::Number(*value));
        }
        self.object_reserved(properties, Some(prototype))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fields(value: f64) -> [(&'static str, f64); 8] {
        FIELDS.map(|key| (key, value))
    }
    #[test]
    fn snapshots_are_distinct_ordinary_own_data_records() {
        let mut vm = Runtime::default();
        let a = vm.numeric_record(&fields(1.)).unwrap();
        let b = vm.numeric_record(&fields(2.)).unwrap();
        assert_ne!(a, b);
        let Value::Object(id) = a else {
            panic!("ordinary object required")
        };
        let crate::Heap::Object(object) = &vm.heap[id] else {
            panic!("ordinary object required")
        };
        assert_eq!(object.properties.len(), 8);
        assert_eq!(object.properties["left"], Value::Number(1.));
        assert!(object.prototype.is_some());
        assert!(object.non_enumerable.is_empty());
    }
    #[test]
    fn invalid_shape_names_and_nonfinite_values_do_not_allocate() {
        let mut vm = Runtime::default();
        let before = vm.heap.len();
        assert!(vm.numeric_record(&[]).is_err());
        assert!(vm.numeric_record(&fields(f64::NAN)).is_err());
        let mut wrong = fields(1.);
        wrong[0].0 = "__proto__";
        assert!(vm.numeric_record(&wrong).is_err());
        assert_eq!(vm.heap.len(), before);
    }
    #[test]
    fn cumulative_existing_work_quota_bounds_repeated_records() {
        let mut vm = Runtime::default();
        let mut failed = false;
        for _ in 0..100_000 {
            if let Err(e) = vm.numeric_record(&fields(1.)) {
                assert_eq!(e.kind, crate::ErrorKind::Resource);
                failed = true;
                break;
            }
        }
        assert!(failed);
        assert!(vm.gc_roots.is_empty());
    }
    #[test]
    fn numeric_snapshot_heap_contents_restore_on_vm_rollback() {
        let mut vm = Runtime::default();
        let snapshot = vm.numeric_record(&fields(1.)).unwrap();
        let previous = vm.clone();
        let Value::Object(id) = snapshot else {
            panic!("ordinary snapshot")
        };
        let crate::Heap::Object(object) = &mut vm.heap[id] else {
            panic!("ordinary snapshot")
        };
        object.properties.insert("x".into(), Value::Number(999.));
        vm.rollback_to(previous);
        let crate::Heap::Object(object) = &vm.heap[id] else {
            panic!("ordinary snapshot")
        };
        assert_eq!(object.properties["x"], Value::Number(1.));
    }
}
