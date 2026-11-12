//! Shared live capacity and non-refundable scheduler work for synchronous eval.
//!
//! Phase table: leaves/explicit bridges 0; unary, binary, conditional, sequence,
//! reference/assignment/update/delete, call/arguments, array/object/template and
//! import each retain at most ONE frame per physical AST ancestor (K=1).
//! A phase is replaced after pop, never linked through a Box. R=1 allows a root
//! cursor. Parser height remains 64, ordinary call depth remains 16. A rounded
//! 128-slot capacity covers K*64+R. Seventeen ordinary dispatchers fit the byte
//! envelope even at the asserted conservative frame layout. Bridges have the
//! separate native-64 and aggregate-slot envelope; the byte gate may bind first.
use super::sync_frames::SyncFrame;
use crate::{JsError, Runtime};
use std::mem::size_of;
use std::sync::Arc;

pub(crate) const MAX_NATIVE_EVAL_DEPTH: usize = 64;
// Independent native Host callback guard, no looser than ordinary calls.
pub(crate) const MAX_HOST_CALLBACK_DEPTH: usize = crate::MAX_CALL_DEPTH;
const PHYSICAL_AST_HEIGHT: usize = 64;
const FRAMES_PER_AST_LEVEL: usize = 1;
const ROOT_CURSOR_SLOTS: usize = 1;
const DISPATCH_FRAME_BOUND: usize = FRAMES_PER_AST_LEVEL * PHYSICAL_AST_HEIGHT + ROOT_CURSOR_SLOTS;
const DISPATCH_CAPACITY: usize = DISPATCH_FRAME_BOUND.next_power_of_two();
const ORDINARY_DISPATCHERS: usize = crate::MAX_CALL_DEPTH + 1;
const MAX_FRAME_LAYOUT_BYTES: usize = 512;
pub(crate) const MAX_LIVE_FRAMES: usize = MAX_NATIVE_EVAL_DEPTH * DISPATCH_FRAME_BOUND;
pub(crate) const MAX_LIVE_SLOTS: usize = MAX_NATIVE_EVAL_DEPTH * DISPATCH_CAPACITY;
pub(crate) const MAX_LIVE_BYTES: usize =
    ORDINARY_DISPATCHERS * DISPATCH_CAPACITY * MAX_FRAME_LAYOUT_BYTES;
