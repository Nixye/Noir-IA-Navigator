//! Bounded image decoding of bytes already authorized by the broker.
//! SVG is a static image subset; resource resolvers never open files or URLs.
use base64::Engine;
use browser_ipc::{ImageAsset, MAX_IMAGE_BYTES};
use image::{ImageFormat, ImageReader, Limits};
use resvg::{tiny_skia, usvg};
use std::io::Cursor;

const MAX_SOURCE_SIDE: u32 = 4096;
const MAX_SOURCE_PIXELS: u64 = 4 * 1024 * 1024;
const MAX_OUTPUT_SIDE: u32 = 1024;
const MAX_SVG_BYTES: usize = 256 * 1024;
const MAX_SVG_NODES: u32 = 1024;
const MAX_SVG_ELEMENTS: usize = 512;
const MAX_SVG_DEPTH: usize = 16;
const MAX_SVG_GEOMETRY_BYTES: usize = 32 * 1024;
const MAX_SVG_SEGMENTS: usize = 8192;
const MAX_SVG_NUMBERS: usize = 8192;
const MAX_SVG_WORK: u64 = 128 * 1024 * 1024;
const MAX_SVG_LAYER_BYTES: u64 = 32 * 1024 * 1024;

pub(super) fn decode_data(src: &str) -> Option<ImageAsset> {
    if src.len() > MAX_IMAGE_BYTES
        || !browser_ipc::valid_image_id(src)
        || !src.get(..5)?.eq_ignore_ascii_case("data:")
    {
        return None;
    }
    let (header, data) = src.get(5..)?.split_once(',')?;
    if header.len() > 256 {
        return None;
    }
    let mut parts = header.split(';');
    let mime = parts.next()?;
    if ![
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/svg+xml",
    ]
    .iter()
    .any(|allowed| mime.eq_ignore_ascii_case(allowed))
    {
        return None;
    }
    let mut is_base64 = false;
    for part in parts {
        if part.eq_ignore_ascii_case("base64") && !is_base64 {
            is_base64 = true;
        } else if !part.eq_ignore_ascii_case("charset=utf-8")
            && !part.eq_ignore_ascii_case("charset=us-ascii")
        {
            return None;
        }
    }
    // Data URI escapes are decoded as bytes. Neither branch can grow beyond
    // its already-bounded input, and neither resolves another resource.
    let unescaped = percent_encoding::percent_decode_str(data).collect::<Vec<_>>();
    let bytes = if is_base64 {
        base64::engine::general_purpose::STANDARD
            .decode(unescaped)
            .ok()?
    } else {
        unescaped
    };
    decode(src, bytes)
}

pub(super) fn decode(id: &str, encoded: Vec<u8>) -> Option<ImageAsset> {
    if !browser_ipc::valid_image_id(id) || encoded.is_empty() || encoded.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let Ok(format) = image::guess_format(&encoded) else {
        return decode_svg(id, &encoded);
    };
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP
    ) {
        return None;
    }
    match format {
        ImageFormat::WebP => preflight_webp(&encoded)?,
        ImageFormat::Gif => preflight_gif(&encoded)?,
        _ => {}
    }
    // Read the header before allocating pixels. Animated formats intentionally
    // expose their first frame through ImageDecoder, not a collected animation.
    let dimensions = ImageReader::with_format(Cursor::new(&encoded), format)
        .into_dimensions()
        .ok()?;
    if !source_dimensions(dimensions.0, dimensions.1) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(32 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(encoded), format);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let image = if image.width() > MAX_OUTPUT_SIDE || image.height() > MAX_OUTPUT_SIDE {
        image.resize(
            MAX_OUTPUT_SIDE,
            MAX_OUTPUT_SIDE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    }
    .to_rgba8();
    asset(
        id,
        image.width(),
        image.height(),
        dimensions.0 as f32,
        dimensions.1 as f32,
        image.into_raw(),
    )
}

fn source_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_SOURCE_SIDE
        && height <= MAX_SOURCE_SIDE
        && u64::from(width) * u64::from(height) <= MAX_SOURCE_PIXELS
}

