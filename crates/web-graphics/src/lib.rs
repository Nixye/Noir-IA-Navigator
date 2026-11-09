//! Original bounded Web display-list driver. Produces immutable GPU geometry,
//! glyph coverage and authorized image textures; no filesystem or network APIs.
use browser_ipc::{Color, DisplayItem, ImageAsset, Rect, RenderedPage};
use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

pub const VERTEX_STRIDE: usize = 36;
pub const MAX_VERTEX_BYTES: usize = 900_000 * VERTEX_STRIDE;
pub const MAX_TEXTURE_BYTES: usize = 16 * 1024 * 1024;
const ATLAS_SIDE: usize = 2048;
const MAX_GLYPHS: usize = 100_000;
const MAX_GLYPH_KINDS: usize = 4096;
const MAX_RASTER_WORK: usize = 16_777_216;
const MAX_VISIBLE_WORK: f64 = 256_000_000.;
static NEXT_SCENE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFormat {
    Rgba8,
    Coverage8,
}
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub bytes: Vec<u8>,
}
pub struct Draw {
    pub item: usize,
    pub texture: usize,
    pub vertices: Range<u32>,
    pub bounds: Rect,
}
pub struct Scene {
    pub id: u64,
    pub vertex_bytes: Vec<u8>,
    pub draws: Vec<Draw>,
    pub textures: Vec<Texture>,
    work_rects: Vec<Rect>,
    clips: Vec<Rect>,
}

fn geometry(rect: Rect) -> Result<(), String> {
    if [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .any(|n| !n.is_finite() || n.abs() > 1_000_000.)
        || rect.width < 0.
        || rect.height < 0.
    {
        return Err("Invalid graphics geometry".into());
    }
    Ok(())
}
fn font() -> Result<&'static fontdue::Font, String> {
    static FONT: OnceLock<Result<fontdue::Font, String>> = OnceLock::new();
    FONT.get_or_init(|| {
        fontdue::Font::from_bytes(
            epaint_default_fonts::UBUNTU_LIGHT,
            fontdue::FontSettings::default(),
        )
        .map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(Clone::clone)
}
fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        width: (right - x).max(0.),
        height: (bottom - y).max(0.),
    })
}

