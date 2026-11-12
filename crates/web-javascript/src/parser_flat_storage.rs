//! One non-refundable parser ledger for all immutable flat AST storage.
//! Vec capacity and aligned Arc data are charged, not allocator size classes.
use super::*;
use std::{alloc::Layout, sync::atomic::AtomicUsize};

// Preserve the Sequence ceiling: 1,572,864 bytes on the supported x64 layout.
pub(super) const MAX_BYTES: usize = {
    let legacy = MAX_PARSER_VISITS * (size_of::<Expr>() + size_of::<Expr>() / 2);
    if legacy < 1_572_864 {
        legacy
    } else {
        1_572_864
    }
};
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<Expr>() == 64);

pub(super) fn header_bytes<T>() -> usize {
    Layout::array::<AtomicUsize>(2)
        .expect("Two Arc counters")
        .extend(Layout::new::<T>())
        .expect("Bounded Arc data layout")
        .0
        .pad_to_align()
        .size()
}
pub(super) fn charge(
    bytes: &mut usize,
    amount: usize,
    message: &'static str,
) -> Result<(), JsError> {
    *bytes = bytes
        .checked_add(amount)
        .ok_or_else(|| JsError::resource(message))?;
    if *bytes > MAX_BYTES {
        return Err(JsError::resource(message));
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(super) fn reserve<T>(
    items: &mut Vec<T>,
    additional: usize,
    minimum: usize,
    maximum: usize,
    bytes: &mut usize,
    budget_message: &'static str,
    width_message: &'static str,
    allocation_message: &'static str,
) -> Result<(), JsError> {
    let required = items
        .len()
        .checked_add(additional)
        .ok_or_else(|| JsError::resource(width_message))?;
    if required > maximum {
        return Err(JsError::resource(width_message));
    }
    let old = items.capacity();
    if required <= old {
        return Ok(());
    }
    let target = required.max(old + old / 2).clamp(minimum, maximum);
    charge(bytes, (target - old) * size_of::<T>(), budget_message)?;
    items
        .try_reserve_exact(target - items.len())
        .map_err(|_| JsError::resource(allocation_message))?;
    let actual = items.capacity();
    if actual > target {
        charge(bytes, (actual - target) * size_of::<T>(), budget_message)?;
    }
    if actual > maximum {
        return Err(JsError::resource(width_message));
    }
    Ok(())
}