fn u24(bytes: &[u8]) -> Option<u32> {
    Some(
        u32::from(*bytes.first()?)
            | u32::from(*bytes.get(1)?) << 8
            | u32::from(*bytes.get(2)?) << 16,
    )
}

/// Validate coded frames as well as the canvas: a small VP8X canvas must not
/// cause a decoder to allocate an oversized inconsistent VP8 frame first.
fn preflight_webp(bytes: &[u8]) -> Option<()> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let declared = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let end = declared.checked_add(8)?;
    // Do not leave trailing chunks that a permissive decoder might inspect
    // outside the validated RIFF range.
    if end != bytes.len() || end < 12 {
        return None;
    }
    let mut frames = 0usize;
    inspect_webp_chunks(bytes.get(12..end)?, false, &mut frames)?;
    (frames > 0).then_some(())
}

fn inspect_webp_chunks(bytes: &[u8], nested: bool, frames: &mut usize) -> Option<()> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let kind = bytes.get(offset..offset.checked_add(4)?)?;
        let length =
            u32::from_le_bytes(bytes.get(offset + 4..offset + 8)?.try_into().ok()?) as usize;
        let start = offset.checked_add(8)?;
        let end = start.checked_add(length)?;
        let data = bytes.get(start..end)?;
        match kind {
            b"VP8 " => {
                if data.first()? & 1 != 0 || data.get(3..6)? != [0x9d, 0x01, 0x2a] {
                    return None;
                }
                let width = u16::from_le_bytes(data.get(6..8)?.try_into().ok()?) & 0x3fff;
                let height = u16::from_le_bytes(data.get(8..10)?.try_into().ok()?) & 0x3fff;
                if !source_dimensions(u32::from(width), u32::from(height)) {
                    return None;
                }
                *frames = frames.checked_add(1)?;
            }
            b"VP8L" => {
                if *data.first()? != 0x2f {
                    return None;
                }
                let packed = u32::from_le_bytes(data.get(1..5)?.try_into().ok()?);
                if !source_dimensions((packed & 0x3fff) + 1, ((packed >> 14) & 0x3fff) + 1) {
                    return None;
                }
                *frames = frames.checked_add(1)?;
            }
            b"VP8X" => {
                if nested
                    || !source_dimensions(u24(data.get(4..7)?)? + 1, u24(data.get(7..10)?)? + 1)
                {
                    return None;
                }
            }
            b"ANMF" => {
                if nested
                    || !source_dimensions(u24(data.get(6..9)?)? + 1, u24(data.get(9..12)?)? + 1)
                {
                    return None;
                }
                inspect_webp_chunks(data.get(16..)?, true, frames)?;
            }
            _ => {}
        }
        offset = end.checked_add(length & 1)?;
        if offset > bytes.len() {
            return None;
        }
    }
    Some(())
}

fn preflight_gif(bytes: &[u8]) -> Option<()> {
    if !matches!(bytes.get(..6)?, b"GIF87a" | b"GIF89a") {
        return None;
    }
    let width = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
    let height = u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?);
    if !source_dimensions(u32::from(width), u32::from(height)) {
        return None;
    }
    let packed = *bytes.get(10)?;
    let mut offset = 13usize;
    if packed & 0x80 != 0 {
        offset = offset.checked_add(3 * (1usize << ((packed & 7) + 1)))?;
    }
    loop {
        match *bytes.get(offset)? {
            0x2c => {
                let frame_width =
                    u16::from_le_bytes(bytes.get(offset + 5..offset + 7)?.try_into().ok()?);
                let frame_height =
                    u16::from_le_bytes(bytes.get(offset + 7..offset + 9)?.try_into().ok()?);
                return source_dimensions(u32::from(frame_width), u32::from(frame_height))
                    .then_some(());
            }
            0x21 => {
                offset = offset.checked_add(2)?;
                loop {
                    let length = usize::from(*bytes.get(offset)?);
                    offset = offset.checked_add(1)?;
                    if length == 0 {
                        break;
                    }
                    offset = offset.checked_add(length)?;
                    if offset > bytes.len() {
                        return None;
                    }
                }
            }
            _ => return None,
        }
    }
}

