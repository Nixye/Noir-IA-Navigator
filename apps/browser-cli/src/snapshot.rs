//! Diagnostic files are untrusted data, not authority to fetch or open assets.
//! Lengths, collection counts and aggregate budgets are checked before allocation.
use browser_ipc::{
    AudioMetadata, DocumentInput, ImageAsset, ScriptAsset, StylesheetAsset, Viewport,
};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

const MAGIC: &[u8; 8] = b"NOIRSNAP";
pub const FORMAT_VERSION: u32 = 1;
pub const MAX_FILE_BYTES: usize = 12 * 1024 * 1024;
const HEADER_BYTES: usize = 20;
const MAX_URL_BYTES: usize = 8192;
const MAX_IMAGES: usize = 32;
const MAX_SCRIPTS: usize = 4;
const MAX_SCRIPT_BYTES: usize = 128 * 1024;
const MAX_SCRIPT_TOTAL_BYTES: usize = 256 * 1024;
const MAX_VERSION_BYTES: usize = 128;
const MAX_BUILD_ID_BYTES: usize = 256;
const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILD_ID: &str = match option_env!("NOIR_BUILD_ID") {
    Some(value) => value,
    None => "development",
};

#[derive(Debug)]
pub struct Snapshot {
    pub document: DocumentInput,
    pub viewport: Viewport,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// The caller supplies only a fresh broker result, never tab/profile storage.
/// create_new is authoritative even if the destination changes after preflight.
pub fn save_new(path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let size = encoded_size(snapshot).map_err(|e| format!("Snapshot: {e}"))?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Snapshot destination (must not exist): {e}"))?;
    let mut writer = BufWriter::new(file);
    write_encoded(&mut writer, snapshot, size)
        .and_then(|()| writer.flush())
        .and_then(|()| writer.get_ref().sync_all())
        .map_err(|e| format!("Snapshot write failed; a partial file may remain: {e}"))
}

/// Does not resolve the stored URL or open any of its resource identifiers.
pub fn load(path: &Path) -> Result<Snapshot, String> {
    let file = File::open(path).map_err(|e| format!("Snapshot open: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.len() < HEADER_BYTES as u64
        || metadata.len() > MAX_FILE_BYTES as u64
    {
        return Err("Snapshot must be a regular file within the 12 MiB limit".into());
    }
    decode(&mut BufReader::new(file), Some(metadata.len())).map_err(|e| format!("Snapshot: {e}"))
}

fn validate(snapshot: &Snapshot) -> io::Result<()> {
    let document = &snapshot.document;
    if CLI_VERSION.len() > MAX_VERSION_BYTES || BUILD_ID.len() > MAX_BUILD_ID_BYTES {
        return Err(invalid("Build identity exceeds snapshot limit"));
    }
    if document.url.len() > MAX_URL_BYTES || document.url.chars().any(char::is_control) {
        return Err(invalid("Invalid document URL"));
    }
    let url = url::Url::parse(&document.url).map_err(|_| invalid("Invalid document URL"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("Snapshot URLs must not contain credentials"));
    }
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => {}
        "file" if url.host_str().is_none() && url.query().is_none() => {}
        "noir" if url.host_str().is_none() => {}
        _ => return Err(invalid("Unsupported document origin")),
    }
    let viewport = snapshot.viewport;
    if !viewport.width.is_finite()
        || !viewport.height.is_finite()
        || !(64.0..=8192.0).contains(&viewport.width)
        || !(64.0..=8192.0).contains(&viewport.height)
    {
        return Err(invalid(
            "Snapshot viewport must be between 64 and 8192 pixels",
        ));
    }
    if document.html.len() > browser_ipc::MAX_HTML_BYTES
        || document.images.len() > MAX_IMAGES
        || document.scripts.len() > MAX_SCRIPTS
        || document.stylesheets.len() > browser_ipc::MAX_STYLESHEETS
    {
        return Err(invalid("Document quota exceeded"));
    }
    browser_ipc::validate_audio_metadata(&document.audio_metadata).map_err(|e| invalid(&e))?;
    if !document.audio_metadata.is_empty() && url.scheme() != "file" {
        return Err(invalid("Audio metadata requires a file document"));
    }
    let mut ids = HashSet::new();
    let mut id_bytes = browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES;
    let mut image_bytes = browser_ipc::MAX_IMAGE_TOTAL_BYTES;
    for image in &document.images {
        consume(&mut id_bytes, image.id.len())?;
        consume(&mut image_bytes, image.rgba.len())?;
        validate_image(image)?;
        if !ids.insert(&image.id) {
            return Err(invalid("Duplicate image identifier"));
        }
    }
    let mut ids = HashSet::new();
    let mut script_bytes = MAX_SCRIPT_TOTAL_BYTES;
    for script in &document.scripts {
        consume(&mut script_bytes, script.source.len())?;
        if script.src.len() > MAX_URL_BYTES
            || script.source.len() > MAX_SCRIPT_BYTES
            || !ids.insert(&script.src)
        {
            return Err(invalid("Invalid script or script quota exceeded"));
        }
    }
    let mut ids = HashSet::new();
    let mut style_bytes = browser_ipc::MAX_STYLESHEET_TOTAL_BYTES;
    for style in &document.stylesheets {
        consume(&mut style_bytes, style.source.len())?;
        if style.href.is_empty()
            || style.href.len() > MAX_URL_BYTES
            || style.href.chars().any(char::is_control)
            || style.source.len() > browser_ipc::MAX_STYLESHEET_BYTES
            || !ids.insert(&style.href)
        {
            return Err(invalid("Invalid stylesheet or stylesheet quota exceeded"));
        }
    }
    Ok(())
}

fn validate_image(image: &ImageAsset) -> io::Result<()> {
    let expected = image_byte_length(image.width, image.height)?;
    if !browser_ipc::valid_image_id(&image.id)
        || !browser_ipc::valid_natural_image_dimensions(image.natural_width, image.natural_height)
        || image.rgba.len() != expected
    {
        return Err(invalid("Invalid snapshot image"));
    }
    Ok(())
}

fn image_byte_length(width: u32, height: u32) -> io::Result<usize> {
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return Err(invalid("Invalid snapshot image dimensions"));
    }
    let length = width as usize * height as usize * 4;
    if length > browser_ipc::MAX_IMAGE_BYTES {
        return Err(invalid("Image byte quota exceeded"));
    }
    Ok(length)
}

fn consume(remaining: &mut usize, amount: usize) -> io::Result<()> {
    *remaining = remaining
        .checked_sub(amount)
        .ok_or_else(|| invalid("Snapshot quota exceeded"))?;
    Ok(())
}

struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|&n| n <= MAX_FILE_BYTES - HEADER_BYTES)
            .ok_or_else(|| invalid("Snapshot file quota exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encoded_size(snapshot: &Snapshot) -> io::Result<usize> {
    validate(snapshot)?;
    let mut counter = Counter(0);
    write_payload(&mut counter, snapshot)?;
    Ok(counter.0)
}

fn write_u32(writer: &mut impl Write, value: usize) -> io::Result<()> {
    let value = u32::try_from(value).map_err(|_| invalid("Snapshot length overflow"))?;
    writer.write_all(&value.to_le_bytes())
}

fn write_bytes(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    write_u32(writer, bytes.len())?;
    writer.write_all(bytes)
}

fn write_encoded(writer: &mut impl Write, snapshot: &Snapshot, size: usize) -> io::Result<()> {
    writer.write_all(MAGIC)?;
    writer.write_all(&FORMAT_VERSION.to_le_bytes())?;
    writer.write_all(&browser_ipc::PROTOCOL_VERSION.to_le_bytes())?;
    write_u32(writer, size)?;
    write_payload(writer, snapshot)
}

fn write_payload(writer: &mut impl Write, snapshot: &Snapshot) -> io::Result<()> {
    write_bytes(writer, CLI_VERSION.as_bytes())?;
    write_bytes(writer, BUILD_ID.as_bytes())?;
    // Exactly Unavailable with zero entries, matching render_in_subprocess.
    writer.write_all(&[0])?;
    write_u32(writer, 0)?;
    writer.write_all(&snapshot.viewport.width.to_le_bytes())?;
    writer.write_all(&snapshot.viewport.height.to_le_bytes())?;
    let document = &snapshot.document;
    write_bytes(writer, document.url.as_bytes())?;
    write_bytes(writer, document.html.as_bytes())?;
    write_u32(writer, document.images.len())?;
    for image in &document.images {
        write_bytes(writer, image.id.as_bytes())?;
        writer.write_all(&image.width.to_le_bytes())?;
        writer.write_all(&image.height.to_le_bytes())?;
        writer.write_all(&image.natural_width.to_le_bytes())?;
        writer.write_all(&image.natural_height.to_le_bytes())?;
        write_bytes(writer, &image.rgba)?;
    }
    write_u32(writer, document.audio_metadata.len())?;
    for audio in &document.audio_metadata {
        write_bytes(writer, audio.src.as_bytes())?;
        writer.write_all(&audio.sample_rate.to_le_bytes())?;
        writer.write_all(&audio.channels.to_le_bytes())?;
        writer.write_all(&audio.bits_per_sample.to_le_bytes())?;
        writer.write_all(&audio.block_align.to_le_bytes())?;
        writer.write_all(&audio.frames.to_le_bytes())?;
    }
    write_u32(writer, document.scripts.len())?;
    for script in &document.scripts {
        write_bytes(writer, script.src.as_bytes())?;
        write_bytes(writer, script.source.as_bytes())?;
    }
    write_u32(writer, document.stylesheets.len())?;
    for style in &document.stylesheets {
        write_bytes(writer, style.href.as_bytes())?;
        write_bytes(writer, style.source.as_bytes())?;
    }
    Ok(())
}

struct BoundedReader<'a, R> {
    reader: &'a mut R,
    remaining: usize,
}

