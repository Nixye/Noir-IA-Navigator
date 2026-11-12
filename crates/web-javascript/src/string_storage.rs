//! Concatenation reserves bounded flat storage before copying its operands.
//! Other producers and UTF-8 host boundaries keep their independent limits.
use crate::{JsError, Runtime, Value};
use std::sync::Arc;

pub(super) const MAX_CONCAT_BYTES: usize = 128 * 1024;
const WORK_BLOCK_BYTES: usize = 64;

enum Units<'a> {
    Utf8(std::str::EncodeUtf16<'a>),
    Utf16(std::iter::Copied<std::slice::Iter<'a, u16>>),
}
impl Iterator for Units<'_> {
    type Item = u16;
    fn next(&mut self) -> Option<u16> {
        match self {
            Self::Utf8(units) => units.next(),
            Self::Utf16(units) => units.next(),
        }
    }
}
fn units(value: &Value) -> Units<'_> {
    match value {
        Value::String(text) => Units::Utf8(text.encode_utf16()),
        Value::Utf16(text) => Units::Utf16(text.iter().copied()),
        _ => unreachable!("concatenation receives string primitives"),
    }
}
fn stored_bytes(value: &Value) -> usize {
    match value {
        Value::String(text) => text.len(),
        Value::Utf16(text) => text.len().saturating_mul(2),
        _ => unreachable!("concatenation receives string primitives"),
    }
}
fn allocation_error() -> JsError {
    JsError::resource("String concatenation storage unavailable")
}
fn size_error(left: usize, right: usize, result: usize) -> JsError {
    let mut error = JsError::resource("String concatenation exceeds limit");
    error.add_diagnostic_context(&format!(
        "string operation=concat left-bytes={left} right-bytes={right} result-bytes={result} limit={MAX_CONCAT_BYTES}"
    ));
    error
}

impl Runtime {
    pub(super) fn string_copy_work(&mut self, bytes: usize) -> Result<(), JsError> {
        let work = bytes.div_ceil(WORK_BLOCK_BYTES).saturating_add(1);
        self.remaining = self
            .remaining
            .checked_sub(work as u64)
            .ok_or_else(|| JsError::resource("String copy work budget exceeded"))?;
        Ok(())
    }

    fn concat_operand(&mut self, value: Value) -> Result<Value, JsError> {
        match value {
            Value::String(_) | Value::Utf16(_) => Ok(value),
            Value::BigInt(number) => {
                self.charge_bigint_decimal(&number)?;
                self.string(number.text(10))
            }
            Value::Symbol(..) => {
                Err(self.property_type_error("Cannot convert a Symbol value to a string"))
            }
            // The language operator has already completed observable ToPrimitive.
            _ => self.string(value.primitive_text()?),
        }
    }

    pub(super) fn concat_strings(&mut self, left: Value, right: Value) -> Result<Value, JsError> {
        let left = self.concat_operand(left)?;
        let right = self.concat_operand(right)?;
        let left_bytes = stored_bytes(&left);
        let right_bytes = stored_bytes(&right);
        // Bound scans even for a value injected by an internal/native caller.
        if left_bytes > MAX_CONCAT_BYTES || right_bytes > MAX_CONCAT_BYTES {
            let mut error = JsError::resource("String operand exceeds concatenation limit");
            error.add_diagnostic_context(&format!("string operation=concat left-bytes={left_bytes} right-bytes={right_bytes} operand-limit={MAX_CONCAT_BYTES}"));
            return Err(error);
        }
        self.string_copy_work(left_bytes.saturating_add(right_bytes))?;
        if let (Value::String(left), Value::String(right)) = (&left, &right) {
            let bytes = left.len().saturating_add(right.len());
            if bytes > MAX_CONCAT_BYTES {
                return Err(size_error(left_bytes, right_bytes, bytes));
            }
            // Account for the temporary String and immutable Arc payload. This is
            // a cumulative reservation, not an exact allocator/RSS measurement.
            self.charge_string(bytes.saturating_mul(2))?;
            let mut output = String::new();
            output
                .try_reserve_exact(bytes)
                .map_err(|_| allocation_error())?;
            output.push_str(left);
            output.push_str(right);
            return Ok(Value::string(output));
        }
        // Inspect borrowed UTF-16 units first. A surrogate pair can straddle the
        // operands, and one unpaired surrogate selects UTF-16 for the whole result.
        let mut utf8_bytes = 0usize;
        let mut well_formed = true;
        let mut unit_count = 0usize;
        for scalar in char::decode_utf16(units(&left).chain(units(&right))) {
            match scalar {
                Ok(ch) => {
                    utf8_bytes += ch.len_utf8();
                    unit_count += ch.len_utf16();
                }
                Err(_) => {
                    well_formed = false;
                    unit_count += 1;
                }
            }
        }
        let bytes = if well_formed {
            utf8_bytes
        } else {
            unit_count.saturating_mul(2)
        };
        if bytes > MAX_CONCAT_BYTES {
            return Err(size_error(left_bytes, right_bytes, bytes));
        }
        self.charge_string(bytes.saturating_mul(2))?;
        if well_formed {
            let mut output = String::new();
            output
                .try_reserve_exact(bytes)
                .map_err(|_| allocation_error())?;
            for scalar in char::decode_utf16(units(&left).chain(units(&right))) {
                output.push(scalar.expect("already checked UTF-16"));
            }
            Ok(Value::string(output))
        } else {
            let mut output = Vec::new();
            output
                .try_reserve_exact(unit_count)
                .map_err(|_| allocation_error())?;
            output.extend(units(&left).chain(units(&right)));
            Ok(Value::Utf16(Arc::from(output)))
        }
    }
}

#[cfg(test)]
mod tests;
