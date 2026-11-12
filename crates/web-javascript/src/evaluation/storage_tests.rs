use super::*;
use crate::{ErrorKind, Expr, Host, Limits, Value, evaluation::sync_frames::CoreFrame};

struct Empty;
impl Host for Empty {
    fn global(&mut self, _: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
        panic!("unexpected get")
    }
    fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
        panic!("unexpected set")
    }
    fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
        panic!("unexpected call")
    }
}

fn frame() -> SyncFrame<'static> {
    SyncFrame::Core(CoreFrame::Unary("!"))
}

#[test]
fn frame_pop_retains_actual_capacity_until_buffer_destruction() {
    let mut runtime = Runtime::default();
    let mut stack = SyncStack::new();
    let work = runtime.sync_execution.work_remaining;
    stack.push(&mut runtime, frame()).unwrap();
    let capacity = stack.frames.capacity();
    assert_eq!(runtime.sync_execution.live_slots, capacity);
    assert_eq!(
        runtime.sync_execution.live_bytes,
        capacity * size_of::<SyncFrame<'static>>()
    );
    assert_eq!(runtime.sync_execution.live_frames, 1);
    drop(stack.pop(&mut runtime).unwrap());
    assert_eq!(runtime.sync_execution.live_frames, 0);
    assert_eq!(runtime.sync_execution.live_slots, capacity);
    stack.finish(&mut runtime);
    assert_eq!(
        (
            runtime.sync_execution.live_slots,
            runtime.sync_execution.live_bytes
        ),
        (0, 0)
    );
    assert!(runtime.sync_execution.work_remaining < work);
}

#[test]
fn simultaneous_dispatchers_share_capacity_and_release_only_their_own_buffer() {
    let mut runtime = Runtime::default();
    let mut ancestor = SyncStack::new();
    ancestor.push(&mut runtime, frame()).unwrap();
    let ancestral = runtime.sync_execution.live_slots;
    let mut child = SyncStack::new();
    child.push(&mut runtime, frame()).unwrap();
    assert_eq!(
        runtime.sync_execution.live_slots,
        ancestral + child.frames.capacity()
    );
    child.finish(&mut runtime);
    assert_eq!(runtime.sync_execution.live_slots, ancestral);
    assert_eq!(runtime.sync_execution.live_frames, 1);
    ancestor.finish(&mut runtime);
    assert_eq!(runtime.sync_execution.live_slots, 0);
}

#[test]
fn logical_frame_limit_is_independent_of_reserved_slots() {
    let mut runtime = Runtime::default();
    let mut stack = SyncStack::new();
    stack.ensure_slot(&mut runtime).unwrap();
    runtime.sync_execution.live_frames = MAX_LIVE_FRAMES - 1;
    stack.push(&mut runtime, frame()).unwrap();
    assert_eq!(runtime.sync_execution.live_frames, MAX_LIVE_FRAMES);
    let error = stack.push(&mut runtime, frame()).unwrap_err();
    assert_eq!(error.message, "Synchronous evaluator frame budget exceeded");
    assert_eq!(error.kind, ErrorKind::Resource);
    assert!(error.thrown.is_none());
    stack.finish(&mut runtime);
    assert_eq!(runtime.sync_execution.live_frames, MAX_LIVE_FRAMES - 1);
}

#[test]
fn actual_capacity_and_byte_preflight_check_below_at_and_above_limits() {
    let mut state = SyncExecution::new(100);
    state.live_slots = MAX_LIVE_SLOTS - 1;
    assert!(state.preflight_capacity(0).is_ok());
    assert!(state.preflight_capacity(1).is_ok());
    assert_eq!(
        state.preflight_capacity(2).unwrap_err().message,
        "Synchronous evaluator capacity budget exceeded"
    );
    state.live_slots = 0;
    let size = size_of::<SyncFrame<'static>>();
    state.live_bytes = MAX_LIVE_BYTES - size;
    assert!(state.preflight_capacity(0).is_ok());
    assert!(state.preflight_capacity(1).is_ok());
    assert_eq!(
        state.preflight_capacity(2).unwrap_err().message,
        "Synchronous evaluator byte budget exceeded"
    );
    state.live_slots = usize::MAX;
    assert_eq!(
        state.preflight_capacity(1).unwrap_err().message,
        "Synchronous evaluator storage overflow"
    );
}

