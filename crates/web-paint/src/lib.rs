//! Bounded CPU display-list rasterizer, independent from the windowing/GPU stack.
//! Validation and a work preflight complete before allocating the output surface
//! or rasterizing glyphs, so excessive overlapping paint cannot partially render.
use browser_ipc::{Color, DisplayItem, ImageAsset, Rect, RenderedPage};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::OnceLock;

const MAX_ITEMS: usize = 100_000;
const MAX_GLYPHS: usize = 100_000;
const MAX_PIXEL_WORK: u64 = 256_000_000;
const MAX_COORDINATE: f32 = 1_000_000.0;
const MAX_CLIP_DEPTH: usize = 128;

#[derive(Debug)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl Surface {
    pub fn write_ppm(&self, out: &mut impl Write) -> io::Result<()> {
        let expected = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(3));
        if self.width == 0 || self.height == 0 || expected != Some(self.rgb.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid RGB surface",
            ));
        }
        write!(out, "P6\n{} {}\n255\n", self.width, self.height)?;
        out.write_all(&self.rgb)
    }
    fn blend(&mut self, x: u32, y: u32, color: Color, coverage: u8) {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        let a = u32::from(color.3) * u32::from(coverage) / 255;
        for (c, v) in [color.0, color.1, color.2].iter().enumerate() {
            self.rgb[i + c] =
                ((u32::from(*v) * a + u32::from(self.rgb[i + c]) * (255 - a)) / 255) as u8;
        }
    }
    fn fill(&mut self, clip: PixelRect, color: Color) {
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                self.blend(x, y, color, 255);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct PixelRect {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}
impl PixelRect {
    fn area(self) -> u64 {
        u64::from(self.right - self.left) * u64::from(self.bottom - self.top)
    }
}
enum Operation<'a> {
    Fill {
        clip: PixelRect,
        color: Color,
    },
    Image {
        clip: PixelRect,
        rect: Rect,
        image: &'a ImageAsset,
    },
    Glyph {
        clip: PixelRect,
        ch: char,
        size: f32,
        color: Color,
        left: i32,
        top: i32,
    },
}
struct Budget {
    remaining_pixels: u64,
    glyphs: usize,
}
impl Budget {
    fn pixels(&mut self, count: u64) -> Result<(), String> {
        self.remaining_pixels = self
            .remaining_pixels
            .checked_sub(count)
            .ok_or("CPU pixel-work budget exceeded")?;
        Ok(())
    }
    fn glyphs(&mut self, count: usize) -> Result<(), String> {
        self.glyphs = self
            .glyphs
            .checked_add(count)
            .ok_or("CPU glyph budget exceeded")?;
        if self.glyphs > MAX_GLYPHS {
            return Err("CPU glyph budget exceeded".into());
        }
        Ok(())
    }
}
fn font() -> Result<&'static fontdue::Font, String> {
    static FONT: OnceLock<Result<fontdue::Font, String>> = OnceLock::new();
    FONT.get_or_init(|| {
        fontdue::Font::from_bytes(
            epaint_default_fonts::UBUNTU_LIGHT,
            fontdue::FontSettings::default(),
        )
        .map_err(str::to_string)
    })
    .as_ref()
    .map_err(Clone::clone)
}
fn validate_rect(rect: Rect) -> Result<(), String> {
    if [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > MAX_COORDINATE)
        || rect.width < 0.
        || rect.height < 0.
    {
        return Err("Invalid CPU paint geometry".into());
    }
    Ok(())
}
fn validate_clip_stack(items: &[DisplayItem]) -> Result<(), String> {
    let mut depth = 0usize;
    for item in items {
        match item {
            DisplayItem::PushClip { rect } => {
                validate_rect(*rect)?;
                depth += 1;
                if depth > MAX_CLIP_DEPTH {
                    return Err("CPU clip stack depth exceeded".into());
                }
            }
            DisplayItem::PopClip => {
                depth = depth.checked_sub(1).ok_or("CPU clip stack underflow")?;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("CPU clip stack is unbalanced".into());
    }
    Ok(())
}
fn intersect_rects(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > left && bottom > top).then_some(Rect {
        x: left,
        y: top,
        width: (right - left).max(0.),
        height: (bottom - top).max(0.),
    })
}
fn clip(rect: Rect, width: u32, height: u32) -> Option<PixelRect> {
    if rect.width <= 0. || rect.height <= 0. {
        return None;
    }
    let left = rect.x.floor().clamp(0., width as f32) as u32;
    let top = rect.y.floor().clamp(0., height as f32) as u32;
    let right = (rect.x + rect.width).ceil().clamp(0., width as f32) as u32;
    let bottom = (rect.y + rect.height).ceil().clamp(0., height as f32) as u32;
    (right > left && bottom > top).then_some(PixelRect {
        left,
        top,
        right,
        bottom,
    })
}
fn clipped_pixels(
    rect: Rect,
    active_clip: Option<Rect>,
    width: u32,
    height: u32,
) -> Option<PixelRect> {
    active_clip
        .and_then(|active| intersect_rects(rect, active))
        .and_then(|intersection| clip(intersection, width, height))
}
fn validate_images(images: &[ImageAsset]) -> Result<HashMap<&str, &ImageAsset>, String> {
    if images.len() > 32 {
        return Err("CPU image count limit exceeded".into());
    }
    let mut lookup = HashMap::new();
    let mut total = 0usize;
    let mut id_bytes = 0usize;
    for image in images {
        id_bytes = id_bytes
            .checked_add(image.id.len())
            .ok_or("CPU image identifier budget exceeded")?;
        let expected = (image.width as usize)
            .checked_mul(image.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("CPU image dimensions overflow")?;
        if image.width == 0
            || image.height == 0
            || !browser_ipc::valid_natural_image_dimensions(
                image.natural_width,
                image.natural_height,
            )
            || image.width > 1024
            || image.height > 1024
            || expected != image.rgba.len()
            || expected > browser_ipc::MAX_IMAGE_BYTES
            || !browser_ipc::valid_image_id(&image.id)
            || id_bytes > browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES
            || lookup.insert(image.id.as_str(), image).is_some()
        {
            return Err("Invalid CPU image asset".into());
        }
        total = total
            .checked_add(expected)
            .ok_or("CPU image total overflow")?;
        if total > browser_ipc::MAX_IMAGE_TOTAL_BYTES {
            return Err("CPU image budget exceeded".into());
        }
    }
    Ok(lookup)
}
fn prepare<'a>(
    page: &RenderedPage,
    images: &'a [ImageAsset],
    width: u32,
    height: u32,
) -> Result<Vec<Operation<'a>>, String> {
    if !page.width.is_finite()
        || !page.height.is_finite()
        || page.width < 0.
        || page.height < 0.
        || page.width > MAX_COORDINATE
        || page.height > MAX_COORDINATE
        || page.items.len() > MAX_ITEMS
    {
        return Err("Invalid CPU page metadata".into());
    }
    validate_clip_stack(&page.items)?;
    let images = validate_images(images)?;
    let mut operations = Vec::new();
    let mut budget = Budget {
        remaining_pixels: MAX_PIXEL_WORK,
        glyphs: 0,
    };
    budget.pixels(u64::from(width) * u64::from(height))?;
    let mut text_bytes = 0usize;
    let mut clip_stack = Vec::new();
    let mut active_clip = Some(Rect {
        x: 0.,
        y: 0.,
        width: width as f32,
        height: height as f32,
    });
    for item in &page.items {
        let rect = match item {
            DisplayItem::PushClip { rect } => {
                clip_stack.push(active_clip);
                active_clip = active_clip.and_then(|active| intersect_rects(active, *rect));
                continue;
            }
            DisplayItem::PopClip => {
                active_clip = clip_stack.pop().ok_or("CPU clip stack underflow")?;
                continue;
            }
            DisplayItem::Rect { rect, .. }
            | DisplayItem::Text { rect, .. }
            | DisplayItem::Image { rect, .. }
            | DisplayItem::Control { rect, .. } => *rect,
        };
        validate_rect(rect)?;
        match item {
            DisplayItem::Control { .. } => {
                return Err("CPU controls must be expanded before paint".into());
            }
            DisplayItem::Rect { color, .. } => {
                if let Some(clip) = clipped_pixels(rect, active_clip, width, height) {
                    budget.pixels(clip.area())?;
                    operations.push(Operation::Fill {
                        clip,
                        color: *color,
                    });
                }
            }
            DisplayItem::Image { asset_id, .. } => {
                let image = *images
                    .get(asset_id.as_str())
                    .ok_or("Missing CPU image asset")?;
                if let Some(clip) = clipped_pixels(rect, active_clip, width, height) {
                    budget.pixels(clip.area())?;
                    operations.push(Operation::Image { clip, rect, image });
                }
            }
            DisplayItem::Text {
                text, size, color, ..
            } => {
                if !size.is_finite() || !(1.0..=256.0).contains(size) {
                    return Err("Invalid CPU font size".into());
                }
                text_bytes = text_bytes
                    .checked_add(text.len())
                    .ok_or("CPU text budget exceeded")?;
                if text_bytes > 2 * browser_ipc::MAX_HTML_BYTES {
                    return Err("CPU text budget exceeded".into());
                }
                // Count even invisible text; oversized input is refused rather than
                // silently truncated. No glyph bitmap is produced in this phase.
                budget.glyphs(text.chars().count())?;
                if active_clip.is_none() || clip(rect, width, height).is_none() {
                    continue;
                }
                let font = font()?;
                let ascent = font
                    .horizontal_line_metrics(*size)
                    .map_or(*size, |m| m.ascent);
                let mut pen = rect.x;
                let mut previous = None;
                for ch in text.chars() {
                    if let Some(previous) = previous {
                        pen += font.horizontal_kern(previous, ch, *size).unwrap_or(0.);
                    }
                    let metrics = font.metrics(ch, *size);
                    let left = (pen + metrics.xmin as f32).floor() as i32;
                    let top = (rect.y + ascent - metrics.height as f32 - metrics.ymin as f32)
                        .floor() as i32;
                    let glyph = Rect {
                        x: left as f32,
                        y: top as f32,
                        width: metrics.width as f32,
                        height: metrics.height as f32,
                    };
                    if let Some(clip) = clipped_pixels(glyph, active_clip, width, height) {
                        let pixels = (metrics.width as u64)
                            .checked_mul(metrics.height as u64)
                            .ok_or("CPU glyph bitmap overflow")?;
                        // Charge rasterization plus visible compositing.
                        budget.pixels(pixels + clip.area())?;
                        operations.push(Operation::Glyph {
                            clip,
                            ch,
                            size: *size,
                            color: *color,
                            left,
                            top,
                        });
                    }
                    pen += metrics.advance_width;
                    previous = Some(ch);
                }
            }
            DisplayItem::PushClip { .. } | DisplayItem::PopClip => {}
        }
    }
    Ok(operations)
}