fn asset(
    id: &str,
    width: u32,
    height: u32,
    natural_width: f32,
    natural_height: f32,
    rgba: Vec<u8>,
) -> Option<ImageAsset> {
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)?;
    if width == 0
        || height == 0
        || width > MAX_OUTPUT_SIDE
        || height > MAX_OUTPUT_SIDE
        || bytes > MAX_IMAGE_BYTES as u64
        || bytes != rgba.len() as u64
        || !browser_ipc::valid_image_id(id)
        || !browser_ipc::valid_natural_image_dimensions(natural_width, natural_height)
    {
        return None;
    }
    Some(ImageAsset {
        id: id.to_owned(),
        width,
        height,
        natural_width,
        natural_height,
        rgba,
    })
}

fn decode_svg(id: &str, encoded: &[u8]) -> Option<ImageAsset> {
    if encoded.len() > MAX_SVG_BYTES {
        return None;
    }
    let text = std::str::from_utf8(encoded).ok()?;
    let document = usvg::roxmltree::Document::parse_with_options(
        text,
        usvg::roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_SVG_NODES,
            entity_resolver: None,
        },
    )
    .ok()?;
    validate_svg(&document)?;
    let options = usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    // Using from_xmltree retains our explicit DTD and XML node policy instead
    // of invoking usvg's more permissive XML defaults.
    let tree = usvg::Tree::from_xmltree(&document, &options).ok()?;
    let size = tree.size();
    if !browser_ipc::valid_natural_image_dimensions(size.width(), size.height()) {
        return None;
    }
    let scale = (MAX_OUTPUT_SIDE as f32 / size.width().max(size.height())).min(1.);
    let width = (size.width() * scale)
        .ceil()
        .clamp(1., MAX_OUTPUT_SIDE as f32) as u32;
    let height = (size.height() * scale)
        .ceil()
        .clamp(1., MAX_OUTPUT_SIDE as f32) as u32;
    let mut cost = SvgCost::default();
    inspect_group(tree.root(), scale, 0, &mut cost)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels.checked_mul(cost.segments.max(1) as u64)? > MAX_SVG_WORK {
        return None;
    }
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut rgba = pixmap.take();
    // tiny-skia returns premultiplied pixels; the IPC protocol uses straight RGBA.
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = (u32::from(*channel) * 255 + alpha / 2)
                .checked_div(alpha)
                .unwrap_or(0)
                .min(255) as u8;
        }
    }
    asset(id, width, height, size.width(), size.height(), rgba)
}