#[test]
fn consumed_scheduler_work_is_bounded_non_refundable_and_never_refreshed() {
    let mut runtime = Runtime::new(Limits {
        instructions: 100,
        ..Limits::default()
    });
    assert_eq!(
        runtime.sync_execution.work_remaining,
        100 * WORK_PER_INSTRUCTION
    );
    assert_eq!(
        SyncExecution::new(u64::MAX).work_remaining,
        MAX_SCHEDULER_WORK
    );
    let snapshot = runtime.clone();
    runtime
        .eval(&Expr::Literal(Value::Number(1.)), 0, &mut Empty)
        .unwrap();
    let remaining = runtime.sync_execution.work_remaining;
    runtime.rollback_to(snapshot);
    assert_eq!(runtime.sync_execution.work_remaining, remaining);
    runtime.sync_execution.work_remaining = 0;
    let snapshot = runtime.clone();
    let error = runtime
        .eval(&Expr::Literal(Value::Number(1.)), 0, &mut Empty)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "Synchronous evaluator work budget exceeded");
    assert!(error.thrown.is_none());
    assert_eq!((runtime.sync_execution.active, runtime.eval_depth), (0, 0));
    runtime.rollback_to(snapshot);
    assert_eq!(runtime.sync_execution.work_remaining, 0);
}

#[test]
fn native_guard_counts_dispatchers_not_expression_children_and_restores_entry() {
    let mut runtime = Runtime {
        eval_depth: MAX_NATIVE_EVAL_DEPTH - 1,
        ..Runtime::default()
    };
    runtime.sync_execution.active = 3;
    let expression = Expr::Binary(
        "+".into(),
        Box::new(Expr::Literal(Value::Number(2.))),
        Box::new(Expr::Literal(Value::Number(3.))),
    );
    assert_eq!(
        runtime.eval(&expression, 0, &mut Empty).unwrap(),
        Value::Number(5.)
    );
    assert_eq!(
        (runtime.eval_depth, runtime.sync_execution.active),
        (MAX_NATIVE_EVAL_DEPTH - 1, 3)
    );
    runtime.eval_depth = MAX_NATIVE_EVAL_DEPTH;
    let before = runtime.remaining;
    let error = runtime.eval(&expression, 0, &mut Empty).unwrap_err();
    assert_eq!(error.message, "Expression evaluation depth exceeded");
    assert_eq!(runtime.remaining, before - 1);
    assert_eq!(
        (runtime.eval_depth, runtime.sync_execution.active),
        (MAX_NATIVE_EVAL_DEPTH, 3)
    );
    assert_eq!(
        (
            runtime.sync_execution.live_slots,
            runtime.sync_execution.live_frames
        ),
        (0, 0)
    );
}

#[test]
fn normal_error_destroys_all_frames_and_preserves_native_parent_guard() {
    let mut runtime = Runtime {
        eval_depth: 2,
        ..Runtime::default()
    };
    runtime.sync_execution.active = 2;
    let expression = Expr::Binary(
        "+".into(),
        Box::new(Expr::Literal(Value::Number(1.))),
        Box::new(Expr::Name("missing".into())),
    );
    let error = runtime.eval(&expression, 0, &mut Empty).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Runtime);
    assert_eq!((runtime.eval_depth, runtime.sync_execution.active), (2, 2));
    assert_eq!(
        (
            runtime.sync_execution.live_slots,
            runtime.sync_execution.live_frames
        ),
        (0, 0)
    );
}

#[test]
fn idle_clones_do_not_share_capacity_or_scheduler_credit() {
    let mut runtime = Runtime::default();
    let mut clone = runtime.clone();
    let before = runtime.sync_execution.work_remaining;
    clone
        .eval(&Expr::Literal(Value::Number(1.)), 0, &mut Empty)
        .unwrap();
    assert_eq!(runtime.sync_execution.work_remaining, before);
    let mut stack = SyncStack::new();
    stack.push(&mut runtime, frame()).unwrap();
    assert_eq!(clone.sync_execution.live_slots, 0);
    stack.finish(&mut runtime);
}