pub fn rasterize(
    page: &RenderedPage,
    images: &[ImageAsset],
    width: u32,
    height: u32,
) -> Result<Surface, String> {
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > 16_777_216
    {
        return Err("CPU surface exceeds 16 megapixels or 8192px per axis".into());
    }
    if page.items.len() > MAX_ITEMS {
        return Err("CPU display item count exceeded".into());
    }
    // Refuse malformed nesting before control expansion, font work or a surface
    // allocation. Markers are copied unchanged into the expanded display list.
    validate_clip_stack(&page.items)?;
    // Expand controls without cloning page strings or other unvalidated data.
    let mut printable = RenderedPage {
        title: String::new(),
        width: page.width,
        height: page.height,
        background: page.background,
        items: Vec::new(),
        links: Vec::new(),
        metrics: Default::default(),
        warnings: Vec::new(),
        next_timer_ms: None,
        animation_frame_pending: false,
    };
    let mut control_count = 0usize;
    let mut copy_bytes = 0usize;
    let mut copy_glyphs = 0usize;
    let mut image_id_bytes = 0usize;
    for item in &page.items {
        if let DisplayItem::Control {
            rect,
            kind,
            value,
            placeholder,
            appearance,
            options,
            ..
        } = item
        {
            validate_rect(*rect)?;
            control_count += 1;
            if control_count > 256
                || value.len() > 32768
                || placeholder.len() > 2048
                || options.len() > 256
                || options
                    .iter()
                    .any(|option| option.value.len() > 1024 || option.label.len() > 256)
            {
                return Err("CPU control data exceeds limit".into());
            }
            if !appearance.font_size.is_finite()
                || !(1.0..=256.0).contains(&appearance.font_size)
                || !appearance.border_width.is_finite()
                || !(0.0..=128.0).contains(&appearance.border_width)
            {
                return Err("Invalid CPU control appearance".into());
            }
            if *kind == browser_ipc::ControlKind::Hidden {
                continue;
            }
            printable.items.push(DisplayItem::Rect {
                rect: *rect,
                color: appearance.background,
            });
            let border = if *kind == browser_ipc::ControlKind::Checkbox {
                appearance.border_width.max(1.)
            } else {
                appearance.border_width
            }
            .min(rect.width / 2.)
            .min(rect.height / 2.);
            if border > 0. {
                for border_rect in [
                    Rect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: border,
                    },
                    Rect {
                        x: rect.x,
                        y: rect.y + rect.height - border,
                        width: rect.width,
                        height: border,
                    },
                    Rect {
                        x: rect.x,
                        y: rect.y,
                        width: border,
                        height: rect.height,
                    },
                    Rect {
                        x: rect.x + rect.width - border,
                        y: rect.y,
                        width: border,
                        height: rect.height,
                    },
                ] {
                    printable.items.push(DisplayItem::Rect {
                        rect: border_rect,
                        color: appearance.border_color,
                    });
                }
            }
            if *kind == browser_ipc::ControlKind::Checkbox {
                if !value.is_empty() {
                    printable.items.push(DisplayItem::Rect {
                        rect: Rect {
                            x: rect.x + rect.width * 0.25,
                            y: rect.y + rect.height * 0.25,
                            width: rect.width * 0.5,
                            height: rect.height * 0.5,
                        },
                        color: appearance.color,
                    });
                }
                continue;
            }
            let text = if *kind == browser_ipc::ControlKind::Password {
                "•".repeat(value.chars().count())
            } else if *kind == browser_ipc::ControlKind::Select {
                options
                    .iter()
                    .find(|option| option.value == *value)
                    .map_or_else(String::new, |option| option.label.clone())
            } else if value.is_empty() {
                placeholder.clone()
            } else {
                value.clone()
            };
            copy_bytes = copy_bytes
                .checked_add(text.len())
                .ok_or("CPU text budget exceeded")?;
            copy_glyphs = copy_glyphs
                .checked_add(text.chars().count())
                .ok_or("CPU glyph budget exceeded")?;
            if copy_glyphs > MAX_GLYPHS {
                return Err("CPU glyph budget exceeded".into());
            }
            if copy_bytes > 2 * browser_ipc::MAX_HTML_BYTES {
                return Err("CPU control expansion text budget exceeded".into());
            }
            let size = appearance.font_size;
            let mut left = rect.x + border + 4.;
            if matches!(
                kind,
                browser_ipc::ControlKind::Button | browser_ipc::ControlKind::Submit
            ) {
                let font = font()?;
                let mut previous = None;
                let mut text_width = 0.;
                for ch in text.chars() {
                    if let Some(prev) = previous {
                        text_width += font.horizontal_kern(prev, ch, size).unwrap_or(0.);
                    }
                    text_width += font.metrics(ch, size).advance_width;
                    previous = Some(ch);
                }
                left = rect.x + ((rect.width - text_width) / 2.).max(border);
            }
            printable.items.push(DisplayItem::Text {
                rect: Rect {
                    x: left,
                    y: rect.y + ((rect.height - size) / 2.).max(border),
                    width: (rect.x + rect.width - border - left).max(0.),
                    height: (rect.height - 2. * border).max(0.),
                },
                text,
                size,
                color: appearance.color,
                bold: false,
            });
        } else {
            if let DisplayItem::Text { text, .. } = item {
                copy_bytes = copy_bytes
                    .checked_add(text.len())
                    .ok_or("CPU text budget exceeded")?;
                copy_glyphs = copy_glyphs
                    .checked_add(text.chars().count())
                    .ok_or("CPU glyph budget exceeded")?;
                if copy_glyphs > MAX_GLYPHS {
                    return Err("CPU glyph budget exceeded".into());
                }
                if copy_bytes > 2 * browser_ipc::MAX_HTML_BYTES {
                    return Err("CPU text copy budget exceeded".into());
                }
            }
            if let DisplayItem::Image { asset_id, .. } = item {
                image_id_bytes = image_id_bytes
                    .checked_add(asset_id.len())
                    .ok_or("CPU image identifier budget exceeded")?;
                if !browser_ipc::valid_image_id(asset_id)
                    || image_id_bytes > 2 * browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES
                {
                    return Err("CPU image identifier exceeds limit".into());
                }
            }
            printable.items.push(item.clone());
        }
        if printable.items.len() > MAX_ITEMS {
            return Err("CPU expanded display item count exceeded".into());
        }
    }
    let operations = prepare(&printable, images, width, height)?;
    let mut surface = Surface {
        width,
        height,
        rgb: vec![0; width as usize * height as usize * 3],
    };
    surface.fill(
        PixelRect {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
        page.background,
    );
    for operation in operations {
        match operation {
            Operation::Fill { clip, color } => surface.fill(clip, color),
            Operation::Image { clip, rect, image } => {
                for y in clip.top..clip.bottom {
                    for x in clip.left..clip.right {
                        let sx = (((x as f32 - rect.x) / rect.width * image.width as f32) as u32)
                            .min(image.width - 1);
                        let sy = (((y as f32 - rect.y) / rect.height * image.height as f32) as u32)
                            .min(image.height - 1);
                        let i = (sy as usize * image.width as usize + sx as usize) * 4;
                        surface.blend(
                            x,
                            y,
                            Color(
                                image.rgba[i],
                                image.rgba[i + 1],
                                image.rgba[i + 2],
                                image.rgba[i + 3],
                            ),
                            255,
                        );
                    }
                }
            }
            Operation::Glyph {
                clip,
                ch,
                size,
                color,
                left,
                top,
            } => {
                let (metrics, coverage) = font()?.rasterize(ch, size);
                let expected = metrics
                    .width
                    .checked_mul(metrics.height)
                    .ok_or("CPU glyph bitmap overflow")?;
                if coverage.len() != expected {
                    return Err("Invalid CPU glyph coverage".into());
                }
                for y in clip.top..clip.bottom {
                    for x in clip.left..clip.right {
                        let col = (x as i32 - left) as usize;
                        let row = (y as i32 - top) as usize;
                        surface.blend(x, y, color, coverage[row * metrics.width + col]);
                    }
                }
            }
        }
    }
    Ok(surface)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(items: Vec<DisplayItem>) -> RenderedPage {
        RenderedPage {
            next_timer_ms: None,
            animation_frame_pending: false,
            title: String::new(),
            width: 2.,
            height: 2.,
            background: Color(255, 255, 255, 255),
            items,
            links: vec![],
            metrics: Default::default(),
            warnings: vec![],
        }
    }
    #[test]
    fn clips_and_alpha_composites() {
        let page = page(vec![DisplayItem::Rect {
            rect: Rect {
                x: -1.,
                y: -1.,
                width: 2.,
                height: 2.,
            },
            color: Color(255, 0, 0, 128),
        }]);
        let s = rasterize(&page, &[], 2, 2).unwrap();
        assert_eq!(&s.rgb[0..3], &[255, 127, 127]);
        assert_eq!(&s.rgb[9..12], &[255, 255, 255]);
        assert!(rasterize(&page, &[], 8192, 8192).is_err());
    }
    #[test]
    fn malformed_geometry_and_assets_fail_even_when_offscreen() {
        for value in [f32::NAN, f32::INFINITY, -f32::INFINITY, 1_000_001.] {
            let page = page(vec![DisplayItem::Rect {
                rect: Rect {
                    x: value,
                    y: 0.,
                    width: 1.,
                    height: 1.,
                },
                color: Color::default(),
            }]);
            assert!(rasterize(&page, &[], 2, 2).is_err());
        }
        let rect = Rect {
            x: 100.,
            y: 100.,
            width: 1.,
            height: 1.,
        };
        let images_page = page(vec![DisplayItem::Image {
            rect,
            asset_id: "bad".into(),
        }]);
        assert!(rasterize(&images_page, &[], 2, 2).is_err());
        for image in [
            ImageAsset {
                id: "bad".into(),
                width: u32::MAX,
                height: u32::MAX,
                natural_width: u32::MAX as f32,
                natural_height: u32::MAX as f32,
                rgba: vec![],
            },
            ImageAsset {
                id: "bad".into(),
                width: 1,
                height: 1,
                natural_width: 1.,
                natural_height: 1.,
                rgba: vec![0; 3],
            },
        ] {
            assert!(rasterize(&images_page, &[image], 2, 2).is_err());
        }
        let invalid_text = page(vec![DisplayItem::Text {
            rect,
            text: "x".into(),
            size: f32::NAN,
            color: Color::default(),
            bold: false,
        }]);
        assert!(rasterize(&invalid_text, &[], 2, 2).is_err());
    }
    #[test]
    fn excessive_overdraw_and_text_are_refused_before_paint() {
        let huge = DisplayItem::Rect {
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 4096.,
                height: 4096.,
            },
            color: Color::default(),
        };
        let overdraw = page(vec![huge; 16]);
        assert!(
            rasterize(&overdraw, &[], 4096, 4096)
                .unwrap_err()
                .contains("pixel-work")
        );
        let text = page(vec![DisplayItem::Text {
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 2.,
                height: 2.,
            },
            text: "x".repeat(MAX_GLYPHS + 1),
            size: 16.,
            color: Color::default(),
            bold: false,
        }]);
        assert!(
            rasterize(&text, &[], 2, 2)
                .unwrap_err()
                .contains("glyph budget")
        );
    }
    #[test]
    fn natural_metadata_is_validated_without_changing_texture_budget() {
        let mut image = ImageAsset {
            id: "a".into(),
            width: 1,
            height: 1,
            natural_width: 2048.,
            natural_height: 1024.,
            rgba: vec![1, 2, 3, 255],
        };
        assert!(validate_images(std::slice::from_ref(&image)).is_ok());
        for (width, height) in [
            (0., 1.),
            (-1., 1.),
            (f32::NAN, 1.),
            (1., f32::INFINITY),
            (4097., 1.),
            (4096., 1024.25),
            (1., 0.),
            (1., -1.),
        ] {
            image.natural_width = width;
            image.natural_height = height;
            assert!(validate_images(std::slice::from_ref(&image)).is_err());
            assert!(rasterize(&page(vec![]), std::slice::from_ref(&image), 1, 1).is_err());
        }
    }
    #[test]
    fn image_sampling_produces_known_pixels() {
        let page = page(vec![DisplayItem::Image {
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 2.,
                height: 2.,
            },
            asset_id: "a".into(),
        }]);
        let image = ImageAsset {
            id: "a".into(),
            width: 2,
            height: 1,
            natural_width: 2048.,
            natural_height: 1024.,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let surface = rasterize(&page, &[image], 2, 2).unwrap();
        assert_eq!(surface.rgb, [255, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 255]);
    }
    #[test]
    fn data_image_ids_follow_the_same_bounds_as_the_ipc() {
        let id = format!("data:image/png;base64,{}", "A".repeat(9000));
        let image = ImageAsset {
            id: id.clone(),
            width: 1,
            height: 1,
            natural_width: 1.,
            natural_height: 1.,
            rgba: vec![9, 8, 7, 255],
        };
        let rendered = page(vec![DisplayItem::Image {
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.,
            },
            asset_id: id,
        }]);
        assert_eq!(
            rasterize(&rendered, &[image], 1, 1).unwrap().rgb,
            vec![9, 8, 7]
        );
        let invalid = ImageAsset {
            id: "x".repeat(9000),
            width: 1,
            height: 1,
            natural_width: 1.,
            natural_height: 1.,
            rgba: vec![9, 8, 7, 255],
        };
        assert!(
            rasterize(&page(vec![]), &[invalid], 1, 1)
                .unwrap_err()
                .contains("image asset")
        );
    }
    #[test]
    fn text_uses_kerning_and_offscreen_glyphs_have_no_paint_operation() {
        let font = font().unwrap();
        let rect = Rect {
            x: 0.,
            y: 0.,
            width: 200.,
            height: 40.,
        };
        let text = DisplayItem::Text {
            rect,
            text: "AV".into(),
            size: 32.,
            color: Color::default(),
            bold: false,
        };
        let document = page(vec![text]);
        let operations = prepare(&document, &[], 200, 40).unwrap();
        let lefts: Vec<_> = operations
            .iter()
            .filter_map(|op| {
                if let Operation::Glyph { left, .. } = op {
                    Some(*left)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(lefts.len(), 2);
        let expected = (font.metrics('A', 32.).advance_width
            + font.horizontal_kern('A', 'V', 32.).unwrap_or(0.)
            + font.metrics('V', 32.).xmin as f32)
            .floor() as i32;
        assert_eq!(lefts[1], expected);
        let offscreen = page(vec![DisplayItem::Text {
            rect: Rect {
                x: 0.,
                y: 100.,
                width: 200.,
                height: 40.,
            },
            text: "é".repeat(1000),
            size: 256.,
            color: Color::default(),
            bold: false,
        }]);
        assert!(prepare(&offscreen, &[], 200, 40).unwrap().is_empty());
    }
}