impl Scene {
    /// scale is CSS pixels to physical pixels, including zoom and DPI. Geometry
    /// stays in CSS coordinates so scrolling never rebuilds glyphs or buffers.
    pub fn prepare(page: &RenderedPage, images: &[ImageAsset], scale: f32) -> Result<Self, String> {
        if !scale.is_finite()
            || !(0.5..=4.).contains(&scale)
            || !page.width.is_finite()
            || !page.height.is_finite()
            || !(0. ..=1_000_000.).contains(&page.width)
            || !(0. ..=1_000_000.).contains(&page.height)
            || page.items.len() > 40_000
            || images.len() > 32
        {
            return Err("Graphics scene exceeds supported dimensions or counts".into());
        }
        let mut lookup = HashMap::new();
        let mut image_bytes = 0usize;
        let mut image_ids = 0usize;
        for (index, image) in images.iter().enumerate() {
            let expected = (image.width as usize)
                .checked_mul(image.height as usize)
                .and_then(|n| n.checked_mul(4))
                .ok_or("Graphics image size overflow")?;
            image_bytes = image_bytes
                .checked_add(expected)
                .ok_or("Graphics image budget overflow")?;
            image_ids = image_ids
                .checked_add(image.id.len())
                .ok_or("Graphics image ID overflow")?;
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
                || image_bytes > browser_ipc::MAX_IMAGE_TOTAL_BYTES
                || image_ids > browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES
                || !browser_ipc::valid_image_id(&image.id)
                || lookup.insert(image.id.as_str(), index + 2).is_some()
            {
                return Err("Invalid graphics image asset".into());
            }
        }
        let mut text_bytes = 0usize;
        let mut glyph_count = 0usize;
        let mut reference_bytes = 0usize;
        let mut controls = 0usize;
        let mut clip_depth = 0usize;
        for item in &page.items {
            let rect = match item {
                DisplayItem::PushClip { rect } => {
                    if clip_depth >= 128 {
                        return Err("Graphics clip nesting exceeded".into());
                    }
                    clip_depth += 1;
                    *rect
                }
                DisplayItem::PopClip => {
                    clip_depth = clip_depth
                        .checked_sub(1)
                        .ok_or("Unbalanced graphics clip")?;
                    continue;
                }
                DisplayItem::Rect { rect, .. }
                | DisplayItem::Text { rect, .. }
                | DisplayItem::Image { rect, .. }
                | DisplayItem::Control { rect, .. } => *rect,
            };
            geometry(rect)?;
            match item {
                DisplayItem::Text { text, size, .. } => {
                    text_bytes = text_bytes
                        .checked_add(text.len())
                        .ok_or("Graphics text overflow")?;
                    glyph_count = glyph_count
                        .checked_add(text.chars().count())
                        .ok_or("Graphics glyph overflow")?;
                    if text_bytes > 2 * browser_ipc::MAX_HTML_BYTES
                        || glyph_count > MAX_GLYPHS
                        || !size.is_finite()
                        || !(1. ..=256.).contains(size)
                        || size * scale > 512.
                    {
                        return Err("Graphics text or raster scale exceeds limit".into());
                    }
                }
                DisplayItem::Image { asset_id, .. } => {
                    reference_bytes = reference_bytes
                        .checked_add(asset_id.len())
                        .ok_or("Graphics image reference overflow")?;
                    if reference_bytes > 2 * browser_ipc::MAX_HTML_BYTES
                        || !lookup.contains_key(asset_id.as_str())
                    {
                        return Err("Missing or excessive graphics image references".into());
                    }
                }
                DisplayItem::Control { .. } => {
                    controls += 1;
                    if controls > 256 {
                        return Err("Graphics control count exceeded".into());
                    }
                }
                _ => {}
            }
        }
        // All broker-provided sizes and text counts are checked before copying
        // texture bytes or allocating the atlas and retained vertex storage.
        if clip_depth != 0 {
            return Err("Unbalanced graphics clip".into());
        }
        let mut scene = Scene {
            id: NEXT_SCENE.fetch_add(1, Ordering::Relaxed),
            vertex_bytes: Vec::new(),
            draws: Vec::new(),
            work_rects: Vec::new(),
            clips: Vec::new(),
            textures: vec![
                Texture {
                    width: 1,
                    height: 1,
                    format: TextureFormat::Rgba8,
                    bytes: vec![255; 4],
                },
                Texture {
                    width: ATLAS_SIDE as u32,
                    height: ATLAS_SIDE as u32,
                    format: TextureFormat::Coverage8,
                    bytes: vec![0; ATLAS_SIDE * ATLAS_SIDE],
                },
            ],
        };
        for image in images {
            scene.textures.push(Texture {
                width: image.width,
                height: image.height,
                format: TextureFormat::Rgba8,
                bytes: image.rgba.clone(),
            });
        }
        let mut atlas = Atlas::default();
        for (index, item) in page.items.iter().enumerate() {
            match item {
                DisplayItem::PushClip { rect } => {
                    let effective = match scene.clips.last() {
                        Some(parent) => intersection(*parent, *rect).unwrap_or(Rect {
                            x: 0.,
                            y: 0.,
                            width: 0.,
                            height: 0.,
                        }),
                        None => *rect,
                    };
                    scene.clips.push(effective);
                }
                DisplayItem::PopClip => {
                    scene.clips.pop();
                }
                DisplayItem::Rect { rect, color } => {
                    scene.quad(index, 0, *rect, [0., 0., 1., 1.], *color, 0.)?
                }
                DisplayItem::Image { rect, asset_id } => scene.quad(
                    index,
                    lookup[asset_id.as_str()],
                    *rect,
                    [0., 0., 1., 1.],
                    Color(255, 255, 255, 255),
                    0.,
                )?,
                DisplayItem::Text {
                    rect,
                    text,
                    size,
                    color,
                    bold,
                } => {
                    let raster_size = size * scale;
                    let ascent = font()?
                        .horizontal_line_metrics(raster_size)
                        .map_or(raster_size, |line| line.ascent)
                        / scale;
                    let mut pen = rect.x;
                    let mut previous = None;
                    for ch in text.chars() {
                        if let Some(previous) = previous {
                            pen += font()?
                                .horizontal_kern(previous, ch, raster_size)
                                .unwrap_or(0.)
                                / scale;
                        }
                        // Metrics do not allocate bitmap/atlas storage. Hidden
                        // glyphs still affect kerning and the next pen position.
                        let metrics = atlas.metrics(ch, raster_size, *bold)?;
                        let bounds = Rect {
                            x: pen + metrics.xmin / scale,
                            y: rect.y + ascent - (metrics.height as f32 + metrics.ymin) / scale,
                            width: metrics.width as f32 / scale,
                            height: metrics.height as f32 / scale,
                        };
                        geometry(bounds)?;
                        // Only the display-list clip applies here, not the
                        // viewport: the retained scene must survive scrolling.
                        if bounds.width > 0.
                            && bounds.height > 0.
                            && color.3 > 0
                            && scene
                                .clips
                                .last()
                                .is_none_or(|clip| intersection(bounds, *clip).is_some())
                        {
                            let glyph = atlas.glyph(
                                ch,
                                raster_size,
                                *bold,
                                metrics,
                                &mut scene.textures[1].bytes,
                            )?;
                            scene.quad(index, 1, bounds, glyph.uv, *color, 1.)?;
                        }
                        pen += metrics.advance / scale;
                        previous = Some(ch);
                    }
                }
                DisplayItem::Control { .. } => {}
            }
        }
        Ok(scene)
    }
    fn quad(
        &mut self,
        item: usize,
        texture: usize,
        rect: Rect,
        uv: [f32; 4],
        color: Color,
        mode: f32,
    ) -> Result<(), String> {
        if rect.width <= 0. || rect.height <= 0. || color.3 == 0 {
            return Ok(());
        }
        let (rect, uv) = if let Some(clip) = self.clips.last() {
            let Some(cropped) = intersection(rect, *clip) else {
                return Ok(());
            };
            let du = uv[2] - uv[0];
            let dv = uv[3] - uv[1];
            let uv = [
                uv[0] + du * (cropped.x - rect.x) / rect.width,
                uv[1] + dv * (cropped.y - rect.y) / rect.height,
                uv[0] + du * (cropped.x + cropped.width - rect.x) / rect.width,
                uv[1] + dv * (cropped.y + cropped.height - rect.y) / rect.height,
            ];
            (cropped, uv)
        } else {
            (rect, uv)
        };
        let required = self
            .vertex_bytes
            .len()
            .checked_add(6 * VERTEX_STRIDE)
            .ok_or("Graphics vertex storage overflow")?;
        if required > MAX_VERTEX_BYTES {
            return Err("Graphics vertex storage limit exceeded".into());
        }
        self.work_rects.push(rect);
        let start = (self.vertex_bytes.len() / VERTEX_STRIDE) as u32;
        let rgba = [
            color.0 as f32 / 255.,
            color.1 as f32 / 255.,
            color.2 as f32 / 255.,
            color.3 as f32 / 255.,
        ];
        for (x, y, u, v) in [
            (rect.x, rect.y, uv[0], uv[1]),
            (rect.x + rect.width, rect.y, uv[2], uv[1]),
            (rect.x, rect.y + rect.height, uv[0], uv[3]),
            (rect.x, rect.y + rect.height, uv[0], uv[3]),
            (rect.x + rect.width, rect.y, uv[2], uv[1]),
            (rect.x + rect.width, rect.y + rect.height, uv[2], uv[3]),
        ] {
            for value in [x, y, u, v, rgba[0], rgba[1], rgba[2], rgba[3], mode] {
                self.vertex_bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let end = start + 6;
        if let Some(last) = self
            .draws
            .last_mut()
            .filter(|last| last.item == item && last.texture == texture)
        {
            last.vertices.end = end;
            let right = (last.bounds.x + last.bounds.width).max(rect.x + rect.width);
            let bottom = (last.bounds.y + last.bounds.height).max(rect.y + rect.height);
            last.bounds.x = last.bounds.x.min(rect.x);
            last.bounds.y = last.bounds.y.min(rect.y);
            last.bounds.width = right - last.bounds.x;
            last.bounds.height = bottom - last.bounds.y;
        } else {
            self.draws.push(Draw {
                item,
                texture,
                vertices: start..end,
                bounds: rect,
            });
        }
        Ok(())
    }
    /// Bound actual visible overdraw before issuing any scene commands. The
    /// caller checks this once for the complete scene, not once per control run.
    pub fn visible_work(&self, clip: Rect, physical_scale: f32) -> Result<(), String> {
        geometry(clip)?;
        if !physical_scale.is_finite() || !(0.5..=4.).contains(&physical_scale) {
            return Err("Invalid graphics viewport scale".into());
        }
        let mut work =
            f64::from(clip.width) * f64::from(clip.height) * f64::from(physical_scale).powi(2);
        // Count each quad, including coincident glyphs. Merged draw bounds must
        // not hide expensive overdraw in combining marks or overlapping text.
        for bounds in &self.work_rects {
            if let Some(rect) = intersection(*bounds, clip) {
                work += f64::from(rect.width)
                    * f64::from(rect.height)
                    * f64::from(physical_scale).powi(2);
            }
            if work > MAX_VISIBLE_WORK {
                return Err("Graphics visible overdraw budget exceeded".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct GlyphMetrics {
    bitmap_width: usize,
    width: usize,
    height: usize,
    xmin: f32,
    ymin: f32,
    advance: f32,
}
#[derive(Clone, Copy)]
struct Glyph {
    metrics: GlyphMetrics,
    uv: [f32; 4],
}
#[derive(Default)]
struct Atlas {
    glyphs: HashMap<(char, u32, bool), Glyph>,
    x: usize,
    y: usize,
    row: usize,
    work: usize,
}
impl Atlas {
    fn metrics(&self, ch: char, size: f32, bold: bool) -> Result<GlyphMetrics, String> {
        if let Some(glyph) = self.glyphs.get(&(ch, size.to_bits(), bold)) {
            return Ok(glyph.metrics);
        }
        // Reuse metrics for materialized glyphs. Do not grow a second cache
        // with hidden glyph kinds; traversal is bounded by MAX_GLYPHS.
        let metrics = font()?.metrics(ch, size);
        Ok(GlyphMetrics {
            bitmap_width: metrics.width,
            width: metrics.width + usize::from(bold && metrics.width > 0),
            height: metrics.height,
            xmin: metrics.xmin as f32,
            ymin: metrics.ymin as f32,
            advance: metrics.advance_width,
        })
    }
    fn glyph(
        &mut self,
        ch: char,
        size: f32,
        bold: bool,
        metrics: GlyphMetrics,
        pixels: &mut [u8],
    ) -> Result<Glyph, String> {
        let key = (ch, size.to_bits(), bold);
        if let Some(glyph) = self.glyphs.get(&key) {
            return Ok(*glyph);
        }
        if self.glyphs.len() >= MAX_GLYPH_KINDS {
            return Err("Graphics distinct glyph limit exceeded".into());
        }
        let width = metrics.width;
        let height = metrics.height;
        let area = width
            .checked_mul(height)
            .ok_or("Graphics glyph area overflow")?;
        self.work = self
            .work
            .checked_add(area)
            .ok_or("Graphics glyph work overflow")?;
        if self.work > MAX_RASTER_WORK || width + 2 > ATLAS_SIDE || height + 2 > ATLAS_SIDE {
            return Err("Graphics glyph raster budget exceeded".into());
        }
        if self.x + width + 2 > ATLAS_SIDE {
            self.x = 0;
            self.y += self.row;
            self.row = 0;
        }
        if self.y + height + 2 > ATLAS_SIDE {
            return Err("Graphics glyph atlas is full".into());
        }
        let x = self.x + 1;
        let y = self.y + 1;
        if area > 0 {
            let (_, coverage) = font()?.rasterize(ch, size);
            if coverage.len() != metrics.bitmap_width * metrics.height {
                return Err("Invalid graphics glyph bitmap".into());
            }
            for row in 0..height {
                for col in 0..width {
                    let value = if col < metrics.bitmap_width {
                        coverage[row * metrics.bitmap_width + col]
                    } else {
                        0
                    };
                    let previous = if bold && col > 0 {
                        coverage[row * metrics.bitmap_width + col - 1]
                    } else {
                        0
                    };
                    pixels[(y + row) * ATLAS_SIDE + x + col] = value.max(previous);
                }
            }
        }
        let glyph = Glyph {
            metrics,
            uv: [
                x as f32 / ATLAS_SIDE as f32,
                y as f32 / ATLAS_SIDE as f32,
                (x + width) as f32 / ATLAS_SIDE as f32,
                (y + height) as f32 / ATLAS_SIDE as f32,
            ],
        };
        self.x += width + 2;
        self.row = self.row.max(height + 2);
        self.glyphs.insert(key, glyph);
        Ok(glyph)
    }
}

#[cfg(test)]
mod natural_size_tests {
    use super::*;
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
        let page = RenderedPage {
            title: String::new(),
            width: 1.,
            height: 1.,
            background: Color::default(),
            items: vec![],
            links: vec![],
            metrics: Default::default(),
            warnings: vec![],
            next_timer_ms: None,
            animation_frame_pending: false,
        };
        let scene = Scene::prepare(&page, std::slice::from_ref(&image), 1.).unwrap();
        let texture = scene.textures.last().unwrap();
        assert_eq!((texture.width, texture.height), (1, 1));
        assert_eq!(texture.bytes, image.rgba);
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
            assert!(Scene::prepare(&page, std::slice::from_ref(&image), 1.).is_err());
        }
    }
}
#[cfg(test)]
mod glyph_clip_tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
    fn page(items: Vec<DisplayItem>) -> RenderedPage {
        RenderedPage {
            title: String::new(),
            width: 100.,
            height: 100.,
            background: Color::default(),
            items,
            links: vec![],
            metrics: Default::default(),
            warnings: vec![],
            next_timer_ms: None,
            animation_frame_pending: false,
        }
    }
    fn text(value: &str, x: f32, y: f32, size: f32, bold: bool) -> DisplayItem {
        DisplayItem::Text {
            // Deliberately not ink bounds: glyphs can extend beyond this box.
            rect: rect(x, y, 0., 0.),
            text: value.into(),
            size,
            bold,
            color: Color(20, 30, 40, 255),
        }
    }
    fn clipped(items: Vec<DisplayItem>, clip: Rect) -> RenderedPage {
        let mut result = vec![DisplayItem::PushClip { rect: clip }];
        result.extend(items);
        result.push(DisplayItem::PopClip);
        page(result)
    }
    fn empty(scene: &Scene) {
        assert!(scene.draws.is_empty());
        assert!(scene.vertex_bytes.is_empty());
        assert!(scene.work_rects.is_empty());
        assert!(scene.textures[1].bytes.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn empty_clip_never_materializes_glyphs_and_pop_restores_paint() {
        let hidden = text("AV large hidden glyphs", 0., 0., 256., true);
        let scene = Scene::prepare(
            &clipped(vec![hidden.clone()], rect(0., 0., 0., 100.)),
            &[],
            2.,
        )
        .unwrap();
        empty(&scene);
        let mut restored = clipped(vec![hidden], rect(0., 0., 0., 100.));
        restored.items.push(text("A", 10., 10., 32., false));
        let scene = Scene::prepare(&restored, &[], 1.).unwrap();
        assert_eq!(scene.draws.len(), 1);
        assert_eq!(scene.draws[0].item, 3);
    }

    #[test]
    fn hidden_large_kinds_do_not_spend_atlas_but_visible_kinds_do() {
        // Distinct sizes prevent reuse; input counts remain far below their caps.
        let items: Vec<_> = (0..5000)
            .map(|n| text("W", 0., 0., 200. + n as f32 / 100., true))
            .collect();
        let hidden = clipped(items.clone(), rect(0., 0., 0., 0.));
        empty(&Scene::prepare(&hidden, &[], 2.).unwrap());
        let error = Scene::prepare(&page(items), &[], 2.).err().unwrap();
        assert!(
            error.contains("atlas")
                || error.contains("raster budget")
                || error.contains("distinct glyph")
        );
    }

    #[test]
    fn hidden_input_and_derived_geometry_are_still_validated() {
        for item in [
            text("A", 0., 0., f32::NAN, false),
            text("A", f32::INFINITY, 0., 32., false),
            text("A", 0., 0., 257., false),
            text(&"A".repeat(MAX_GLYPHS + 1), 0., 0., 1., false),
            DisplayItem::Image {
                rect: rect(0., 0., 1., 1.),
                asset_id: "missing".into(),
            },
            // The item is valid but advancing the pen makes ink coordinates invalid.
            text("WW", 1_000_000., 0., 256., false),
        ] {
            assert!(Scene::prepare(&clipped(vec![item], rect(0., 0., 0., 0.)), &[], 1.).is_err());
        }
        assert!(Scene::prepare(&clipped(vec![], rect(0., 0., 0., 0.)), &[], f32::NAN).is_err());
        assert!(Scene::prepare(&page(vec![DisplayItem::PopClip]), &[], 1.).is_err());
        assert!(
            Scene::prepare(
                &page(vec![
                    DisplayItem::PushClip {
                        rect: rect(0., 0., 0., 0.)
                    };
                    129
                ]),
                &[],
                1.
            )
            .is_err()
        );
    }

    #[test]
    fn hidden_prefix_preserves_advance_kerning_and_visible_bitmap() {
        let size = 32.;
        let font = font().unwrap();
        let a = font.metrics('A', size);
        let v = font.metrics('V', size);
        let pen = a.advance_width + font.horizontal_kern('A', 'V', size).unwrap_or(0.);
        let left = (a.xmin as f32 + a.width as f32).max(pen + v.xmin as f32);
        let clip = rect(left, 0., 100., 100.);
        let pair = Scene::prepare(
            &clipped(vec![text("AV", 0., 0., size, false)], clip),
            &[],
            1.,
        )
        .unwrap();
        let suffix = Scene::prepare(
            &clipped(vec![text("V", pen, 0., size, false)], clip),
            &[],
            1.,
        )
        .unwrap();
        assert!(!pair.draws.is_empty());
        assert_eq!(pair.vertex_bytes, suffix.vertex_bytes);
        assert_eq!(pair.textures[1].bytes, suffix.textures[1].bytes);
    }

    #[test]
    fn synthetic_bold_extra_column_intersects_clip_at_scaled_size() {
        let scale = 2.;
        let metrics = font().unwrap().metrics('W', 32. * scale);
        let clip = rect(
            (metrics.xmin as f32 + metrics.width as f32) / scale,
            0.,
            0.5,
            100.,
        );
        let plain = Scene::prepare(
            &clipped(vec![text("W", 0., 0., 32., false)], clip),
            &[],
            scale,
        )
        .unwrap();
        empty(&plain);
        let bold = Scene::prepare(
            &clipped(vec![text("W", 0., 0., 32., true)], clip),
            &[],
            scale,
        )
        .unwrap();
        assert_eq!(bold.draws.len(), 1);
        assert_eq!(bold.draws[0].bounds.width, 0.5);
        assert!(bold.textures[1].bytes.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn nested_disjoint_clip_stays_empty_until_pop() {
        let scene = Scene::prepare(
            &page(vec![
                DisplayItem::PushClip {
                    rect: rect(0., 0., 100., 100.),
                },
                DisplayItem::PushClip {
                    rect: rect(200., 200., 100., 100.),
                },
                text("W", 0., 0., 256., true),
                DisplayItem::PushClip {
                    rect: rect(0., 0., 100., 100.),
                },
                text("W", 0., 0., 255., true),
                DisplayItem::PopClip,
                DisplayItem::PopClip,
                text("A", 10., 10., 32., false),
                DisplayItem::PopClip,
            ]),
            &[],
            1.,
        )
        .unwrap();
        let reference =
            Scene::prepare(&page(vec![text("A", 10., 10., 32., false)]), &[], 1.).unwrap();
        assert_eq!(scene.draws.len(), 1);
        assert_eq!(scene.draws[0].item, 7);
        assert_eq!(scene.vertex_bytes, reference.vertex_bytes);
        assert_eq!(scene.textures[1].bytes, reference.textures[1].bytes);
    }

    #[test]
    fn unclipped_text_below_viewport_is_retained() {
        let scene = Scene::prepare(&page(vec![text("A", 0., 5000., 32., false)]), &[], 1.).unwrap();
        assert_eq!(scene.draws.len(), 1);
        assert!(scene.draws[0].bounds.y >= 5000.);
        assert!(scene.textures[1].bytes.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn fully_visible_clip_preserves_geometry_order_and_images() {
        let image = ImageAsset {
            id: "a".into(),
            width: 1,
            height: 1,
            natural_width: 1.,
            natural_height: 1.,
            rgba: vec![1, 2, 3, 255],
        };
        let items = vec![
            DisplayItem::Rect {
                rect: rect(0., 0., 100., 100.),
                color: Color(1, 2, 3, 255),
            },
            text("AVA", 10., 10., 32., true),
            DisplayItem::Image {
                rect: rect(20., 20., 1., 1.),
                asset_id: "a".into(),
            },
            text("AV", 10., 50., 32., true),
        ];
        let plain = Scene::prepare(&page(items.clone()), std::slice::from_ref(&image), 1.).unwrap();
        let clipped = Scene::prepare(
            &clipped(items, rect(0., 0., 1000., 1000.)),
            std::slice::from_ref(&image),
            1.,
        )
        .unwrap();
        assert_eq!(plain.vertex_bytes, clipped.vertex_bytes);
        assert_eq!(plain.draws.len(), 4);
        for (a, b) in plain.draws.iter().zip(&clipped.draws) {
            assert_eq!(a.item + 1, b.item);
            assert_eq!(a.texture, b.texture);
            assert_eq!(a.vertices, b.vertices);
        }
        for (a, b) in plain.textures.iter().zip(&clipped.textures) {
            assert_eq!(a.bytes, b.bytes);
        }
        assert_eq!(plain.textures[2].bytes, image.rgba);
    }
}
