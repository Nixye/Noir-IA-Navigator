//! Character-bounded copies and a byte-capped model observation boundary.
use crate::{BrowserElement, BrowserSnapshot, BrowserTab};
use std::{fmt, io};

/// Local extraction limits are Unicode scalar counts, not UTF-8 byte counts.
pub mod snapshot_limits {
    pub const TITLE_CHARS: usize = 160;
    pub const URL_CHARS: usize = 2048;
    pub const TEXT_CHARS: usize = 4000;
    pub const SELECTION_CHARS: usize = 1000;
    pub const TABS: usize = 24;
    pub const TAB_TITLE_CHARS: usize = 120;
    pub const ELEMENTS: usize = 64;
    pub const ELEMENT_ID_CHARS: usize = 48;
    pub const ELEMENT_KIND_CHARS: usize = 32;
    pub const ELEMENT_LABEL_CHARS: usize = 160;
    pub const ELEMENT_VALUE_CHARS: usize = 160;
    pub const CONTROLS: usize = 40;
    pub const MODEL_TEXT_CHARS: usize = 2000;
    pub const MODEL_TABS: usize = 8;
    pub const MODEL_ELEMENTS: usize = 24;
    pub const MODEL_TAB_URL_CHARS: usize = 160;
    pub const MODEL_TARGET_BYTES: usize = 7000;
    pub const MODEL_MAX_BYTES: usize = 10000;
}
use snapshot_limits::*;

/// Copies only the permitted prefix, without first cloning the whole source.
pub fn bounded_text(text: &str, max_chars: usize) -> String {
    let mut output = BoundedText::new(max_chars);
    output.push_str(text);
    output.finish()
}

/// Incremental scalar-counted builder for text, labels and option lists.
/// Callers can stop visiting source fragments once `is_full()` is true.
pub struct BoundedText {
    text: String,
    remaining: usize,
}

impl BoundedText {
    pub fn new(max_chars: usize) -> Self {
        Self {
            text: String::new(),
            remaining: max_chars,
        }
    }

    pub fn push_str(&mut self, text: &str) {
        for ch in text.chars().take(self.remaining) {
            self.text.push(ch);
            self.remaining -= 1;
        }
    }

    pub fn is_full(&self) -> bool {
        self.remaining == 0
    }

    pub fn finish(self) -> String {
        self.text
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationError {
    TooLarge { max_bytes: usize },
    Serialization,
}

impl fmt::Display for ObservationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { .. } => {
                f.write_str("A observação da página excede o limite do assistente")
            }
            Self::Serialization => f.write_str("Observação de navegador inválida"),
        }
    }
}
impl std::error::Error for ObservationError {}

impl BrowserSnapshot {
    /// Compatibility API for owned snapshots. This does not limit source memory.
    pub fn bounded(mut self) -> Self {
        self.title = bounded_text(&self.title, TITLE_CHARS);
        self.url = bounded_text(&self.url, URL_CHARS);
        self.text = bounded_text(&self.text, TEXT_CHARS);
        self.selection = bounded_text(&self.selection, SELECTION_CHARS);
        self.tabs.truncate(TABS);
        for tab in &mut self.tabs {
            tab.title = bounded_text(&tab.title, TAB_TITLE_CHARS);
            tab.url = bounded_text(&tab.url, URL_CHARS);
        }
        self.elements.truncate(ELEMENTS);
        for element in &mut self.elements {
            element.id = bounded_text(&element.id, ELEMENT_ID_CHARS);
            element.kind = bounded_text(&element.kind, ELEMENT_KIND_CHARS);
            element.label = bounded_text(&element.label, ELEMENT_LABEL_CHARS);
            element.value = bounded_text(&element.value, ELEMENT_VALUE_CHARS);
        }
        self
    }