impl<R: Read> BoundedReader<'_, R> {
    fn fixed<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        consume(&mut self.remaining, N)?;
        let mut bytes = [0; N];
        self.reader.read_exact(&mut bytes)?;
        Ok(bytes)
    }
    fn count(&mut self, maximum: usize) -> io::Result<usize> {
        let count = u32::from_le_bytes(self.fixed()?) as usize;
        if count > maximum {
            return Err(invalid("Snapshot length/count quota exceeded"));
        }
        Ok(count)
    }
    fn bytes(&mut self, maximum: usize) -> io::Result<Vec<u8>> {
        let length = self.count(maximum)?;
        consume(&mut self.remaining, length)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(io::Error::other)?;
        bytes.resize(length, 0);
        self.reader.read_exact(&mut bytes)?;
        Ok(bytes)
    }
    fn text(&mut self, maximum: usize) -> io::Result<String> {
        String::from_utf8(self.bytes(maximum)?).map_err(|_| invalid("Invalid snapshot UTF-8"))
    }
    fn float(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.fixed()?))
    }
}

fn decode(reader: &mut impl Read, file_size: Option<u64>) -> io::Result<Snapshot> {
    let mut header = [0; HEADER_BYTES];
    reader.read_exact(&mut header)?;
    if &header[..8] != MAGIC {
        return Err(invalid("Invalid snapshot magic"));
    }
    if header[8..12] != FORMAT_VERSION.to_le_bytes()
        || header[12..16] != browser_ipc::PROTOCOL_VERSION.to_le_bytes()
    {
        return Err(invalid("Incompatible snapshot or IPC version"));
    }
    let size = u32::from_le_bytes(header[16..20].try_into().expect("fixed header")) as usize;
    if size == 0 || size > MAX_FILE_BYTES - HEADER_BYTES {
        return Err(invalid("Snapshot file quota exceeded"));
    }
    if file_size.is_some_and(|length| length != (HEADER_BYTES + size) as u64) {
        return Err(invalid("Snapshot file length mismatch"));
    }
    let mut input = BoundedReader {
        reader,
        remaining: size,
    };
    if input.text(MAX_VERSION_BYTES)? != CLI_VERSION || input.text(MAX_BUILD_ID_BYTES)? != BUILD_ID
    {
        return Err(invalid("Incompatible snapshot CLI/build identity"));
    }
    if input.fixed::<1>()? != [0] || input.count(0)? != 0 {
        return Err(invalid("Snapshot storage must be unavailable and empty"));
    }
    let viewport = Viewport {
        width: input.float()?,
        height: input.float()?,
    };
    let url = input.text(MAX_URL_BYTES)?;
    let html = input.text(browser_ipc::MAX_HTML_BYTES)?;
    let count = input.count(MAX_IMAGES)?;
    let mut images = Vec::with_capacity(count);
    let mut id_bytes = browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES;
    let mut image_bytes = browser_ipc::MAX_IMAGE_TOTAL_BYTES;
    for _ in 0..count {
        let id = input.text(id_bytes)?;
        consume(&mut id_bytes, id.len())?;
        if !browser_ipc::valid_image_id(&id) {
            return Err(invalid("Invalid image identifier"));
        }
        let width = u32::from_le_bytes(input.fixed()?);
        let height = u32::from_le_bytes(input.fixed()?);
        let natural_width = input.float()?;
        let natural_height = input.float()?;
        let expected = image_byte_length(width, height)?;
        if !browser_ipc::valid_natural_image_dimensions(natural_width, natural_height) {
            return Err(invalid("Invalid intrinsic image dimensions"));
        }
        let rgba = input.bytes(expected.min(image_bytes))?;
        consume(&mut image_bytes, rgba.len())?;
        if rgba.len() != expected {
            return Err(invalid("Image byte length mismatch"));
        }
        images.push(ImageAsset {
            id,
            width,
            height,
            natural_width,
            natural_height,
            rgba,
        });
    }
    let count = input.count(browser_ipc::MAX_AUDIO_METADATA)?;
    let mut audio_metadata = Vec::with_capacity(count);
    let mut audio_src_bytes = browser_ipc::MAX_AUDIO_SRC_TOTAL_BYTES;
    for _ in 0..count {
        let src = input.text(browser_ipc::MAX_AUDIO_SRC_BYTES.min(audio_src_bytes))?;
        consume(&mut audio_src_bytes, src.len())?;
        audio_metadata.push(AudioMetadata {
            src,
            sample_rate: u32::from_le_bytes(input.fixed()?),
            channels: u16::from_le_bytes(input.fixed()?),
            bits_per_sample: u16::from_le_bytes(input.fixed()?),
            block_align: u16::from_le_bytes(input.fixed()?),
            frames: u64::from_le_bytes(input.fixed()?),
        });
    }
    let count = input.count(MAX_SCRIPTS)?;
    let mut scripts = Vec::with_capacity(count);
    let mut script_bytes = MAX_SCRIPT_TOTAL_BYTES;
    for _ in 0..count {
        let src = input.text(MAX_URL_BYTES)?;
        let source = input.text(MAX_SCRIPT_BYTES.min(script_bytes))?;
        consume(&mut script_bytes, source.len())?;
        scripts.push(ScriptAsset { src, source });
    }
    let count = input.count(browser_ipc::MAX_STYLESHEETS)?;
    let mut stylesheets = Vec::with_capacity(count);
    let mut style_bytes = browser_ipc::MAX_STYLESHEET_TOTAL_BYTES;
    for _ in 0..count {
        let href = input.text(MAX_URL_BYTES)?;
        let source = input.text(browser_ipc::MAX_STYLESHEET_BYTES.min(style_bytes))?;
        consume(&mut style_bytes, source.len())?;
        stylesheets.push(StylesheetAsset { href, source });
    }
    if input.remaining != 0 {
        return Err(invalid("Unexpected trailing snapshot payload"));
    }
    let mut extra = [0; 1];
    if input.reader.read(&mut extra)? != 0 {
        return Err(invalid("Unexpected trailing snapshot bytes"));
    }
    let snapshot = Snapshot {
        document: DocumentInput {
            url,
            html,
            images,
            audio_metadata,
            scripts,
            stylesheets,
        },
        viewport,
    };
    validate(&snapshot)?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fixture() -> Snapshot {
        Snapshot {
            document: DocumentInput {
                url: "https://EXAMPLE.test:443/final?q=%2F#fragment".into(),
                html: "<!doctype html>\r\n<title>Olá 🌒</title>\0<script src='a.js'></script>"
                    .into(),
                images: vec![ImageAsset {
                    id: "a.png?x=1&y=2".into(),
                    width: 1,
                    height: 1,
                    natural_width: 0.5,
                    natural_height: 0.25,
                    rgba: vec![0, 127, 255, 64],
                }],
                audio_metadata: vec![],
                scripts: vec![ScriptAsset {
                    src: "a.js".into(),
                    source: "document.title = 'Olá';\r\n// untouched bytes\n".into(),
                }],
                stylesheets: vec![StylesheetAsset {
                    href: "a.css".into(),
                    source: "p::before { content: '\\00a0'; }\r\n".into(),
                }],
            },
            viewport: Viewport {
                width: 743.125,
                height: 481.5,
            },
        }
    }

    fn encoded(snapshot: &Snapshot) -> Vec<u8> {
        let size = encoded_size(snapshot).unwrap();
        let mut bytes = Vec::new();
        write_encoded(&mut bytes, snapshot, size).unwrap();
        bytes
    }

    // Hostile files need not have passed the writer's semantic validation.
    fn encoded_unchecked(snapshot: &Snapshot) -> Vec<u8> {
        let mut counter = Counter(0);
        write_payload(&mut counter, snapshot).unwrap();
        let mut bytes = Vec::new();
        write_encoded(&mut bytes, snapshot, counter.0).unwrap();
        bytes
    }

    fn storage_offset() -> usize {
        HEADER_BYTES + 4 + CLI_VERSION.len() + 4 + BUILD_ID.len()
    }

    fn image_count_offset(snapshot: &Snapshot) -> usize {
        storage_offset()
            + 1
            + 4
            + 8
            + 4
            + snapshot.document.url.len()
            + 4
            + snapshot.document.html.len()
    }

    fn replace_u32(bytes: &mut [u8], offset: usize, value: usize) {
        bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }

    #[test]
    fn roundtrip_preserves_origin_source_bytes_assets_viewport_and_stable_encoding() {
        let original = fixture();
        let bytes = encoded(&original);
        let decoded = decode(&mut bytes.as_slice(), Some(bytes.len() as u64)).unwrap();
        assert_eq!(
            serde_json::to_value(&original.document).unwrap(),
            serde_json::to_value(&decoded.document).unwrap()
        );
        assert_eq!(original.document.url, decoded.document.url);
        assert_eq!(
            decoded.viewport.width.to_bits(),
            original.viewport.width.to_bits()
        );
        assert_eq!(
            decoded.viewport.height.to_bits(),
            original.viewport.height.to_bits()
        );
        assert_eq!(encoded(&decoded), bytes);
        assert_eq!(&bytes[storage_offset()..storage_offset() + 5], &[0; 5]);
    }

    #[test]
    fn local_audio_metadata_is_preserved_without_samples_or_profile_storage() {
        let mut original = fixture();
        original.document.url = "file:///snapshot/fixture.html".into();
        original.document.audio_metadata = vec![AudioMetadata {
            src: "tone.wav".into(),
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
            block_align: 2,
            frames: 32,
        }];
        let bytes = encoded(&original);
        let decoded = decode(&mut bytes.as_slice(), None).unwrap();
        assert_eq!(
            decoded.document.audio_metadata,
            original.document.audio_metadata
        );
        original.document.url = "https://example.test/audio".into();
        assert!(encoded_size(&original).is_err());
    }

    #[test]
    fn rejects_every_truncation_and_all_trailing_bytes() {
        let bytes = encoded(&fixture());
        for end in 0..bytes.len() {
            assert!(
                decode(&mut &bytes[..end], None).is_err(),
                "accepted prefix {end}"
            );
        }
        for extra in [b"x".as_slice(), b"\0", MAGIC] {
            let mut with_extra = bytes.clone();
            with_extra.extend_from_slice(extra);
            assert!(decode(&mut with_extra.as_slice(), None).is_err());
            let size = with_extra.len() - HEADER_BYTES;
            replace_u32(&mut with_extra, 16, size);
            assert!(decode(&mut with_extra.as_slice(), None).is_err());
        }
    }

    #[test]
    fn rejects_magic_versions_build_identity_storage_and_file_length_mismatch() {
        let bytes = encoded(&fixture());
        for index in [
            0,
            8,
            12,
            HEADER_BYTES + 4,
            HEADER_BYTES + 4 + CLI_VERSION.len() + 4,
            storage_offset(),
            storage_offset() + 1,
        ] {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            assert!(
                decode(&mut changed.as_slice(), None).is_err(),
                "accepted mutation {index}"
            );
        }
        assert!(decode(&mut bytes.as_slice(), Some(bytes.len() as u64 + 1)).is_err());
        for size in [0, MAX_FILE_BYTES - HEADER_BYTES + 1, u32::MAX as usize] {
            let mut changed = bytes.clone();
            replace_u32(&mut changed, 16, size);
            assert!(decode(&mut changed.as_slice(), None).is_err());
        }
    }

    #[test]
    fn rejects_declared_lengths_before_reading_or_allocating_payload() {
        for (length, maximum, remaining) in [(5, 4, 100), (5, 100, 7), (u32::MAX, 1024, 1024)] {
            let mut cursor = Cursor::new(length.to_le_bytes());
            let mut reader = BoundedReader {
                reader: &mut cursor,
                remaining,
            };
            assert!(reader.bytes(maximum).is_err());
            assert_eq!(cursor.position(), 4);
        }
        let mut cursor = Cursor::new([3, 0, 0, 0, b'a', b'b', b'c']);
        let mut reader = BoundedReader {
            reader: &mut cursor,
            remaining: 7,
        };
        assert_eq!(reader.text(3).unwrap(), "abc");
        assert_eq!(reader.remaining, 0);
    }

    #[test]
    fn rejects_oversized_counts_and_text_fields_before_their_bodies() {
        let original = fixture();
        let bytes = encoded(&original);
        for (offset, count) in [
            (HEADER_BYTES, MAX_VERSION_BYTES + 1),
            (storage_offset() + 13, MAX_URL_BYTES + 1),
            (
                storage_offset() + 13 + 4 + original.document.url.len(),
                browser_ipc::MAX_HTML_BYTES + 1,
            ),
            (image_count_offset(&original), MAX_IMAGES + 1),
            (
                image_count_offset(&original) + 4,
                browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES + 1,
            ),
        ] {
            let mut changed = bytes.clone();
            replace_u32(&mut changed, offset, count);
            assert!(decode(&mut changed.as_slice(), None).is_err());
        }
        let mut empty = fixture();
        empty.document.images.clear();
        empty.document.audio_metadata.clear();
        empty.document.scripts.clear();
        empty.document.stylesheets.clear();
        let bytes = encoded(&empty);
        let offset = image_count_offset(&empty);
        for (at, count) in [
            (offset + 4, browser_ipc::MAX_AUDIO_METADATA + 1),
            (offset + 8, MAX_SCRIPTS + 1),
            (offset + 12, browser_ipc::MAX_STYLESHEETS + 1),
        ] {
            let mut changed = bytes.clone();
            replace_u32(&mut changed, at, count);
            assert!(decode(&mut changed.as_slice(), None).is_err());
        }
    }

    #[test]
    fn aggregate_budgets_duplicates_invalid_utf8_and_geometry_are_rejected() {
        let mut snapshot = fixture();
        snapshot.document.scripts = (0..3)
            .map(|i| ScriptAsset {
                src: i.to_string(),
                source: "x".repeat(MAX_SCRIPT_BYTES),
            })
            .collect();
        assert!(encoded_size(&snapshot).is_err());
        assert!(decode(&mut encoded_unchecked(&snapshot).as_slice(), None).is_err());
        snapshot.document.scripts.pop();
        let bytes = encoded(&snapshot);
        assert!(decode(&mut bytes.as_slice(), None).is_ok());
        snapshot.document.stylesheets = (0..3)
            .map(|i| StylesheetAsset {
                href: i.to_string(),
                source: "x".repeat(browser_ipc::MAX_STYLESHEET_BYTES),
            })
            .collect();
        assert!(encoded_size(&snapshot).is_err());
        assert!(decode(&mut encoded_unchecked(&snapshot).as_slice(), None).is_err());
        snapshot = fixture();
        snapshot
            .document
            .images
            .push(snapshot.document.images[0].clone());
        assert!(encoded_size(&snapshot).is_err());
        assert!(decode(&mut encoded_unchecked(&snapshot).as_slice(), None).is_err());
        snapshot = fixture();
        snapshot.document.images[0].rgba.pop();
        assert!(encoded_size(&snapshot).is_err());
        assert!(decode(&mut encoded_unchecked(&snapshot).as_slice(), None).is_err());
        snapshot = fixture();
        snapshot.viewport.width = f32::NAN;
        assert!(encoded_size(&snapshot).is_err());
        assert!(decode(&mut encoded_unchecked(&snapshot).as_slice(), None).is_err());
        snapshot = fixture();
        snapshot.document.html = "x".repeat(browser_ipc::MAX_HTML_BYTES + 1);
        assert!(encoded_size(&snapshot).is_err());
        assert!(decode(&mut encoded_unchecked(&snapshot).as_slice(), None).is_err());
        let original = fixture();
        let mut bytes = encoded(&original);
        bytes[storage_offset() + 17] = 0xff;
        assert!(decode(&mut bytes.as_slice(), None).is_err());
    }

    #[test]
    fn image_aggregate_budget_is_enforced_during_decode_and_exact_limit_roundtrips() {
        let mut snapshot = fixture();
        snapshot.document.images = (0..2)
            .map(|i| ImageAsset {
                id: format!("image-{i}"),
                width: 1024,
                height: 1024,
                natural_width: 1024.,
                natural_height: 1024.,
                rgba: vec![127; browser_ipc::MAX_IMAGE_BYTES],
            })
            .collect();
        let bytes = encoded(&snapshot);
        assert!(decode(&mut bytes.as_slice(), None).is_ok());
        snapshot
            .document
            .images
            .push(fixture().document.images.remove(0));
        assert!(encoded_size(&snapshot).is_err());
        assert!(decode(&mut encoded_unchecked(&snapshot).as_slice(), None).is_err());
        let mut counter = Counter(MAX_FILE_BYTES - HEADER_BYTES - 1);
        assert_eq!(counter.write(b"x").unwrap(), 1);
        assert!(counter.write(b"x").is_err());
    }

    #[test]
    fn rejects_credential_urls_and_unsupported_origins_without_rewriting() {
        for url in [
            "https://user:secret@example.test/",
            "file://server/share/a.html",
            "javascript:alert(1)",
            "data:text/html,test",
            "invalid",
            "https://example.test/\n",
        ] {
            let mut snapshot = fixture();
            snapshot.document.url = url.into();
            assert!(encoded_size(&snapshot).is_err(), "accepted {url}");
            assert!(decode(&mut encoded_unchecked(&snapshot).as_slice(), None).is_err());
        }
        for url in [
            "https://EXAMPLE.test:443/final?original=%2f#x",
            "http://example.test/final",
            "file:///missing/local.html",
            "noir:specimen",
        ] {
            let mut snapshot = fixture();
            snapshot.document.url = url.into();
            let bytes = encoded(&snapshot);
            assert_eq!(
                decode(&mut bytes.as_slice(), None).unwrap().document.url,
                url
            );
        }
    }
}
