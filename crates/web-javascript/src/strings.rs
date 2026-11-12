//! UTF-16 code-unit operations. Unpaired surrogates retain their identity in the
//! VM; conversions to UTF-8-only host APIs are a separate checked boundary.
use crate::{Heap, Host, JsError, Runtime, Value, to_uint32};
use std::sync::Arc;
use unicode_normalization::UnicodeNormalization;

const MAX_UNITS: usize = 65_536;

pub(super) fn units(value: &Value) -> Option<Vec<u16>> {
    match value {
        Value::String(text) => Some(text.encode_utf16().collect()),
        Value::Utf16(text) => Some(text.to_vec()),
        _ => None,
    }
}

pub(super) fn method(property: &str) -> Option<Value> {
    Some(Value::Native(match property {
        "at" => "String.at",
        "charAt" => "String.charAt",
        "charCodeAt" => "String.charCodeAt",
        "codePointAt" => "String.codePointAt",
        "slice" => "String.slice",
        "substring" => "String.substring",
        "substr" => "String.substr",
        "includes" => "String.includes",
        "indexOf" => "String.indexOf",
        "lastIndexOf" => "String.lastIndexOf",
        "startsWith" => "String.startsWith",
        "endsWith" => "String.endsWith",
        "concat" => "String.concat",
        "repeat" => "String.repeat",
        "padStart" => "String.padStart",
        "padEnd" => "String.padEnd",
        "trim" => "String.trim",
        "trimStart" => "String.trimStart",
        "trimEnd" => "String.trimEnd",
        "trimLeft" => "String.trimStart",
        "trimRight" => "String.trimEnd",
        "toLowerCase" => "String.toLowerCase",
        "toUpperCase" => "String.toUpperCase",
        "normalize" => "String.normalize",
        "toString" => "String.toString",
        "valueOf" => "String.valueOf",
        "isWellFormed" => "String.isWellFormed",
        "toWellFormed" => "String.toWellFormed",
        "replace" => "String.replace",
        "replaceAll" => "String.replaceAll",
        "split" => "String.split",
        "match" => "String.match",
        "search" => "String.search",
        _ => return None,
    }))
}

fn whitespace(unit: u16) -> bool {
    matches!(unit,0x9..=0xd|0x20|0xa0|0x1680|0x2000..=0x200a|0x2028|0x2029|0x202f|0x205f|0x3000|0xfeff)
}
fn append(output: &mut Vec<u16>, text: &[u16]) -> Result<(), JsError> {
    if output.len().saturating_add(text.len()) > MAX_UNITS {
        return Err(JsError::resource("UTF-16 string exceeds 65536 units"));
    }
    output.extend_from_slice(text);
    Ok(())
}
fn find(text: &[u16], needle: &[u16], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(text.len()));
    }
    if start > text.len() || needle.len() > text.len() - start {
        return None;
    }
    let prefix = prefix_table(needle);
    let mut matched = 0;
    for (offset, unit) in text[start..].iter().enumerate() {
        while matched > 0 && needle[matched] != *unit {
            matched = prefix[matched - 1];
        }
        if needle[matched] == *unit {
            matched += 1;
        }
        if matched == needle.len() {
            return Some(start + offset + 1 - matched);
        }
    }
    None
}

fn prefix_table(needle: &[u16]) -> Vec<usize> {
    let mut prefix = vec![0; needle.len()];
    let mut length = 0;
    for index in 1..needle.len() {
        while length > 0 && needle[length] != needle[index] {
            length = prefix[length - 1];
        }
        if needle[length] == needle[index] {
            length += 1;
        }
        prefix[index] = length;
    }
    prefix
}
fn find_last(text: &[u16], needle: &[u16], position: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(position.min(text.len()));
    }
    if needle.len() > text.len() {
        return None;
    }
    let prefix = prefix_table(needle);
    let mut matched = 0;
    let mut result = None;
    for (index, unit) in text
        .iter()
        .take(position.saturating_add(needle.len()).min(text.len()))
        .enumerate()
    {
        while matched > 0 && needle[matched] != *unit {
            matched = prefix[matched - 1];
        }
        if needle[matched] == *unit {
            matched += 1;
        }
        if matched == needle.len() {
            result = Some(index + 1 - matched);
            matched = prefix[matched - 1];
        }
    }
    result
}

