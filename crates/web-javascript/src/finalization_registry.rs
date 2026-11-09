//! Bounded FinalizationRegistry. Collection discovers work; checkpoints run it.
use crate::{ErrorKind, Heap, Host, JsError, Runtime, Value, descriptors::PropertyMetadata};
use std::collections::BTreeMap;

const MAX_REGISTRIES: usize = 256;
const MAX_CELLS: usize = 4096;

#[derive(Clone, Debug)]
struct Cell {
    target: Option<Value>,
    held: Value,
    token: Option<Value>,
}
#[derive(Clone, Debug)]
struct Registry {
    callback: Value,
    cells: Vec<Cell>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct FinalizationRegistryState {
    records: BTreeMap<usize, Registry>,
    prototype: Option<usize>,
    pub(super) draining: bool,
}
pub(super) fn global(name: &str) -> Option<Value> {
    (name == "FinalizationRegistry").then_some(Value::Native("FinalizationRegistry"))
}
impl Runtime {
    fn finalization_registry_charge(&mut self, work: usize) -> Result<(), JsError> {
        self.remaining = self
            .remaining
            .checked_sub(work as u64)
            .ok_or_else(|| JsError::resource("FinalizationRegistry instruction budget exceeded"))?;
        Ok(())
    }
    fn finalization_registry_prototype(&mut self) -> Result<Value, JsError> {
        if let Some(id) = self.finalization_registries.prototype {
            return Ok(Value::Object(id));
        }
        let Value::Symbol(tag_key, _) = self.well_known_symbol("toStringTag")? else {
            unreachable!()
        };
        let tag = self.string("FinalizationRegistry".into())?;
        let prototype = self.ordinary_object(BTreeMap::from([
            ("constructor".into(), Value::Native("FinalizationRegistry")),
            (
                "register".into(),
                Value::Native("FinalizationRegistry.register"),
            ),
            (
                "unregister".into(),
                Value::Native("FinalizationRegistry.unregister"),
            ),
        ]))?;
        self.hide_properties(&prototype, &["constructor", "register", "unregister"]);
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
        self.finalization_registries.prototype = Some(id);
        Ok(prototype)
    }
    pub(super) fn finalization_registry_get(
        &mut self,
        object: &Value,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if *object == Value::Native("FinalizationRegistry") && property == "prototype" {
            return self.finalization_registry_prototype().map(Some);
        }
        Ok(None)
    }
    fn finalization_registry_weakable(&self, value: &Value) -> bool {
        match value {
            Value::Object(id) => matches!(self.heap.get(*id), Some(Heap::Object(_))),
            Value::Array(id) => matches!(self.heap.get(*id), Some(Heap::Array(_))),
            Value::Function(id) => matches!(self.heap.get(*id), Some(Heap::Function(_))),
            _ => false,
        }
    }
    pub(super) fn finalization_registry_call(
        &mut self,
        name: &str,
        args: &[Value],
        this: Value,
        construct: bool,
        host: &impl Host,
    ) -> Result<Option<Value>, JsError> {
        if name == "FinalizationRegistry" {
            if !construct {
                return Err(self.property_type_error("FinalizationRegistry requires new"));
            }
            let callback = args.first().cloned().unwrap_or(Value::Undefined);
            if !self.is_callable_value(&callback, host) {
                return Err(
                    self.property_type_error("FinalizationRegistry callback must be callable")
                );
            }
            if self.finalization_registries.records.len() >= MAX_REGISTRIES {
                return Err(JsError::resource(
                    "FinalizationRegistry instance limit exceeded",
                ));
            }
            self.tick()?;
            let prototype = self.finalization_registry_prototype()?;
            let object = self.object(BTreeMap::new(), Some(prototype))?;
            let Value::Object(id) = object else {
                unreachable!()
            };
            self.finalization_registries.records.insert(
                id,
                Registry {
                    callback,
                    cells: vec![],
                },
            );
            return Ok(Some(object));
        }
        if !matches!(
            name,
            "FinalizationRegistry.register" | "FinalizationRegistry.unregister"
        ) {
            return Ok(None);
        }
        if construct {
            return Err(
                self.property_type_error("FinalizationRegistry method is not a constructor")
            );
        }
        let id = match this {
            Value::Object(id)
                if matches!(self.heap.get(id), Some(Heap::Object(_)))
                    && self.finalization_registries.records.contains_key(&id) =>
            {
                id
            }
            _ => {
                return Err(self.property_type_error(
                    "FinalizationRegistry method requires a registry receiver",
                ));
            }
        };
        self.tick()?;
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        if !self.finalization_registry_weakable(&first) {
            return Err(self.property_type_error(
                "FinalizationRegistry target or token must be a supported VM heap object",
            ));
        }
        if name == "FinalizationRegistry.unregister" {
            self.finalization_registry_charge(
                self.finalization_registries.records[&id].cells.len(),
            )?;
            let record = self.finalization_registries.records.get_mut(&id).unwrap();
            let before = record.cells.len();
            record
                .cells
                .retain(|cell| cell.token.as_ref() != Some(&first));
            return Ok(Some(Value::Bool(record.cells.len() != before)));
        }
        let held = args.get(1).cloned().unwrap_or(Value::Undefined);
        // Supported targets have object identity, for which equality is SameValue.
        if first == held {
            return Err(
                self.property_type_error("FinalizationRegistry target and held value must differ")
            );
        }
        let token = args
            .get(2)
            .filter(|value| **value != Value::Undefined)
            .cloned();
        if token
            .as_ref()
            .is_some_and(|token| !self.finalization_registry_weakable(token))
        {
            return Err(self.property_type_error(
                "FinalizationRegistry token must be a supported VM heap object",
            ));
        }
        self.finalization_registry_charge(self.finalization_registries.records.len())?;
        let count: usize = self
            .finalization_registries
            .records
            .values()
            .map(|record| record.cells.len())
            .sum();
        if count >= MAX_CELLS {
            return Err(JsError::resource(
                "FinalizationRegistry cell limit exceeded",
            ));
        }
        self.finalization_registries
            .records
            .get_mut(&id)
            .unwrap()
            .cells
            .push(Cell {
                target: Some(first),
                held,
                token,
            });
        Ok(Some(Value::Undefined))
    }
    pub(super) fn finalization_registry_cached_roots(&self) -> Vec<Value> {
        self.finalization_registries
            .prototype
            .map(Value::Object)
            .into_iter()
            .collect()
    }
    pub(super) fn finalization_registry_trace(&self, id: usize) -> Vec<Value> {
        let Some(record) = self.finalization_registries.records.get(&id) else {
            return vec![];
        };
        let mut values = vec![record.callback.clone()];
        values.extend(record.cells.iter().map(|cell| cell.held.clone()));
        values
    }
    // Charge the bounded side-table sweep before any collection mutation.
    pub(super) fn finalization_registry_gc_work(&self) -> usize {
        self.finalization_registries
            .records
            .values()
            .map(|record| 1 + record.cells.len() * 2)
            .sum()
    }
    pub(super) fn finalization_registry_sweep(&mut self, live: &[bool]) {
        fn marked(value: &Value, live: &[bool]) -> bool {
            match value {
                Value::Object(id) | Value::Array(id) | Value::Function(id) => {
                    live.get(*id) == Some(&true)
                }
                _ => false,
            }
        }
        self.finalization_registries.records.retain(|id, record| {
            if live.get(*id) != Some(&true) {
                return false;
            }
            for cell in &mut record.cells {
                if cell
                    .target
                    .as_ref()
                    .is_some_and(|value| !marked(value, live))
                {
                    cell.target = None;
                }
                if cell
                    .token
                    .as_ref()
                    .is_some_and(|value| !marked(value, live))
                {
                    cell.token = None;
                }
            }
            true
        });
    }
    /// Called only by public entry checkpoints, after their Promise queue drain.
    #[cfg(test)]
    fn drain_finalization_registry_cleanup(&mut self, host: &mut impl Host) -> Result<(), JsError> {
        self.drain_finalization_registry_cleanup_with_budget(host, &mut 0)
    }
    pub(super) fn drain_finalization_registry_cleanup_with_budget(
        &mut self,
        host: &mut impl Host,
        microtask_work: &mut usize,
    ) -> Result<(), JsError> {
        if self.finalization_registries.draining
            || self.promise_jobs_are_draining()
            || self.call_depth != 0
            || self.eval_depth != 0
            || self.dynamic_depth != 0
        {
            return Ok(());
        }
        self.finalization_registries.draining = true;
        let result = (|| {
            // Snapshot owners, not cells: unregister in a callback cancels pending cells.
            let owners: Vec<_> = self
                .finalization_registries
                .records
                .keys()
                .copied()
                .collect();
            let mut count = 0;

            for id in owners {
                let mut did_work = false;
                while let Some(record) = self.finalization_registries.records.get(&id) {
                    let work = 1 + record.cells.len() * 2;
                    self.finalization_registry_charge(work)?;
                    let record = &self.finalization_registries.records[&id];
                    let Some(index) = record.cells.iter().position(|cell| cell.target.is_none())
                    else {
                        break;
                    };
                    if count >= MAX_CELLS {
                        return Err(JsError::resource(
                            "FinalizationRegistry checkpoint work limit exceeded",
                        ));
                    }
                    self.tick()?;
                    if self.remaining == 0 || self.call_depth >= self.limits.call_depth {
                        return Err(JsError::resource(
                            "FinalizationRegistry callback entry budget exceeded",
                        ));
                    }
                    count += 1;
                    let record = self.finalization_registries.records.get_mut(&id).unwrap();
                    let callback = record.callback.clone();
                    let held = record.cells.remove(index).held;
                    did_work = true;
                    if let Err(error) = self.call(callback, vec![held], Value::Undefined, host) {
                        if error.kind == ErrorKind::Resource {
                            return Err(error);
                        }
                        host.report_microtask_error(&error);
                        break;
                    }
                }
                if did_work {
                    self.drain_promise_jobs_with_budget(host, microtask_work)?;
                }
            }
            Ok(())
        })();
        self.finalization_registries.draining = false;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Empty {
        errors: usize,
    }
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
        fn report_microtask_error(&mut self, _: &JsError) {
            self.errors += 1;
        }
    }
    fn make(vm: &mut Runtime) -> Value {
        vm.finalization_registry_call(
            "FinalizationRegistry",
            &[Value::Native("Object")],
            Value::Undefined,
            true,
            &Empty::default(),
        )
        .unwrap()
        .unwrap()
    }
    fn register(vm: &mut Runtime, registry: &Value, target: Value, held: Value, token: Value) {
        vm.finalization_registry_call(
            "FinalizationRegistry.register",
            &[target, held, token],
            registry.clone(),
            false,
            &Empty::default(),
        )
        .unwrap();
    }
    #[test]
    fn language_surface_and_rejections() {
        for source in [
            "const r=new FinalizationRegistry(()=>{});r.register({},1)===undefined;",
            "FinalizationRegistry.length===1&&FinalizationRegistry.prototype.register.length===2&&FinalizationRegistry.prototype.unregister.length===1;",
            "new FinalizationRegistry(()=>{}) instanceof FinalizationRegistry;",
            "String(new FinalizationRegistry(()=>{}))==='[object FinalizationRegistry]';",
            "FinalizationRegistry.prototype.cleanupSome===undefined;",
            "const r=new FinalizationRegistry(()=>{});const t={};r.register(t,1,t);r.register({},2,t);r.unregister(t)&&!r.unregister(t);",
            "const d=Object.getOwnPropertyDescriptor(FinalizationRegistry.prototype,Symbol.toStringTag);d.value==='FinalizationRegistry'&&!d.writable&&!d.enumerable&&d.configurable;",
        ] {
            assert_eq!(
                Runtime::default()
                    .execute(source, &mut Empty::default())
                    .unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
        for source in [
            "FinalizationRegistry(()=>{})",
            "new FinalizationRegistry(1)",
            "FinalizationRegistry.prototype.register({},1)",
            "FinalizationRegistry.prototype.unregister({})",
            "new FinalizationRegistry.prototype.register({},1)",
            "new FinalizationRegistry.prototype.unregister({})",
            "const r=new FinalizationRegistry(()=>{});const t={};r.register(t,t)",
            "new FinalizationRegistry(()=>{}).register(Symbol(),1)",
            "new FinalizationRegistry(()=>{}).register(Object,1)",
            "new FinalizationRegistry(()=>{}).register({},1,null)",
            "new FinalizationRegistry(()=>{}).unregister(undefined)",
        ] {
            let source =
                format!("let ok=false;try{{{source};}}catch(e){{ok=e.name==='TypeError';}}ok;");
            assert_eq!(
                Runtime::default()
                    .execute(&source, &mut Empty::default())
                    .unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn collection_defers_cleanup_and_preserves_microtask_order() {
        let mut vm = Runtime::default();
        let mut host = Empty::default();
        vm.execute("let log='';let r=new FinalizationRegistry(x=>{log+=x;queueMicrotask(()=>{log+='m';});});r.register({},'f');", &mut host).unwrap();
        vm.collect_garbage(&[], &host).unwrap();
        assert_eq!(vm.global_value("log"), Some(Value::String("".into())));
        vm.execute("queueMicrotask(()=>{log+='p';});", &mut host)
            .unwrap();
        assert_eq!(vm.global_value("log"), Some(Value::String("pfm".into())));
    }
    #[test]
    fn held_edges_keep_targets_alive_and_dead_registries_are_discarded() {
        let mut vm = Runtime::default();
        let registry = make(&mut vm);
        let target = vm.object(BTreeMap::new(), None).unwrap();
        let held = vm
            .object(BTreeMap::from([("target".into(), target.clone())]), None)
            .unwrap();
        register(&mut vm, &registry, target.clone(), held, Value::Undefined);
        vm.collect_garbage(std::slice::from_ref(&registry), &Empty::default())
            .unwrap();
        let Value::Object(id) = registry else {
            unreachable!()
        };
        assert_eq!(
            vm.finalization_registries.records[&id].cells[0].target,
            Some(target)
        );
        vm.collect_garbage(&[], &Empty::default()).unwrap();
        assert!(vm.finalization_registries.records.is_empty());
    }
    #[test]
    fn dead_token_is_cleared_before_reuse_and_live_token_cancels_pending_cells() {
        let mut vm = Runtime::default();
        let registry = make(&mut vm);
        let target = vm.object(BTreeMap::new(), None).unwrap();
        register(
            &mut vm,
            &registry,
            target.clone(),
            Value::Number(1.0),
            target,
        );
        vm.collect_garbage(std::slice::from_ref(&registry), &Empty::default())
            .unwrap();
        let Value::Object(id) = registry else {
            unreachable!()
        };
        let cell = &vm.finalization_registries.records[&id].cells[0];
        assert!(cell.target.is_none() && cell.token.is_none());
        let token = vm.object(BTreeMap::new(), None).unwrap();
        assert_eq!(
            vm.finalization_registry_call(
                "FinalizationRegistry.unregister",
                std::slice::from_ref(&token),
                registry.clone(),
                false,
                &Empty::default()
            )
            .unwrap(),
            Some(Value::Bool(false))
        );
        for _ in 0..2 {
            let target = vm.object(BTreeMap::new(), None).unwrap();
            register(&mut vm, &registry, target, Value::Undefined, token.clone());
        }
        vm.collect_garbage(&[registry.clone(), token.clone()], &Empty::default())
            .unwrap();
        assert_eq!(
            vm.finalization_registry_call(
                "FinalizationRegistry.unregister",
                &[token],
                registry,
                false,
                &Empty::default()
            )
            .unwrap(),
            Some(Value::Bool(true))
        );
        assert_eq!(vm.finalization_registries.records[&id].cells.len(), 1);
    }
    #[test]
    fn cleanup_errors_preserve_remaining_cells_until_later_checkpoint() {
        let mut vm = Runtime::default();
        let mut host = Empty::default();
        vm.execute("let n=0;let r=new FinalizationRegistry(()=>{n++;throw 1;});r.register({},1);r.register({},2);", &mut host).unwrap();
        vm.collect_garbage(&[], &host).unwrap();
        vm.execute("0;", &mut host).unwrap();
        assert_eq!(vm.global_value("n"), Some(Value::Number(1.0)));
        assert_eq!(host.errors, 1);
        vm.execute("0;", &mut host).unwrap();
        assert_eq!(vm.global_value("n"), Some(Value::Number(2.0)));
        assert_eq!(host.errors, 2);
    }
    #[test]
    fn bounded_cells_include_pending_and_tick_failure_does_not_consume() {
        let mut vm = Runtime::default();
        let registry = make(&mut vm);
        let target = vm.object(BTreeMap::new(), None).unwrap();
        for _ in 0..MAX_CELLS {
            register(
                &mut vm,
                &registry,
                target.clone(),
                Value::Undefined,
                Value::Undefined,
            );
        }
        vm.collect_garbage(std::slice::from_ref(&registry), &Empty::default())
            .unwrap();
        let target = vm.object(BTreeMap::new(), None).unwrap();
        let error = vm
            .finalization_registry_call(
                "FinalizationRegistry.register",
                &[target, Value::Undefined],
                registry,
                false,
                &Empty::default(),
            )
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        vm.remaining = 0;
        assert_eq!(
            vm.drain_finalization_registry_cleanup(&mut Empty::default())
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            vm.finalization_registries
                .records
                .values()
                .next()
                .unwrap()
                .cells
                .len(),
            MAX_CELLS
        );
        assert!(!vm.finalization_registries.draining);
    }
    #[test]
    fn callback_entry_budget_does_not_drop_pending_cell() {
        let mut vm = Runtime::default();
        let registry = make(&mut vm);
        let target = vm.object(BTreeMap::new(), None).unwrap();
        register(
            &mut vm,
            &registry,
            target,
            Value::Undefined,
            Value::Undefined,
        );
        vm.collect_garbage(&[registry], &Empty::default()).unwrap();
        vm.remaining = 4; // one-cell scan/removal charge plus cleanup tick, no call tick.
        assert!(
            vm.drain_finalization_registry_cleanup(&mut Empty::default())
                .is_err()
        );
        assert_eq!(
            vm.finalization_registries
                .records
                .values()
                .next()
                .unwrap()
                .cells
                .len(),
            1
        );
    }
    #[test]
    fn weakmap_fixed_point_reaches_registry_then_held_value_then_another_entry() {
        let mut vm = Runtime::default();
        let mut host = Empty::default();
        let registry = make(&mut vm);
        let target = vm.object(BTreeMap::new(), None).unwrap();
        let key = vm.object(BTreeMap::new(), None).unwrap();
        let held = vm.object(BTreeMap::new(), None).unwrap();
        register(
            &mut vm,
            &registry,
            target.clone(),
            held.clone(),
            Value::Undefined,
        );
        let map = vm
            .weak_collection_call("WeakMap", &[], Value::Undefined, true, &mut host)
            .unwrap()
            .unwrap();
        vm.weak_collection_call(
            "WeakMap.set",
            &[key.clone(), registry.clone()],
            map.clone(),
            false,
            &mut host,
        )
        .unwrap();
        vm.weak_collection_call(
            "WeakMap.set",
            &[held, target.clone()],
            map.clone(),
            false,
            &mut host,
        )
        .unwrap();
        vm.collect_garbage(&[map, key], &host).unwrap();
        let Value::Object(id) = registry else {
            unreachable!()
        };
        assert_eq!(
            vm.finalization_registries.records[&id].cells[0].target,
            Some(target)
        );
    }
    #[test]
    fn registry_limit_recovers_after_collection() {
        let mut vm = Runtime::default();
        for _ in 0..MAX_REGISTRIES {
            make(&mut vm);
        }
        assert_eq!(
            vm.finalization_registry_call(
                "FinalizationRegistry",
                &[Value::Native("Object")],
                Value::Undefined,
                true,
                &Empty::default()
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );
        vm.collect_garbage(&[], &Empty::default()).unwrap();
        make(&mut vm);
        assert_eq!(vm.finalization_registries.records.len(), 1);
    }
}