    /// Builds permitted prefixes directly from borrowed input, without cloning
    /// unbounded strings/collections. Denied content is not copied or serialized.
    /// The absolute UTF-8 byte cap covers this JSON only, including escaping;
    /// prompt prefixes/history and the original snapshot are separate budgets.
    pub fn model_observation(&self, allowed: [bool; 6]) -> Result<String, ObservationError> {
        let page = allowed[0];
        let copy_page = |value: &str, limit| {
            if page {
                bounded_text(value, limit)
            } else {
                String::new()
            }
        };
        let mut snapshot = Self {
            scope: self.scope,
            title: copy_page(&self.title, TITLE_CHARS),
            url: copy_page(&self.url, URL_CHARS),
            text: copy_page(&self.text, MODEL_TEXT_CHARS),
            selection: copy_page(&self.selection, SELECTION_CHARS),
            tabs: if allowed[2] {
                self.tabs
                    .iter()
                    .take(MODEL_TABS)
                    .map(|tab| BrowserTab {
                        id: tab.id,
                        title: bounded_text(&tab.title, TAB_TITLE_CHARS),
                        url: bounded_text(&tab.url, MODEL_TAB_URL_CHARS),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            elements: if page {
                self.elements
                    .iter()
                    .take(MODEL_ELEMENTS)
                    .map(|element| BrowserElement {
                        id: bounded_text(&element.id, ELEMENT_ID_CHARS),
                        kind: bounded_text(&element.kind, ELEMENT_KIND_CHARS),
                        label: bounded_text(&element.label, ELEMENT_LABEL_CHARS),
                        value: bounded_text(&element.value, ELEMENT_VALUE_CHARS),
                    })
                    .collect()
            } else {
                Vec::new()
            },
        };
        loop {
            let encoded = serialize_capped(&snapshot);
            match &encoded {
                Ok(json) if json.len() <= MODEL_TARGET_BYTES => return encoded,
                Err(ObservationError::Serialization) => return encoded,
                _ => {}
            }
            // Preserve the legacy reduction order: pop elements, then halve
            // text by scalar count while its UTF-8 length exceeds 256 bytes.
            // A capped attempt is necessarily above the 7000-byte target too.
            if snapshot.elements.pop().is_some() {
                continue;
            }
            if snapshot.text.len() > 256 {
                snapshot.text = bounded_text(&snapshot.text, snapshot.text.chars().count() / 2);
                continue;
            }
            return encoded;
        }
    }
}

/// Private to this concrete, bounded data type: not a generic serialization or
/// CPU/memory quota. No partial JSON escapes on an error.
fn serialize_capped(snapshot: &BrowserSnapshot) -> Result<String, ObservationError> {
    let mut writer = CappedWriter {
        bytes: Vec::with_capacity(MODEL_MAX_BYTES),
        exceeded: false,
    };
    if serde_json::to_writer(&mut writer, snapshot).is_err() {
        return Err(if writer.exceeded {
            ObservationError::TooLarge {
                max_bytes: MODEL_MAX_BYTES,
            }
        } else {
            ObservationError::Serialization
        });
    }
    String::from_utf8(writer.bytes).map_err(|_| ObservationError::Serialization)
}

struct CappedWriter {
    bytes: Vec<u8>,
    exceeded: bool,
}

impl io::Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MODEL_MAX_BYTES - self.bytes.len() {
            self.exceeded = true;
            return Err(io::Error::other("observation byte limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentScope;

    fn empty_snapshot() -> BrowserSnapshot {
        BrowserSnapshot {
            scope: DocumentScope {
                tab: 7,
                generation: 91,
            },
            title: String::new(),
            url: String::new(),
            text: String::new(),
            selection: String::new(),
            tabs: Vec::new(),
            elements: Vec::new(),
        }
    }

    fn decode(snapshot: &BrowserSnapshot, allowed: [bool; 6]) -> BrowserSnapshot {
        let json = snapshot.model_observation(allowed).unwrap();
        assert!(json.len() <= MODEL_MAX_BYTES);
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn incremental_copy_counts_scalars_and_stops_at_limit() {
        let mut text = BoundedText::new(3);
        text.push_str("é🦀");
        text.push_str("中tail");
        text.push_str("ignored");
        assert!(text.is_full());
        assert_eq!(text.finish(), "é🦀中");
        assert_eq!(bounded_text("🦀".repeat(100_000).as_str(), 2), "🦀🦀");
        assert_eq!(bounded_text("anything", 0), "");
    }

    #[test]
    fn huge_fields_are_copied_as_prefixes_and_source_is_unchanged() {
        let mut source = empty_snapshot();
        source.title = "é".repeat(100_000);
        source.url = "u".repeat(100_000);
        source.text = "t".repeat(100_000);
        source.selection = "s".repeat(100_000);
        let observed = decode(&source, [true; 6]);
        assert_eq!(observed.title, "é".repeat(TITLE_CHARS));
        assert_eq!(observed.url.len(), URL_CHARS);
        assert_eq!(observed.text.len(), MODEL_TEXT_CHARS);
        assert_eq!(observed.selection.len(), SELECTION_CHARS);
        assert_eq!(observed.scope, source.scope);
        assert_eq!(source.text.len(), 100_000);
    }

    #[test]
    fn permission_filters_are_independent_and_preserve_scope() {
        let mut source = empty_snapshot();
        source.title = "private".into();
        source.url = "private".into();
        source.text = "private".into();
        source.selection = "private".into();
        source.tabs.push(BrowserTab {
            id: 2,
            title: "tab".into(),
            url: "url".into(),
        });
        source.elements.push(BrowserElement {
            id: "id".into(),
            kind: "input".into(),
            label: "private".into(),
            value: "private".into(),
        });
        let denied = decode(&source, [false; 6]);
        assert_eq!(denied.scope, source.scope);
        assert!(denied.title.is_empty() && denied.url.is_empty() && denied.text.is_empty());
        assert!(
            denied.selection.is_empty() && denied.elements.is_empty() && denied.tabs.is_empty()
        );
        let tabs_only = decode(&source, [false, false, true, false, false, false]);
        assert_eq!(tabs_only.tabs.len(), 1);
        assert!(tabs_only.text.is_empty() && tabs_only.elements.is_empty());
        let page_only = decode(&source, [true, false, false, false, false, false]);
        assert_eq!(page_only.text, "private");
        assert_eq!(page_only.elements.len(), 1);
        assert!(page_only.tabs.is_empty());
    }

    #[test]
    fn model_caps_tab_count_and_each_tab_prefix() {
        let mut source = empty_snapshot();
        source.tabs = (0..100)
            .map(|id| BrowserTab {
                id,
                title: "a".repeat(50_000),
                url: "b".repeat(50_000),
            })
            .collect();
        let observed = decode(&source, [true; 6]);
        assert_eq!(observed.tabs.len(), MODEL_TABS);
        assert_eq!(observed.tabs[7].id, 7);
        assert!(
            observed
                .tabs
                .iter()
                .all(|tab| tab.title.len() == TAB_TITLE_CHARS
                    && tab.url.len() == MODEL_TAB_URL_CHARS)
        );
    }

    #[test]
    fn escaping_counts_toward_cap_and_failure_is_structured() {
        let mut source = empty_snapshot();
        source.url = "\0".repeat(URL_CHARS);
        // 2048 chars would fit as raw bytes, but JSON escapes exceed 10000.
        assert_eq!(
            source.model_observation([true; 6]),
            Err(ObservationError::TooLarge {
                max_bytes: MODEL_MAX_BYTES
            })
        );
        // Denied fields are not serialized, including otherwise oversized JSON.
        assert!(source.model_observation([false; 6]).is_ok());
        let mut writer = CappedWriter {
            bytes: Vec::with_capacity(MODEL_MAX_BYTES),
            exceeded: false,
        };
        io::Write::write_all(&mut writer, &vec![b'x'; MODEL_MAX_BYTES]).unwrap();
        assert!(io::Write::write_all(&mut writer, b"x").is_err());
        assert_eq!(writer.bytes.len(), MODEL_MAX_BYTES);
    }

    #[test]
    fn reductions_match_legacy_order_on_bounded_data() {
        let mut source = empty_snapshot();
        source.text = "\n".repeat(MODEL_TEXT_CHARS);
        source.selection = "s".repeat(SELECTION_CHARS);
        source.url = "u".repeat(URL_CHARS);
        source.elements = (0..40)
            .map(|_| BrowserElement {
                id: "i".repeat(100),
                kind: "k".repeat(100),
                label: "l".repeat(500),
                value: "v".repeat(500),
            })
            .collect();
        let observed = decode(&source, [true; 6]);
        assert!(observed.elements.is_empty());
        assert_eq!(observed.text.chars().count(), 1000);
        assert!(source.elements.len() > MODEL_ELEMENTS);
        assert_eq!(source.text.chars().count(), MODEL_TEXT_CHARS);
    }

    #[test]
    fn permitted_elements_and_escaped_text_round_trip() {
        let mut source = empty_snapshot();
        source.text = "\"quoted\"\nslash\\and\tcontrol\0".into();
        source.elements = (0..100)
            .map(|_| BrowserElement {
                id: "i".repeat(100_000),
                kind: String::new(),
                label: String::new(),
                value: String::new(),
            })
            .collect();
        let observed = decode(&source, [true; 6]);
        assert_eq!(observed.elements.len(), MODEL_ELEMENTS);
        assert!(
            observed
                .elements
                .iter()
                .all(|element| element.id.len() == ELEMENT_ID_CHARS)
        );
        assert_eq!(observed.text, source.text);
    }
    #[test]
    fn target_is_soft_and_multibyte_json_remains_valid() {
        let mut source = empty_snapshot();
        source.url = "🦀".repeat(1900);
        source.text = "🦀".repeat(MODEL_TEXT_CHARS);
        let json = source.model_observation([true; 6]).unwrap();
        assert!(json.len() > MODEL_TARGET_BYTES && json.len() <= MODEL_MAX_BYTES);
        let observed: BrowserSnapshot = serde_json::from_str(&json).unwrap();
        assert!(observed.text.len() <= 256);
        assert_eq!(observed.url, source.url);
        assert_eq!(observed.scope, source.scope);
    }
}
