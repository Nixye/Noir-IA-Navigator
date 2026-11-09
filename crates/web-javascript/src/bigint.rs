//! Signed, normalized base-2^32 integers. No floating-point arithmetic is used
//! for integer operations. The VM deliberately caps magnitudes at 4096 bits.
use crate::{JsError, Runtime, Value};
use std::{cmp::Ordering, sync::Arc};
pub const MAX_BITS: usize = 4096;
const MAX_LIMBS: usize = MAX_BITS / 32;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigInt {
    negative: bool,
    limbs: Vec<u32>,
}
impl BigInt {
    pub(super) fn from_64_le(raw: [u8; 8], signed: bool) -> Self {
        let bits = u64::from_le_bytes(raw);
        let negative = signed && bits >> 63 != 0;
        let magnitude = if negative { bits.wrapping_neg() } else { bits };
        // A decoded 64-bit storage element always fits the 4096-bit magnitude cap.
        Self::normalized(negative, vec![magnitude as u32, (magnitude >> 32) as u32])
            .expect("64-bit element fits BigInt limit")
    }
    pub(super) fn low_64_le(&self) -> [u8; 8] {
        let words = self.twos(2);
        ((words[0] as u64) | ((words[1] as u64) << 32)).to_le_bytes()
    }

    fn normalized(negative: bool, mut limbs: Vec<u32>) -> Result<Self, JsError> {
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        if limbs.len() > MAX_LIMBS {
            return Err(JsError::resource("BigInt exceeds 4096 bits"));
        }
        Ok(Self {
            negative: negative && !limbs.is_empty(),
            limbs,
        })
    }
    pub fn zero() -> Self {
        Self {
            negative: false,
            limbs: Vec::new(),
        }
    }
    fn one() -> Self {
        Self {
            negative: false,
            limbs: vec![1],
        }
    }
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }
    pub(super) fn bits(&self) -> usize {
        self.limbs.last().map_or(0, |n| {
            (self.limbs.len() - 1) * 32 + (32 - n.leading_zeros()) as usize
        })
    }
    pub fn negated(&self) -> Self {
        let mut n = self.clone();
        n.negative = !n.negative && !n.is_zero();
        n
    }
    fn magnitude_cmp(&self, other: &Self) -> Ordering {
        self.limbs
            .len()
            .cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }
    pub fn compare(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => other.magnitude_cmp(self),
            _ => self.magnitude_cmp(other),
        }
    }
    pub fn parse(text: &str) -> Result<Self, JsError> {
        let text = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
        if text.len() > MAX_BITS + 3 {
            return Err(JsError::resource("BigInt input exceeds limit"));
        }
        if text.is_empty() {
            return Ok(Self::zero());
        }
        let signed = text.starts_with(['+', '-']);
        let negative = text.starts_with('-');
        let mut digits = if signed { &text[1..] } else { text };
        let mut radix = 10;
        if digits.starts_with("0x") || digits.starts_with("0X") {
            radix = 16;
        }
        if digits.starts_with("0o") || digits.starts_with("0O") {
            radix = 8;
        }
        if digits.starts_with("0b") || digits.starts_with("0B") {
            radix = 2;
        }
        if radix != 10 {
            if signed {
                return Err(JsError::runtime("Invalid BigInt string"));
            }
            digits = &digits[2..];
        }
        if digits.is_empty() {
            return Err(JsError::runtime("Invalid BigInt string"));
        }
        let mut value = Self::zero();
        for c in digits.chars() {
            let digit = c
                .to_digit(radix)
                .filter(|_| c.is_ascii())
                .ok_or_else(|| JsError::runtime("Invalid BigInt string"))?;
            let mut carry = digit as u64;
            for limb in &mut value.limbs {
                let n = *limb as u64 * radix as u64 + carry;
                *limb = n as u32;
                carry = n >> 32;
            }
            if carry != 0 {
                value.limbs.push(carry as u32);
            }
            if value.limbs.len() > MAX_LIMBS {
                return Err(JsError::resource("BigInt exceeds 4096 bits"));
            }
        }
        value.negative = negative && !value.is_zero();
        Ok(value)
    }
    pub fn add(&self, other: &Self) -> Result<Self, JsError> {
        if self.negative != other.negative {
            return if self.magnitude_cmp(other).is_lt() {
                Self::normalized(
                    other.negative,
                    Self::subtract_magnitude(&other.limbs, &self.limbs),
                )
            } else {
                Self::normalized(
                    self.negative,
                    Self::subtract_magnitude(&self.limbs, &other.limbs),
                )
            };
        }
        let mut output = Vec::new();
        let mut carry = 0u64;
        for i in 0..self.limbs.len().max(other.limbs.len()) {
            let n = self.limbs.get(i).copied().unwrap_or(0) as u64
                + other.limbs.get(i).copied().unwrap_or(0) as u64
                + carry;
            output.push(n as u32);
            carry = n >> 32;
        }
        if carry != 0 {
            output.push(carry as u32);
        }
        Self::normalized(self.negative, output)
    }
    fn subtract_magnitude(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut borrow = 0i64;
        a.iter()
            .enumerate()
            .map(|(i, &x)| {
                let n = x as i64 - b.get(i).copied().unwrap_or(0) as i64 - borrow;
                borrow = i64::from(n < 0);
                n as u32
            })
            .collect()
    }
    pub fn multiply(&self, other: &Self) -> Result<Self, JsError> {
        if self.is_zero() || other.is_zero() {
            return Ok(Self::zero());
        }
        if self.bits() + other.bits() > MAX_BITS + 1 {
            return Err(JsError::resource("BigInt exceeds 4096 bits"));
        }
        let mut out = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                let n = a as u64 * b as u64 + out[i + j] as u64 + carry;
                out[i + j] = n as u32;
                carry = n >> 32;
            }
            out[i + other.limbs.len()] = carry as u32;
        }
        Self::normalized(self.negative != other.negative, out)
    }
    pub fn divide(&self, other: &Self) -> Result<(Self, Self), JsError> {
        if other.is_zero() {
            return Err(JsError::runtime("Division by zero"));
        }
        let mut rem = Self::zero();
        let mut quotient = vec![0; self.limbs.len()];
        for bit in (0..self.bits()).rev() {
            let mut carry = (self.limbs[bit / 32] >> (bit % 32)) & 1;
            for limb in &mut rem.limbs {
                let next = *limb >> 31;
                *limb = (*limb << 1) | carry;
                carry = next;
            }
            if carry != 0 {
                rem.limbs.push(carry);
            }
            if !rem.magnitude_cmp(other).is_lt() {
                rem = Self::normalized(false, Self::subtract_magnitude(&rem.limbs, &other.limbs))?;
                quotient[bit / 32] |= 1 << (bit % 32);
            }
        }
        rem.negative = self.negative && !rem.is_zero();
        Ok((
            Self::normalized(self.negative != other.negative, quotient)?,
            rem,
        ))
    }
    fn small(&self) -> Option<usize> {
        if self.limbs.len() > 1 {
            None
        } else {
            Some(self.limbs.first().copied().unwrap_or(0) as usize)
        }
    }
    pub fn power(&self, exp: &Self) -> Result<Self, JsError> {
        if exp.negative {
            return Err(JsError::runtime("Negative BigInt exponent"));
        }
        if exp.is_zero() {
            return Ok(Self::one());
        }
        if self.is_zero() {
            return Ok(Self::zero());
        }
        if self.limbs == [1] {
            return Ok(if self.negative && exp.limbs[0] & 1 != 0 {
                self.clone()
            } else {
                Self::one()
            });
        }
        let mut n = exp
            .small()
            .filter(|n| *n <= MAX_BITS)
            .ok_or_else(|| JsError::resource("BigInt exponent exceeds limit"))?;
        let mut result = Self::one();
        let mut base = self.clone();
        while n != 0 {
            if n & 1 != 0 {
                result = result.multiply(&base)?;
            }
            n >>= 1;
            if n != 0 {
                base = base.multiply(&base)?;
            }
        }
        Ok(result)
    }
    pub fn shift(&self, count: &Self, left: bool) -> Result<Self, JsError> {
        let left = left != count.negative;
        let count = count.small().unwrap_or(MAX_BITS + 1);
        if self.is_zero() {
            return Ok(Self::zero());
        }
        if left {
            if count > MAX_BITS || self.bits() + count > MAX_BITS {
                return Err(JsError::resource("BigInt shift exceeds 4096 bits"));
            }
            let mut out = vec![0; count / 32];
            let mut carry = 0u64;
            for &limb in &self.limbs {
                let n = ((limb as u64) << (count % 32)) | carry;
                out.push(n as u32);
                carry = n >> 32;
            }
            if carry != 0 {
                out.push(carry as u32);
            }
            Self::normalized(self.negative, out)
        } else {
            if count >= self.bits() {
                return Ok(if self.negative {
                    Self::one().negated()
                } else {
                    Self::zero()
                });
            }
            let words = count / 32;
            let bits = count % 32;
            let mut out = Vec::new();
            for i in words..self.limbs.len() {
                let upper = self.limbs.get(i + 1).copied().unwrap_or(0) as u64;
                out.push((((self.limbs[i] as u64) | (upper << 32)) >> bits) as u32);
            }
            let lost = self.limbs[..words].iter().any(|n| *n != 0)
                || (bits != 0 && self.limbs[words] & ((1u32 << bits) - 1) != 0);
            let mut value = Self::normalized(self.negative, out)?;
            if self.negative && lost {
                value = value.add(&Self::one().negated())?;
            }
            Ok(value)
        }
    }
    fn twos(&self, len: usize) -> Vec<u32> {
        let mut out: Vec<u32> = self.limbs.iter().take(len).copied().collect();
        out.resize(len, 0);
        if self.negative {
            let mut carry = 1u64;
            for limb in &mut out {
                let n = (!*limb) as u64 + carry;
                *limb = n as u32;
                carry = n >> 32;
            }
        }
        out
    }
    pub fn bitwise(&self, other: &Self, op: &str) -> Result<Self, JsError> {
        let len = self.limbs.len().max(other.limbs.len()) + 1;
        let a = self.twos(len);
        let b = other.twos(len);
        let mut out: Vec<u32> = a
            .iter()
            .zip(b)
            .map(|(&a, b)| match op {
                "&" => a & b,
                "|" => a | b,
                _ => a ^ b,
            })
            .collect();
        let negative = out[len - 1] >> 31 != 0;
        if negative {
            let mut carry = 1u64;
            for limb in &mut out {
                let n = (!*limb) as u64 + carry;
                *limb = n as u32;
                carry = n >> 32;
            }
        }
        Self::normalized(negative, out)
    }
    pub fn text(&self, radix: u32) -> String {
        if self.is_zero() {
            return "0".into();
        }
        let mut work = self.limbs.clone();
        let mut chars = Vec::new();
        while !work.is_empty() {
            let mut rem = 0u64;
            for limb in work.iter_mut().rev() {
                let n = (rem << 32) | *limb as u64;
                *limb = (n / radix as u64) as u32;
                rem = n % radix as u64;
            }
            chars.push(char::from_digit(rem as u32, radix).unwrap());
            while work.last() == Some(&0) {
                work.pop();
            }
        }
        if self.negative {
            chars.push('-');
        }
        chars.iter().rev().collect()
    }
    pub fn from_number(n: f64) -> Result<Self, JsError> {
        if !n.is_finite() || n.fract() != 0. {
            return Err(JsError::runtime("Number is not an integer"));
        }
        if n == 0. {
            return Ok(Self::zero());
        }
        let bits = n.abs().to_bits();
        let exponent = ((bits >> 52) & 2047) as i32 - 1023;
        let mantissa = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
        let mut value = Self::normalized(n < 0., vec![mantissa as u32, (mantissa >> 32) as u32])?;
        let count = Self::parse(&(exponent - 52).unsigned_abs().to_string())?;
        // Shift magnitude first: this conversion never rounds negative values.
        value.negative = false;
        value = value.shift(&count, exponent >= 52)?;
        value.negative = n < 0.;
        Ok(value)
    }
    pub fn to_number(&self) -> f64 {
        self.text(10).parse().unwrap_or(if self.negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        })
    }
    fn compare_number(&self, n: f64) -> Option<Ordering> {
        if n.is_nan() {
            return None;
        }
        if n == f64::INFINITY {
            return Some(Ordering::Less);
        }
        if n == f64::NEG_INFINITY {
            return Some(Ordering::Greater);
        }
        let whole = Self::from_number(n.trunc()).ok()?;
        let order = self.compare(&whole);
        Some(if order.is_eq() && n.fract() != 0. {
            if n > 0. {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        } else {
            order
        })
    }
}