impl Runtime {
    pub(super) fn utf16_string(&mut self, units: Vec<u16>) -> Result<Value, JsError> {
        if units.len() > MAX_UNITS {
            return Err(JsError::resource("UTF-16 string exceeds 65536 units"));
        }
        if let Ok(text) = String::from_utf16(&units) {
            return self.string(text);
        }
        let bytes = units
            .len()
            .checked_mul(2)
            .ok_or_else(|| JsError::resource("UTF-16 string size overflow"))?;
        if bytes > 65_536 {
            return Err(JsError::resource("UTF-16 string exceeds 64 KiB"));
        }
        self.charge_string(bytes)?;
        Ok(Value::Utf16(Arc::from(units)))
    }

    pub(super) fn string_get_property(
        &mut self,
        value: &Value,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if !matches!(value, Value::String(_) | Value::Utf16(_)) {
            return Ok(None);
        }
        self.tick()?;
        if property == "length" {
            let length = match value {
                Value::String(text) => {
                    self.string_copy_work(text.len())?;
                    text.encode_utf16().count()
                }
                Value::Utf16(text) => text.len(),
                _ => unreachable!(),
            };
            return Ok(Some(Value::Number(length as f64)));
        }
        if let Ok(index) = property.parse::<usize>()
            && index.to_string() == property
        {
            let unit = match value {
                Value::String(text) => {
                    self.string_copy_work(text.len())?;
                    text.encode_utf16().nth(index)
                }
                Value::Utf16(text) => text.get(index).copied(),
                _ => unreachable!(),
            };
            return Ok(Some(match unit {
                Some(unit) => self.utf16_string(vec![unit])?,
                None => Value::Undefined,
            }));
        }
        if property == "constructor" {
            return Ok(Some(Value::Native("String")));
        }
        Ok(method(property))
    }

    pub(super) fn string_units_coerce(&mut self, value: &Value) -> Result<Vec<u16>, JsError> {
        if let Some(units) = units(value) {
            return Ok(units);
        }
        if matches!(value, Value::Symbol(..)) {
            return Err(self.property_type_error("Cannot convert a Symbol value to a string"));
        }
        Ok(self.text(value, 0)?.encode_utf16().collect())
    }

    fn string_integer(&mut self, value: Option<&Value>, default: f64) -> Result<f64, JsError> {
        if matches!(value, Some(Value::Symbol(..) | Value::BigInt(_)))
            || value.is_some_and(|value| self.is_bigint_wrapper(value))
        {
            return Err(self.property_type_error("Cannot convert Symbol or BigInt to a number"));
        }
        let value = value.map_or(default, |value| self.number(value));
        Ok(if value.is_nan() { 0. } else { value.trunc() })
    }

