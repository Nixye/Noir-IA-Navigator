//! Non-moving mark/sweep at explicit VM entry safe points. IDs remain stable.
use crate::{Heap, Host, JsError, Runtime, Value};

#[derive(Clone, Copy, Debug, Default)]
pub struct CollectionStats {
    pub reclaimed_objects: usize,
    pub reclaimed_scopes: usize,
    pub live_objects: usize,
}
enum Root {
    Value(Value),
    Scope(usize),
}
impl Runtime {
    pub fn retain_value(&mut self, value: Value) -> Result<u64, JsError> {
        if self.gc_roots.len() >= 4096 {
            return Err(JsError::resource("External VM root limit exceeded"));
        }
        self.next_gc_root = self
            .next_gc_root
            .checked_add(1)
            .ok_or_else(|| JsError::resource("External root identity exhausted"))?;
        let token = self.next_gc_root;
        self.gc_roots.insert(token, value);
        Ok(token)
    }
    pub fn release_value(&mut self, token: u64) -> bool {
        self.gc_roots.remove(&token).is_some()
    }
    /// Embedders retaining values outside the Host must supply those values here
    /// before the next execution. No collection occurs inside an active frame.
    /// An explicit idle collection is a job boundary, including for WeakRef.
    /// Retain returned values with retain_value or pass them as roots here.
    pub fn collect_garbage(
        &mut self,
        roots: &[Value],
        host: &impl Host,
    ) -> Result<CollectionStats, JsError> {
        if self.call_depth != 0
            || self.eval_depth != 0
            || self.dynamic_depth != 0
            || self.nested_script_depth != 0
            || self.resumable_execution_active()
            || self.finalization_registries.draining
        {
            return Ok(CollectionStats::default());
        }
        let mut heap_live = vec![false; self.heap.len()];
        let mut scope_live = vec![false; self.scopes.len()];
        let mut pending = vec![Root::Scope(0)];
        let mut external = host.trace_roots();
        if external.len() > 65_536 || roots.len() > 65_536 {
            return Err(JsError::resource("GC external root count exceeded"));
        }
        external.extend_from_slice(roots);
        external.extend(self.gc_roots.values().cloned());
        external.extend(self.global_properties.map(Value::Object));
        external.extend(self.array_prototype.map(Value::Array));
        external.extend(self.json_namespace.map(Value::Object));
        external.extend(self.stdlib_cached_roots());
        external.extend(self.wasm_roots());
        external.extend(self.crypto_roots());
        external.extend(self.reflection_cached_roots());
        external.extend(self.promise_gc_cached_roots());
        external.extend(self.weak_ref_cached_roots());
        external.extend(self.finalization_registry_cached_roots());
        external.extend(self.weak_collection_cached_roots());
        external.extend(self.resumable_function_roots());
        external.extend(self.async_generator_roots());
        let (language_values, language_scopes) = self.language_gc_cached_roots();
        external.extend(language_values);
        pending.extend(language_scopes.into_iter().map(Root::Scope));
        let (module_values, module_scopes) = self.module_cached_roots();
        external.extend(module_values);
        pending.extend(module_scopes.into_iter().map(Root::Scope));
        pending.extend(external.into_iter().map(Root::Value));
        let mut work = self.finalization_registry_gc_work();
        let mut ephemerons = std::collections::BTreeSet::new();
        loop {
            while let Some(root) = pending.pop() {
                work = work.saturating_add(1);
                if work > 1_000_000 || pending.len() > 65_536 {
                    return Err(JsError::resource("GC traversal work exceeded"));
                }
                match root {
                    Root::Scope(id) => {
                        let Some(marked) = scope_live.get_mut(id) else {
                            return Err(JsError::runtime("Invalid GC scope root"));
                        };
                        if *marked {
                            continue;
                        }
                        *marked = true;
                        let scope = &self.scopes[id];
                        if let Some(parent) = scope.parent {
                            pending.push(Root::Scope(parent));
                        }
                        pending.extend(
                            scope
                                .bindings
                                .values()
                                .filter(|binding| binding.initialized)
                                .map(|binding| Root::Value(binding.value.clone())),
                        );
                    }
                    Root::Value(value) => {
                        let id = match value {
                            Value::Object(id) | Value::Array(id) | Value::Function(id) => id,
                            _ => continue,
                        };
                        let Some(marked) = heap_live.get_mut(id) else {
                            return Err(JsError::runtime("Invalid GC value root"));
                        };
                        if *marked {
                            continue;
                        }
                        *marked = true;
                        match &self.heap[id] {
                            Heap::Vacant => {
                                return Err(JsError::runtime(
                                    "A retained value references a collected object",
                                ));
                            }
                            Heap::Array(values) => {
                                pending.extend(values.iter().cloned().map(Root::Value))
                            }
                            Heap::Object(object) => {
                                pending
                                    .extend(object.properties.values().cloned().map(Root::Value));
                                pending.extend(object.prototype.clone().map(Root::Value));
                            }
                            Heap::Function(function) => {
                                pending.push(Root::Scope(function.scope));
                                pending.push(Root::Value(function.lexical_this.clone()));
                                pending
                                    .extend(function.properties.values().cloned().map(Root::Value));
                            }
                        }
                        if let Some(values) = self.array_properties.get(&id) {
                            pending.extend(values.values().cloned().map(Root::Value));
                        }
                        if let Some(values) = self.symbol_properties.get(&id) {
                            pending.extend(values.values().cloned().map(Root::Value));
                        }
                        for metadata in [
                            self.property_metadata
                                .get(&id)
                                .map(|map| map.values().collect::<Vec<_>>()),
                            self.symbol_metadata
                                .get(&id)
                                .map(|map| map.values().collect::<Vec<_>>()),
                        ]
                        .into_iter()
                        .flatten()
                        .flatten()
                        {
                            if let Some((getter, setter)) = &metadata.accessor {
                                pending.extend([
                                    Root::Value(getter.clone()),
                                    Root::Value(setter.clone()),
                                ]);
                            }
                        }
                        if let Some(bound) = self.bound_functions.get(&id) {
                            pending.extend([
                                Root::Value(bound.target.clone()),
                                Root::Value(bound.this.clone()),
                            ]);
                            pending.extend(bound.arguments.iter().cloned().map(Root::Value));
                        }
                        pending.extend(self.stdlib_trace(id).into_iter().map(Root::Value));
                        pending.extend(self.wasm_trace(id).into_iter().map(Root::Value));
                        pending.extend(self.reflection_trace(id).into_iter().map(Root::Value));
                        pending.extend(self.promise_gc_trace(id).into_iter().map(Root::Value));
                        pending.extend(self.async_generator_trace(id).into_iter().map(Root::Value));
                        pending.extend(
                            self.finalization_registry_trace(id)
                                .into_iter()
                                .map(Root::Value),
                        );
                        let (resumable_values, resumable_scopes) = self.resumable_trace(id);
                        pending.extend(resumable_values.into_iter().map(Root::Value));
                        pending.extend(resumable_scopes.into_iter().map(Root::Scope));
                        let (values, scopes) = self.language_gc_trace(id);
                        pending.extend(values.into_iter().map(Root::Value));
                        pending.extend(scopes.into_iter().map(Root::Scope));
                    }
                }
            }
            let values = self.weak_collection_ephemerons(&heap_live, &mut ephemerons, &mut work)?;
            if values.is_empty() {
                break;
            }
            pending.extend(values.into_iter().map(Root::Value));
        }
        // All roots and edges are validated before mutating the arena.
        // Clear weak targets before any reclaimed identity can be reused.
        self.weak_ref_sweep(&heap_live);
        self.wasm_sweep(&heap_live);
        self.finalization_registry_sweep(&heap_live);
        self.weak_collection_sweep(&heap_live);
        let mut stats = CollectionStats::default();
        for (id, marked) in heap_live.iter().copied().enumerate() {
            if marked {
                stats.live_objects += 1;
                continue;
            }
            if !matches!(self.heap[id], Heap::Vacant) {
                self.heap[id] = Heap::Vacant;
                self.free_heap.push(id);
                stats.reclaimed_objects += 1;
            }
        }
        let mut was_free = vec![false; self.scopes.len()];
        for id in &self.free_scopes {
            if let Some(mark) = was_free.get_mut(*id) {
                *mark = true;
            }
        }
        for (id, marked) in scope_live.iter().copied().enumerate() {
            if !marked && !was_free[id] {
                self.scopes[id].bindings.clear();
                self.scopes[id].parent = None;
                self.free_scopes.push(id);
                stats.reclaimed_scopes += 1;
            }
        }
        self.binding_count = self.scopes.iter().map(|scope| scope.bindings.len()).sum();
        self.array_properties
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.array_holes
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.array_non_enumerable
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.property_metadata
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.deleted_synthetics
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.property_order
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.symbol_properties
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.symbol_metadata
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.symbol_order
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        self.bound_functions
            .retain(|id, _| heap_live.get(*id) == Some(&true));
        let live_ids = heap_live
            .iter()
            .enumerate()
            .filter_map(|(id, live)| live.then_some(id))
            .collect();
        self.stdlib_sweep(&live_ids);
        self.reflection_sweep(&live_ids);
        self.promise_gc_sweep(&heap_live);
        self.resumable_sweep(&heap_live);
        self.resumable_function_sweep(&heap_live);
        self.async_generator_sweep(&heap_live);
        self.language_gc_sweep(&heap_live, &scope_live);
        self.regexp_count = self
            .heap
            .iter()
            .filter(|object| matches!(object,Heap::Object(object) if object.regexp.is_some()))
            .count();
        self.regexp_pattern_bytes = self
            .heap
            .iter()
            .filter_map(|object| match object {
                Heap::Object(object) => object.regexp.as_ref().map(|regex| regex.source.len()),
                _ => None,
            })
            .sum();
        Ok(stats)
    }
}
