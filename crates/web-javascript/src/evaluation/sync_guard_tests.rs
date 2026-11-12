use crate::{ErrorKind, Host, JsError, Runtime, Value};

#[derive(Default)]
struct Reenter {
    checks: usize,
    jobs: usize,
    max_slots: usize,
}

impl Host for Reenter {
    fn global(&mut self, name: &str) -> Option<Value> {
        (name == "h").then_some(Value::Host(1))
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        Ok(Value::Undefined)
    }
    fn set(&mut self, _: u64, property: &str, _: Value) -> Result<(), JsError> {
        if property == "job" {
            self.jobs += 1;
        }
        Ok(())
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("unexpected call")
    }
    fn set_with_runtime(
        &mut self,
        id: u64,
        property: &str,
        value: Value,
        runtime: &mut Runtime,
    ) -> Result<(), JsError> {
        if property != "check" {
            return self.set(id, property, value);
        }
        self.checks += 1;
        assert!(runtime.execution_is_active());
        assert!(!runtime.execution_job_boundary());
        let active = runtime.sync_execution.active;
        let native = runtime.eval_depth;
        let slots = runtime.sync_execution.live_slots;
        let frames = runtime.sync_execution.live_frames;
        self.max_slots = self.max_slots.max(slots);
        // The array's first object exists ONLY in an in-flight literal buffer.
        let stats = runtime.collect_garbage(&[], self)?;
        assert_eq!(stats.reclaimed_objects, 0);
        runtime.drain_promise_jobs(self)?;
        runtime.drain_execution_jobs(self)?;
        assert_eq!(self.jobs, 0);
        let mut snapshot = runtime.clone();
        assert_eq!(snapshot.sync_execution.live_slots, 0);
        assert_eq!(snapshot.sync_execution.live_frames, 0);
        assert_eq!(snapshot.sync_execution.active, 0);
        assert!(snapshot.sync_execution.snapshot_only);
        let error = snapshot.execute("h.job=1", self).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(self.jobs, 0);
        assert_eq!(snapshot.collect_garbage(&[], self)?.reclaimed_objects, 0);
        let before_work = runtime.sync_execution.work_remaining;
        let error = runtime
            .execute_nested_script("1 + missing", self)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Runtime);
        let spent = runtime.sync_execution.work_remaining;
        assert!(spent < before_work);
        runtime.rollback_to(snapshot);
        assert_eq!(runtime.sync_execution.work_remaining, spent);
        assert_eq!(
            (runtime.sync_execution.active, runtime.eval_depth),
            (active, native)
        );
        assert_eq!(
            (
                runtime.sync_execution.live_slots,
                runtime.sync_execution.live_frames
            ),
            (slots, frames)
        );
        assert!(!runtime.sync_execution.snapshot_only);
        Ok(())
    }
}

#[test]
fn host_setter_cannot_collect_checkpoint_or_duplicate_ancestral_leases() {
    let mut runtime = Runtime::default();
    let mut host = Reenter::default();
    assert_eq!(
        runtime
            .execute(
                "queueMicrotask(()=>h.job=1);let result=[{x:7},h.check=1];result[0].x",
                &mut host
            )
            .unwrap(),
        Value::Number(7.)
    );
    assert_eq!((host.checks, host.jobs), (1, 1));
    assert!(host.max_slots > 0);
    assert_eq!(
        (
            runtime.sync_execution.active,
            runtime.sync_execution.live_slots,
            runtime.sync_execution.live_frames
        ),
        (0, 0, 0)
    );
}

#[test]
fn width_uses_cursors_and_does_not_accumulate_parent_frames() {
    // Each grouped left spine stays within the parser's local chain limit.
    // The parser still produces one flat Sequence with 1000 borrowed items.
    let source = format!(
        "{}h.check=1,h.check=1{}",
        "(".repeat(998),
        "),h.check=1".repeat(998)
    );
    let mut runtime = Runtime::default();
    let mut host = Reenter::default();
    assert_eq!(
        runtime.execute(&source, &mut host).unwrap(),
        Value::Number(1.)
    );
    assert_eq!(host.checks, 1000);
    assert_eq!(host.max_slots, 4);
    assert_eq!(
        (
            runtime.sync_execution.live_slots,
            runtime.sync_execution.live_frames
        ),
        (0, 0)
    );
}

