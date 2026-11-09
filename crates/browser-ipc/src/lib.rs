//! Versioned, bounded data-only protocol between the privileged shell and renderer.
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

mod bounded;
mod dynamic_scripts;
mod encoding;
pub use dynamic_scripts::*;

pub const PROTOCOL_VERSION: u32 = 9;
/// JavaScript's largest safe integer, in milliseconds. Frame
/// timestamps are document-relative host pulses, not the logical timer clock.
pub const MAX_ANIMATION_FRAME_TIMESTAMP_MS: f64 = 9_007_199_254_740_991.;

/// Reject non-finite, negative, oversized or decreasing pulses. Equality is
/// allowed; callers store the accepted value before invoking page callbacks.
pub fn validate_animation_frame_timestamp(
    timestamp_ms: f64,
    previous: Option<f64>,
) -> Result<(), String> {
    if !timestamp_ms.is_finite()
        || !(0.0..=MAX_ANIMATION_FRAME_TIMESTAMP_MS).contains(&timestamp_ms)
        || previous.is_some_and(|last| timestamp_ms < last)
    {
        return Err("Invalid or regressing animation frame timestamp".into());
    }
    Ok(())
}
// JSON byte arrays expand to at most four bytes per input byte; reserve room
// for the 8 MiB image budget, escaped HTML and the versioned envelope.
pub const MAX_MESSAGE_BYTES: usize = 48 * 1024 * 1024;
// Collection quotas apply during deserialization as well as semantic validation.
pub const MAX_DISPLAY_ITEMS: usize = 100_000;
pub const MAX_PAGE_LINKS: usize = 100_000;
pub const MAX_PAGE_WARNINGS: usize = 128;
pub const MAX_PAGE_CONTROLS: usize = 256;
pub const MAX_CONTROL_OPTIONS: usize = 256;
pub const MAX_CLIP_DEPTH: usize = 128;
pub const MAX_HTML_BYTES: usize = 1024 * 1024;
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_IMAGE_TOTAL_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_ID_TOTAL_BYTES: usize = MAX_HTML_BYTES;
pub const MAX_STYLESHEETS: usize = 8;
pub const MAX_STYLESHEET_BYTES: usize = 128 * 1024;
pub const MAX_STYLESHEET_TOTAL_BYTES: usize = 256 * 1024;

