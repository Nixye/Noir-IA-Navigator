//! Bounded weak references to VM-managed heap objects.
//!
//! Collection only happens at public execution entry points or an explicit idle
//! collect_garbage call. No allocation or microtask checkpoint collects, and
//! active call/evaluation/dynamic frames reject collection, including host
//! re-entry. Thus a constructed/dereferenced target stays alive throughout the
//! current execution and its microtask checkpoint without a kept-alive table.
//! An explicit idle collection is a job boundary; embedders must retain values
//! that cross it. If intra-job GC is introduced, add bounded kept-alive roots
//! and clear them at job completion before enabling that collection path.
use crate::{Heap, JsError, Runtime, Value, descriptors::PropertyMetadata};
use std::collections::BTreeMap;

const MAX_WEAK_REFS: usize = 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct WeakRefState {
    // These targets must never be visited by the strong-edge tracer.
    records: BTreeMap<usize, Option<Value>>,
    prototype: Option<usize>,
}

pub(super) fn global(name: &str) -> Option<Value> {
    (name == "WeakRef").then_some(Value::Native("WeakRef"))
}

impl Runtime {
    fn weak_ref_prototype(&mut self) -> Result<Value, JsError> {
        if let Some(id) = self.weak_refs.prototype {
            return Ok(Value::Object(id));
        }
        let Value::Symbol(tag_key, _) = self.well_known_symbol("toStringTag")? else {
            unreachable!()
        };
        let tag = self.string("WeakRef".into())?;
        let prototype = self.ordinary_object(BTreeMap::from([
            ("constructor".into(), Value::Native("WeakRef")),
            ("deref".into(), Value::Native("WeakRef.deref")),
        ]))?;
        self.hide_properties(&prototype, &["constructor", "deref"]);
        let Value::Object(id) = prototype else {
            unreachable!()
        };
        self.symbol_properties
            .entry(id)
            .or_default()
            .insert(tag_key, tag);
        self.symbol_order.entry(id).or_default().push(tag_key);
        self.symbol_metadata.entry(id).or_default().insert(
            tag_key,
            PropertyMetadata {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: None,
            },
        );
        self.weak_refs.prototype = Some(id);
        Ok(prototype)
    }

    pub(super) fn weak_ref_get(
        &mut self,
        object: &Value,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if *object == Value::Native("WeakRef") && property == "prototype" {
            return self.weak_ref_prototype().map(Some);
        }
        Ok(None)
    }

    fn weak_ref_target_is_live(&self, value: &Value) -> bool {
        match value {
            Value::Object(id) => matches!(self.heap.get(*id), Some(Heap::Object(_))),
            Value::Array(id) => matches!(self.heap.get(*id), Some(Heap::Array(_))),
            Value::Function(id) => matches!(self.heap.get(*id), Some(Heap::Function(_))),
            // Symbols (including unregistered symbols), hosts and native
            // functions do not have independently collectible VM identities.
            _ => false,
        }
    }

    pub(super) fn weak_ref_call(
        &mut self,
        name: &str,
        args: &[Value],
        this: Value,
        construct: bool,
    ) -> Result<Option<Value>, JsError> {
        match name {
            "WeakRef" => {
                if !construct {
                    return Err(self.property_type_error("WeakRef requires new"));
                }
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                if !self.weak_ref_target_is_live(&target) {
                    return Err(self
                        .property_type_error("WeakRef target must be a supported VM heap object"));
                }
                if self.weak_refs.records.len() >= MAX_WEAK_REFS {
                    return Err(JsError::resource("WeakRef instance limit exceeded"));
                }
                self.tick()?;
                let prototype = self.weak_ref_prototype()?;
                let object = self.object(BTreeMap::new(), Some(prototype))?;
                let Value::Object(id) = object else {
                    unreachable!()
                };
                self.weak_refs.records.insert(id, Some(target));
                Ok(Some(object))
            }
            "WeakRef.deref" => {
                if construct {
                    return Err(self.property_type_error("WeakRef.deref is not a constructor"));
                }
                self.tick()?;
                let target = match this {
                    Value::Object(id) if matches!(self.heap.get(id), Some(Heap::Object(_))) => {
                        self.weak_refs.records.get(&id).cloned()
                    }
                    _ => None,
                };
                let Some(target) = target else {
                    return Err(
                        self.property_type_error("WeakRef.deref requires a WeakRef receiver")
                    );
                };
                Ok(Some(target.unwrap_or(Value::Undefined)))
            }
            _ => Ok(None),
        }
    }