fn validate_svg(document: &usvg::roxmltree::Document<'_>) -> Option<()> {
    let root = document.root_element();
    if root.tag_name().name() != "svg"
        || root
            .tag_name()
            .namespace()
            .is_some_and(|ns| ns != "http://www.w3.org/2000/svg")
    {
        return None;
    }
    let mut elements = 0;
    let mut geometry_bytes = 0usize;
    let mut numbers = 0usize;
    for node in document.descendants().filter(|node| node.is_element()) {
        elements += 1;
        if elements > MAX_SVG_ELEMENTS || node.ancestors().count() > MAX_SVG_DEPTH {
            return None;
        }
        if !matches!(
            node.tag_name().name(),
            "svg"
                | "g"
                | "defs"
                | "path"
                | "rect"
                | "circle"
                | "ellipse"
                | "line"
                | "polyline"
                | "polygon"
                | "linearGradient"
                | "radialGradient"
                | "stop"
                | "clipPath"
                | "title"
                | "desc"
        ) {
            return None;
        }
        let inside_clip = node
            .ancestors()
            .any(|ancestor| ancestor.tag_name().name() == "clipPath");
        for attribute in node.attributes() {
            let name = attribute.name();
            let value = attribute.value();
            if name.starts_with("on")
                || matches!(
                    name,
                    "href"
                        | "filter"
                        | "mask"
                        | "marker-start"
                        | "marker-mid"
                        | "marker-end"
                        | "stroke-dasharray"
                        | "stroke-dashoffset"
                )
            {
                return None;
            }
            if matches!(name, "d" | "points") {
                geometry_bytes = geometry_bytes.checked_add(value.len())?;
                if geometry_bytes > MAX_SVG_GEOMETRY_BYTES {
                    return None;
                }
            }
            if matches!(
                name,
                "d" | "points"
                    | "transform"
                    | "gradientTransform"
                    | "viewBox"
                    | "x"
                    | "y"
                    | "x1"
                    | "x2"
                    | "y1"
                    | "y2"
                    | "cx"
                    | "cy"
                    | "r"
                    | "rx"
                    | "ry"
                    | "width"
                    | "height"
                    | "stroke-width"
                    | "stroke-miterlimit"
            ) {
                bounded_svg_numbers(value, &mut numbers)?;
            }
            // Stylesheets, external URLs, expansion through use/image, filters,
            // dashed strokes and recursive clipping are outside this subset.
            if name == "style" {
                // Do not accept escaped property names that could hide a
                // prohibited filter, dash or other expanding paint effect.
                if value.contains('\\') {
                    return None;
                }
                let lower = value.to_ascii_lowercase();
                if ["filter", "mask", "marker", "stroke-dash", "clip-path"]
                    .iter()
                    .any(|name| lower.contains(name))
                {
                    return None;
                }
                for declaration in lower.split(';').filter(|part| !part.trim().is_empty()) {
                    let (property, value) = declaration.split_once(':')?;
                    match property.trim() {
                        "fill" | "stroke" | "fill-rule" | "clip-rule" | "stroke-linecap"
                        | "stroke-linejoin" | "color" | "display" | "visibility" => {}
                        "opacity" | "fill-opacity" | "stroke-opacity" | "stroke-width"
                        | "stroke-miterlimit" => bounded_svg_numbers(value, &mut numbers)?,
                        _ => return None,
                    }
                }
            }
            if inside_clip && name == "clip-path" {
                return None;
            }
            if value.contains("url(") {
                for reference in value.split("url(").skip(1) {
                    let target = reference
                        .split(')')
                        .next()?
                        .trim()
                        .trim_matches(['\'', '"']);
                    if !target.starts_with('#') || target.len() > 128 {
                        return None;
                    }
                }
            }
        }
    }
    Some(())
}

/// A small linear numeric preflight avoids feeding huge/non-finite geometry
/// into conversion, flattening or stroking before the post-conversion budget.
fn bounded_svg_numbers(value: &str, count: &mut usize) -> Option<()> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let starts_number = bytes[index].is_ascii_digit()
            || (matches!(bytes[index], b'.' | b'+' | b'-')
                && bytes
                    .get(index + 1)
                    .is_some_and(|b| b.is_ascii_digit() || *b == b'.'));
        if !starts_number {
            index += 1;
            continue;
        }
        let start = index;
        if matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if bytes.get(index) == Some(&b'.') {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
        }
        if bytes.get(index).is_some_and(|b| matches!(b, b'e' | b'E')) {
            let mut end = index + 1;
            if bytes.get(end).is_some_and(|b| matches!(b, b'+' | b'-')) {
                end += 1;
            }
            let exponent_start = end;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end > exponent_start {
                index = end;
            }
        }
        let number: f64 = value.get(start..index)?.parse().ok()?;
        if !number.is_finite() || number.abs() > 1_000_000. {
            return None;
        }
        *count = count.checked_add(1)?;
        if *count > MAX_SVG_NUMBERS {
            return None;
        }
    }
    Some(())
}

#[derive(Default)]
struct SvgCost {
    nodes: usize,
    segments: usize,
    layer_bytes: u64,
}

