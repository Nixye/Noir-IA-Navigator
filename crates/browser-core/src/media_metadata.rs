//! Local, opt-in WAV metadata. PCM bytes never leave the privileged broker.
use browser_ipc::AudioMetadata;
use std::{collections::HashSet, fs::File, io::Read, path::Path};
use url::Url;

const FILE_LIMIT: usize = 4 * 1024 * 1024;
const TOTAL_LIMIT: usize = 8 * 1024 * 1024;
const SOURCE_LIMIT: usize = 8192;
const SOURCE_TOTAL_LIMIT: usize = 64 * 1024;
const ATTEMPT_LIMIT: usize = 8;
const CHUNK_LIMIT: usize = 128;

pub(super) fn load(html: &str, root: &Path, base: &str) -> Vec<AudioMetadata> {
    let (Ok(document), Ok(base)) = (web_html::parse(html), Url::parse(base)) else {
        return vec![];
    };
    let mut remaining = TOTAL_LIMIT;
    let mut attempts = 0;
    let mut source_bytes = 0usize;
    let mut seen = HashSet::new();
    let mut assets = Vec::new();
    for (id, element) in document.elements() {
        if element.tag != "audio"
            || !element
                .attr("preload")
                .is_some_and(|value| value.eq_ignore_ascii_case("metadata"))
            || document.is_inactive_subtree(id)
        {
            continue;
        }
        let Some(src) = element.attr("src") else {
            continue;
        };
        if attempts == ATTEMPT_LIMIT || remaining == 0 {
            break;
        }
        // Every candidate counts, including invalid paths, failed reads and repeats.
        attempts += 1;
        if src.len() > SOURCE_LIMIT || source_bytes.saturating_add(src.len()) > SOURCE_TOTAL_LIMIT {
            continue;
        }
        source_bytes += src.len();
        if !seen.insert(src.to_owned()) {
            continue;
        }
        let Some(bytes) = local_bytes(src, root, &base, &mut remaining) else {
            continue;
        };
        if let Some(metadata) = parse(&bytes, src) {
            assets.push(metadata);
        }
    }
    assets
}

fn local_bytes(src: &str, root: &Path, base: &Url, remaining: &mut usize) -> Option<Vec<u8>> {
    super::check_input(src).ok()?;
    if src.starts_with('/')
        || src.starts_with(char::from(92))
        || Path::new(src).has_root()
        || Path::new(src)
            .components()
            .any(|part| matches!(part, std::path::Component::Prefix(_)))
        || Url::parse(src).is_ok()
    {
        return None;
    }
    let resolved = base.join(src).ok()?;
    let path = super::local_path(resolved.as_str())
        .ok()?
        .canonicalize()
        .ok()?;
    if !path.starts_with(root)
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    {
        return None;
    }
    // Same confinement policy as other local assets. This does not eliminate
    // the existing canonicalize/open race. Check regular-file status on the handle.
    let mut file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    read_counted(&mut file, remaining, FILE_LIMIT)
}