#[test]
fn finalizer_microtask_checkpoint_relaxes_only_the_outer_cleanup_loop_marker() {
    let mut runtime = Runtime::default();
    let mut host = Reenter::default();
    // Queue work without performing an outer execution checkpoint yet.
    runtime.call_depth = 1;
    runtime
        .execute("let jobs=0;queueMicrotask(()=>jobs++);", &mut host)
        .unwrap();
    runtime.call_depth = 0;
    runtime.finalization_registries.draining = true;
    for guard in 0..9 {
        runtime.call_depth = usize::from(guard == 0);
        runtime.eval_depth = usize::from(guard == 1);
        runtime.statement_depth = usize::from(guard == 2);
        runtime.conversion_depth = usize::from(guard == 3);
        runtime.dynamic_depth = usize::from(guard == 4);
        runtime.nested_script_depth = usize::from(guard == 5);
        runtime.sync_execution.active = usize::from(guard == 6);
        runtime.sync_execution.host_callbacks = usize::from(guard == 7);
        runtime.sync_execution.snapshot_only = guard == 8;
        runtime
            .drain_promise_jobs_after_finalizer(&mut host, &mut 0)
            .unwrap();
        assert_eq!(runtime.global_value("jobs"), Some(Value::Number(0.)));
        assert!(runtime.execution_is_active());
    }
    runtime.call_depth = 0;
    runtime.eval_depth = 0;
    runtime.statement_depth = 0;
    runtime.conversion_depth = 0;
    runtime.dynamic_depth = 0;
    runtime.nested_script_depth = 0;
    runtime.sync_execution.active = 0;
    runtime.sync_execution.host_callbacks = 0;
    runtime.sync_execution.snapshot_only = false;
    assert!(runtime.execution_is_active());
    assert!(!runtime.page_execution_is_active());
    // Ordinary callers still cannot drain while the cleanup loop is marked.
    runtime.drain_promise_jobs(&mut host).unwrap();
    assert_eq!(runtime.global_value("jobs"), Some(Value::Number(0.)));
    runtime
        .drain_promise_jobs_after_finalizer(&mut host, &mut 0)
        .unwrap();
    assert_eq!(runtime.global_value("jobs"), Some(Value::Number(1.)));
    assert!(runtime.execution_is_active());
    runtime.finalization_registries.draining = false;
    assert!(!runtime.execution_is_active());
}

#[test]
fn scheduler_resource_remains_fatal_and_never_runs_catch_finally_or_jobs() {
    let mut runtime = Runtime::default();
    let mut host = Reenter::default();
    runtime
        .execute("let caught=0;let finalized=0;", &mut host)
        .unwrap();
    runtime.sync_execution.work_remaining = 1;
    let error = runtime
        .execute(
            "try{h.check=1}catch(e){caught=1}finally{finalized=1}",
            &mut host,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.thrown.is_none());
    assert_eq!(runtime.global_value("caught"), Some(Value::Number(0.)));
    assert_eq!(runtime.global_value("finalized"), Some(Value::Number(0.)));
    assert_eq!((host.checks, host.jobs), (0, 0));
    assert_eq!(
        (
            runtime.sync_execution.active,
            runtime.sync_execution.live_slots
        ),
        (0, 0)
    );
}

#[derive(Default)]
struct NativeSnapshots {
    late: Option<Runtime>,
    invalidate: bool,
}

impl NativeSnapshots {
    fn callback(&mut self, runtime: &mut Runtime) -> Result<Value, JsError> {
        assert_eq!(runtime.eval_depth, 0);
        assert_eq!(runtime.call_depth, 1);
        if self.invalidate {
            runtime.rollback_to(self.late.take().unwrap());
            return Ok(Value::Host(9));
        }
        let snapshot = runtime.clone();
        self.late = Some(runtime.clone());
        assert!(snapshot.sync_execution.snapshot_only);
        runtime.execute_nested_script("1 + 2", self)?;
        runtime.rollback_to(snapshot);
        assert_eq!(runtime.call_depth, 1);
        assert!(!runtime.sync_execution.snapshot_only);
        Ok(Value::Host(9))
    }
}

impl Host for NativeSnapshots {
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
        panic!("legacy call")
    }
    fn is_callable(&self, _: u64) -> bool {
        true
    }
    fn is_constructor(&self, _: u64) -> bool {
        true
    }
    fn call_with_runtime(
        &mut self,
        _: u64,
        _: &str,
        _: &[Value],
        _: &Value,
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        self.callback(runtime)
    }
    fn construct_with_runtime(
        &mut self,
        _: u64,
        _: &[Value],
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        self.callback(runtime)
    }
}

