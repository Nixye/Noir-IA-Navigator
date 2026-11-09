//! Explicit renderer opportunities, independent of the logical timer clock.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub use browser_ipc::MAX_ANIMATION_FRAME_TIMESTAMP_MS;
pub(super) const REQUEST: u64 = 17;
pub(super) const CANCEL: u64 = 18;
const MAX_PENDING: usize = 32;
const MAX_HANDLE: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone)]
pub(super) struct FrameCallback {
    id: u64,
    pub(super) callback: Value,
}
#[derive(Debug, Clone, Default)]
pub(super) struct AnimationState {
    pub(super) pending: Vec<FrameCallback>,
    // State transaction snapshots share identity consumption, never rewind it.
    next_handle: Arc<AtomicU64>,
}

impl DocumentSession {
    pub fn animation_frame_pending(&self) -> bool {
        !self.scripts_disabled && !self.state.animation.pending.is_empty()
    }
    /// Runs one complete bounded snapshot before the caller's next display list.
    /// The host supplies real monotonic milliseconds since this document's origin.
    /// Equal timestamps are permitted; invalid/backwards pulses change nothing.
    pub fn run_animation_frame(&mut self, timestamp_ms: f64) -> Result<(), JsError> {
        browser_ipc::validate_animation_frame_timestamp(
            timestamp_ms,
            self.last_animation_frame_timestamp_ms,
        )
        .map_err(JsError::runtime)?;
        self.last_animation_frame_timestamp_ms = Some(timestamp_ms);
        self.document_url_frozen = true;
        self.storage_started = true;
        self.media_started = true;
        if self.scripts_disabled {
            return Ok(());
        }
        // Keep only IDs in the snapshot. Pending callbacks remain host GC roots,
        // and cancellation can remove any not-yet-invoked member of this batch.
        let ready = self
            .state
            .animation
            .pending
            .iter()
            .map(|frame| frame.id)
            .collect::<Vec<_>>();
        let mut first_error = None;
        for id in ready {
            let Some(index) = self
                .state
                .animation
                .pending
                .iter()
                .position(|frame| frame.id == id)
            else {
                continue;
            };
            let frame = self.state.animation.pending.remove(index);
            let old_document = self.document.clone();
            let old_state = self.state.clone();
            let old_runtime = self.runtime.clone();
            // invoke_with_this roots callback/argument/receiver before any GC,
            // and performs the existing execution-job checkpoint after the call.
            let mut result = self.runtime.invoke_with_this(
                frame.callback,
                vec![Value::Number(timestamp_ms)],
                Value::Host(WINDOW),
                &mut DomHost {
                    document: &mut self.document,
                    state: &mut self.state,
                },
            );
            if let Err(error) = &result
                && error.kind != ErrorKind::Resource
                && let Err(handler_error) = self.notify_window_error(error)
            {
                result = Err(handler_error);
            }
            if let Err(error) = result {
                if error.kind == ErrorKind::Resource {
                    self.document = old_document;
                    self.state = old_state;
                    self.runtime.rollback_to(old_runtime);
                    self.disable_scripts();
                    return Err(error);
                }
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

impl DomHost<'_> {
    pub(super) fn animation_call(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
        this: &Value,
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        if method != "call"
            || (!matches!(this, Value::Undefined | Value::Null) && *this != Value::Host(WINDOW))
        {
            return Err(JsError::runtime("Illegal animation frame Window receiver"));
        }
        let value = args
            .first()
            .cloned()
            .ok_or_else(|| JsError::runtime("Animation frame argument required"))?;
        if object == CANCEL {
            // Full observable ToNumber, without pretending these safe-integer
            // handles implement the browser's unsigned-long wraparound contract.
            let number = runtime.coerce_to_number(value, self)?;
            if number.is_finite()
                && number.fract() == 0.0
                && (1.0..=MAX_HANDLE as f64).contains(&number)
            {
                self.state
                    .animation
                    .pending
                    .retain(|frame| frame.id != number as u64);
            }
            return Ok(Value::Undefined);
        }
        if !runtime.is_callable_value(&value, self) {
            return Err(JsError::runtime(
                "requestAnimationFrame requires a callable callback",
            ));
        }
        if self.state.animation.pending.len() >= MAX_PENDING {
            return Err(JsError::resource(
                "Animation frame pending callback budget exceeded",
            ));
        }
        let previous = self.state.animation.next_handle.load(Ordering::Relaxed);
        if previous >= MAX_HANDLE {
            return Err(JsError::resource("Animation frame handle budget exceeded"));
        }
        let id = previous + 1;
        self.state
            .animation
            .next_handle
            .store(id, Ordering::Relaxed);
        self.state.animation.pending.push(FrameCallback {
            id,
            callback: value,
        });
        Ok(Value::Number(id as f64))
    }
}

#[cfg(test)]
#[path = "animation_tests.rs"]
mod tests;