impl Runtime {
    pub(super) fn is_bigint_wrapper(&self, value: &Value) -> bool {
        matches!(value, Value::Object(id) if matches!(self.reflection.boxed.get(id), Some(Value::BigInt(_))))
    }
    pub(super) fn bigint_method(
        &mut self,
        name: &str,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = match receiver {
            Value::BigInt(_) => receiver.clone(),
            Value::Object(id) => self
                .reflection
                .boxed
                .get(id)
                .cloned()
                .unwrap_or(Value::Undefined),
            _ => Value::Undefined,
        };
        let Value::BigInt(number) = &value else {
            return Err(
                self.property_type_error("BigInt method requires a BigInt primitive or wrapper")
            );
        };
        if name == "BigInt.valueOf" {
            return Ok(value);
        }
        if name == "BigInt.toLocaleString" {
            return Err(self.property_type_error("BigInt locale formatting is not implemented"));
        }
        let radix = match args.first() {
            None | Some(Value::Undefined) => 10.,
            Some(Value::BigInt(_) | Value::Symbol(..)) => {
                return Err(self.property_type_error("Invalid BigInt radix"));
            }
            Some(value) if self.is_bigint_wrapper(value) => {
                return Err(self.property_type_error("BigInt wrapper coercion is not implemented"));
            }
            Some(value) => self.number(value).trunc(),
        };
        if !(2. ..=36.).contains(&radix) {
            return Err(self.bigint_error("RangeError", "BigInt radix must be between 2 and 36"));
        }
        self.remaining = self
            .remaining
            .checked_sub((number.limbs.len().max(1) * MAX_BITS) as u64)
            .ok_or_else(|| JsError::resource("BigInt formatting work budget exceeded"))?;
        self.string(number.text(radix as u32))
    }