#[test]
fn host_call_and_constructor_snapshot_anchors_cover_entries_without_eval() {
    for construct in [false, true] {
        let mut runtime = Runtime::default();
        let mut host = NativeSnapshots::default();
        let result = if construct {
            runtime.construct(Value::Host(7), Vec::new(), &mut host)
        } else {
            runtime.invoke(Value::Host(7), Vec::new(), &mut host)
        };
        assert_eq!(result.unwrap(), Value::Host(9));
        assert_eq!((runtime.call_depth, runtime.eval_depth), (0, 0));
        let snapshot = host.late.take().unwrap();
        runtime.rollback_to(snapshot);
        assert!(runtime.sync_execution.snapshot_only);
        assert_eq!(
            runtime.execute("1", &mut host).unwrap_err().kind,
            ErrorKind::Resource
        );
    }
}

#[test]
fn invalid_rollback_aborts_the_current_host_call_or_construct() {
    for construct in [false, true] {
        let mut runtime = Runtime::default();
        let mut host = NativeSnapshots::default();
        let first = if construct {
            runtime.construct(Value::Host(7), Vec::new(), &mut host)
        } else {
            runtime.invoke(Value::Host(7), Vec::new(), &mut host)
        };
        assert_eq!(first.unwrap(), Value::Host(9));
        host.invalidate = true;
        let error = if construct {
            runtime.construct(Value::Host(7), Vec::new(), &mut host)
        } else {
            runtime.invoke(Value::Host(7), Vec::new(), &mut host)
        }
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(error.thrown.is_none());
        assert_eq!(
            (
                runtime.call_depth,
                runtime.eval_depth,
                runtime.sync_execution.host_callbacks
            ),
            (0, 0, 0)
        );
    }
}

#[derive(Default)]
struct DelayedSnapshot {
    saved: Option<Runtime>,
    restores: usize,
    added_objects: usize,
}

impl Host for DelayedSnapshot {
    fn global(&mut self, name: &str) -> Option<Value> {
        (name == "h").then_some(Value::Host(1))
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        Ok(Value::Undefined)
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("missing runtime")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("unexpected call")
    }
    fn set_with_runtime(
        &mut self,
        _: u64,
        property: &str,
        _: Value,
        runtime: &mut Runtime,
    ) -> Result<(), JsError> {
        if property == "save" {
            self.saved = Some(runtime.clone());
        } else if property == "restore" {
            let snapshot = self.saved.take().unwrap();
            self.added_objects = runtime.heap.len() - snapshot.heap.len();
            let current_heap = runtime.heap.len();
            let current_slots = runtime.sync_execution.live_slots;
            runtime.rollback_to(snapshot);
            self.restores += 1;
            // A stale rollback must not remove objects held by a cursor or by
            // the owned resumable machine. The callback wrapper then emits the
            // fatal Resource before any pending operation can use those Values.
            assert_eq!(runtime.heap.len(), current_heap);
            assert_eq!(runtime.sync_execution.live_slots, current_slots);
            assert!(runtime.sync_execution.snapshot_only);
        }
        Ok(())
    }
}

#[test]
fn a_later_host_callback_cannot_restore_an_earlier_phase_of_the_same_dispatcher() {
    for source in [
        "[h.save=1,{x:42},h.restore=1]",
        "function* g(){let a=[h.save=1,{x:42},h.restore=1];yield a;}let i=g();i.next()",
        // Pattern traversal is one explicit synchronous bridge; multiple Host
        // setters within that same phase still receive distinct callback epochs.
        "[h.save,h.restore]=[1,2]",
    ] {
        let mut runtime = Runtime::default();
        let mut host = DelayedSnapshot::default();
        let error = runtime.execute(source, &mut host).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource, "{source}");
        assert!(error.thrown.is_none());
        assert_eq!(host.restores, 1);
        if !source.starts_with("[h.save,h.restore]") {
            assert!(host.added_objects > 0);
        }
        assert_eq!(
            (
                runtime.eval_depth,
                runtime.call_depth,
                runtime.statement_depth
            ),
            (0, 0, 0)
        );
        assert_eq!(
            (
                runtime.sync_execution.active,
                runtime.sync_execution.host_callbacks,
                runtime.sync_execution.live_slots,
                runtime.sync_execution.live_frames
            ),
            (0, 0, 0, 0)
        );
    }
}