fn inspect_group(group: &usvg::Group, scale: f32, depth: usize, cost: &mut SvgCost) -> Option<()> {
    if depth > MAX_SVG_DEPTH || group.mask().is_some() || !group.filters().is_empty() {
        return None;
    }
    if let Some(clip) = group.clip_path() {
        // Explicit clip chains are outside the static subset. Walk clip shapes
        // too so their segments and temporary masks participate in the budget.
        if clip.clip_path().is_some() || !clip.transform().is_identity() {
            return None;
        }
        inspect_group(clip.root(), scale, depth + 1, cost)?;
    }
    if group.should_isolate() {
        let bounds = group.abs_layer_bounding_box();
        let width = (f64::from(bounds.width()) * f64::from(scale)).ceil();
        let height = (f64::from(bounds.height()) * f64::from(scale)).ceil();
        if !width.is_finite() || !height.is_finite() || width > 16_384. || height > 16_384. {
            return None;
        }
        let width = (width as u64).checked_add(4)?;
        let height = (height as u64).checked_add(4)?;
        cost.layer_bytes = cost
            .layer_bytes
            .checked_add(width.checked_mul(height)?.checked_mul(4)?)?;
        if cost.layer_bytes > MAX_SVG_LAYER_BYTES {
            return None;
        }
    }
    for node in group.children() {
        cost.nodes += 1;
        if cost.nodes > MAX_SVG_ELEMENTS {
            return None;
        }
        if let Some(bounds) = node.abs_layer_bounding_box()
            && [bounds.x(), bounds.y(), bounds.width(), bounds.height()]
                .iter()
                .any(|value| !value.is_finite() || (value * scale).abs() > 16_384.)
        {
            return None;
        }
        match node {
            usvg::Node::Group(group) => inspect_group(group, scale, depth + 1, cost)?,
            usvg::Node::Path(path) => {
                cost.segments = cost.segments.checked_add(path.data().segments().count())?;
                if cost.segments > MAX_SVG_SEGMENTS {
                    return None;
                }
            }
            // Text and nested images are rejected even if a future dependency
            // feature unification would add these capabilities to usvg.
            usvg::Node::Image(_) | usvg::Node::Text(_) => return None,
        }
    }
    Some(())
}

#[cfg(test)]
mod natural_size_tests {
    use super::*;

    #[test]
    fn raster_retains_original_size_after_texture_reduction() {
        let source = image::RgbaImage::from_pixel(2048, 1024, image::Rgba([12, 34, 56, 255]));
        let mut encoded = Cursor::new(Vec::new());
        source.write_to(&mut encoded, ImageFormat::Png).unwrap();
        let decoded = decode("large.png", encoded.into_inner()).unwrap();
        assert_eq!((decoded.width, decoded.height), (1024, 512));
        assert_eq!(
            (decoded.natural_width, decoded.natural_height),
            (2048., 1024.)
        );
        assert_eq!(decoded.rgba.len(), 1024 * 512 * 4);
    }

    #[test]
    fn svg_retains_fractional_css_size_before_rounded_texture() {
        let decoded = decode_svg("fraction.svg", br#"<svg xmlns="http://www.w3.org/2000/svg" width="2048.5" height="1023.25"><rect width="2048.5" height="1023.25" fill="red"/></svg>"#).unwrap();
        assert_eq!(
            (decoded.natural_width, decoded.natural_height),
            (2048.5, 1023.25)
        );
        assert_eq!((decoded.width, decoded.height), (1024, 512));
        assert_eq!(decoded.rgba.len(), 1024 * 512 * 4);
    }

    #[test]
    fn natural_metadata_never_changes_texture_byte_validation() {
        let decoded = asset("a", 1, 1, 2048., 1024., vec![1, 2, 3, 255]).unwrap();
        assert_eq!(decoded.rgba.len(), 4);
        assert!(asset("a", 1, 1, 2048., 1024., vec![0; 8]).is_none());
        assert!(asset("a", 1025, 1, 1., 1., vec![0; 1025 * 4]).is_none());
        assert!(asset("a", 1, 1, 4096., 4096., vec![0; 4]).is_none());
        assert!(source_dimensions(4096, 1024));
        assert!(!source_dimensions(4097, 1));
        assert!(!source_dimensions(4096, 1025));
    }
}
