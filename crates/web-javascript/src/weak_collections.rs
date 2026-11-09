//! Bounded weak collections. Only VM Object/Array/Function keys are supported.
//! At most 256 collections and 4096 entries across both kinds per Runtime.
//! Entries are ephemerons, never ordinary strong tracing edges.
use crate::{Heap, Host, JsError, Runtime, Value, descriptors::PropertyMetadata};
use std::collections::{BTreeMap, BTreeSet};

const MAX_COLLECTIONS: usize = 256;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Debug)]
struct Collection {
    map: bool,
    entries: BTreeMap<usize, Value>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct WeakCollectionState {
    records: BTreeMap<usize, Collection>,
    prototypes: BTreeMap<bool, usize>,
    entries: usize,
}

pub(super) fn global(name: &str) -> Option<Value> {
    match name {
        "WeakMap" => Some(Value::Native("WeakMap")),
        "WeakSet" => Some(Value::Native("WeakSet")),
        _ => None,
    }
}

impl Runtime {
    fn weak_collection_prototype(&mut self, map: bool) -> Result<Value, JsError> {
        if let Some(id) = self.weak_collections.prototypes.get(&map) {
            return Ok(Value::Object(*id));
        }
        let methods = if map {
            vec![
                ("constructor", "WeakMap"),
                ("get", "WeakMap.get"),
                ("set", "WeakMap.set"),
                ("has", "WeakMap.has"),
                ("delete", "WeakMap.delete"),
            ]
        } else {
            vec![
                ("constructor", "WeakSet"),
                ("add", "WeakSet.add"),
                ("has", "WeakSet.has"),
                ("delete", "WeakSet.delete"),
            ]
        };
        let Value::Symbol(tag_key, _) = self.well_known_symbol("toStringTag")? else {
            unreachable!()
        };
        let tag = self.string(if map { "WeakMap" } else { "WeakSet" }.into())?;
        let prototype = self.ordinary_object(
            methods
                .iter()
                .map(|(key, value)| ((*key).into(), Value::Native(value)))
                .collect(),
        )?;
        let names: Vec<_> = methods.iter().map(|(key, _)| *key).collect();
        self.hide_properties(&prototype, &names);
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
        self.weak_collections.prototypes.insert(map, id);
        Ok(prototype)
    }

    pub(super) fn weak_collection_get(
        &mut self,
        object: &Value,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if property == "prototype" {
            match object {
                Value::Native("WeakMap") => return self.weak_collection_prototype(true).map(Some),
                Value::Native("WeakSet") => return self.weak_collection_prototype(false).map(Some),
                _ => {}
            }
        }
        Ok(None)
    }

    fn weak_collection_key(&self, value: &Value) -> Option<usize> {
        match value {
            Value::Object(id) if matches!(self.heap.get(*id), Some(Heap::Object(_))) => Some(*id),
            Value::Array(id) if matches!(self.heap.get(*id), Some(Heap::Array(_))) => Some(*id),
            Value::Function(id) if matches!(self.heap.get(*id), Some(Heap::Function(_))) => {
                Some(*id)
            }
            _ => None,
        }
    }