pub const SESSION_STORAGE_MAX_ENTRIES: usize = 128;
pub const SESSION_STORAGE_MAX_KEY_BYTES: usize = 1024;
pub const SESSION_STORAGE_MAX_VALUE_BYTES: usize = 16 * 1024;
pub const SESSION_STORAGE_MAX_TOTAL_BYTES: usize = 64 * 1024;
pub const SESSION_STORAGE_MAX_ORIGINS: usize = 16;
pub type SessionStorageSnapshot = Vec<(String, String)>;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionStorageAccess {
    #[default]
    Unavailable,
    ReadWrite,
    QuotaExceeded,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStorageSeed {
    pub access: SessionStorageAccess,
    #[serde(deserialize_with = "bounded::deserialize::<_, _, SESSION_STORAGE_MAX_ENTRIES>")]
    pub entries: SessionStorageSnapshot,
}
/// Validated after bounded frame deserialization, before retaining any snapshot.
/// The transport frame cap does not replace the smaller storage quotas.
pub fn validate_session_storage_entries(entries: &[(String, String)]) -> Result<(), String> {
    if entries.len() > SESSION_STORAGE_MAX_ENTRIES {
        return Err("Session storage entry quota exceeded".into());
    }
    let mut keys = std::collections::HashSet::new();
    let mut bytes = 0usize;
    for (key, value) in entries {
        if key.len() > SESSION_STORAGE_MAX_KEY_BYTES
            || value.len() > SESSION_STORAGE_MAX_VALUE_BYTES
            || !keys.insert(key)
        {
            return Err("Invalid session storage entry".into());
        }
        bytes = bytes
            .checked_add(key.len())
            .and_then(|n| n.checked_add(value.len()))
            .ok_or("Session storage size overflow")?;
        if bytes > SESSION_STORAGE_MAX_TOTAL_BYTES {
            return Err("Session storage byte quota exceeded".into());
        }
    }
    Ok(())
}
pub fn validate_session_storage_seed(seed: &SessionStorageSeed) -> Result<(), String> {
    validate_session_storage_entries(&seed.entries)?;
    if seed.access != SessionStorageAccess::ReadWrite && !seed.entries.is_empty() {
        return Err("Unavailable session storage cannot contain entries".into());
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

impl Default for Color {
    fn default() -> Self {
        Self(25, 28, 35, 255)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAsset {
    /// Exact src attribute. The renderer never opens this as a path or URL.
    pub id: String,
    /// Physical dimensions of the bounded RGBA texture, never CSS layout size.
    pub width: u32,
    pub height: u32,
    /// Intrinsic CSS pixel dimensions at density 1, before texture reduction.
    pub natural_width: f32,
    pub natural_height: f32,
    pub rgba: Vec<u8>,
}

/// Validate metadata independently of texture allocation and byte budgets.
pub fn valid_natural_image_dimensions(width: f32, height: f32) -> bool {
    width.is_finite()
        && height.is_finite()
        && width > 0.
        && height > 0.
        && width <= 4096.
        && height <= 4096.
        && (width.ceil() as u64)
            .checked_mul(height.ceil() as u64)
            .is_some_and(|pixels| pixels <= 4 * 1024 * 1024)
}

pub fn valid_image_id(id: &str) -> bool {
    !id.is_empty()
        && (id.len() <= 8192
            || (id.len() <= MAX_IMAGE_ID_TOTAL_BYTES
                && id
                    .get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))))
}

pub const MAX_AUDIO_METADATA: usize = 8;
pub const MAX_AUDIO_SRC_BYTES: usize = 8192;
pub const MAX_AUDIO_SRC_TOTAL_BYTES: usize = 64 * 1024;
pub const MAX_AUDIO_PCM_BYTES: u64 = 4 * 1024 * 1024;

/// PCM facts only; no samples or floating-point duration cross IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioMetadata {
    pub src: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub block_align: u16,
    pub frames: u64,
}

pub fn validate_audio_metadata(entries: &[AudioMetadata]) -> Result<(), String> {
    if entries.len() > MAX_AUDIO_METADATA {
        return Err("Audio metadata count exceeded".into());
    }
    let mut sources = std::collections::HashSet::new();
    let mut bytes = 0usize;
    for entry in entries {
        bytes = bytes
            .checked_add(entry.src.len())
            .ok_or("Audio source size overflow")?;
        if entry.src.is_empty()
            || entry.src.len() > MAX_AUDIO_SRC_BYTES
            || bytes > MAX_AUDIO_SRC_TOTAL_BYTES
            || !sources.insert(&entry.src)
            || !matches!(entry.channels, 1 | 2)
            || !matches!(entry.bits_per_sample, 8 | 16 | 24 | 32)
            || !(8000..=192000).contains(&entry.sample_rate)
            || entry.channels.checked_mul(entry.bits_per_sample / 8) != Some(entry.block_align)
            || !entry
                .frames
                .checked_mul(u64::from(entry.block_align))
                .is_some_and(|bytes| bytes <= MAX_AUDIO_PCM_BYTES)
        {
            return Err("Invalid audio metadata".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod audio_tests {
    use super::*;
    fn valid() -> AudioMetadata {
        AudioMetadata {
            src: "a.wav".into(),
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
            block_align: 2,
            frames: 12000,
        }
    }
    #[test]
    fn rejects_invalid_pcm_sizes_sources_and_counts() {
        assert!(validate_audio_metadata(&[valid()]).is_ok());
        assert!(validate_audio_metadata(&vec![valid(); 9]).is_err());
        assert!(validate_audio_metadata(&[valid(), valid()]).is_err());
        for src in [String::new(), "a".repeat(MAX_AUDIO_SRC_BYTES + 1)] {
            let mut a = valid();
            a.src = src;
            assert!(validate_audio_metadata(&[a]).is_err());
        }
        for frames in [u64::MAX, MAX_AUDIO_PCM_BYTES] {
            let mut a = valid();
            a.frames = frames;
            assert!(validate_audio_metadata(&[a]).is_err());
        }
        let mut a = valid();
        a.block_align = 1;
        assert!(validate_audio_metadata(&[a]).is_err());
        let mut a = valid();
        a.channels = 3;
        assert!(validate_audio_metadata(&[a]).is_err());
        let mut a = valid();
        a.sample_rate = 0;
        assert!(validate_audio_metadata(&[a]).is_err());
        let mut a = valid();
        a.bits_per_sample = 64;
        assert!(validate_audio_metadata(&[a]).is_err());
    }
    #[test]
    fn missing_audio_field_is_not_silently_accepted() {
        assert!(
            serde_json::from_str::<DocumentInput>(
                r#"{"url":"file:///a.html","html":"","images":[]}"#
            )
            .is_err()
        );
        assert_eq!(PROTOCOL_VERSION, 9);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentInput {
    pub url: String,
    pub html: String,
    pub images: Vec<ImageAsset>,
    pub audio_metadata: Vec<AudioMetadata>,
    /// Public, credential-free sources authorized by the broker; never opened by the VM.
    #[serde(default)]
    pub scripts: Vec<ScriptAsset>,
    /// Authorized stylesheet bytes, matched against link hrefs in DOM order.
    #[serde(default)]
    pub stylesheets: Vec<StylesheetAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StylesheetAsset {
    pub href: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptAsset {
    /// Classic sources use their HTML src key. Authorized module sources use
    /// `noir-module:` followed by the canonical URL. The tag is data, not a
    /// fetchable scheme; renderers must never interchange these asset kinds.
    pub src: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1024.0,
            height: 768.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DisplayItem {
    /// Intersect the current paint clip until the matching PopClip. Maximum
    /// nesting and balanced pairs are enforced by every consuming boundary.
    PushClip {
        rect: Rect,
    },
    PopClip,
    Control {
        rect: Rect,
        id: u64,
        kind: ControlKind,
        name: String,
        value: String,
        placeholder: String,
        form: Option<FormInfo>,
        disabled: bool,
        #[serde(default)]
        appearance: ControlAppearance,
        #[serde(
            default,
            deserialize_with = "bounded::deserialize::<_, _, MAX_CONTROL_OPTIONS>"
        )]
        options: Vec<ControlOption>,
    },
    Rect {
        rect: Rect,
        color: Color,
    },
    Text {
        rect: Rect,
        text: String,
        size: f32,
        color: Color,
        bold: bool,
    },
    Image {
        rect: Rect,
        asset_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ControlKind {
    Text,
    Password,
    Hidden,
    Submit,
    Button,
    Checkbox,
    TextArea,
    Select,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ControlAppearance {
    pub background: Color,
    pub border_color: Color,
    pub border_width: f32,
    pub color: Color,
    pub font_size: f32,
}
impl Default for ControlAppearance {
    fn default() -> Self {
        Self {
            background: Color(246, 248, 250, 255),
            border_color: Color(160, 170, 180, 255),
            border_width: 1.,
            color: Color(35, 45, 55, 255),
            font_size: 16.,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlOption {
    pub value: String,
    pub label: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormInfo {
    pub id: u64,
    pub action: String,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Link {
    pub rect: Rect,
    pub href: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenderMetrics {
    pub parse_us: u64,
    pub style_us: u64,
    pub layout_us: u64,
    pub node_count: usize,
    pub worker_pid: u32,
    pub resident_memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedPage {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub background: Color,
    #[serde(deserialize_with = "bounded::display_items")]
    pub items: Vec<DisplayItem>,
    #[serde(deserialize_with = "bounded::deserialize::<_, _, MAX_PAGE_LINKS>")]
    pub links: Vec<Link>,
    pub metrics: RenderMetrics,
    #[serde(deserialize_with = "bounded::deserialize::<_, _, MAX_PAGE_WARNINGS>")]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub next_timer_ms: Option<u64>,
    /// A pending callback requires an explicit host pulse, not an implicit Tick.
    pub animation_frame_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderRequest {
    pub session_storage: SessionStorageSeed,
    pub protocol_version: u32,
    pub id: u64,
    pub document: DocumentInput,
    pub viewport: Viewport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderResponse {
    #[serde(deserialize_with = "bounded::deserialize::<_, _, MAX_DYNAMIC_SCRIPTS>")]
    pub pending_scripts: Vec<DynamicScriptRequest>,
    #[serde(deserialize_with = "bounded::deserialize::<_, _, SESSION_STORAGE_MAX_ENTRIES>")]
    pub session_storage: SessionStorageSnapshot,
    pub protocol_version: u32,
    pub id: u64,
    pub result: Result<RenderedPage, String>,
}

/// Persistent renderer protocol. IDs are explicit so a late/stale response can
/// never be committed as the result of another operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionRequest {
    Begin(RenderRequest),
    Scripts {
        id: u64,
        #[serde(deserialize_with = "bounded::deserialize::<_, _, MAX_DYNAMIC_SCRIPTS>")]
        scripts: Vec<DynamicScriptResponse>,
        viewport: Viewport,
    },
    AnimationFrame {
        id: u64,
        timestamp_ms: f64,
        viewport: Viewport,
    },
    Reflow {
        id: u64,
        viewport: Viewport,
    },
    Event {
        id: u64,
        node: u64,
        event: String,
        viewport: Viewport,
        #[serde(default)]
        inputs: Vec<(u64, String)>,
    },
    Tick {
        id: u64,
        elapsed_ms: u64,
        viewport: Viewport,
    },
}

pub fn write_message<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    encoding::write_message_with_limit(writer, value, MAX_MESSAGE_BYTES)
}

pub fn read_message<R: Read, T: for<'de> Deserialize<'de>>(reader: &mut R) -> io::Result<T> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid IPC frame length",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_frame_timestamp_bounds_and_request_round_trip() {
        for value in [0., 1.25, MAX_ANIMATION_FRAME_TIMESTAMP_MS] {
            assert!(validate_animation_frame_timestamp(value, None).is_ok());
            assert!(validate_animation_frame_timestamp(value, Some(value)).is_ok());
            let request = SessionRequest::AnimationFrame {
                id: 42,
                timestamp_ms: value,
                viewport: Viewport::default(),
            };
            let mut bytes = Vec::new();
            write_message(&mut bytes, &request).unwrap();
            let decoded: SessionRequest = read_message(&mut bytes.as_slice()).unwrap();
            assert!(
                matches!(decoded, SessionRequest::AnimationFrame { id: 42, timestamp_ms, .. } if timestamp_ms == value),
                "timestamp {value:?} changed over IPC: {decoded:?}; JSON: {}",
                String::from_utf8_lossy(&bytes[4..])
            );
        }
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.,
            MAX_ANIMATION_FRAME_TIMESTAMP_MS + 1.,
        ] {
            assert!(validate_animation_frame_timestamp(value, None).is_err());
        }
        assert!(validate_animation_frame_timestamp(1., Some(2.)).is_err());
        assert!(validate_animation_frame_timestamp(2., Some(1.)).is_ok());
    }

    #[test]
    fn natural_image_metadata_has_independent_bounded_dimensions() {
        for (width, height) in [(2048., 2048.), (4096., 1024.), (0.25, 1.5)] {
            assert!(valid_natural_image_dimensions(width, height));
        }
        for invalid in [0., -1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 4096.5] {
            assert!(!valid_natural_image_dimensions(invalid, 1.));
            assert!(!valid_natural_image_dimensions(1., invalid));
        }
        assert!(!valid_natural_image_dimensions(4096., 1024.25));
        assert!(!valid_natural_image_dimensions(2048.25, 2048.));
    }

    #[test]
    fn image_metadata_is_required_and_round_trips_fractional_values() {
        let old = r#"{"id":"a","width":1,"height":1,"rgba":[0,0,0,255]}"#;
        assert!(serde_json::from_str::<ImageAsset>(old).is_err());
        let image = ImageAsset {
            id: "a".into(),
            width: 1,
            height: 1,
            natural_width: 20.25,
            natural_height: 10.5,
            rgba: vec![0, 0, 0, 255],
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &image).unwrap();
        let result: ImageAsset = read_message(&mut bytes.as_slice()).unwrap();
        assert_eq!((result.natural_width, result.natural_height), (20.25, 10.5));
        assert_eq!((result.width, result.height), (1, 1));
        assert_eq!(result.rgba.len(), 4);
    }

    #[test]
    fn storage_quota_boundaries_are_utf8_bounded_and_unique() {
        assert!(
            validate_session_storage_entries(&[(
                "k".repeat(SESSION_STORAGE_MAX_KEY_BYTES),
                "v".repeat(SESSION_STORAGE_MAX_VALUE_BYTES)
            )])
            .is_ok()
        );
        for entries in [
            vec![("k".repeat(SESSION_STORAGE_MAX_KEY_BYTES + 1), String::new())],
            vec![(
                String::new(),
                "v".repeat(SESSION_STORAGE_MAX_VALUE_BYTES + 1),
            )],
            vec![("same".into(), "a".into()), ("same".into(), "b".into())],
            (0..=SESSION_STORAGE_MAX_ENTRIES)
                .map(|i| (i.to_string(), String::new()))
                .collect(),
            (0..4)
                .map(|i| (i.to_string(), "v".repeat(SESSION_STORAGE_MAX_VALUE_BYTES)))
                .collect(),
            vec![(
                "é".repeat(SESSION_STORAGE_MAX_KEY_BYTES / 2 + 1),
                String::new(),
            )],
        ] {
            assert!(validate_session_storage_entries(&entries).is_err());
        }
        let exact: Vec<_> = (0..4)
            .map(|i| {
                (
                    i.to_string(),
                    "v".repeat(SESSION_STORAGE_MAX_VALUE_BYTES - 1),
                )
            })
            .collect();
        assert!(validate_session_storage_entries(&exact).is_ok());
        for access in [
            SessionStorageAccess::Unavailable,
            SessionStorageAccess::QuotaExceeded,
        ] {
            assert!(
                validate_session_storage_seed(&SessionStorageSeed {
                    access,
                    entries: vec![("k".into(), "v".into())]
                })
                .is_err()
            );
        }
    }
    #[test]
    fn bounded_frames_and_truncation() {
        assert!(read_message::<_, String>(&mut &[255, 255, 255, 255][..]).is_err());
        assert!(read_message::<_, String>(&mut &[5, 0, 0, 0, b'"'][..]).is_err());
        let mut bytes = Vec::new();
        write_message(&mut bytes, &"Olá, Noir").unwrap();
        assert_eq!(
            read_message::<_, String>(&mut bytes.as_slice()).unwrap(),
            "Olá, Noir"
        );
    }
}

#[cfg(test)]
mod collection_tests {
    use super::*;

    fn page() -> RenderedPage {
        RenderedPage {
            title: "bounded".into(),
            width: 800.,
            height: 600.,
            background: Color::default(),
            items: vec![],
            links: vec![],
            metrics: RenderMetrics::default(),
            warnings: vec![],
            next_timer_ms: None,
            animation_frame_pending: false,
        }
    }
    #[test]
    fn pending_frame_field_is_required_and_round_trips() {
        let mut p = page();
        p.animation_frame_pending = true;
        let mut json = serde_json::to_value(&p).unwrap();
        assert!(
            serde_json::from_value::<RenderedPage>(json.clone())
                .unwrap()
                .animation_frame_pending
        );
        json.as_object_mut()
            .unwrap()
            .remove("animation_frame_pending");
        assert!(serde_json::from_value::<RenderedPage>(json).is_err());
    }
    fn control() -> DisplayItem {
        DisplayItem::Control {
            rect: Rect::default(),
            id: 1,
            kind: ControlKind::Select,
            name: String::new(),
            value: String::new(),
            placeholder: String::new(),
            form: None,
            disabled: false,
            appearance: ControlAppearance::default(),
            options: vec![],
        }
    }
    fn frame<T: Serialize>(value: &T) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_message(&mut bytes, value).unwrap();
        bytes
    }
    fn rejects_page(page: RenderedPage) {
        let bytes = frame(&RenderResponse {
            pending_scripts: vec![],
            protocol_version: PROTOCOL_VERSION,
            id: 1,
            session_storage: vec![],
            result: Ok(page),
        });
        assert!(bytes.len() < MAX_MESSAGE_BYTES);
        let error = read_message::<_, RenderResponse>(&mut bytes.as_slice()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("collection limit"));
    }
    #[test]
    fn each_page_collection_is_bounded_during_real_frame_read() {
        let mut p = page();
        p.items = vec![DisplayItem::PopClip; MAX_DISPLAY_ITEMS + 1];
        rejects_page(p); // No semantic validator has run at this point.
        let mut p = page();
        p.links = vec![
            Link {
                rect: Rect::default(),
                href: String::new()
            };
            MAX_PAGE_LINKS + 1
        ];
        rejects_page(p);
        let mut p = page();
        p.warnings = vec![String::new(); MAX_PAGE_WARNINGS + 1];
        rejects_page(p);
        let mut item = control();
        if let DisplayItem::Control { options, .. } = &mut item {
            *options = vec![
                ControlOption {
                    value: String::new(),
                    label: String::new(),
                    disabled: false
                };
                MAX_CONTROL_OPTIONS + 1
            ];
        }
        let mut p = page();
        p.items.push(item);
        rejects_page(p);
    }
    #[test]
    fn storage_limit_also_applies_to_error_responses_and_seeds() {
        let entries: Vec<_> = (0..=SESSION_STORAGE_MAX_ENTRIES)
            .map(|n| (n.to_string(), String::new()))
            .collect();
        let bytes = frame(&RenderResponse {
            pending_scripts: vec![],
            protocol_version: PROTOCOL_VERSION,
            id: 1,
            session_storage: entries.clone(),
            result: Err("worker error".into()),
        });
        assert!(
            read_message::<_, RenderResponse>(&mut bytes.as_slice())
                .unwrap_err()
                .to_string()
                .contains("collection limit")
        );
        let bytes = frame(&SessionStorageSeed {
            access: SessionStorageAccess::ReadWrite,
            entries,
        });
        assert!(read_message::<_, SessionStorageSeed>(&mut bytes.as_slice()).is_err());
    }
    #[test]
    fn control_default_is_unchanged_and_total_controls_are_bounded() {
        let mut json = serde_json::to_value(control()).unwrap();
        json["Control"].as_object_mut().unwrap().remove("options");
        assert!(
            matches!(serde_json::from_value::<DisplayItem>(json).unwrap(), DisplayItem::Control { options, .. } if options.is_empty())
        );
        let mut p = page();
        p.items = vec![control(); MAX_PAGE_CONTROLS + 1];
        let bytes = frame(&p);
        assert!(
            read_message::<_, RenderedPage>(&mut bytes.as_slice())
                .unwrap_err()
                .to_string()
                .contains("control count")
        );
    }
    #[test]
    fn valid_collections_round_trip_at_their_limits() {
        let mut p = page();
        p.items = vec![
            DisplayItem::Rect {
                rect: Rect::default(),
                color: Color::default()
            };
            MAX_DISPLAY_ITEMS
        ];
        p.links = vec![
            Link {
                rect: Rect::default(),
                href: String::new()
            };
            MAX_PAGE_LINKS
        ];
        p.warnings = vec![String::new(); MAX_PAGE_WARNINGS];
        let mut item = control();
        if let DisplayItem::Control { options, .. } = &mut item {
            *options = vec![
                ControlOption {
                    value: String::new(),
                    label: String::new(),
                    disabled: false
                };
                MAX_CONTROL_OPTIONS
            ];
        }
        p.items[0] = item;
        let response = RenderResponse {
            pending_scripts: vec![],
            protocol_version: PROTOCOL_VERSION,
            id: 42,
            session_storage: (0..SESSION_STORAGE_MAX_ENTRIES)
                .map(|n| (n.to_string(), String::new()))
                .collect(),
            result: Ok(p),
        };
        let bytes = frame(&response);
        let decoded: RenderResponse = read_message(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded.protocol_version, 9);
        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.session_storage.len(), SESSION_STORAGE_MAX_ENTRIES);
        let p = decoded.result.unwrap();
        assert_eq!(p.items.len(), MAX_DISPLAY_ITEMS);
        assert_eq!(p.links.len(), MAX_PAGE_LINKS);
        assert_eq!(p.warnings.len(), MAX_PAGE_WARNINGS);
    }
}