    pub(super) fn weak_ref_cached_roots(&self) -> Vec<Value> {
        self.weak_refs
            .prototype
            .map(Value::Object)
            .into_iter()
            .collect()
    }

    /// Must run after successful marking and BEFORE heap IDs enter the free list.
    pub(super) fn weak_ref_sweep(&mut self, live: &[bool]) {
        self.weak_refs.records.retain(|id, target| {
            if live.get(*id) != Some(&true) {
                return false;
            }
            let target_live = match target.as_ref() {
                Some(Value::Object(id) | Value::Array(id) | Value::Function(id)) => {
                    live.get(*id) == Some(&true)
                }
                _ => false,
            };
            if !target_live {
                *target = None;
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorKind, Host, Limits};

    struct Empty;
    impl Host for Empty {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Ok(())
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
    }

    fn make_ref(vm: &mut Runtime, target: Value) -> Value {
        vm.weak_ref_call("WeakRef", &[target], Value::Undefined, true)
            .unwrap()
            .unwrap()
    }
    fn deref(vm: &mut Runtime, object: Value) -> Value {
        vm.weak_ref_call("WeakRef.deref", &[], object, false)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn collected_target_cannot_observe_recycled_heap_identity() {
        let mut vm = Runtime::default();
        let target = vm.object(BTreeMap::new(), None).unwrap();
        let weak = make_ref(&mut vm, target.clone());
        vm.collect_garbage(std::slice::from_ref(&weak), &Empty)
            .unwrap();
        assert_eq!(deref(&mut vm, weak.clone()), Value::Undefined);
        let replacement = vm.object(BTreeMap::new(), None).unwrap();
        assert_eq!(
            replacement, target,
            "exercise actual reuse of the collected ID"
        );
        assert_eq!(deref(&mut vm, weak), Value::Undefined);
    }

    #[test]
    fn strong_roots_and_ordinary_properties_keep_targets_alive() {
        let mut vm = Runtime::default();
        let target = vm.object(BTreeMap::new(), None).unwrap();
        let weak = make_ref(&mut vm, target.clone());
        let token = vm.retain_value(target.clone()).unwrap();
        vm.collect_garbage(std::slice::from_ref(&weak), &Empty)
            .unwrap();
        assert_eq!(deref(&mut vm, weak.clone()), target);
        vm.release_value(token);
        vm.set(weak.clone(), "strong", target.clone(), &mut Empty)
            .unwrap();
        vm.collect_garbage(std::slice::from_ref(&weak), &Empty)
            .unwrap();
        assert_eq!(deref(&mut vm, weak), target);
    }

    #[test]
    fn receiver_validation_and_supported_heap_kinds() {
        for source in [
            "'prototype' in WeakRef && new WeakRef({}) instanceof WeakRef;",
            "const t={};const w=new WeakRef(t);w.x=3;w.deref()===t&&w.x===3;",
            "const t=[];new WeakRef(t).deref()===t;",
            "const t=function(){};new WeakRef(t).deref()===t;",
            "const t=Object(1n);new WeakRef(t).deref()===t;",
            "const w=new WeakRef({});new WeakRef(w).deref()===w;",
            "const t={};const w=new WeakRef(t);const d=w.deref;d.call(w)===t;",
            "WeakRef.length===1&&WeakRef.prototype.deref.length===0&&WeakRef.prototype.constructor===WeakRef;",
            "const d=Object.getOwnPropertyDescriptor(WeakRef.prototype,Symbol.toStringTag);d.value==='WeakRef'&&!d.writable&&!d.enumerable&&d.configurable;",
            "Object.getPrototypeOf(WeakRef.prototype)===Object.prototype;",
            "String(new WeakRef({}))==='[object WeakRef]';",
            "Object.prototype.toString.call(new WeakRef({}))==='[object WeakRef]';",
            "let ok=false;try{WeakRef({});}catch(e){ok=e.name==='TypeError';}ok;",
            "let ok=false;try{WeakRef.prototype.deref.call({});}catch(e){ok=e.name==='TypeError';}ok;",
            "let ok=false;try{WeakRef.prototype.deref();}catch(e){ok=e.name==='TypeError';}ok;",
            "let ok=false;try{const d=(new WeakRef({})).deref;d();}catch(e){ok=e.name==='TypeError';}ok;",
            "let ok=false;try{new WeakRef.prototype.deref();}catch(e){ok=e.name==='TypeError';}ok;",
            // Targets survive the whole job, including the microtask checkpoint.
            "let w=new WeakRef({});let ok=false;queueMicrotask(()=>{ok=w.deref()!==undefined;});",
        ] {
            let mut vm = Runtime::default();
            let value = vm.execute(source, &mut Empty).unwrap();
            if source.starts_with("let w=") {
                assert_eq!(vm.global_value("ok"), Some(Value::Bool(true)));
            } else {
                assert_eq!(value, Value::Bool(true), "{source}");
            }
        }
        for source in [
            "new WeakRef()",
            "new WeakRef(null)",
            "new WeakRef(1)",
            "new WeakRef(true)",
            "new WeakRef('x')",
            "new WeakRef(1n)",
            "new WeakRef(Symbol('x'))",
            "new WeakRef(Symbol.for('x'))",
            "new WeakRef(Object)",
        ] {
            let mut vm = Runtime::default();
            let wrapped =
                format!("let ok=false;try{{{source};}}catch(e){{ok=e.name==='TypeError';}}ok;");
            assert_eq!(
                vm.execute(&wrapped, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
        let mut vm = Runtime::default();
        assert!(
            vm.weak_ref_call("WeakRef", &[Value::Host(1)], Value::Undefined, true)
                .is_err()
        );
    }

    #[test]
    fn active_frames_block_collection_and_next_job_can_clear_target() {
        let mut vm = Runtime::default();
        let target = vm.object(BTreeMap::new(), None).unwrap();
        let weak = make_ref(&mut vm, target.clone());
        for frame in 0..3 {
            vm.call_depth = usize::from(frame == 0);
            vm.eval_depth = usize::from(frame == 1);
            vm.dynamic_depth = usize::from(frame == 2);
            vm.collect_garbage(std::slice::from_ref(&weak), &Empty)
                .unwrap();
            assert_eq!(deref(&mut vm, weak.clone()), target);
        }
        vm.call_depth = 0;
        vm.eval_depth = 0;
        vm.dynamic_depth = 0;
        vm.collect_garbage(std::slice::from_ref(&weak), &Empty)
            .unwrap();
        assert_eq!(deref(&mut vm, weak), Value::Undefined);
        let mut vm = Runtime::default();
        vm.execute("let w=new WeakRef({});", &mut Empty).unwrap();
        assert_eq!(
            vm.execute("w.deref()===undefined;", &mut Empty).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn weak_wrappers_do_not_trace_other_weak_wrappers() {
        let mut vm = Runtime::default();
        let target = vm.object(BTreeMap::new(), None).unwrap();
        let inner = make_ref(&mut vm, target);
        let outer = make_ref(&mut vm, inner);
        vm.collect_garbage(std::slice::from_ref(&outer), &Empty)
            .unwrap();
        assert_eq!(deref(&mut vm, outer), Value::Undefined);
        assert_eq!(vm.weak_refs.records.len(), 1);
        vm.collect_garbage(&[], &Empty).unwrap();
        assert!(vm.weak_refs.records.is_empty());
        let prototype = vm.weak_ref_prototype().unwrap();
        assert!(vm.weak_ref_target_is_live(&prototype));
        assert!(
            vm.weak_ref_call("WeakRef.deref", &[], prototype, false)
                .is_err()
        );
    }

    #[test]
    fn instance_heap_and_instruction_limits_and_sweep_recovery() {
        let mut vm = Runtime::default();
        let target = vm.object(BTreeMap::new(), None).unwrap();
        for _ in 0..MAX_WEAK_REFS {
            make_ref(&mut vm, target.clone());
        }
        assert_eq!(
            vm.weak_ref_call(
                "WeakRef",
                std::slice::from_ref(&target),
                Value::Undefined,
                true
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );
        vm.collect_garbage(std::slice::from_ref(&target), &Empty)
            .unwrap();
        assert!(vm.weak_refs.records.is_empty());
        let weak = make_ref(&mut vm, target.clone());
        assert_eq!(deref(&mut vm, weak), target);
        let mut vm = Runtime::new(Limits {
            heap_objects: 2,
            ..Limits::default()
        });
        let target = vm.object(BTreeMap::new(), None).unwrap();
        assert_eq!(
            vm.weak_ref_call("WeakRef", &[target], Value::Undefined, true)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert!(vm.weak_refs.records.is_empty());
        let mut vm = Runtime::new(Limits {
            instructions: 0,
            ..Limits::default()
        });
        let target = vm.object(BTreeMap::new(), None).unwrap();
        assert_eq!(
            vm.weak_ref_call("WeakRef", &[target], Value::Undefined, true)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert!(vm.weak_refs.records.is_empty());
    }
}