// Work units: one per loop transition/push/pop and one per requested or moved
// frame slot on growth. These are not bytes or JavaScript semantic ticks.
const WORK_PER_INSTRUCTION: u64 = 16;
const MAX_SCHEDULER_WORK: u64 = 32_000_000;
const _: () = assert!(size_of::<SyncFrame<'static>>() <= MAX_FRAME_LAYOUT_BYTES);
const _: () = assert!(DISPATCH_CAPACITY == 128);

#[derive(Debug)]
pub(crate) struct SyncExecution {
    pub(crate) active: usize,
    pub(crate) host_callbacks: usize,
    pub(crate) snapshot_only: bool,
    // Retained Vec entries only. A frame moved to apply no longer counts here;
    // its capacity remains charged and the active barrier still protects it.
    pub(crate) live_frames: usize,
    pub(crate) live_slots: usize,
    pub(crate) live_bytes: usize,
    pub(crate) work_remaining: u64,
    // Identity is shared only to authenticate snapshots, never to share credit.
    identity: Arc<()>,
    epoch: u64,
    next_epoch: u64,
    snapshot_anchor: Option<SnapshotAnchor>,
}

#[derive(Clone, Debug)]
struct SnapshotAnchor {
    identity: Arc<()>,
    epoch: u64,
    active: usize,
    native: usize,
    call: usize,
    statement: usize,
    conversion: usize,
    dynamic: usize,
    nested: usize,
    host_callbacks: usize,
}

impl SnapshotAnchor {
    fn capture(runtime: &Runtime) -> Self {
        Self {
            identity: Arc::clone(&runtime.sync_execution.identity),
            epoch: runtime.sync_execution.epoch,
            active: runtime.sync_execution.active,
            native: runtime.eval_depth,
            call: runtime.call_depth,
            statement: runtime.statement_depth,
            conversion: runtime.conversion_depth,
            dynamic: runtime.dynamic_depth,
            nested: runtime.nested_script_depth,
            host_callbacks: runtime.sync_execution.host_callbacks,
        }
    }

    fn matches(&self, runtime: &Runtime) -> bool {
        Arc::ptr_eq(&self.identity, &runtime.sync_execution.identity)
            && self.epoch == runtime.sync_execution.epoch
            && self.active == runtime.sync_execution.active
            && self.native == runtime.eval_depth
            && self.call == runtime.call_depth
            && self.statement == runtime.statement_depth
            && self.conversion == runtime.conversion_depth
            && self.dynamic == runtime.dynamic_depth
            && self.nested == runtime.nested_script_depth
            && self.host_callbacks == runtime.sync_execution.host_callbacks
    }
}

impl SyncExecution {
    pub(crate) fn new(instructions: u64) -> Self {
        Self {
            active: 0,
            host_callbacks: 0,
            snapshot_only: false,
            live_frames: 0,
            live_slots: 0,
            live_bytes: 0,
            work_remaining: instructions
                .checked_mul(WORK_PER_INSTRUCTION)
                .unwrap_or(MAX_SCHEDULER_WORK)
                .min(MAX_SCHEDULER_WORK),
            identity: Arc::new(()),
            epoch: 0,
            next_epoch: 0,
            snapshot_anchor: None,
        }
    }

    fn charge_work(&mut self, work: usize) -> Result<(), JsError> {
        let work = u64::try_from(work)
            .map_err(|_| JsError::resource("Synchronous evaluator work overflow"))?;
        if work > self.work_remaining {
            self.work_remaining = 0;
            return Err(JsError::resource(
                "Synchronous evaluator work budget exceeded",
            ));
        }
        self.work_remaining -= work;
        Ok(())
    }

    fn preflight_capacity(&self, additional: usize) -> Result<(), JsError> {
        let slots = self
            .live_slots
            .checked_add(additional)
            .ok_or_else(|| JsError::resource("Synchronous evaluator storage overflow"))?;
        let bytes = additional
            .checked_mul(size_of::<SyncFrame<'static>>())
            .and_then(|bytes| self.live_bytes.checked_add(bytes))
            .ok_or_else(|| JsError::resource("Synchronous evaluator storage overflow"))?;
        if slots > MAX_LIVE_SLOTS {
            return Err(JsError::resource(
                "Synchronous evaluator capacity budget exceeded",
            ));
        }
        if bytes > MAX_LIVE_BYTES {
            return Err(JsError::resource(
                "Synchronous evaluator byte budget exceeded",
            ));
        }
        Ok(())
    }
}

// A clone never owns the original dispatchers. Runtime::clone additionally
// marks active clones snapshot-only: they preserve active language context for
// rollback but cannot execute or collect as independent VMs. rollback_to keeps
// the CURRENT live ledger after nested children have been dismantled.
impl Clone for SyncExecution {
    fn clone(&self) -> Self {
        Self {
            active: 0,
            host_callbacks: 0,
            snapshot_only: self.snapshot_only,
            live_frames: 0,
            live_slots: 0,
            live_bytes: 0,
            work_remaining: self.work_remaining,
            identity: Arc::new(()),
            epoch: 0,
            next_epoch: 0,
            snapshot_anchor: self.snapshot_anchor.clone(),
        }
    }
}

pub(crate) struct SyncStack<'ast> {
    frames: Vec<SyncFrame<'ast>>,
    charged_capacity: usize,
}

impl<'ast> SyncStack<'ast> {
    pub(crate) fn new() -> Self {
        Self {
            frames: Vec::new(),
            charged_capacity: 0,
        }
    }

    /// Preflight before a getter/coercion/builder effect whose continuation must
    /// be retained. The reservation remains live until finish destroys the Vec.
    pub(crate) fn ensure_slot(&mut self, runtime: &mut Runtime) -> Result<(), JsError> {
        if runtime.sync_execution.live_frames >= MAX_LIVE_FRAMES {
            return Err(JsError::resource(
                "Synchronous evaluator frame budget exceeded",
            ));
        }
        if self.frames.len() < self.frames.capacity() {
            return Ok(());
        }
        let old = self.frames.capacity();
        let next = old
            .checked_mul(2)
            .ok_or_else(|| JsError::resource("Synchronous evaluator capacity overflow"))?
            .clamp(4, MAX_LIVE_SLOTS);
        if next <= old {
            return Err(JsError::resource(
                "Synchronous evaluator capacity budget exceeded",
            ));
        }
        let additional = next
            .checked_sub(old)
            .ok_or_else(|| JsError::resource("Synchronous evaluator capacity overflow"))?;
        runtime.sync_execution.preflight_capacity(additional)?;
        // Growth/copy cost is cumulative, including failed allocation attempts.
        runtime.sync_execution.charge_work(
            next.checked_add(old)
                .ok_or_else(|| JsError::resource("Synchronous evaluator work overflow"))?,
        )?;
        self.frames
            .try_reserve_exact(next - self.frames.len())
            .map_err(|_| JsError::resource("Synchronous evaluator storage unavailable"))?;
        let actual = self.frames.capacity() - old;
        // Validate the allocator's actual result. If it rounded beyond the
        // budget, destroy this buffer before releasing its old reservation.
        // There is no callback/collection window during this reconciliation.
        if let Err(error) = runtime.sync_execution.preflight_capacity(actual) {
            let logical = self.frames.len();
            drop(std::mem::take(&mut self.frames));
            runtime.sync_execution.live_frames -= logical;
            runtime.sync_execution.live_slots -= self.charged_capacity;
            runtime.sync_execution.live_bytes -=
                self.charged_capacity * size_of::<SyncFrame<'ast>>();
            self.charged_capacity = 0;
            return Err(error);
        }
        runtime.sync_execution.live_slots += actual;
        runtime.sync_execution.live_bytes += actual * size_of::<SyncFrame<'ast>>();
        self.charged_capacity = self.frames.capacity();
        runtime
            .sync_execution
            .charge_work(actual.saturating_sub(additional))
    }

    pub(crate) fn push(
        &mut self,
        runtime: &mut Runtime,
        frame: SyncFrame<'ast>,
    ) -> Result<(), JsError> {
        self.ensure_slot(runtime)?;
        runtime.sync_execution.charge_work(1)?;
        self.frames.push(frame);
        runtime.sync_execution.live_frames += 1;
        Ok(())
    }

    pub(crate) fn pop(
        &mut self,
        runtime: &mut Runtime,
    ) -> Result<Option<SyncFrame<'ast>>, JsError> {
        runtime.sync_execution.charge_work(1)?;
        let frame = self.frames.pop();
        if frame.is_some() {
            runtime.sync_execution.live_frames -= 1;
        }
        Ok(frame)
    }

    /// No page code runs during teardown; active remains set until after this.
    pub(crate) fn finish(self, runtime: &mut Runtime) {
        let logical = self.frames.len();
        let capacity = self.charged_capacity;
        drop(self.frames);
        runtime.sync_execution.live_frames -= logical;
        runtime.sync_execution.live_slots -= capacity;
        runtime.sync_execution.live_bytes -= capacity * size_of::<SyncFrame<'ast>>();
    }
}