    pub(super) fn string_from_char_code(&mut self, args: &[Value]) -> Result<Value, JsError> {
        if args.iter().any(|value| {
            matches!(value, Value::Symbol(..) | Value::BigInt(_)) || self.is_bigint_wrapper(value)
        }) {
            return Err(self.property_type_error("Cannot convert Symbol or BigInt to a number"));
        }
        self.utf16_string(
            args.iter()
                .map(|value| (to_uint32(self.number(value)) & 0xffff) as u16)
                .collect(),
        )
    }
    pub(super) fn string_from_code_point(&mut self, args: &[Value]) -> Result<Value, JsError> {
        let mut output = Vec::new();
        for value in args {
            if matches!(value, Value::Symbol(..) | Value::BigInt(_))
                || self.is_bigint_wrapper(value)
            {
                return Err(self.property_type_error("Cannot convert Symbol or BigInt to a number"));
            }
            let number = self.number(value);
            if !number.is_finite()
                || number.fract() != 0.
                || !(0. ..=0x10ffff as f64).contains(&number)
            {
                return Err(self.string_range_error("Invalid code point"));
            }
            let value = number as u32;
            if value <= 0xffff {
                output.push(value as u16);
            } else {
                let value = value - 0x10000;
                output.push(0xd800 + (value >> 10) as u16);
                output.push(0xdc00 + (value & 0x3ff) as u16);
            }
        }
        self.utf16_string(output)
    }
    fn string_range_error(&mut self, message: &str) -> JsError {
        match self.native("RangeError", &[Value::string(message)]) {
            Ok(value) => {
                let mut error = JsError::runtime(message);
                error.thrown = Some(value);
                error
            }
            Err(error) => error,
        }
    }