    pub(super) fn weak_collection_call(
        &mut self,
        name: &str,
        args: &[Value],
        this: Value,
        construct: bool,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        let (map, method) = match name {
            "WeakMap" => (true, "constructor"),
            "WeakSet" => (false, "constructor"),
            "WeakMap.get" => (true, "get"),
            "WeakMap.set" => (true, "set"),
            "WeakMap.has" => (true, "has"),
            "WeakMap.delete" => (true, "delete"),
            "WeakSet.add" => (false, "add"),
            "WeakSet.has" => (false, "has"),
            "WeakSet.delete" => (false, "delete"),
            _ => return Ok(None),
        };
        if method == "constructor" {
            if !construct {
                return Err(self.property_type_error("Weak collection constructor requires new"));
            }
            if self.weak_collections.records.len() >= MAX_COLLECTIONS {
                return Err(JsError::resource("Weak collection instance limit exceeded"));
            }
            self.tick()?;
            let prototype = self.weak_collection_prototype(map)?;
            let object = self.object(BTreeMap::new(), Some(prototype))?;
            let Value::Object(id) = object else {
                unreachable!()
            };
            self.weak_collections.records.insert(
                id,
                Collection {
                    map,
                    entries: BTreeMap::new(),
                },
            );
            let iterable = args.first().cloned().unwrap_or(Value::Undefined);
            if !matches!(iterable, Value::Undefined | Value::Null) {
                let adder = self.get(object.clone(), if map { "set" } else { "add" }, host)?;
                if !self.is_callable_value(&adder, host) {
                    return Err(self.property_type_error("Weak collection adder must be callable"));
                }
                let mut iterator = self.iterator(iterable, host)?;
                let mut close_on_error = false;
                let result = (|| -> Result<(), JsError> {
                    let mut count = 0usize;
                    loop {
                        close_on_error = false;
                        let Some(item) = self.iterator_next(&mut iterator, host)? else {
                            break;
                        };
                        close_on_error = true;
                        count += 1;
                        if count > MAX_ENTRIES {
                            return Err(JsError::resource(
                                "Weak collection iterable limit exceeded",
                            ));
                        }
                        let arguments = if map {
                            if !matches!(
                                item,
                                Value::Object(_)
                                    | Value::Array(_)
                                    | Value::Function(_)
                                    | Value::Host(_)
                                    | Value::Native(_)
                            ) {
                                return Err(self.property_type_error(
                                    "WeakMap iterable entry must be an object",
                                ));
                            }
                            vec![
                                self.get(item.clone(), "0", host)?,
                                self.get(item, "1", host)?,
                            ]
                        } else {
                            vec![item]
                        };
                        self.call(adder.clone(), arguments, object.clone(), host)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    // Resource exhaustion cannot safely execute arbitrary user cleanup.
                    if close_on_error
                        && error.kind != crate::ErrorKind::Resource
                        && let Err(close_error) = self.iterator_close(&mut iterator, host)
                        && close_error.kind == crate::ErrorKind::Resource
                    {
                        return Err(close_error);
                    }
                    return Err(error);
                }
            }
            return Ok(Some(object));
        }
        if construct {
            return Err(self.property_type_error("Weak collection method is not a constructor"));
        }
        self.tick()?;
        let id = match this {
            Value::Object(id)
                if matches!(self.heap.get(id), Some(Heap::Object(_)))
                    && self
                        .weak_collections
                        .records
                        .get(&id)
                        .is_some_and(|record| record.map == map) =>
            {
                id
            }
            _ => return Err(self.property_type_error("Incompatible weak collection receiver")),
        };
        let key = self.weak_collection_key(args.first().unwrap_or(&Value::Undefined));
        if matches!(method, "set" | "add") {
            let Some(key) = key else {
                return Err(self.property_type_error(
                    "Weak collection key must be a supported VM heap object",
                ));
            };
            let record = self.weak_collections.records.get_mut(&id).unwrap();
            let new = !record.entries.contains_key(&key);
            if new && self.weak_collections.entries >= MAX_ENTRIES {
                return Err(JsError::resource("Weak collection entry limit exceeded"));
            }
            let value = if map {
                args.get(1).cloned().unwrap_or(Value::Undefined)
            } else {
                Value::Undefined
            };
            record.entries.insert(key, value);
            self.weak_collections.entries += usize::from(new);
            return Ok(Some(this));
        }
        let record = self.weak_collections.records.get_mut(&id).unwrap();
        Ok(Some(match method {
            "get" => key
                .and_then(|key| record.entries.get(&key).cloned())
                .unwrap_or(Value::Undefined),
            "has" => Value::Bool(key.is_some_and(|key| record.entries.contains_key(&key))),
            "delete" => {
                let removed = key.is_some_and(|key| record.entries.remove(&key).is_some());
                self.weak_collections.entries -= usize::from(removed);
                Value::Bool(removed)
            }
            _ => unreachable!(),
        }))
    }

    pub(super) fn weak_collection_cached_roots(&self) -> Vec<Value> {
        self.weak_collections
            .prototypes
            .values()
            .copied()
            .map(Value::Object)
            .collect()
    }

    /// No arena mutation: an exhausted budget leaves heap and weak tables intact.
    pub(super) fn weak_collection_ephemerons(
        &self,
        live: &[bool],
        seen: &mut BTreeSet<(usize, usize)>,
        work: &mut usize,
    ) -> Result<Vec<Value>, JsError> {
        let mut values = Vec::new();
        for (id, record) in &self.weak_collections.records {
            *work = work.saturating_add(1);
            if *work > 1_000_000 {
                return Err(JsError::resource("GC traversal work exceeded"));
            }
            if !record.map || live.get(*id) != Some(&true) {
                continue;
            }
            for (key, value) in &record.entries {
                *work = work.saturating_add(1);
                if *work > 1_000_000 {
                    return Err(JsError::resource("GC traversal work exceeded"));
                }
                if live.get(*key) == Some(&true) && seen.insert((*id, *key)) {
                    values.push(value.clone());
                }
            }
        }
        Ok(values)
    }

    /// Clear entries and dead collection brands before heap IDs can be recycled.
    pub(super) fn weak_collection_sweep(&mut self, live: &[bool]) {
        self.weak_collections.records.retain(|id, record| {
            if live.get(*id) != Some(&true) {
                return false;
            }
            record
                .entries
                .retain(|key, _| live.get(*key) == Some(&true));
            true
        });
        self.weak_collections.entries = self
            .weak_collections
            .records
            .values()
            .map(|record| record.entries.len())
            .sum();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorKind, Limits};
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
    fn object(vm: &mut Runtime) -> Value {
        vm.object(BTreeMap::new(), None).unwrap()
    }
    fn make(vm: &mut Runtime, map: bool) -> Value {
        vm.weak_collection_call(
            if map { "WeakMap" } else { "WeakSet" },
            &[],
            Value::Undefined,
            true,
            &mut Empty,
        )
        .unwrap()
        .unwrap()
    }
    fn invoke(vm: &mut Runtime, name: &str, receiver: &Value, args: &[Value]) -> Value {
        vm.weak_collection_call(name, args, receiver.clone(), false, &mut Empty)
            .unwrap()
            .unwrap()
    }
    fn id(value: &Value) -> usize {
        match value {
            Value::Object(id) => *id,
            _ => panic!(),
        }
    }

    #[test]
    fn ephemeron_retains_value_only_when_map_and_key_are_live() {
        let mut vm = Runtime::default();
        let map = make(&mut vm, true);
        let key = object(&mut vm);
        let value = object(&mut vm);
        invoke(&mut vm, "WeakMap.set", &map, &[key.clone(), value.clone()]);
        vm.collect_garbage(&[map.clone(), key.clone()], &Empty)
            .unwrap();
        assert!(matches!(vm.heap[id(&value)], Heap::Object(_)));
        assert_eq!(invoke(&mut vm, "WeakMap.get", &map, &[key]), value);
        vm.collect_garbage(std::slice::from_ref(&map), &Empty)
            .unwrap();
        assert!(matches!(vm.heap[id(&value)], Heap::Vacant));
        assert_eq!(vm.weak_collections.entries, 0);
    }

    #[test]
    fn value_to_key_cycle_is_not_a_root_and_recycled_ids_are_clean() {
        let mut vm = Runtime::default();
        let map = make(&mut vm, true);
        let key = object(&mut vm);
        let value = vm
            .object(BTreeMap::from([("key".into(), key.clone())]), None)
            .unwrap();
        invoke(&mut vm, "WeakMap.set", &map, &[key.clone(), value.clone()]);
        vm.collect_garbage(std::slice::from_ref(&map), &Empty)
            .unwrap();
        assert!(matches!(vm.heap[id(&key)], Heap::Vacant));
        assert!(matches!(vm.heap[id(&value)], Heap::Vacant));
        let replacement = object(&mut vm);
        let replacement2 = object(&mut vm);
        assert!(replacement == key || replacement2 == key);
        assert_eq!(
            invoke(&mut vm, "WeakMap.has", &map, &[replacement]),
            Value::Bool(false)
        );
        assert_eq!(
            invoke(&mut vm, "WeakMap.get", &map, &[replacement2]),
            Value::Undefined
        );
    }

    #[test]
    fn reverse_ephemeron_chain_reaches_fixed_point() {
        let mut vm = Runtime::default();
        let map = make(&mut vm, true);
        let end = object(&mut vm);
        let middle = object(&mut vm);
        let start = object(&mut vm);
        let value = object(&mut vm);
        invoke(&mut vm, "WeakMap.set", &map, &[end.clone(), value.clone()]);
        invoke(&mut vm, "WeakMap.set", &map, &[middle.clone(), end.clone()]);
        invoke(
            &mut vm,
            "WeakMap.set",
            &map,
            &[start.clone(), middle.clone()],
        );
        vm.collect_garbage(&[map, start], &Empty).unwrap();
        for value in [middle, end, value] {
            assert!(matches!(vm.heap[id(&value)], Heap::Object(_)));
        }
    }

    #[test]
    fn dead_maps_and_sets_do_not_keep_entries_alive_and_brands_are_removed() {
        let mut vm = Runtime::default();
        let map = make(&mut vm, true);
        let set = make(&mut vm, false);
        let key = object(&mut vm);
        let value = object(&mut vm);
        invoke(&mut vm, "WeakMap.set", &map, &[key.clone(), value.clone()]);
        invoke(&mut vm, "WeakSet.add", &set, std::slice::from_ref(&key));
        vm.collect_garbage(std::slice::from_ref(&key), &Empty)
            .unwrap();
        assert!(matches!(vm.heap[id(&value)], Heap::Vacant));
        assert!(vm.weak_collections.records.is_empty());
        assert_eq!(vm.weak_collections.entries, 0);
        let fresh = object(&mut vm);
        assert!(
            vm.weak_collection_call("WeakSet.has", &[key], fresh, false, &mut Empty)
                .is_err()
        );
    }

    #[test]
    fn weak_set_keys_are_weak_but_ordinary_properties_are_strong() {
        let mut vm = Runtime::default();
        let set = make(&mut vm, false);
        let key = object(&mut vm);
        invoke(&mut vm, "WeakSet.add", &set, std::slice::from_ref(&key));
        vm.set(set.clone(), "strong", key.clone(), &mut Empty)
            .unwrap();
        vm.collect_garbage(std::slice::from_ref(&set), &Empty)
            .unwrap();
        assert_eq!(
            invoke(&mut vm, "WeakSet.has", &set, std::slice::from_ref(&key)),
            Value::Bool(true)
        );
        vm.set(set.clone(), "strong", Value::Undefined, &mut Empty)
            .unwrap();
        vm.collect_garbage(std::slice::from_ref(&set), &Empty)
            .unwrap();
        assert_eq!(vm.weak_collections.entries, 0);
    }

    #[test]
    fn methods_brands_iterables_and_native_metadata() {
        for source in [
            "const k={};const m=new WeakMap([[k,7]]);m.get(k)===7&&m.set(k,8)===m&&m.get(k)===8&&m.delete(k)&&!m.has(k);",
            "const k=[];const s=new WeakSet([k,k]);s.has(k)&&s.add(k)===s&&s.delete(k)&&!s.delete(k);",
            "const k=function(){};new WeakMap([[k,1]]).has(k);",
            "new WeakMap(null).get(1)===undefined&&!new WeakSet().has(Symbol('x'))&&!new WeakMap().delete(null);",
            "WeakMap.length===0&&WeakSet.length===0;",
            "WeakMap.prototype.set.length===2&&WeakMap.prototype.get.length===1&&WeakSet.prototype.add.length===1;",
            "'prototype' in WeakMap&&'prototype' in WeakSet&&WeakMap.prototype.constructor===WeakMap&&Object.getPrototypeOf(WeakSet.prototype)===Object.prototype;",
            "String(new WeakMap())==='[object WeakMap]'&&String(new WeakSet())==='[object WeakSet]';",
            "new WeakMap().size===undefined&&new WeakSet().clear===undefined&&new WeakMap().keys===undefined;",
            "let ok=false;try{WeakMap();}catch(e){ok=e.name==='TypeError';}ok;",
            "let ok=false;try{new WeakMap().set(Symbol('x'),1);}catch(e){ok=e.name==='TypeError';}ok;",
            "let ok=false;try{WeakMap.prototype.has.call(new WeakSet(),{});}catch(e){ok=e.name==='TypeError';}ok;",
            "let ok=false;try{WeakSet.prototype.add.call({},{});}catch(e){ok=e.name==='TypeError';}ok;",
            "let ok=false;try{new WeakSet.prototype.add({});}catch(e){ok=e.name==='TypeError';}ok;",
            "let closed=0;const it={next(){return {done:false,value:1};},return(){closed++;return {};}};it[Symbol.iterator]=function(){return this};try{new WeakSet(it);}catch(e){}closed===1;",
            "let closed=0;const it={next(){throw 7;},return(){closed++;return {};}};it[Symbol.iterator]=function(){return this};try{new WeakMap(it);}catch(e){}closed===0;",
            "let calls=0;WeakSet.prototype.add=function(k){calls++;return this;};new WeakSet([{},{}]);calls===2;",
        ] {
            let mut vm = Runtime::default();
            assert_eq!(
                vm.execute(source, &mut Empty).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }

    #[test]
    fn unsupported_keys_receivers_and_heap_limit() {
        let mut vm = Runtime::default();
        let map = make(&mut vm, true);
        let set = make(&mut vm, false);
        for key in [
            Value::Undefined,
            Value::Null,
            Value::Bool(true),
            Value::Number(1.0),
            Value::Host(1),
            Value::Native("Object"),
        ] {
            assert_eq!(
                invoke(&mut vm, "WeakMap.get", &map, std::slice::from_ref(&key)),
                Value::Undefined
            );
            assert_eq!(
                invoke(&mut vm, "WeakSet.has", &set, std::slice::from_ref(&key)),
                Value::Bool(false)
            );
            assert!(
                vm.weak_collection_call(
                    "WeakMap.set",
                    std::slice::from_ref(&key),
                    map.clone(),
                    false,
                    &mut Empty
                )
                .is_err()
            );
            assert!(
                vm.weak_collection_call("WeakSet.add", &[key], set.clone(), false, &mut Empty)
                    .is_err()
            );
        }
        let prototype = vm.weak_collection_prototype(true).unwrap();
        assert!(
            vm.weak_collection_call("WeakMap.get", &[], prototype, false, &mut Empty)
                .is_err()
        );
        let mut vm = Runtime::new(Limits {
            heap_objects: 1,
            ..Limits::default()
        });
        assert_eq!(
            vm.weak_collection_call("WeakMap", &[], Value::Undefined, true, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert!(vm.weak_collections.records.is_empty());
    }
    #[test]
    fn resource_error_during_iterator_close_stays_fatal() {
        let source = "const it={next(){return {done:false,value:1}},return(){while(true){}}};it[Symbol.iterator]=function(){return this};try{new WeakSet(it)}catch(e){42}";
        assert_eq!(
            Runtime::default()
                .execute(source, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }

    #[test]
    fn explicit_instance_entry_instruction_and_gc_work_budgets() {
        let mut vm = Runtime::new(Limits {
            heap_objects: 8192,
            ..Limits::default()
        });
        let map = make(&mut vm, true);
        for _ in 0..MAX_ENTRIES {
            let key = object(&mut vm);
            invoke(&mut vm, "WeakMap.set", &map, &[key]);
        }
        let key = object(&mut vm);
        assert_eq!(
            vm.weak_collection_call("WeakMap.set", &[key], map.clone(), false, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let before = vm.weak_collections.entries;
        let mut work = 1_000_000;
        assert!(
            vm.weak_collection_ephemerons(
                &vec![true; vm.heap.len()],
                &mut BTreeSet::new(),
                &mut work
            )
            .is_err()
        );
        assert_eq!(vm.weak_collections.entries, before);
        vm.collect_garbage(std::slice::from_ref(&map), &Empty)
            .unwrap();
        assert_eq!(vm.weak_collections.entries, 0);
        for _ in 1..MAX_COLLECTIONS {
            make(&mut vm, false);
        }
        assert_eq!(
            vm.weak_collection_call("WeakSet", &[], Value::Undefined, true, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let mut vm = Runtime::new(Limits {
            instructions: 0,
            ..Limits::default()
        });
        assert_eq!(
            vm.weak_collection_call("WeakMap", &[], Value::Undefined, true, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }
}
