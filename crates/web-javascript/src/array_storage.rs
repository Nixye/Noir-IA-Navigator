//! Array limits and compact absence metadata. Values remain budgeted dense storage.
use crate::JsError;

pub(super) const MAX_ARRAY_LENGTH: usize = 8192;
pub(super) const MAX_ARRAY_SLOTS: usize = 65_536;

/// One bit per absent index, allocated only through the highest absent word.
#[derive(Debug, Clone, Default)]
pub(super) struct ArrayHoles {
    words: Vec<u64>,
}
impl ArrayHoles {
    pub(super) fn new() -> Self {
        Self::default()
    }
    pub(super) fn full(length: usize) -> Result<Self, JsError> {
        let mut holes = Self::new();
        holes.insert_range(0, length)?;
        Ok(holes)
    }
    pub(super) fn insert_range(&mut self, start: usize, end: usize) -> Result<(), JsError> {
        if end > MAX_ARRAY_LENGTH {
            return Err(JsError::resource(
                "Array hole metadata exceeds length limit",
            ));
        }
        if start >= end {
            return Ok(());
        }
        let words = end.div_ceil(64);
        if words > self.words.len() {
            if words > self.words.capacity() {
                let capacity = (self.words.capacity() * 2)
                    .max(words)
                    .min(MAX_ARRAY_LENGTH / 64);
                self.words
                    .try_reserve_exact(capacity - self.words.len())
                    .map_err(|_| JsError::resource("Array hole allocation failed"))?;
            }
            self.words.resize(words, 0);
        }
        for index in start / 64..words {
            let low = start.saturating_sub(index * 64).min(64);
            let high = (end - index * 64).min(64);
            self.words[index] |= (u64::MAX << low) & (u64::MAX >> (64 - high));
        }
        Ok(())
    }
    pub(super) fn insert(&mut self, index: usize) -> Result<(), JsError> {
        if index >= MAX_ARRAY_LENGTH {
            return Err(JsError::resource(
                "Array hole metadata exceeds length limit",
            ));
        }
        self.insert_range(index, index + 1)
    }
    pub(super) fn contains(&self, index: &usize) -> bool {
        self.words
            .get(index / 64)
            .is_some_and(|word| word & (1u64 << (index % 64)) != 0)
    }
    pub(super) fn remove(&mut self, index: &usize) {
        if let Some(word) = self.words.get_mut(index / 64) {
            *word &= !(1u64 << (index % 64));
        }
    }
    pub(super) fn truncate(&mut self, length: usize) {
        self.words.truncate(length.div_ceil(64));
        if self.words.len() == length.div_ceil(64)
            && !length.is_multiple_of(64)
            && let Some(word) = self.words.last_mut()
        {
            *word &= (1u64 << (length % 64)) - 1;
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }
}
#[cfg(test)]
mod tests;