    pub(super) fn string_method(
        &mut self,
        receiver: &Value,
        property: &str,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if matches!(receiver, Value::Null | Value::Undefined) {
            return Err(self.property_type_error("String method called on null or undefined"));
        }
        if matches!(property, "toString" | "valueOf") {
            return if matches!(receiver, Value::String(_) | Value::Utf16(_)) {
                Ok(receiver.clone())
            } else {
                Err(self.property_type_error("String receiver required"))
            };
        }
        if property == "concat" {
            let mut output = self.language_string(receiver.clone(), host)?;
            for value in args {
                let value = self.language_string(value, host)?;
                output = self.concat_strings(output, value)?;
            }
            return Ok(output);
        }
        let text = self.string_units_coerce(receiver)?;
        let length = text.len();
        self.remaining = self
            .remaining
            .checked_sub(length as u64 + 1)
            .ok_or_else(|| JsError::resource("String operation work budget exceeded"))?;
        match property {
            "charAt" | "charCodeAt" | "codePointAt" | "at" => {
                let index = self.string_integer(args.first(), 0.)?;
                let index = if property == "at" && index < 0. {
                    length as f64 + index
                } else {
                    index
                };
                if !(0. ..length as f64).contains(&index) {
                    return Ok(match property {
                        "charAt" => self.string(String::new())?,
                        "charCodeAt" => Value::Number(f64::NAN),
                        _ => Value::Undefined,
                    });
                }
                let index = index as usize;
                let unit = text[index];
                match property {
                    "charAt" | "at" => self.utf16_string(vec![unit]),
                    "charCodeAt" => Ok(Value::Number(unit as f64)),
                    _ => {
                        let code = if (0xd800..=0xdbff).contains(&unit)
                            && text
                                .get(index + 1)
                                .is_some_and(|unit| (0xdc00..=0xdfff).contains(unit))
                        {
                            0x10000
                                + ((u32::from(unit) - 0xd800) << 10)
                                + (u32::from(text[index + 1]) - 0xdc00)
                        } else {
                            u32::from(unit)
                        };
                        Ok(Value::Number(code as f64))
                    }
                }
            }
            "slice" | "substring" | "substr" => {
                let start = self.string_integer(args.first(), 0.)?;
                let end = self.string_integer(
                    args.get(1).filter(|value| **value != Value::Undefined),
                    length as f64,
                )?;
                let relative = |value: f64| {
                    if value < 0. {
                        (length as f64 + value).max(0.) as usize
                    } else {
                        value.min(length as f64) as usize
                    }
                };
                let (start, end) = if property == "substring" {
                    let a = start.max(0.).min(length as f64) as usize;
                    let b = end.max(0.).min(length as f64) as usize;
                    (a.min(b), a.max(b))
                } else if property == "substr" {
                    let start = relative(start);
                    let count = if args.get(1).is_none_or(|value| *value == Value::Undefined) {
                        length - start
                    } else {
                        end.max(0.).min((length - start) as f64) as usize
                    };
                    (start, start + count)
                } else {
                    let start = relative(start);
                    (start, relative(end).max(start))
                };
                self.utf16_string(text[start..end].to_vec())
            }
            "includes" | "indexOf" | "lastIndexOf" | "startsWith" | "endsWith" => {
                let argument = args.first().cloned().unwrap_or(Value::Undefined);
                if matches!(property, "includes" | "startsWith" | "endsWith")
                    && matches!(argument,Value::Object(id)if self.regexp(id).is_ok())
                {
                    return Err(self.property_type_error("String search does not accept a RegExp"));
                }
                let needle = self.string_units_coerce(&argument)?;
                if matches!(args.get(1), Some(Value::Symbol(..) | Value::BigInt(_)))
                    || args
                        .get(1)
                        .is_some_and(|value| self.is_bigint_wrapper(value))
                {
                    return Err(
                        self.property_type_error("Cannot convert Symbol or BigInt to a number")
                    );
                }
                let default = if matches!(property, "lastIndexOf" | "endsWith") {
                    length as f64
                } else {
                    0.
                };
                let mut position = args
                    .get(1)
                    .filter(|value| **value != Value::Undefined)
                    .map_or(default, |value| self.number(value));
                if position.is_nan() {
                    position = if property == "lastIndexOf" {
                        length as f64
                    } else {
                        0.
                    };
                }
                let position = position.trunc().clamp(0., length as f64) as usize;
                let found = match property {
                    "lastIndexOf" => find_last(&text, &needle, position),
                    "startsWith" => text
                        .get(position..position.saturating_add(needle.len()))
                        .filter(|value| **value == needle)
                        .map(|_| position),
                    "endsWith" => position
                        .checked_sub(needle.len())
                        .filter(|start| text[*start..position] == needle),
                    _ => find(&text, &needle, position),
                };
                Ok(
                    if matches!(property, "includes" | "startsWith" | "endsWith") {
                        Value::Bool(found.is_some())
                    } else {
                        Value::Number(found.map_or(-1., |index| index as f64))
                    },
                )
            }
            "repeat" => {
                let count = self.string_integer(args.first(), 0.)?;
                if count < 0. || !count.is_finite() {
                    return Err(self.string_range_error("Invalid repeat count"));
                }
                if text.is_empty() {
                    return self.string(String::new());
                }
                if count > MAX_UNITS as f64 / length as f64 {
                    return Err(JsError::resource("Repeated string exceeds limit"));
                }
                self.utf16_string(text.repeat(count as usize))
            }
            "padStart" | "padEnd" => {
                let target = self.string_integer(args.first(), 0.)?.max(0.);
                if target <= length as f64 {
                    return self.utf16_string(text);
                }
                if target > MAX_UNITS as f64 {
                    return Err(JsError::resource("Padded string exceeds limit"));
                }
                let pad = if args.get(1).is_none_or(|value| *value == Value::Undefined) {
                    vec![0x20]
                } else {
                    self.string_units_coerce(&args[1])?
                };
                if pad.is_empty() {
                    return self.utf16_string(text);
                }
                let needed = target as usize - length;
                let padding = pad.into_iter().cycle().take(needed).collect::<Vec<_>>();
                let mut output = Vec::with_capacity(target as usize);
                if property == "padStart" {
                    append(&mut output, &padding)?;
                    append(&mut output, &text)?;
                } else {
                    append(&mut output, &text)?;
                    append(&mut output, &padding)?;
                }
                self.utf16_string(output)
            }
            "trim" | "trimStart" | "trimEnd" => {
                let mut start = 0;
                let mut end = length;
                if property != "trimEnd" {
                    while start < end && whitespace(text[start]) {
                        start += 1;
                    }
                }
                if property != "trimStart" {
                    while end > start && whitespace(text[end - 1]) {
                        end -= 1;
                    }
                }
                self.utf16_string(text[start..end].to_vec())
            }
            "toLowerCase" | "toUpperCase" | "normalize" => {
                let form = if property == "normalize" {
                    if matches!(args.first(), Some(Value::Symbol(..))) {
                        return Err(
                            self.property_type_error("Cannot convert a Symbol value to a string")
                        );
                    }
                    let form = args
                        .first()
                        .filter(|value| **value != Value::Undefined)
                        .map_or(Ok("NFC".into()), |value| self.text(value, 0))?;
                    if !matches!(form.as_str(), "NFC" | "NFD" | "NFKC" | "NFKD") {
                        return Err(self.string_range_error("Invalid normalization form"));
                    }
                    form
                } else {
                    String::new()
                };
                let mut output = Vec::new();
                let mut run = String::new();
                for scalar in char::decode_utf16(text) {
                    match scalar {
                        Ok(ch) => run.push(ch),
                        Err(error) => {
                            self.transform_string_run(&run, property, &form, &mut output)?;
                            run.clear();
                            append(&mut output, &[error.unpaired_surrogate()])?;
                        }
                    }
                }
                self.transform_string_run(&run, property, &form, &mut output)?;
                self.utf16_string(output)
            }
            "isWellFormed" => Ok(Value::Bool(
                char::decode_utf16(text).all(|value| value.is_ok()),
            )),
            "toWellFormed" => {
                let mut output = Vec::new();
                for scalar in char::decode_utf16(text) {
                    let ch = scalar.unwrap_or('\u{fffd}');
                    append(&mut output, ch.encode_utf16(&mut [0; 2]))?;
                }
                self.utf16_string(output)
            }
            "replace" | "replaceAll" | "split" | "match" | "search" => {
                let original = if units(receiver).is_some() {
                    receiver.clone()
                } else {
                    self.utf16_string(text.clone())?
                };
                self.string_pattern_method(&original, &text, property, args, host)
            }
            _ => Err(JsError::runtime(format!(
                "Unsupported string method {property}"
            ))),
        }
    }

