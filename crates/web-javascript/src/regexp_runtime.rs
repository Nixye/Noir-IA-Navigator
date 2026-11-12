use crate::{Heap, Host, JsError, Runtime, Value, regexp};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub(super) struct Match {
    start: usize,
    end: usize,
    groups: Vec<Option<String>>,
    names: Vec<(String, usize)>,
    indices: Option<Vec<Option<std::ops::Range<usize>>>>,
}
pub(super) fn regexp_source(source: &str) -> String {
    if source.is_empty() {
        return "(?:)".into();
    }
    let mut result = String::new();
    let mut slash = false;
    for c in source.chars() {
        match c {
            '/' if !slash => result.push_str("\\/"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\u{2028}' => result.push_str("\\u2028"),
            '\u{2029}' => result.push_str("\\u2029"),
            _ => result.push(c),
        }
        slash = c == '\\' && !slash;
    }
    result
}
impl Runtime {
    pub(super) fn regexp(&self, id: usize) -> Result<Arc<regexp::Compiled>, JsError> {
        match self.heap.get(id) {
            Some(Heap::Object(object)) => object
                .regexp
                .clone()
                .ok_or_else(|| JsError::runtime("RegExp method requires a RegExp receiver")),
            _ => Err(JsError::runtime("Invalid RegExp object")),
        }
    }
    pub(super) fn make_regexp(&mut self, pattern: &str, flags: &str) -> Result<Value, JsError> {
        if self.regexp_count >= regexp::MAX_REGEXES
            || self.regexp_pattern_bytes.saturating_add(pattern.len()) > regexp::MAX_PATTERN_TOTAL
        {
            return Err(JsError::resource("RegExp object/pattern budget exceeded"));
        }
        self.regex_work(pattern.len().saturating_mul(16).max(1))?;
        let compiled = Arc::new(regexp::Compiled::compile(pattern, flags)?);
        let source = self.string(regexp_source(pattern))?;
        let canonical = self.string(compiled.flags.clone())?;
        let properties = BTreeMap::from([
            ("source".into(), source),
            ("flags".into(), canonical),
            ("lastIndex".into(), Value::Number(0.)),
            ("global".into(), Value::Bool(compiled.global)),
            ("sticky".into(), Value::Bool(compiled.sticky)),
            ("unicode".into(), Value::Bool(compiled.unicode)),
            ("ignoreCase".into(), Value::Bool(compiled.insensitive)),
            ("multiline".into(), Value::Bool(compiled.multiline)),
            ("dotAll".into(), Value::Bool(flags.contains('s'))),
            ("hasIndices".into(), Value::Bool(compiled.has_indices)),
            ("test".into(), Value::Native("RegExp.test")),
            ("exec".into(), Value::Native("RegExp.exec")),
            ("toString".into(), Value::Native("RegExp.toString")),
        ]);
        let object = self.object(properties, None)?;
        self.hide_properties(
            &object,
            &[
                "source",
                "flags",
                "lastIndex",
                "global",
                "sticky",
                "unicode",
                "ignoreCase",
                "multiline",
                "dotAll",
                "hasIndices",
                "test",
                "exec",
                "toString",
            ],
        );
        let Value::Object(id) = object else {
            unreachable!()
        };
        let Heap::Object(object) = &mut self.heap[id] else {
            unreachable!()
        };
        object.regexp = Some(compiled);
        self.property_metadata.entry(id).or_default().insert(
            "lastIndex".into(),
            crate::descriptors::PropertyMetadata {
                writable: true,
                enumerable: false,
                configurable: false,
                accessor: None,
            },
        );
        self.regexp_count += 1;
        self.regexp_pattern_bytes += pattern.len();
        Ok(Value::Object(id))
    }
    pub(super) fn regexp_constructor(
        &mut self,
        args: &[Value],
        function: bool,
    ) -> Result<Value, JsError> {
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        let explicit_flags = args
            .get(1)
            .filter(|flags| !matches!(flags, Value::Undefined));
        if matches!(first, Value::Symbol(..)) || matches!(explicit_flags, Some(Value::Symbol(..))) {
            return Err(self.property_type_error("Cannot convert a Symbol value to a string"));
        }
        let existing = if let Value::Object(id) = first {
            self.regexp(id).ok()
        } else {
            None
        };
        if function && explicit_flags.is_none() && existing.is_some() {
            return Ok(first);
        }
        let pattern = if let Some(compiled) = &existing {
            compiled.source.clone()
        } else if first == Value::Undefined {
            String::new()
        } else {
            self.text(&first, 0)?
        };
        let flags = if let Some(flags) = explicit_flags {
            self.text(flags, 0)?
        } else {
            existing.map_or_else(String::new, |compiled| compiled.flags.clone())
        };
        self.make_regexp(&pattern, &flags)
    }
    fn regex_work(&mut self, work: usize) -> Result<(), JsError> {
        self.regexp_work = self
            .regexp_work
            .checked_add(work)
            .ok_or_else(|| JsError::resource("RegExp work overflow"))?;
        if self.regexp_work > regexp::MAX_SEARCH_WORK {
            return Err(JsError::resource(
                "RegExp search/compilation work budget exceeded",
            ));
        }
        self.tick()
    }
    fn regexp_last_index(&mut self, id: usize) -> Result<usize, JsError> {
        let Some(Heap::Object(object)) = self.heap.get(id) else {
            return Err(JsError::runtime("Invalid RegExp object"));
        };
        if matches!(
            object.properties.get("lastIndex"),
            Some(Value::BigInt(_) | Value::Symbol(..))
        ) || object
            .properties
            .get("lastIndex")
            .is_some_and(|value| self.is_bigint_wrapper(value))
        {
            return Err(
                self.property_type_error("Cannot implicitly convert BigInt or Symbol to a number")
            );
        }
        let n = self.number(
            object
                .properties
                .get("lastIndex")
                .unwrap_or(&Value::Undefined),
        );
        Ok(if n.is_nan() || n <= 0. {
            0
        } else {
            n.floor() as usize
        })
    }
    fn set_last_index(&mut self, id: usize, index: usize) -> Result<(), JsError> {
        if self
            .property_metadata
            .get(&id)
            .and_then(|properties| properties.get("lastIndex"))
            .is_some_and(|metadata| !metadata.writable)
        {
            return Err(self.property_type_error("RegExp lastIndex is not writable"));
        }
        self.reserve_named_property(id, "lastIndex")?;
        if let Heap::Object(object) = &mut self.heap[id] {
            object
                .properties
                .insert("lastIndex".into(), Value::Number(index as f64));
        }
        Ok(())
    }
    fn regexp_find(
        &mut self,
        compiled: &regexp::Compiled,
        text: &str,
        start: usize,
        sticky: bool,
    ) -> Result<Option<Match>, JsError> {
        compiled.validate_input(text)?;
        self.regex_work(
            text.len()
                .saturating_add(1)
                .saturating_mul(compiled.source.len().max(1))
                .saturating_add(compiled.extra_search_work()),
        )?;
        let Some(captures) = compiled.captures_at(text, start)? else {
            return Ok(None);
        };
        let found = captures
            .groups
            .first()
            .and_then(Option::as_ref)
            .ok_or_else(|| JsError::runtime("Invalid RegExp match result"))?;
        if sticky && found.start != start {
            return Ok(None);
        }
        let groups = captures
            .groups
            .iter()
            .map(|part| {
                part.as_ref()
                    .map(|range| {
                        text.get(range.clone())
                            .map(str::to_owned)
                            .ok_or_else(|| JsError::runtime("Invalid RegExp capture range"))
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Match {
            start: found.start,
            end: found.end,
            groups,
            names: compiled.names.clone(),
            indices: compiled.has_indices.then_some(captures.groups),
        }))
    }
    pub(super) fn regexp_exec(&mut self, id: usize, text: &str) -> Result<Option<Match>, JsError> {
        let compiled = self.regexp(id)?;
        let stateful = compiled.global || compiled.sticky;
        let last_index = self.regexp_last_index(id)?;
        let units = if stateful { last_index } else { 0 };
        if units > text.encode_utf16().count() {
            if stateful {
                self.set_last_index(id, 0)?;
            }
            return Ok(None);
        }
        let start = regexp::byte_index(text, units).ok_or_else(|| {
            JsError::runtime("RegExp lastIndex inside a surrogate pair is not implemented")
        })?;
        let result = self.regexp_find(&compiled, text, start, compiled.sticky)?;
        if stateful {
            self.set_last_index(
                id,
                result
                    .as_ref()
                    .map_or(0, |found| regexp::utf16_index(text, found.end)),
            )?;
        }
        Ok(result)
    }
    pub(super) fn regexp_array(&mut self, found: Match, text: &str) -> Result<Value, JsError> {
        let named = self.regexp_named_groups(&found)?;
        let indices = self.regexp_indices(&found, text)?;
        let values = found
            .groups
            .into_iter()
            .map(|group| group.map_or(Ok(Value::Undefined), |text| self.string(text)))
            .collect::<Result<Vec<_>, _>>()?;
        let id = self.allocate(Heap::Array(values))?;
        let input = self.string(text.into())?;
        for key in ["index", "input", "groups"] {
            self.reserve_named_property(id, key)?;
        }
        self.array_properties.insert(
            id,
            BTreeMap::from([
                (
                    "index".into(),
                    Value::Number(regexp::utf16_index(text, found.start) as f64),
                ),
                ("input".into(), input),
                ("groups".into(), named),
            ]),
        );
        if let Some(indices) = indices {
            self.reserve_named_property(id, "indices")?;
            self.array_properties
                .get_mut(&id)
                .expect("capture metadata")
                .insert("indices".into(), indices);
        }
        Ok(Value::Array(id))
    }
    fn regexp_indices(&mut self, found: &Match, text: &str) -> Result<Option<Value>, JsError> {
        let Some(indices) = &found.indices else {
            return Ok(None);
        };
        let mut values = Vec::with_capacity(indices.len());
        for range in indices {
            self.tick()?;
            values.push(if let Some(range) = range {
                Value::Array(self.allocate(Heap::Array(vec![
                    Value::Number(regexp::utf16_index(text, range.start) as f64),
                    Value::Number(regexp::utf16_index(text, range.end) as f64),
                ]))?)
            } else {
                Value::Undefined
            });
        }
        let named = if found.names.is_empty() {
            Value::Undefined
        } else {
            let properties = found
                .names
                .iter()
                .map(|(name, index)| {
                    (
                        name.clone(),
                        values.get(*index).cloned().unwrap_or(Value::Undefined),
                    )
                })
                .collect();
            self.object(properties, None)?
        };
        let id = self.allocate(Heap::Array(values))?;
        self.reserve_named_property(id, "groups")?;
        self.array_properties
            .insert(id, BTreeMap::from([("groups".into(), named)]));
        Ok(Some(Value::Array(id)))
    }
    fn regexp_named_groups(&mut self, found: &Match) -> Result<Value, JsError> {
        if found.names.is_empty() {
            return Ok(Value::Undefined);
        }
        let mut properties = BTreeMap::new();
        for (name, index) in &found.names {
            let value = found
                .groups
                .get(*index)
                .and_then(Option::as_ref)
                .map_or(Ok(Value::Undefined), |value| self.string(value.clone()))?;
            properties.insert(name.clone(), value);
        }
        self.object(properties, None)
    }
    fn regex_argument(&mut self, value: &Value) -> Result<usize, JsError> {
        if matches!(value, Value::Symbol(..)) {
            return Err(self.property_type_error("Cannot convert a Symbol value to a string"));
        }
        if let Value::Object(id) = value
            && self.regexp(*id).is_ok()
        {
            return Ok(*id);
        }
        let pattern = if *value == Value::Undefined {
            String::new()
        } else {
            self.text(value, 0)?
        };
        let Value::Object(id) = self.make_regexp(&pattern, "")? else {
            unreachable!()
        };
        Ok(id)
    }
    pub(super) fn regexp_string_method(
        &mut self,
        text: &str,
        method: &str,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        if method == "replace" {
            return self.replace_string(
                text,
                &first,
                args.get(1).unwrap_or(&Value::Undefined),
                host,
            );
        }
        if method == "split" {
            return self.split_string(text, &first, args.get(1));
        }
        let id = self.regex_argument(&first)?;
        let compiled = self.regexp(id)?;
        if method == "search" {
            let saved = if let Heap::Object(object) = &self.heap[id] {
                object
                    .properties
                    .get("lastIndex")
                    .cloned()
                    .unwrap_or(Value::Undefined)
            } else {
                Value::Undefined
            };
            if !matches!(saved, Value::Number(number) if number == 0. && number.is_sign_positive())
            {
                self.set_last_index(id, 0)?;
            }
            let found = self.regexp_exec(id, text);
            if let Heap::Object(object) = &mut self.heap[id] {
                object.properties.insert("lastIndex".into(), saved);
            }
            return found.map(|found| {
                Value::Number(
                    found.map_or(-1., |found| regexp::utf16_index(text, found.start) as f64),
                )
            });
        }
        if !compiled.global {
            return match self.regexp_exec(id, text)? {
                Some(found) => self.regexp_array(found, text),
                None => Ok(Value::Null),
            };
        }
        self.set_last_index(id, 0)?;
        let mut matches = Vec::new();
        while let Some(found) = self.regexp_exec(id, text)? {
            if matches.len() >= 4096 {
                return Err(JsError::resource("RegExp match array exceeds 4096 items"));
            }
            matches.push(self.string(found.groups[0].clone().unwrap_or_default())?);
            if found.start == found.end {
                self.set_last_index(id, advance_units(text, found.end))?;
            }
        }
        if matches.is_empty() {
            Ok(Value::Null)
        } else {
            Ok(Value::Array(self.allocate(Heap::Array(matches))?))
        }
    }
    fn replace_string(
        &mut self,
        text: &str,
        pattern: &Value,
        replacement: &Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let regex = if let Value::Object(id) = pattern {
            self.regexp(*id).ok().map(|compiled| (*id, compiled))
        } else {
            None
        };
        let mut matches = Vec::new();
        if let Some((id, compiled)) = &regex {
            if compiled.global {
                self.set_last_index(*id, 0)?;
            }
            while let Some(found) = self.regexp_exec(*id, text)? {
                if matches.len() >= 4096 {
                    return Err(JsError::resource("RegExp replacement count exceeded"));
                }
                let empty = found.start == found.end;
                let end = found.end;
                matches.push(found);
                if !compiled.global {
                    break;
                }
                if empty {
                    self.set_last_index(*id, advance_units(text, end))?;
                }
            }
        } else {
            let literal = self.text(pattern, 0)?;
            self.regex_work(text.len() + literal.len() + 1)?;
            if let Some(start) = text.find(&literal) {
                matches.push(Match {
                    start,
                    end: start + literal.len(),
                    groups: vec![Some(literal)],
                    names: Vec::new(),
                    indices: None,
                });
            }
        }
        let callable = matches!(replacement, Value::Function(_) | Value::Native(_))
            || matches!(replacement,Value::Host(id)if host.is_callable(*id));
        let replacement_text = if callable {
            String::new()
        } else {
            if matches!(replacement, Value::Symbol(..)) {
                return Err(self.property_type_error("Cannot convert a Symbol value to a string"));
            }
            self.text(replacement, 0)?
        };
        let mut result = String::new();
        let mut previous = 0;
        for found in matches {
            append(&mut result, &text[previous..found.start])?;
            let value = if callable {
                let length = found.groups.len() + 2 + usize::from(!found.names.is_empty());
                crate::call_storage::check_arguments(length)?;
                let mut args = self.value_buffer(length)?;
                for value in &found.groups {
                    args.push(
                        value
                            .as_ref()
                            .map_or(Ok(Value::Undefined), |value| self.string(value.clone()))?,
                    );
                }
                args.push(Value::Number(regexp::utf16_index(text, found.start) as f64));
                args.push(self.string(text.into())?);
                if !found.names.is_empty() {
                    args.push(self.regexp_named_groups(&found)?);
                }
                let value = self.call(replacement.clone(), args, Value::Undefined, host)?;
                if matches!(value, Value::Symbol(..)) {
                    return Err(
                        self.property_type_error("Cannot convert a Symbol value to a string")
                    );
                }
                self.text(&value, 0)?
            } else {
                substitution(&replacement_text, text, &found)?
            };
            append(&mut result, &value)?;
            previous = found.end;
        }
        append(&mut result, &text[previous..])?;
        self.string(result)
    }
    fn split_string(
        &mut self,
        text: &str,
        separator: &Value,
        limit: Option<&Value>,
    ) -> Result<Value, JsError> {
        if matches!(limit, Some(Value::Symbol(..) | Value::BigInt(_)))
            || limit.is_some_and(|value| self.is_bigint_wrapper(value))
        {
            return Err(self.property_type_error("Cannot convert a Symbol value to a number"));
        }
        let limit = limit
            .filter(|value| **value != Value::Undefined)
            .map_or(u32::MAX, |value| crate::to_uint32(self.number(value)))
            as usize;
        if limit == 0 {
            return Ok(Value::Array(self.allocate(Heap::Array(Vec::new()))?));
        }
        if *separator == Value::Undefined {
            let text = self.string(text.into())?;
            return Ok(Value::Array(self.allocate(Heap::Array(vec![text]))?));
        }
        let compiled = if let Value::Object(id) = separator {
            self.regexp(*id).ok()
        } else {
            None
        };
        let mut values = Vec::new();
        if let Some(compiled) = compiled {
            let mut cursor = 0;
            let mut previous = 0;
            if text.is_empty() {
                if self.regexp_find(&compiled, text, 0, false)?.is_none() {
                    values.push(self.string(String::new())?);
                }
                return Ok(Value::Array(self.allocate(Heap::Array(values))?));
            }
            while cursor <= text.len() {
                let Some(found) = self.regexp_find(&compiled, text, cursor, false)? else {
                    break;
                };
                if found.start == found.end && (found.end == previous || found.end == text.len()) {
                    if found.end == text.len() {
                        break;
                    }
                    cursor = advance_byte(text, found.end);
                    continue;
                }
                push(
                    &mut values,
                    self.string(text[previous..found.start].into())?,
                    limit,
                )?;
                if values.len() == limit {
                    return Ok(Value::Array(self.allocate(Heap::Array(values))?));
                }
                for group in found.groups.iter().skip(1) {
                    let value = group
                        .as_ref()
                        .map_or(Ok(Value::Undefined), |value| self.string(value.clone()))?;
                    push(&mut values, value, limit)?;
                    if values.len() == limit {
                        return Ok(Value::Array(self.allocate(Heap::Array(values))?));
                    }
                }
                previous = found.end;
                cursor = if found.start == found.end {
                    advance_byte(text, found.end)
                } else {
                    found.end
                };
            }
            push(&mut values, self.string(text[previous..].into())?, limit)?;
        } else {
            let literal = self.text(separator, 0)?;
            self.regex_work(text.len() + literal.len() + 1)?;
            if literal.is_empty() {
                if !text.is_ascii() {
                    return Err(JsError::runtime(
                        "UTF-16 empty string splitting is not implemented",
                    ));
                }
                for c in text.chars() {
                    push(&mut values, self.string(c.to_string())?, limit)?;
                    if values.len() == limit {
                        break;
                    }
                }
            } else {
                for part in text.split(&literal) {
                    push(&mut values, self.string(part.into())?, limit)?;
                    if values.len() == limit {
                        break;
                    }
                }
            }
        }
        Ok(Value::Array(self.allocate(Heap::Array(values))?))
    }
}
fn append(target: &mut String, text: &str) -> Result<(), JsError> {
    if target.len().saturating_add(text.len()) > 65536 {
        return Err(JsError::resource("RegExp string result exceeds 64 KiB"));
    }
    target.push_str(text);
    Ok(())
}
fn push(values: &mut Vec<Value>, value: Value, limit: usize) -> Result<(), JsError> {
    if values.len() >= 4096 {
        return Err(JsError::resource("Split array exceeds 4096 items"));
    }
    if values.len() < limit {
        values.push(value);
    }
    Ok(())
}
fn advance_byte(text: &str, index: usize) -> usize {
    text[index..]
        .chars()
        .next()
        .map_or(text.len() + 1, |c| index + c.len_utf8())
}
fn advance_units(text: &str, index: usize) -> usize {
    if index == text.len() {
        text.encode_utf16().count() + 1
    } else {
        regexp::utf16_index(text, advance_byte(text, index))
    }
}
fn substitution(replacement: &str, text: &str, found: &Match) -> Result<String, JsError> {
    let mut output = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            append(&mut output, &c.to_string())?;
            continue;
        }
        let Some(next) = chars.peek().copied() else {
            append(&mut output, "$")?;
            break;
        };
        match next {
            '$' => {
                chars.next();
                append(&mut output, "$")?;
            }
            '&' => {
                chars.next();
                append(&mut output, &text[found.start..found.end])?;
            }
            '`' => {
                chars.next();
                append(&mut output, &text[..found.start])?;
            }
            '\'' => {
                chars.next();
                append(&mut output, &text[found.end..])?;
            }
            '<' if !found.names.is_empty() => {
                let mut copy = chars.clone();
                copy.next();
                let mut name = String::new();
                let mut complete = false;
                for c in copy.by_ref() {
                    if c == '>' {
                        complete = true;
                        break;
                    }
                    name.push(c);
                }
                if complete {
                    chars = copy;
                    if let Some((_, index)) = found.names.iter().find(|(key, _)| key == &name)
                        && let Some(Some(group)) = found.groups.get(*index)
                    {
                        append(&mut output, group)?;
                    }
                } else {
                    append(&mut output, "$")?;
                }
            }
            '1'..='9' => {
                let first = (next as u8 - b'0') as usize;
                let mut copy = chars.clone();
                copy.next();
                let second = copy.peek().and_then(|c| c.to_digit(10));
                let number = second.map_or(first, |second| first * 10 + second as usize);
                if number < found.groups.len() {
                    chars.next();
                    if second.is_some() {
                        chars.next();
                    }
                    if let Some(group) = &found.groups[number] {
                        append(&mut output, group)?;
                    }
                } else if first < found.groups.len() {
                    chars.next();
                    if let Some(group) = &found.groups[first] {
                        append(&mut output, group)?;
                    }
                } else {
                    append(&mut output, "$")?;
                }
            }
            _ => append(&mut output, "$")?,
        }
    }
    Ok(output)
}
