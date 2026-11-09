//! Bounded display metadata only. No values, source excerpts, getters or I/O.
use super::*;
use std::fmt::{self, Write};

pub(super) const MAX_WARNING_BYTES: usize = 2048;
const MAX_SOURCE_LABEL_BYTES: usize = 256;
const MAX_URL_INPUT_BYTES: usize = 8192;

struct BoundedText {
    text: String,
    limit: usize,
    truncated: bool,
}
impl BoundedText {
    fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
            truncated: false,
        }
    }
    fn finish(mut self) -> String {
        if self.truncated && self.limit >= 3 {
            while self.text.len() > self.limit - 3 {
                self.text.pop();
            }
            self.text.push_str("...");
        }
        self.text
    }
}
impl Write for BoundedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.truncated {
            return Ok(());
        }
        for character in text.chars() {
            let character = if character.is_control()
                || matches!(character, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                ' '
            } else {
                character
            };
            if character.len_utf8() > self.limit.saturating_sub(self.text.len()) {
                self.truncated = true;
                break;
            }
            self.text.push(character);
        }
        Ok(())
    }
}

pub(super) fn bounded_text(text: &str, limit: usize) -> String {
    let mut output = BoundedText::new(limit);
    let _ = output.write_str(text);
    output.finish()
}

/// Bound before parsing/copying. Only hierarchical resource addresses are
/// displayed, with credentials, query and fragment removed before formatting.
pub(super) fn source_label(src: &str, base: Option<&url::Url>) -> String {
    if src.len() > MAX_URL_INPUT_BYTES {
        return "<oversized source URL>".into();
    }
    let parsed =
        url::Url::parse(src).or_else(|error| base.map_or(Err(error), |base| base.join(src)));
    let Ok(mut url) = parsed else {
        // A synthetic base safely parses relative src without inventing an
        // authorized document URL. The displayed label remains relative.
        let base = url::Url::parse("https://noir.invalid/").expect("constant URL");
        let Ok(mut url) = base.join(src) else {
            return "<invalid source URL>".into();
        };
        if url.host_str() != Some("noir.invalid")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return "<unresolved source URL>".into();
        }
        url.set_query(None);
        url.set_fragment(None);
        return bounded_text(url.path(), MAX_SOURCE_LABEL_BYTES);
    };
    if !matches!(url.scheme(), "http" | "https" | "file") {
        return "<unsupported source URL scheme>".into();
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    bounded_text(url.as_str(), MAX_SOURCE_LABEL_BYTES)
}

#[derive(Debug)]
pub(super) struct ScriptOrigin {
    index: usize,
    pub(super) node: NodeId,
    kind: &'static str,
    schedule: &'static str,
    source: String,
}
impl ScriptOrigin {
    pub(super) fn capture(
        index: usize,
        node: NodeId,
        module: bool,
        deferred: bool,
        src: Option<&str>,
        base: Option<&url::Url>,
    ) -> Self {
        Self {
            index: index + 1,
            node,
            kind: if module { "module" } else { "classic" },
            schedule: if deferred { "defer" } else { "immediate" },
            source: src.map_or_else(|| "inline".into(), |src| source_label(src, base)),
        }
    }
    pub(super) fn warning(
        &self,
        phase: &'static str,
        error: &JsError,
        source: Option<&str>,
    ) -> String {
        let mut output = BoundedText::new(MAX_WARNING_BYTES);
        let stage = if phase == "preparation" {
            "preparation"
        } else {
            "error"
        };
        let _ = write!(
            output,
            "JavaScript script {} {stage} [phase={phase}, kind={}, node={}, source={}, schedule={}]",
            self.index, self.kind, self.node, self.source, self.schedule
        );
        // Raw offsets from JSON/eval/link errors need not address this input.
        if let Some((offset, line, column)) = source.and_then(|source| {
            error.input_source_offset().and_then(|offset| {
                source_position(source, offset).map(|(line, column)| (offset, line, column))
            })
        }) {
            let _ = write!(output, " [byte={offset}, line={line}, column={column}]");
        }
        let _ = write!(output, ": {error}");
        output.finish()
    }
}

/// Zero-based UTF-8 byte offset; one-based line and Unicode-scalar column.
/// CRLF counts as one newline; CR, LF and JS line separators are supported.
/// Unknown/out-of-range/non-boundary offsets never produce a guessed position.
pub(super) fn source_position(source: &str, offset: usize) -> Option<(usize, usize)> {
    let prefix = source.get(..offset)?;
    let mut line = 1;
    let mut column = 1;
    let mut previous_cr = false;
    for character in prefix.chars() {
        match character {
            '\n' if previous_cr => {}
            '\n' | '\r' | '\u{2028}' | '\u{2029}' => {
                line += 1;
                column = 1;
            }
            _ => column += 1,
        }
        previous_cr = character == '\r';
    }
    Some((line, column))
}

pub(super) fn callback_warning(
    phase: &'static str,
    event: Option<&str>,
    error: &JsError,
) -> String {
    let mut output = BoundedText::new(MAX_WARNING_BYTES);
    if let Some(event) = event {
        let _ = write!(output, "JavaScript {} callback", bounded_text(event, 64));
    } else {
        let _ = output.write_str("JavaScript timer");
    }
    let _ = write!(output, " [phase={phase}, source=unavailable]: {error}");
    output.finish()
}

pub(super) fn module_error(mut error: JsError, source_url: &str, source: Option<&str>) -> JsError {
    let label = source_label(source_url, None);
    error.add_diagnostic_context(&format!("module source={}", bounded_text(&label, 96)));
    if let Some((offset, line, column)) = error.input_source_offset().and_then(|offset| {
        source
            .and_then(|source| source_position(source, offset))
            .map(|(line, column)| (offset, line, column))
    }) {
        error.add_diagnostic_context(&format!("module byte={offset} line={line} column={column}"));
    }
    error
}

pub(super) fn unsupported_dom(
    document: &Document,
    node: NodeId,
    property: &str,
    value: Option<&Value>,
) -> JsError {
    let operation = if value.is_some() {
        "assignment"
    } else {
        "property"
    };
    let mut error = JsError::runtime(format!("Unsupported DOM {operation} {property}"));
    let (kind, tag) = match &document.nodes[node].kind {
        NodeKind::Document => ("document", None),
        NodeKind::Text(_) => ("text", None),
        NodeKind::Element(element) => ("element", Some(element.tag.as_str())),
    };
    let mut context = format!("DOM receiver node={node} kind={kind}");
    if let Some(tag) = tag {
        let _ = write!(context, " tag={}", bounded_text(tag, 32));
    }
    if let Some(value) = value {
        let value_type = match value {
            Value::Undefined => "undefined",
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::BigInt(_) => "bigint",
            Value::String(_) | Value::Utf16(_) => "string",
            Value::Symbol(..) => "symbol",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::Function(_) | Value::Native(_) => "function",
            Value::Host(_) => "host",
        };
        let _ = write!(context, " value-type={value_type}");
    }
    error.add_diagnostic_context(&context);
    error
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