    fn transform_string_run(
        &mut self,
        text: &str,
        method: &str,
        form: &str,
        output: &mut Vec<u16>,
    ) -> Result<(), JsError> {
        if method == "normalize" {
            let chars: Box<dyn Iterator<Item = char> + '_> = match form {
                "NFD" => Box::new(text.nfd()),
                "NFKC" => Box::new(text.nfkc()),
                "NFKD" => Box::new(text.nfkd()),
                _ => Box::new(text.nfc()),
            };
            for ch in chars {
                self.tick()?;
                append(output, ch.encode_utf16(&mut [0; 2]))?;
            }
        } else {
            let transformed = if method == "toLowerCase" {
                text.to_lowercase()
            } else {
                text.to_uppercase()
            };
            for ch in transformed.chars() {
                self.tick()?;
                append(output, ch.encode_utf16(&mut [0; 2]))?;
            }
        }
        Ok(())
    }

    fn string_pattern_method(
        &mut self,
        original: &Value,
        text: &[u16],
        method: &str,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let pattern = args.first().cloned().unwrap_or(Value::Undefined);
        if method == "split"
            && (matches!(args.get(1), Some(Value::BigInt(_) | Value::Symbol(..)))
                || args
                    .get(1)
                    .is_some_and(|value| self.is_bigint_wrapper(value)))
        {
            return Err(self.property_type_error("Cannot convert Symbol or BigInt to a number"));
        }
        if matches!(method, "match" | "search")
            || matches!(pattern,Value::Object(id)if self.regexp(id).is_ok())
        {
            if method == "replaceAll"
                && let Value::Object(id) = pattern
                && !self.regexp(id)?.global
            {
                return Err(self.property_type_error("replaceAll requires a global RegExp"));
            }
            let utf8 = String::from_utf16(text).map_err(|_| {
                JsError::runtime("RegExp matching on unpaired UTF-16 surrogates is not implemented")
            })?;
            return self.regexp_string_method(
                &utf8,
                if method == "replaceAll" {
                    "replace"
                } else {
                    method
                },
                args,
                host,
            );
        }
        if method == "split" {
            if matches!(args.get(1), Some(Value::Symbol(..) | Value::BigInt(_)))
                || args
                    .get(1)
                    .is_some_and(|value| self.is_bigint_wrapper(value))
            {
                return Err(self.property_type_error("Cannot convert Symbol or BigInt to a number"));
            }
            let limit = args
                .get(1)
                .filter(|value| **value != Value::Undefined)
                .map_or(u32::MAX, |value| to_uint32(self.number(value)))
                as usize;
            if limit == 0 {
                return Ok(Value::Array(self.allocate(Heap::Array(Vec::new()))?));
            }
            if pattern == Value::Undefined {
                return Ok(Value::Array(
                    self.allocate(Heap::Array(vec![original.clone()]))?,
                ));
            }
            let needle = self.string_units_coerce(&pattern)?;
            let mut values = Vec::new();
            if needle.is_empty() {
                for unit in text.iter().take(limit) {
                    if values.len() >= 4096 {
                        return Err(JsError::resource("String split item limit exceeded"));
                    }
                    values.push(self.utf16_string(vec![*unit])?);
                }
            } else {
                let mut start = 0;
                while let Some(index) = find(text, &needle, start) {
                    self.tick()?;
                    if values.len() >= 4096 {
                        return Err(JsError::resource("String split item limit exceeded"));
                    }
                    values.push(self.utf16_string(text[start..index].to_vec())?);
                    if values.len() >= limit {
                        return Ok(Value::Array(self.allocate(Heap::Array(values))?));
                    }
                    start = index + needle.len();
                }
                if values.len() >= 4096 {
                    return Err(JsError::resource("String split item limit exceeded"));
                }
                values.push(self.utf16_string(text[start..].to_vec())?);
            }
            return Ok(Value::Array(self.allocate(Heap::Array(values))?));
        }
        let needle = self.string_units_coerce(&pattern)?;
        let replacement = args.get(1).cloned().unwrap_or(Value::Undefined);
        let callable = self.is_callable_value(&replacement, host);
        let replacement_text = if callable {
            Vec::new()
        } else {
            self.string_units_coerce(&replacement)?
        };
        let mut output = Vec::new();
        let mut consumed = 0;
        let mut search = 0;
        while let Some(index) = find(text, &needle, search) {
            self.tick()?;
            append(&mut output, &text[consumed..index])?;
            if callable {
                let matched = self.utf16_string(needle.clone())?;
                let result = self.call(
                    replacement.clone(),
                    vec![matched, Value::Number(index as f64), original.clone()],
                    Value::Undefined,
                    host,
                )?;
                append(&mut output, &self.string_units_coerce(&result)?)?;
            } else {
                self.string_substitution(&replacement_text, &needle, text, index, &mut output)?;
            }
            consumed = index + needle.len();
            if method != "replaceAll" {
                break;
            }
            if needle.is_empty() {
                if index == text.len() {
                    break;
                }
                search = index + 1;
            } else {
                search = consumed;
            }
        }
        append(&mut output, &text[consumed..])?;
        self.utf16_string(output)
    }

    fn string_substitution(
        &mut self,
        replacement: &[u16],
        matched: &[u16],
        input: &[u16],
        index: usize,
        output: &mut Vec<u16>,
    ) -> Result<(), JsError> {
        let mut position = 0;
        while position < replacement.len() {
            self.tick()?;
            if replacement[position] == 0x24
                && let Some(next) = replacement.get(position + 1)
            {
                let special = match *next {
                    0x24 => Some(&[0x24][..]),
                    0x26 => Some(matched),
                    0x60 => Some(&input[..index]),
                    0x27 => Some(&input[index + matched.len()..]),
                    _ => None,
                };
                if let Some(text) = special {
                    append(output, text)?;
                    position += 2;
                    continue;
                }
            }
            append(output, &replacement[position..position + 1])?;
            position += 1;
        }
        Ok(())
    }
}