    pub(super) fn bigint_error(&mut self, name: &str, message: &str) -> JsError {
        match self.native(name, &[Value::string(message)]) {
            Ok(value) => {
                let mut error = JsError::runtime(message);
                error.thrown = Some(value);
                error
            }
            Err(error) => error,
        }
    }
    /// Truncate without constructing 2^bits, which would exceed the magnitude
    /// cap when bits is 4096. Temporary storage is at most 128 u32 limbs.
    fn bigint_truncate(
        &mut self,
        bits: Value,
        value: Value,
        signed: bool,
    ) -> Result<Value, JsError> {
        let width = self.bigint_width(bits)?;
        self.bigint_truncate_width(width, value, signed)
    }

    pub(super) fn bigint_width(&mut self, bits: Value) -> Result<usize, JsError> {
        let width_bytes = match &bits {
            Value::String(text) => text.len(),
            Value::Utf16(text) => text.len().saturating_mul(2),
            _ => 0,
        };
        if width_bytes > 16_384 {
            return Err(JsError::resource(
                "BigInt width conversion input exceeds limit",
            ));
        }
        self.remaining = self
            .remaining
            .checked_sub(width_bytes as u64 + 1)
            .ok_or_else(|| JsError::resource("BigInt width conversion work budget exceeded"))?;
        self.charge_string(width_bytes)?;
        let width_number = match &bits {
            Value::BigInt(_) | Value::Symbol(..) => {
                return Err(self.property_type_error("BigInt width cannot be a BigInt or Symbol"));
            }
            Value::Object(_)
            | Value::Array(_)
            | Value::Function(_)
            | Value::Native(_)
            | Value::Host(_) => {
                return Err(
                    self.property_type_error("Object coercion for BigInt width is not implemented")
                );
            }
            _ => self.number(&bits),
        };
        let width_number = if width_number.is_nan() {
            0.
        } else {
            width_number.trunc()
        };
        if !width_number.is_finite() || width_number < 0. || width_number > 9_007_199_254_740_991. {
            return Err(self.bigint_error(
                "RangeError",
                "BigInt width must be a nonnegative safe integer",
            ));
        }
        if width_number > MAX_BITS as f64 {
            return Err(JsError::resource("BigInt width exceeds 4096 bits"));
        }
        Ok(width_number as usize)
    }