fn read_counted(reader: &mut impl Read, remaining: &mut usize, limit: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        // Never perform an unbudgeted read, including a probe for EOF.
        let request = chunk
            .len()
            .min(*remaining)
            .min(limit.checked_add(1)?.checked_sub(bytes.len())?);
        if request == 0 {
            return None;
        }
        let count = reader.read(&mut chunk[..request]).ok()?;
        *remaining -= count;
        if count == 0 {
            return Some(bytes);
        }
        // The over-limit sentinel and bytes preceding any error remain charged.
        if bytes.len().checked_add(count)? > limit {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn parse(bytes: &[u8], src: &str) -> Option<AudioMetadata> {
    if bytes.len() > FILE_LIMIT || bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let riff_end = usize::try_from(u32_at(bytes, 4)?).ok()?.checked_add(8)?;
    if riff_end != bytes.len() {
        return None;
    }
    let mut offset = 12usize;
    let mut chunks = 0;
    let mut format = None;
    let mut data_size = None;
    while offset < riff_end {
        chunks += 1;
        if chunks > CHUNK_LIMIT {
            return None;
        }
        let header_end = offset.checked_add(8)?;
        let id = bytes.get(offset..offset.checked_add(4)?)?;
        let size = usize::try_from(u32_at(bytes, offset.checked_add(4)?)?).ok()?;
        let end = header_end.checked_add(size)?;
        let padded_end = end.checked_add(size & 1)?;
        if padded_end > riff_end {
            return None;
        }
        match id {
            b"fmt " => {
                if format.is_some() || !matches!(size, 16 | 18) {
                    return None;
                }
                let fmt = bytes.get(header_end..end)?;
                if u16_at(fmt, 0)? != 1 || (size == 18 && u16_at(fmt, 16)? != 0) {
                    return None;
                }
                let channels = u16_at(fmt, 2)?;
                let sample_rate = u32_at(fmt, 4)?;
                let byte_rate = u32_at(fmt, 8)?;
                let block_align = u16_at(fmt, 12)?;
                let bits_per_sample = u16_at(fmt, 14)?;
                if !matches!(channels, 1 | 2)
                    || !matches!(bits_per_sample, 8 | 16 | 24 | 32)
                    || !(8000..=192000).contains(&sample_rate)
                    || block_align != channels.checked_mul(bits_per_sample / 8)?
                    || byte_rate != sample_rate.checked_mul(u32::from(block_align))?
                {
                    return None;
                }
                format = Some((channels, sample_rate, block_align, bits_per_sample));
            }
            // Record the first data chunk even when the duplicate guard is false.
            b"data" if data_size.replace(size).is_some() => return None,
            _ => {} // Other chunks are bounded and skipped, never interpreted.
        }
        offset = padded_end;
    }
    let (channels, sample_rate, block_align, bits_per_sample) = format?;
    let size = data_size?;
    if size % usize::from(block_align) != 0 {
        return None;
    }
    Some(AudioMetadata {
        src: src.to_owned(),
        sample_rate,
        channels,
        bits_per_sample,
        block_align,
        frames: u64::try_from(size / usize::from(block_align)).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    fn chunk(bytes: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        if !payload.len().is_multiple_of(2) {
            bytes.push(0);
        }
    }
    fn finish(bytes: &mut [u8]) {
        let length = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
    }
    fn wav(channels: u16, bits: u16, frames: usize) -> Vec<u8> {
        let align = channels * (bits / 8);
        let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&8000u32.to_le_bytes());
        fmt.extend_from_slice(&(8000 * u32::from(align)).to_le_bytes());
        fmt.extend_from_slice(&align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        chunk(&mut bytes, b"fmt ", &fmt);
        chunk(&mut bytes, b"data", &vec![0; frames * usize::from(align)]);
        finish(&mut bytes);
        bytes
    }

    #[test]
    fn pcm_formats_and_odd_padding_are_validated_without_decoding() {
        for channels in [1, 2] {
            for bits in [8, 16, 24, 32] {
                let metadata = parse(&wav(channels, bits, 3), "a.wav").unwrap();
                assert_eq!(metadata.frames, 3);
                assert_eq!(metadata.channels, channels);
                assert_eq!(metadata.bits_per_sample, bits);
                assert_eq!(metadata.sample_rate, 8000);
            }
        }
        let mut bytes = wav(1, 8, 3);
        bytes.pop();
        finish(&mut bytes);
        assert!(parse(&bytes, "a.wav").is_none());
    }

    #[test]
    fn malformed_headers_formats_duplicates_and_truncation_are_rejected() {
        let original = wav(2, 16, 3);
        for length in 0..original.len() {
            assert!(parse(&original[..length], "a.wav").is_none());
        }
        for signature in [b"RF64", b"RIFX"] {
            let mut bytes = original.clone();
            bytes[..4].copy_from_slice(signature);
            assert!(parse(&bytes, "a.wav").is_none());
        }
        for (offset, value) in [(20, 3u16), (20, 0xfffe), (22, 3), (32, 1), (34, 12)] {
            let mut bytes = original.clone();
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            assert!(parse(&bytes, "a.wav").is_none());
        }
        for offset in [24, 28] {
            let mut bytes = original.clone();
            bytes[offset..offset + 4].copy_from_slice(&1u32.to_le_bytes());
            assert!(parse(&bytes, "a.wav").is_none());
        }
        for duplicate in [&original[12..36], &original[36..]] {
            let mut bytes = original.clone();
            bytes.extend_from_slice(duplicate);
            finish(&mut bytes);
            assert!(parse(&bytes, "a.wav").is_none());
        }
        let mut bytes = original.clone();
        bytes[40..44].copy_from_slice(&11u32.to_le_bytes());
        assert!(parse(&bytes, "a.wav").is_none()); // integral frames required
        let mut bytes = original.clone();
        bytes.extend_from_slice(b"extra");
        assert!(parse(&bytes, "a.wav").is_none());
        let mut bytes = original;
        bytes[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&bytes, "a.wav").is_none());
    }

    #[test]
    fn unknown_chunks_are_bounded_and_never_interpreted() {
        let mut bytes = wav(1, 8, 1);
        for _ in 0..126 {
            chunk(&mut bytes, b"JUNK", b"../secret");
        }
        finish(&mut bytes);
        assert!(parse(&bytes, "a.wav").is_some());
        chunk(&mut bytes, b"JUNK", &[]);
        finish(&mut bytes);
        assert!(parse(&bytes, "a.wav").is_none());
    }

    #[test]
    fn counted_reads_charge_invalid_bytes_errors_sentinels_and_eof() {
        let mut remaining = 12;
        let invalid = read_counted(&mut Cursor::new(b"garbage"), &mut remaining, 8).unwrap();
        assert!(parse(&invalid, "a.wav").is_none());
        assert_eq!(remaining, 5);
        assert!(read_counted(&mut Cursor::new(b"12345"), &mut remaining, 8).is_none());
        assert_eq!(remaining, 0); // no unbudgeted probe for EOF
        let mut remaining = 20;
        assert!(read_counted(&mut Cursor::new(b"1234567890"), &mut remaining, 8).is_none());
        assert_eq!(remaining, 11); // ninth byte is the charged sentinel
        let mut remaining = 20;
        assert_eq!(
            read_counted(&mut Cursor::new(b"12345678"), &mut remaining, 8)
                .unwrap()
                .len(),
            8
        );
        assert_eq!(remaining, 12);
        struct FailsAfterBytes(bool);
        impl Read for FailsAfterBytes {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    return Err(io::Error::other("read failed"));
                }
                self.0 = true;
                out[..3].copy_from_slice(b"bad");
                Ok(3)
            }
        }
        assert!(read_counted(&mut FailsAfterBytes(false), &mut remaining, 8).is_none());
        assert_eq!(remaining, 9);
    }

    #[test]
    fn growing_stream_cannot_escape_per_file_or_aggregate_budget() {
        // A stream that never reaches EOF models a file growing during reads.
        let mut remaining = 20;
        assert!(read_counted(&mut io::repeat(0), &mut remaining, 8).is_none());
        assert_eq!(remaining, 11);
        assert!(read_counted(&mut io::repeat(0), &mut remaining, 8).is_none());
        assert_eq!(remaining, 2);
        assert!(read_counted(&mut io::repeat(0), &mut remaining, 8).is_none());
        assert_eq!(remaining, 0);
    }

    #[test]
    fn only_explicit_active_local_audio_is_collected_and_failures_count() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("a.wav"), wav(1, 16, 8)).unwrap();
        let base = Url::from_file_path(root.join("page.html")).unwrap();
        let html = "<template><audio src=a.wav preload=metadata></audio></template><noscript><audio src=a.wav preload=metadata></audio></noscript><video src=a.wav preload=metadata></video><audio src=a.wav></audio><audio src=a.wav preload=none></audio><audio preload=metadata><source src=a.wav></audio><audio src=a.wav preload=metadata></audio>";
        let assets = load(html, &root, base.as_str());
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].frames, 8);
        let attempts = "<audio src=missing.wav preload=metadata></audio>".repeat(8);
        assert!(load(&format!("{attempts}{html}"), &root, base.as_str()).is_empty());
    }

    #[test]
    fn oversized_sources_consume_attempts_without_loading_later_audio() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("a.wav"), wav(1, 8, 1)).unwrap();
        let base = Url::from_file_path(root.join("page.html")).unwrap();
        let large = "x".repeat(SOURCE_LIMIT + 1);
        let mut html = format!("<audio src='{large}' preload=metadata></audio>").repeat(8);
        html.push_str("<audio src=a.wav preload=metadata></audio>");
        assert!(load(&html, &root, base.as_str()).is_empty());
    }

    #[test]
    fn local_paths_extensions_and_regular_file_policy_stay_confined() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().canonicalize().unwrap();
        let root = outer.join("page");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(outer.join("outside.wav"), wav(1, 8, 1)).unwrap();
        std::fs::write(root.join("a.bin"), wav(1, 8, 1)).unwrap();
        std::fs::create_dir(root.join("directory.wav")).unwrap();
        let base = Url::from_file_path(root.join("page.html")).unwrap();
        let mut remaining = TOTAL_LIMIT;
        for src in [
            "../outside.wav",
            "%2e%2e/outside.wav",
            "https://example.com/a.wav",
            "data:audio/wav,test",
            "//host/a.wav",
            "a.bin",
            "directory.wav",
            "/a.wav",
            "\\a.wav",
            "C:a.wav",
        ] {
            assert!(
                local_bytes(src, &root, &base, &mut remaining).is_none(),
                "{src}"
            );
        }
        let absolute = Url::from_file_path(outer.join("outside.wav")).unwrap();
        assert!(local_bytes(absolute.as_str(), &root, &base, &mut remaining).is_none());
        assert_eq!(remaining, TOTAL_LIMIT);
    }
}