#[test]
fn an_active_snapshot_cannot_restore_a_later_equal_depth_activation() {
    let mut runtime = Runtime::default();
    let first = runtime.enter_execution_epoch().unwrap();
    runtime.sync_execution.active = 1;
    runtime.eval_depth = 1;
    let snapshot = runtime.clone();
    runtime.sync_execution.active = 0;
    runtime.eval_depth = 0;
    runtime.leave_execution_epoch(first);
    let second = runtime.enter_execution_epoch().unwrap();
    runtime.sync_execution.active = 1;
    runtime.eval_depth = 1;
    let mut stack = SyncStack::new();
    stack.push(&mut runtime, frame()).unwrap();
    let slots = runtime.sync_execution.live_slots;
    runtime.rollback_to(snapshot);
    assert!(runtime.sync_execution.snapshot_only);
    assert_eq!(runtime.sync_execution.live_slots, slots);
    assert_eq!(runtime.sync_execution.live_frames, 1);
    stack.finish(&mut runtime);
    runtime.sync_execution.active = 0;
    runtime.eval_depth = 0;
    runtime.leave_execution_epoch(second);
    assert_eq!(
        runtime.execute("1", &mut Empty).unwrap_err().kind,
        ErrorKind::Resource
    );
}

#[test]
fn equal_epochs_and_counters_do_not_authorize_a_foreign_active_snapshot() {
    let mut origin = Runtime::default();
    let mut target = Runtime::default();
    let origin_epoch = origin.enter_execution_epoch().unwrap();
    let target_epoch = target.enter_execution_epoch().unwrap();
    origin.sync_execution.active = 1;
    target.sync_execution.active = 1;
    origin.eval_depth = 1;
    target.eval_depth = 1;
    assert_eq!(origin.sync_execution.epoch, target.sync_execution.epoch);
    let snapshot = origin.clone();
    let mut stack = SyncStack::new();
    stack.push(&mut target, frame()).unwrap();
    let slots = target.sync_execution.live_slots;
    target.rollback_to(snapshot);
    assert!(target.sync_execution.snapshot_only);
    assert!(!origin.sync_execution.snapshot_only);
    assert_eq!(target.sync_execution.live_slots, slots);
    stack.finish(&mut target);
    origin.leave_execution_epoch(origin_epoch);
    target.leave_execution_epoch(target_epoch);
}

#[test]
fn idle_snapshot_cannot_replace_heap_under_a_live_dispatcher() {
    let mut runtime = Runtime::default();
    let snapshot = runtime.clone();
    let mut stack = SyncStack::new();
    stack.push(&mut runtime, frame()).unwrap();
    runtime.sync_execution.active = 1;
    runtime.eval_depth = 1;
    let slots = runtime.sync_execution.live_slots;
    runtime.rollback_to(snapshot);
    assert!(runtime.sync_execution.snapshot_only);
    assert_eq!(
        (
            runtime.sync_execution.live_slots,
            runtime.sync_execution.live_frames
        ),
        (slots, 1)
    );
    stack.finish(&mut runtime);
}

#[test]
fn host_callback_native_guard_is_bounded_and_restores_its_ancestor() {
    let mut runtime = Runtime::default();
    runtime.sync_execution.host_callbacks = MAX_HOST_CALLBACK_DEPTH - 1;
    assert_eq!(
        runtime
            .with_host_callback(|runtime| {
                assert_eq!(
                    runtime.sync_execution.host_callbacks,
                    MAX_HOST_CALLBACK_DEPTH
                );
                Ok(7)
            })
            .unwrap(),
        7
    );
    assert_eq!(
        runtime.sync_execution.host_callbacks,
        MAX_HOST_CALLBACK_DEPTH - 1
    );
    runtime.sync_execution.host_callbacks = MAX_HOST_CALLBACK_DEPTH;
    let error = runtime
        .with_host_callback::<()>(|_| panic!("guard must stop callback"))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, "Host callback depth budget exceeded");
    assert_eq!(
        runtime.sync_execution.host_callbacks,
        MAX_HOST_CALLBACK_DEPTH
    );
}