    /// Width has already passed ToIndex and the VM's 4096-bit bound. Keeping
    /// this separate lets callers convert the value only after width succeeds.
    pub(super) fn bigint_truncate_width(
        &mut self,
        width: usize,
        value: Value,
        signed: bool,
    ) -> Result<Value, JsError> {
        if width > MAX_BITS {
            return Err(JsError::resource("BigInt width exceeds 4096 bits"));
        }
        // ToBigInt differs from the BigInt constructor: Number is forbidden,
        // even for integral values and even if the requested width is zero.
        if matches!(value, Value::Number(_)) {
            return Err(self.property_type_error("Cannot implicitly convert Number to BigInt"));
        }
        let input_bytes = match &value {
            Value::String(text) => text.len(),
            Value::Utf16(text) => text.len().saturating_mul(2),
            _ => 0,
        };
        if input_bytes > 16_384 {
            return Err(JsError::resource("BigInt conversion input exceeds limit"));
        }
        let limbs = width.div_ceil(32);
        let cost = (input_bytes as u64)
            .saturating_mul(MAX_LIMBS as u64)
            .saturating_add((limbs as u64).saturating_mul(6))
            .saturating_add(1);
        self.remaining = self
            .remaining
            .checked_sub(cost)
            .ok_or_else(|| JsError::resource("BigInt truncation work budget exceeded"))?;
        // Reuse the VM's cumulative byte-allocation budget for parsed input and
        // output limbs. Charging before allocation also bounds repeated calls.
        let parse_bytes = match &value {
            Value::String(_) | Value::Utf16(_) => MAX_LIMBS * 4,
            Value::Bool(_) => 4,
            _ => 0,
        };
        self.charge_string(
            input_bytes
                .saturating_add(parse_bytes)
                .saturating_add(limbs.saturating_mul(4)),
        )?;
        let converted = self.bigint_convert(&value)?;
        let Value::BigInt(number) = converted else {
            unreachable!()
        };
        if width == 0 {
            return Ok(Value::BigInt(Arc::new(BigInt::zero())));
        }
        let mut output = number.twos(limbs);
        let tail_bits = width % 32;
        let mask = if tail_bits == 0 {
            u32::MAX
        } else {
            (1u32 << tail_bits) - 1
        };
        output[limbs - 1] &= mask;
        let negative = signed && output[(width - 1) / 32] & (1u32 << ((width - 1) % 32)) != 0;
        if negative {
            let mut carry = 1u64;
            for limb in &mut output {
                let next = (!*limb) as u64 + carry;
                *limb = next as u32;
                carry = next >> 32;
            }
            output[limbs - 1] &= mask;
        }
        Ok(Value::BigInt(Arc::new(BigInt::normalized(
            negative, output,
        )?)))
    }
    // Primitive-only adapter retained for arithmetic source tests. JS callers
    // use the Host-aware path and its already-validated width entrypoint.
    #[cfg(test)]
    pub(super) fn bigint_as_int_n(&mut self, bits: Value, value: Value) -> Result<Value, JsError> {
        self.bigint_truncate(bits, value, true)
    }
    pub(super) fn bigint_as_uint_n(&mut self, bits: Value, value: Value) -> Result<Value, JsError> {
        self.bigint_truncate(bits, value, false)
    }
    pub(super) fn bigint_convert(&mut self, value: &Value) -> Result<Value, JsError> {
        let result = match value {
            Value::BigInt(n) => return Ok(Value::BigInt(n.clone())),
            Value::Bool(b) => Ok(if *b { BigInt::one() } else { BigInt::zero() }),
            Value::Number(n) => BigInt::from_number(*n),
            Value::String(s) => BigInt::parse(s),
            Value::Utf16(s) => String::from_utf16(s)
                .map_err(|_| JsError::runtime("Invalid BigInt string"))
                .and_then(|s| BigInt::parse(&s)),
            _ => return Err(self.property_type_error("Cannot convert value to BigInt")),
        };
        result.map(|n| Value::BigInt(Arc::new(n))).map_err(|e| {
            if e.kind == crate::ErrorKind::Resource {
                e
            } else {
                self.bigint_error(
                    if matches!(value, Value::Number(_)) {
                        "RangeError"
                    } else {
                        "SyntaxError"
                    },
                    &e.message,
                )
            }
        })
    }
    // Invalid numeric strings give an undefined comparison. Exhausting the
    // interpreter's resource limits is different and must never become false.
    fn bigint_comparison_string(&mut self, value: &Value) -> Result<Option<BigInt>, JsError> {
        let (input_units, utf8_bytes) = match value {
            Value::String(text) => (text.len(), 0),
            // Three UTF-8 bytes per code unit bounds even surrogate pairs.
            Value::Utf16(text) => (text.len(), text.len().saturating_mul(3)),
            _ => return Ok(None),
        };
        if input_units > 16_384 {
            return Err(JsError::resource("BigInt comparison input exceeds limit"));
        }
        let work = (input_units as u64)
            .saturating_mul(MAX_LIMBS as u64)
            .saturating_add(1);
        self.remaining = self
            .remaining
            .checked_sub(work)
            .ok_or_else(|| JsError::resource("BigInt comparison work budget exceeded"))?;
        // Charge before either UTF-16 decoding or the parser's limb allocations.
        self.charge_string(utf8_bytes.saturating_add(MAX_LIMBS * 4))?;
        let parsed = match value {
            Value::String(text) => BigInt::parse(text),
            Value::Utf16(text) => match String::from_utf16(text) {
                Ok(text) => BigInt::parse(&text),
                Err(_) => return Ok(None),
            },
            _ => unreachable!(),
        };
        match parsed {
            Ok(number) => Ok(Some(number)),
            Err(error) if error.kind == crate::ErrorKind::Resource => Err(error),
            Err(_) => Ok(None),
        }
    }
    pub(super) fn bigint_binary(
        &mut self,
        op: &str,
        left: &Value,
        right: &Value,
    ) -> Result<Value, JsError> {
        if matches!(op, "===" | "!==") {
            let same = left == right;
            return Ok(Value::Bool(if op == "===" { same } else { !same }));
        }
        let order = if matches!(op, "==" | "!=" | "<" | ">" | "<=" | ">=") {
            match (left, right) {
                (Value::BigInt(a), Value::BigInt(b)) => Some(a.compare(b)),
                (Value::BigInt(a), text @ (Value::String(_) | Value::Utf16(_))) => {
                    self.bigint_comparison_string(text)?.map(|b| a.compare(&b))
                }
                (text @ (Value::String(_) | Value::Utf16(_)), Value::BigInt(b)) => {
                    self.bigint_comparison_string(text)?.map(|a| a.compare(b))
                }
                (Value::BigInt(a), b) => a.compare_number(self.number(b)),
                (a, Value::BigInt(b)) => b.compare_number(self.number(a)).map(Ordering::reverse),
                _ => None,
            }
        } else {
            None
        };
        if matches!(op, "==" | "!=") {
            let eligible = !matches!(left, Value::Null | Value::Undefined | Value::Symbol(..))
                && !matches!(right, Value::Null | Value::Undefined | Value::Symbol(..));
            let same = eligible && order == Some(Ordering::Equal);
            return Ok(Value::Bool(if op == "==" { same } else { !same }));
        }
        if matches!(op, "<" | ">" | "<=" | ">=") {
            if matches!(left, Value::Symbol(..)) || matches!(right, Value::Symbol(..)) {
                return Err(self.property_type_error("Cannot compare Symbol and BigInt"));
            }
            return Ok(Value::Bool(order.is_some_and(|o| match op {
                "<" => o.is_lt(),
                ">" => o.is_gt(),
                "<=" => o.is_le(),
                _ => o.is_ge(),
            })));
        }
        let (Value::BigInt(a), Value::BigInt(b)) = (left, right) else {
            return Err(self.property_type_error("Cannot mix BigInt and other numeric types"));
        };
        // Charge a conservative limb-work estimate before any expensive operation.
        let mut cost = (a.limbs.len().max(1) * b.limbs.len().max(1) * 32) as u64;
        if op == "**" && a.bits() > 1 && !b.negative {
            // Repeated squaring operates on growing operands, not just the
            // initial base. Reserve a conservative bound before doing any work.
            let exponent = b.small().unwrap_or(MAX_BITS + 1).min(MAX_BITS + 1);
            let result_bits = a.bits().saturating_mul(exponent).min(MAX_BITS);
            let limbs = result_bits.div_ceil(32).max(1);
            let stages = usize::BITS - exponent.max(1).leading_zeros();
            cost = cost.saturating_add((limbs * limbs * stages as usize * 2) as u64);
        }
        self.remaining = self
            .remaining
            .checked_sub(cost)
            .ok_or_else(|| JsError::resource("BigInt instruction budget exceeded"))?;
        let result = match op {
            "+" => a.add(b),
            "-" => a.add(&b.negated()),
            "*" => a.multiply(b),
            "/" => a.divide(b).map(|v| v.0),
            "%" => a.divide(b).map(|v| v.1),
            "**" => a.power(b),
            "&" | "|" | "^" => a.bitwise(b, op),
            "<<" => a.shift(b, true),
            ">>" => a.shift(b, false),
            ">>>" => return Err(self.property_type_error("BigInt has no unsigned right shift")),
            _ => return Err(JsError::runtime("Unsupported BigInt operator")),
        };
        result.map(|n| Value::BigInt(Arc::new(n))).map_err(|e| {
            if e.kind == crate::ErrorKind::Resource {
                e
            } else {
                self.bigint_error("RangeError", &e.message)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Empty;
    impl crate::Host for Empty {
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
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty)
    }
    #[test]
    fn exact_limb_arithmetic_beyond_fixed_width() {
        let a = BigInt::parse("340282366920938463463374607431768211456").unwrap();
        assert_eq!(a.bits(), 129);
        let squared = a.multiply(&a).unwrap();
        assert_eq!(squared.bits(), 257);
        assert_eq!(squared.divide(&a).unwrap(), (a.clone(), BigInt::zero()));
        assert_eq!(
            a.add(&BigInt::parse("-1").unwrap()).unwrap().text(16),
            "ffffffffffffffffffffffffffffffff"
        );
        assert_eq!(
            BigInt::parse("-7")
                .unwrap()
                .divide(&BigInt::parse("3").unwrap())
                .unwrap(),
            (BigInt::parse("-2").unwrap(), BigInt::parse("-1").unwrap())
        );
    }
    #[test]
    fn literals_conversions_updates_and_json() {
        assert_eq!(run("let n=9007199254740993n;n++;typeof n==='bigint' && n===9007199254740994n && BigInt('0xff')===255n && 0b101n===5n && 0o77n===63n && String(2n**200n)==='1606938044258990275541962092341162602522202993782792835301376';").unwrap(),Value::Bool(true));
        assert_eq!(run("let ok=false;try{JSON.stringify({x:1n})}catch(e){ok=e.name==='TypeError'}ok && JSON.stringify({x:1n},(k,v)=>typeof v==='bigint'?String(v):v)==='{\"x\":\"1\"}';").unwrap(),Value::Bool(true));
        assert_eq!(
            run("(255n).toString(16)==='ff' && Number(12n)===12 && !0n && Boolean(-1n);").unwrap(),
            Value::Bool(true)
        );
    }
    #[test]
    fn mixed_comparisons_remain_exact() {
        assert_eq!(run("9007199254740993n>9007199254740992 && !(9007199254740993n==9007199254740992) && 2n>1.5 && -2n < -1.5 && 1n==true && 0n!=null && 1n!==1 && 16n=='0x10' && !(2n<'bad');").unwrap(),Value::Bool(true));
    }
    #[test]
    fn signed_bits_and_shifts() {
        assert_eq!(run("(-5n>>1n)===-3n && (-1n & 255n)===255n && (~0n)===-1n && (1n<<100n)===1267650600228229401496703205376n && (8n<<-1n)===4n;").unwrap(),Value::Bool(true));
    }
    #[test]
    fn catchable_type_and_range_errors() {
        for source in [
            "1n+1;",
            "+1n;",
            "1n>>>0n;",
            "new BigInt(1);",
            "Math.abs(1n);",
            "isNaN(1n);",
            "new Uint8Array([1n]);",
            "new ArrayBuffer(1n);",
        ] {
            let source =
                format!("let ok=false;try{{{source}}}catch(e){{ok=e.name==='TypeError'}}ok;");
            assert_eq!(run(&source).unwrap(), Value::Bool(true));
        }
        for source in ["1n/0n;", "2n**-1n;", "BigInt(1.5);"] {
            let source =
                format!("let ok=false;try{{{source}}}catch(e){{ok=e.name==='RangeError'}}ok;");
            assert_eq!(run(&source).unwrap(), Value::Bool(true));
        }
        assert_eq!(
            run("let ok=false;try{BigInt('x')}catch(e){ok=e.name==='SyntaxError'}ok;").unwrap(),
            Value::Bool(true)
        );
    }
    #[test]
    fn invalid_literals_and_resource_limits() {
        for source in ["1.0n;", "1e2n;", "01n;", "0xn;", "12nfoo;", "0b2n;"] {
            assert!(run(source).is_err(), "{source}");
        }
        assert_eq!(
            run("1n << 4096n;").unwrap_err().kind,
            crate::ErrorKind::Resource
        );
        assert!(BigInt::parse(&format!("0x1{}", "0".repeat(1024))).is_err());
        assert_eq!(
            BigInt::parse(&format!("0x{}", "f".repeat(1024)))
                .unwrap()
                .bits(),
            MAX_BITS
        );
    }
    #[test]
    fn truncation_is_modular_across_limb_boundaries() {
        for (bits, input, unsigned, signed) in [
            (0., "-1", "0", "0"),
            (1., "1", "1", "-1"),
            (1., "-2", "0", "0"),
            (8., "255", "255", "-1"),
            (8., "-129", "127", "127"),
            (8., "-256", "0", "0"),
            (8., "256", "0", "0"),
            (32., "4294967295", "4294967295", "-1"),
            (33., "4294967296", "4294967296", "-4294967296"),
            (65., "-1", "36893488147419103231", "-1"),
            (8.9, "511", "255", "-1"),
        ] {
            let value = Value::BigInt(Arc::new(BigInt::parse(input).unwrap()));
            let mut runtime = Runtime::default();
            assert_eq!(
                runtime
                    .bigint_as_uint_n(Value::Number(bits), value.clone())
                    .unwrap()
                    .primitive_text()
                    .unwrap(),
                unsigned
            );
            assert_eq!(
                runtime
                    .bigint_as_int_n(Value::Number(bits), value)
                    .unwrap()
                    .primitive_text()
                    .unwrap(),
                signed
            );
        }
        let mut runtime = Runtime::default();
        let max = Value::BigInt(Arc::new(
            BigInt::parse(&format!("0x{}", "f".repeat(1024))).unwrap(),
        ));
        assert_eq!(
            runtime
                .bigint_as_int_n(Value::Number(4096.), max.clone())
                .unwrap()
                .primitive_text()
                .unwrap(),
            "-1"
        );
        assert_eq!(
            runtime
                .bigint_as_uint_n(Value::Number(4096.), max.clone())
                .unwrap(),
            max
        );
        let negative = Value::BigInt(Arc::new(BigInt::parse("-1").unwrap()));
        assert_eq!(
            runtime
                .bigint_as_uint_n(Value::Number(4096.), negative)
                .unwrap(),
            max
        );
    }
    #[test]
    fn truncation_conversion_source_cases() {
        assert_eq!(run("BigInt.asIntN(8,255n)===-1n && BigInt.asUintN(8,-1n)===255n && BigInt.asUintN('8','511')===255n && BigInt.asUintN(8,true)===1n && BigInt.asIntN(undefined,1n)===0n && BigInt.asIntN(NaN,1n)===0n && BigInt.asIntN(-0.5,1n)===0n;").unwrap(), Value::Bool(true));
        for expression in [
            "BigInt.asIntN(0,1)",
            "BigInt.asUintN(8,1)",
            "BigInt.asIntN(8n,1n)",
            "BigInt.asUintN(Symbol(),1n)",
            "BigInt.asIntN(0,null)",
        ] {
            let source =
                format!("let ok=false;try{{{expression};}}catch(e){{ok=e.name==='TypeError'}}ok;");
            assert_eq!(run(&source).unwrap(), Value::Bool(true), "{expression}");
        }
        for expression in ["BigInt.asIntN(-1,1n)", "BigInt.asUintN(Infinity,1n)"] {
            let source =
                format!("let ok=false;try{{{expression};}}catch(e){{ok=e.name==='RangeError'}}ok;");
            assert_eq!(run(&source).unwrap(), Value::Bool(true));
        }
        assert_eq!(
            run("let ok=false;try{BigInt.asUintN(0,'bad')}catch(e){ok=e.name==='SyntaxError'}ok;")
                .unwrap(),
            Value::Bool(true)
        );
    }
    #[test]
    fn truncation_resource_bounds_precede_allocation() {
        let one = Value::BigInt(Arc::new(BigInt::one()));
        let mut runtime = Runtime::default();
        assert_eq!(
            runtime
                .bigint_as_uint_n(Value::Number(4097.), one.clone())
                .unwrap_err()
                .kind,
            crate::ErrorKind::Resource
        );
        runtime.remaining = 0;
        assert_eq!(
            runtime
                .bigint_as_int_n(Value::Number(8.), one.clone())
                .unwrap_err()
                .kind,
            crate::ErrorKind::Resource
        );
        let mut runtime = Runtime::default();
        runtime.limits.string_bytes = 0;
        assert_eq!(
            runtime
                .bigint_as_uint_n(Value::Number(8.), one)
                .unwrap_err()
                .kind,
            crate::ErrorKind::Resource
        );
    }

    #[test]
    fn comparison_does_not_swallow_resource_limits() {
        for source in [
            "1n == '9'.repeat(5000);",
            "'9'.repeat(5000) < 1n;",
            "1n != '9'.repeat(5000);",
        ] {
            assert_eq!(run(source).unwrap_err().kind, crate::ErrorKind::Resource);
        }
        assert_eq!(
            run("1n !== '9'.repeat(5000) && !(1n === '9'.repeat(5000));").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            run("!(1n=='bad') && 1n!='bad' && !(1n<'bad');").unwrap(),
            Value::Bool(true)
        );
        let one = Value::BigInt(Arc::new(BigInt::one()));
        let text = Value::Utf16(Arc::from(vec![b'9' as u16; 5000]));
        let mut runtime = Runtime {
            remaining: 0,
            ..Runtime::default()
        };
        runtime.limits.string_bytes = 0;
        assert_eq!(
            runtime.bigint_binary("===", &one, &text).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            runtime.bigint_binary("!==", &text, &one).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(runtime.remaining, 0);
        assert_eq!(runtime.string_bytes, 0);
        let mut runtime = Runtime {
            remaining: 10_000_000,
            ..Runtime::default()
        };
        // Sufficient work budget reaches the parser's own magnitude/input cap.
        assert_eq!(
            runtime.bigint_binary("==", &one, &text).unwrap_err().kind,
            crate::ErrorKind::Resource
        );
        let mut runtime = Runtime::default();
        runtime.limits.string_bytes = 0;
        assert_eq!(
            runtime
                .bigint_binary("==", &one, &Value::Utf16(Arc::from(vec![49u16])))
                .unwrap_err()
                .kind,
            crate::ErrorKind::Resource
        );
        let mut runtime = Runtime::default();
        let invalid = Value::Utf16(Arc::from(vec![0xd800u16]));
        assert_eq!(
            runtime.bigint_binary("==", &one, &invalid).unwrap(),
            Value::Bool(false)
        );
        let mut runtime = Runtime::default();
        let error = runtime.bigint_binary("-", &one, &text).unwrap_err();
        assert_eq!(error.kind, crate::ErrorKind::Runtime);
        assert!(error.message.contains("Cannot mix BigInt"));
    }

    #[test]
    fn wrappers_have_real_prototypes_and_brand_checks() {
        for source in [
            "const b=Object(255n);typeof b==='object' && b!==Object(255n) && b.valueOf()===255n && b.toString(16)==='ff' && Object.getPrototypeOf(b)===BigInt.prototype && b instanceof BigInt && !(1n instanceof BigInt);",
            "Object.getPrototypeOf(BigInt.prototype)===Object.prototype && BigInt.prototype.constructor===BigInt && BigInt.prototype.valueOf.call(2n)===2n && BigInt.prototype.toString.call(Object(10n),2)==='1010';",
            "const p=BigInt.prototype;const d=Object.getOwnPropertyDescriptor(p,'valueOf');const c=Object.getOwnPropertyDescriptor(p,'constructor');d.writable&&!d.enumerable&&d.configurable&&c.writable&&!c.enumerable&&c.configurable&&Object.keys(p).length===0;",
            "const p=BigInt.prototype;const d=Object.getOwnPropertyDescriptor(p,Symbol.toStringTag);d.value==='BigInt'&&!d.writable&&!d.enumerable&&d.configurable&&Object.getOwnPropertySymbols(p).includes(Symbol.toStringTag)&&Object.prototype.toString.call(Object(1n))==='[object BigInt]'&&(1n)[Symbol.toStringTag]==='BigInt';",
            "const b=Object(1n);Object.defineProperty(b,Symbol.toStringTag,{value:'Wrapped'});Object.prototype.toString.call(b)==='[object Wrapped]';",
            "BigInt.prototype.toString=function(){return 'changed'};(1n).toString()==='changed'&&Object(1n).toString()==='changed';",
            "Object.defineProperty(BigInt.prototype,'read',{get:function(){return this;}});const b=Object(1n);b.read===b;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
        for source in [
            "BigInt.prototype.valueOf()",
            "BigInt.prototype.toString()",
            "BigInt.prototype.valueOf.call({})",
            "BigInt.prototype.toString.call(Object(1))",
            "Object.create(BigInt.prototype).valueOf()",
            "new BigInt(1)",
            "Object(1n).toLocaleString()",
        ] {
            let script =
                format!("let ok=false;try{{{source};}}catch(e){{ok=e.name==='TypeError'}}ok;");
            assert_eq!(run(&script).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn wrapper_json_and_unsupported_coercions_are_explicit() {
        assert_eq!(
            run("Object(1n)+1n===2n && Object(1n)==1 && Object(1n)<2n;").unwrap(),
            Value::Bool(true)
        );
        for source in [
            "JSON.stringify(Object(1n))",
            "+Object(1n)",
            "[1].at(Object(0n))",
            "'x'.repeat(Object(1n))",
            "Math.abs(Object(1n))",
            "new Uint8Array([Object(1n)])",
            "String.fromCharCode(Object(65n))",
        ] {
            let script =
                format!("let ok=false;try{{{source};}}catch(e){{ok=e.name==='TypeError'}}ok;");
            assert_eq!(run(&script).unwrap(), Value::Bool(true), "{source}");
        }
        assert_eq!(run("JSON.stringify(Object(1n),(k,v)=>typeof v==='object'?v.valueOf().toString():v)==='\"1\"';").unwrap(),Value::Bool(true));
        assert_eq!(
            run("const b=Object(1n);b.toJSON=()=> 'saved';JSON.stringify(b)==='\"saved\"';")
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(run("let ok=false;try{JSON.stringify(Object(1n),()=>{throw 'replacer';})}catch(e){ok=e==='replacer'}ok;").unwrap(),Value::Bool(true));
    }
    #[test]
    fn wrapper_slots_follow_gc_liveness() {
        let mut runtime = Runtime::default();
        let value = Value::BigInt(Arc::new(BigInt::parse("9007199254740993").unwrap()));
        let wrapper = runtime.box_value(value.clone()).unwrap();
        let Value::Object(id) = wrapper else {
            panic!("wrapper must be object")
        };
        runtime
            .collect_garbage(&[Value::Object(id)], &Empty)
            .unwrap();
        assert_eq!(
            runtime
                .bigint_method("BigInt.valueOf", &Value::Object(id), &[])
                .unwrap(),
            value
        );
        runtime.collect_garbage(&[], &Empty).unwrap();
        assert!(!runtime.reflection.boxed.contains_key(&id));
        let prototype = runtime.reflection_prototype("BigInt").unwrap();
        assert!(!runtime.is_bigint_wrapper(&prototype));
    }
}