impl Runtime {
    /// Every Host callback with access to this runtime gets a fresh identity,
    /// including setters that do not enter the ordinary call guard. A snapshot
    /// may roll back nested work within this callback, but cannot be replayed by
    /// a later callback in the same expression/bridge/resumable phase. Host
    /// callbacks without a Runtime argument cannot clone this VM through the
    /// safe Host API. The identity token contains no memory/budget credit.
    pub(crate) fn with_host_callback<T>(
        &mut self,
        callback: impl FnOnce(&mut Self) -> Result<T, JsError>,
    ) -> Result<T, JsError> {
        self.ensure_executable()?;
        let callbacks = self.sync_execution.host_callbacks;
        if callbacks >= MAX_HOST_CALLBACK_DEPTH {
            return Err(JsError::resource("Host callback depth budget exceeded"));
        }
        let entered = callbacks
            .checked_add(1)
            .ok_or_else(|| JsError::resource("Host execution barrier overflow"))?;
        let epoch = self.enter_execution_epoch()?;
        self.sync_execution.host_callbacks = entered;
        let result = callback(self);
        self.sync_execution.host_callbacks = callbacks;
        self.leave_execution_epoch(epoch);
        // Incompatible rollback never lets the caller use pending Values as if
        // it succeeded. Error construction is inert, without page conversion.
        self.ensure_executable()?;
        result
    }

    pub(crate) fn sync_capture_snapshot(&self, snapshot: &mut Runtime) {
        snapshot.sync_execution.snapshot_only = self.execution_is_active();
        if !self.sync_execution.snapshot_only && self.execution_is_active() {
            snapshot.sync_execution.snapshot_anchor = Some(SnapshotAnchor::capture(self));
        }
    }

    pub(crate) fn sync_snapshot_compatible(&self, snapshot: &Runtime) -> bool {
        if self.sync_execution.snapshot_only {
            return false;
        }
        if snapshot.sync_execution.snapshot_only {
            snapshot
                .sync_execution
                .snapshot_anchor
                .as_ref()
                .is_some_and(|anchor| anchor.matches(self))
        } else {
            !self.execution_is_active() && self.sync_execution.live_slots == 0
        }
    }

    /// Enter a native activation. The previous epoch is restored at exit while
    /// next_epoch is never rolled back, distinguishing later equal-depth calls.
    pub(crate) fn enter_execution_epoch(&mut self) -> Result<u64, JsError> {
        self.ensure_executable()?;
        let next = self
            .sync_execution
            .next_epoch
            .checked_add(1)
            .ok_or_else(|| JsError::resource("Execution activation identity exhausted"))?;
        let previous = self.sync_execution.epoch;
        self.sync_execution.next_epoch = next;
        self.sync_execution.epoch = next;
        Ok(previous)
    }

    pub(crate) fn leave_execution_epoch(&mut self, previous: u64) {
        self.sync_execution.epoch = previous;
    }

    pub(crate) fn sync_transition(&mut self) -> Result<(), JsError> {
        self.ensure_executable()?;
        self.sync_execution.charge_work(1)
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
