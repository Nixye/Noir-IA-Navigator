//! Block and inline formatting for the static document subset.
//! Font advances use the same embedded Ubuntu font as the native/CPU painters.
use browser_ipc::{
    Color, ControlAppearance, ControlKind, ControlOption, DisplayItem, FormInfo, ImageAsset, Link,
    Rect, Viewport,
};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use web_css::{Alignment, Display, GridTrack, Length, Position, Style};
use web_dom::{Document, NodeId, NodeKind};

const MAX_ITEMS: usize = 40_000;
const MAX_EXTENT: f32 = 1_000_000.0;
const MAX_CLIP_DEPTH: usize = 128;
const MAX_LINK_CLIPS: usize = MAX_ITEMS / 2;
const MAX_LINK_CLIP_WORK: usize = 2_000_000;

fn resolve_height(length: Length, reference: Option<f32>) -> Option<f32> {
    match length {
        Length::Auto => None,
        Length::Px(n) => Some(n),
        Length::Percent(n) => reference.map(|height| height * n / 100.),
    }
}

// Missing resources use a synthetic 160:48 ratio, never texture dimensions.
fn image_dimensions(
    asset: Option<&ImageAsset>,
    width: Option<f32>,
    height: Option<f32>,
    max_width: Option<f32>,
) -> (f32, f32) {
    let (natural_width, natural_height) = asset
        .map(|asset| (asset.natural_width, asset.natural_height))
        .unwrap_or((160., 48.));
    // f64 avoids overflow for valid, extremely thin intrinsic dimensions.
    let ratio = f64::from(natural_width) / f64::from(natural_height);
    let mut width = width
        .map(f64::from)
        .unwrap_or_else(|| {
            height.map_or(f64::from(natural_width), |height| f64::from(height) * ratio)
        })
        .clamp(0., 16384.) as f32;
    if let Some(max) = max_width {
        width = width.min(max.max(0.));
    }
    // Width constraints scale only an automatic height.
    let height = height
        .map(f64::from)
        .unwrap_or(f64::from(width) / ratio)
        .clamp(0., 16384.) as f32;
    (width, height)
}
fn image_attribute(value: Option<&str>) -> Option<f32> {
    value
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0. && *value <= 16384.)
}

fn alignment_offset(alignment: Alignment, spare: f32) -> f32 {
    match alignment {
        Alignment::Center => spare / 2.,
        Alignment::End => spare,
        _ => 0.,
    }
}
fn distribution(alignment: Alignment, spare: f32, count: usize) -> (f32, f32) {
    match alignment {
        Alignment::SpaceBetween if count > 1 => (0., spare / (count - 1) as f32),
        Alignment::SpaceAround if count > 0 => (spare / (2 * count) as f32, spare / count as f32),
        Alignment::SpaceEvenly if count > 0 => {
            (spare / (count + 1) as f32, spare / (count + 1) as f32)
        }
        _ => (alignment_offset(alignment, spare), 0.),
    }
}

fn font() -> &'static fontdue::Font {
    static FONT: OnceLock<fontdue::Font> = OnceLock::new();
    FONT.get_or_init(|| {
        fontdue::Font::from_bytes(
            epaint_default_fonts::UBUNTU_LIGHT,
            fontdue::FontSettings::default(),
        )
        .expect("bundled font is valid")
    })
}

pub fn measure_text(text: &str, size: f32) -> f32 {
    let size = if size.is_finite() {
        size.clamp(6.0, 256.0)
    } else {
        16.0
    };
    let mut previous = None;
    let mut width = 0.0;
    for ch in text.chars() {
        if let Some(prev) = previous {
            width += font().horizontal_kern(prev, ch, size).unwrap_or(0.0);
        }
        width += font().metrics(ch, size).advance_width;
        previous = Some(ch);
    }
    width
}

// Only ASCII CSS whitespace is collapsed. Typographic Unicode spaces remain
// in runs; this is not Unicode line breaking, shaping or grapheme segmentation.
#[derive(Debug, PartialEq)]
enum TextToken<'a> {
    Run(&'a str),
    Spaces(usize),
    Break,
}
struct TextTokens<'a> {
    text: &'a str,
    offset: usize,
    preserve_breaks: bool,
}
fn text_tokens<'a>(text: &'a str, style: &Style) -> TextTokens<'a> {
    TextTokens {
        text,
        offset: 0,
        preserve_breaks: style.preserve_breaks,
    }
}
fn ascii_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}
impl<'a> Iterator for TextTokens<'a> {
    type Item = TextToken<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.text.as_bytes();
        let start = self.offset;
        let first = *bytes.get(start)?;
        if self.preserve_breaks && matches!(first, b'\r' | b'\n') {
            self.offset += 1;
            if first == b'\r' && bytes.get(self.offset) == Some(&b'\n') {
                self.offset += 1;
            }
            return Some(TextToken::Break);
        }
        let spaces = ascii_space(first);
        self.offset += 1;
        while let Some(&byte) = bytes.get(self.offset) {
            if ascii_space(byte) != spaces
                || (self.preserve_breaks && matches!(byte, b'\r' | b'\n'))
            {
                break;
            }
            self.offset += 1;
        }
        // ASCII separators cannot occur inside a UTF-8 scalar, so both run
        // boundaries are valid, without repeatedly scanning string prefixes.
        Some(if spaces {
            TextToken::Spaces(self.offset - start)
        } else {
            TextToken::Run(&self.text[start..self.offset])
        })
    }
}
// Per-node preferred width only. Sibling intrinsic aggregation still sums
// scalars and does not normalize whitespace across node/style boundaries.
fn preferred_text_width(text: &str, style: &Style) -> f32 {
    let mut line = 0f32;
    let mut widest = 0f32;
    let mut pending = false;
    let space = measure_text(" ", style.font_size);
    for token in text_tokens(text, style) {
        match token {
            TextToken::Run(run) => {
                if pending && line > 0. {
                    line += space;
                }
                pending = false;
                line += measure_text(run, style.font_size);
            }
            TextToken::Spaces(count) if style.pre => line += space * count as f32,
            TextToken::Spaces(_) => pending = true,
            TextToken::Break => {
                widest = widest.max(line);
                line = 0.;
                pending = false;
            }
        }
    }
    widest.max(line).min(16384.)
}

pub struct LayoutResult {
    pub items: Vec<DisplayItem>,
    pub links: Vec<Link>,
    pub width: f32,
    pub height: f32,
    pub background: Color,
    pub warnings: Vec<String>,
    /// Dimensions of actual principal boxes, before viewport clipping. Inline
    /// non-replaced elements have no client box in the supported CSS subset.
    pub boxes: Vec<Option<BoxMetrics>>,
}

#[derive(Clone, Copy, Debug)]
pub struct BoxMetrics {
    pub width: f32,
    pub height: f32,
    pub border: f32,
}

pub fn layout(
    document: &Document,
    styles: &[Style],
    images: &[ImageAsset],
    viewport: Viewport,
) -> Result<LayoutResult, String> {
    if !viewport.width.is_finite()
        || !viewport.height.is_finite()
        || viewport.width < 1.0
        || viewport.width > 16384.0
        || viewport.height < 1.0
        || viewport.height > 16384.0
    {
        return Err("Viewport must be finite and between 1 and 16384 CSS pixels".into());
    }
    if styles.len() != document.nodes.len() || document.nodes.len() > web_dom::MAX_NODES {
        return Err("DOM/style tree mismatch".into());
    }
    let mut layout = Layout {
        document,
        styles,
        images,
        items: Vec::new(),
        links: Vec::new(),
        link_clips: Vec::new(),
        owned_clips: BTreeMap::new(),
        clip_depth: 0,
        warnings: Vec::new(),
        max_x: viewport.width,
        measure_budget: 4_000_000,
        positioned: vec![PositionFrame::default()],
        position_budget: document.nodes.len(),
        height_context: vec![Some(viewport.height)],
        control_count: 0,
        boxes: vec![None; document.nodes.len()],
    };
    let mut flow = Flow::new(0.0, 0.0, viewport.width);
    layout.children(0, &mut flow, None)?;
    layout.flush(&mut flow);
    let frame = layout.positioned.pop().expect("root positioning frame");
    layout.paint_positioned(
        frame,
        Rect {
            x: 0.,
            y: 0.,
            width: viewport.width,
            height: viewport.height,
        },
    )?;
    let mut background = Color(255, 255, 255, 255);
    for (id, e) in document.elements() {
        if matches!(e.tag.as_str(), "html" | "body") && styles[id].background.3 > 0 {
            background = styles[id].background;
        }
    }
    // Include visible overflow and alignment offsets, not only normal-flow
    // cursors. Explicit short heights must not make descendants unreachable.
    let mut width = viewport.width;
    let mut height = flow.y.max(viewport.height);
    let mut clips: Vec<Rect> = Vec::new();
    for item in &mut layout.items {
        let rect = match item {
            DisplayItem::PushClip { rect } => {
                bound_rect(rect)?;
                if clips.len() >= MAX_CLIP_DEPTH {
                    return Err("Layout clip nesting exceeded".into());
                }
                let effective = clips
                    .last()
                    .map_or(*rect, |parent| intersect_rect(*parent, *rect));
                clips.push(effective);
                continue;
            }
            DisplayItem::PopClip => {
                clips.pop().ok_or("Unbalanced layout clip stack")?;
                continue;
            }
            DisplayItem::Rect { rect, .. }
            | DisplayItem::Text { rect, .. }
            | DisplayItem::Image { rect, .. }
            | DisplayItem::Control { rect, .. } => rect,
        };
        bound_rect(rect)?;
        let visible = clips
            .last()
            .map_or(*rect, |clip| intersect_rect(*clip, *rect));
        if visible.width > 0. && visible.height > 0. {
            width = width.max(visible.x + visible.width);
            height = height.max(visible.y + visible.height);
        }
    }
    if !clips.is_empty() {
        return Err("Unbalanced layout clip stack".into());
    }
    for link in &mut layout.links {
        bound_rect(&mut link.rect)?;
    }
    // Geometry and clip dimensions are final only after flex/grid stretching,
    // relative offsets and positioned-child translations have completed.
    // Preflight every recorded range before applying any hit-target clipping.
    let mut clip_work = MAX_LINK_CLIP_WORK;
    for record in &layout.link_clips {
        if record.links.start > record.links.end || record.links.end > layout.links.len() {
            return Err("Invalid layout link clip range".into());
        }
        if !matches!(
            layout.items.get(record.item),
            Some(DisplayItem::PushClip { .. })
        ) {
            return Err("Invalid layout link clip item".into());
        }
        clip_work = clip_work
            .checked_sub(record.links.len())
            .ok_or("Layout link clipping work limit exceeded")?;
    }
    for record in &layout.link_clips {
        let Some(DisplayItem::PushClip { rect }) = layout.items.get(record.item) else {
            return Err("Invalid layout link clip item".into());
        };
        for link in &mut layout.links[record.links.clone()] {
            link.rect = intersect_rect(link.rect, *rect);
        }
    }
    layout
        .links
        .retain(|link| link.rect.width > 0. && link.rect.height > 0.);
    Ok(LayoutResult {
        items: layout.items,
        links: layout.links,
        width: width.min(MAX_EXTENT),
        height: height.min(MAX_EXTENT),
        background,
        warnings: layout.warnings,
        boxes: layout.boxes,
    })
}

fn bound_rect(rect: &mut Rect) -> Result<(), String> {
    if ![rect.x, rect.y, rect.width, rect.height]
        .iter()
        .all(|n| n.is_finite())
    {
        return Err("Layout produced non-finite geometry".into());
    }
    rect.x = rect.x.clamp(-MAX_EXTENT, MAX_EXTENT);
    rect.y = rect.y.clamp(-MAX_EXTENT, MAX_EXTENT);
    rect.width = rect.width.clamp(0., MAX_EXTENT);
    rect.height = rect.height.clamp(0., MAX_EXTENT);
    Ok(())
}
fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rect {
        x,
        y,
        width: ((a.x + a.width).min(b.x + b.width) - x).max(0.),
        height: ((a.y + a.height).min(b.y + b.height) - y).max(0.),
    }
}

struct Flow {
    left: f32,
    width: f32,
    x: f32,
    y: f32,
    line_height: f32,
    item_start: usize,
    link_start: usize,
    space: bool,
    align: Alignment,
}
impl Flow {
    fn new(x: f32, y: f32, width: f32) -> Self {
        Self {
            left: x,
            width: width.max(1.0),
            x,
            y,
            line_height: 0.0,
            item_start: 0,
            link_start: 0,
            space: false,
            align: Alignment::Start,
        }
    }
}
struct Positioned {
    node: NodeId,
    link: Option<String>,
    static_x: f32,
    static_y: f32,
}
#[derive(Default)]
struct PositionFrame {
    children: Vec<Positioned>,
}
struct LinkClip {
    item: usize,
    links: std::ops::Range<usize>,
}
struct Layout<'a> {
    document: &'a Document,
    styles: &'a [Style],
    images: &'a [ImageAsset],
    items: Vec<DisplayItem>,
    links: Vec<Link>,
    link_clips: Vec<LinkClip>,
    owned_clips: BTreeMap<NodeId, Vec<usize>>,
    clip_depth: usize,
    warnings: Vec<String>,
    max_x: f32,
    measure_budget: usize,
    positioned: Vec<PositionFrame>,
    position_budget: usize,
    height_context: Vec<Option<f32>>,
    control_count: usize,
    boxes: Vec<Option<BoxMetrics>>,
}
impl<'a> Layout<'a> {
    fn defer_positioned(
        &mut self,
        id: NodeId,
        flow: &Flow,
        link: Option<&str>,
    ) -> Result<(), String> {
        let own_link = self
            .document
            .element(id)
            .filter(|element| element.tag == "a")
            .and_then(|element| element.attr("href"));
        self.position_budget = self
            .position_budget
            .checked_sub(1)
            .ok_or("Positioned element work limit exceeded")?;
        self.positioned
            .last_mut()
            .ok_or("Missing containing block")?
            .children
            .push(Positioned {
                node: id,
                link: own_link.or(link).map(str::to_owned),
                static_x: flow.x,
                static_y: flow.y,
            });
        Ok(())
    }
    fn element_rect(&self, id: NodeId, start: usize, fallback: Rect) -> Rect {
        if (self.styles[id].display != Display::Inline
            || self.styles[id].position == Position::Absolute)
            && let Some(DisplayItem::Rect { rect, .. }) = self.items.get(start)
        {
            return *rect;
        }
        let mut bounds: Option<Rect> = None;
        for item in &self.items[start..] {
            let rect = match item {
                DisplayItem::PushClip { .. } | DisplayItem::PopClip => continue,
                DisplayItem::Rect { rect, .. }
                | DisplayItem::Text { rect, .. }
                | DisplayItem::Image { rect, .. }
                | DisplayItem::Control { rect, .. } => *rect,
            };
            bounds = Some(if let Some(old) = bounds {
                let x = old.x.min(rect.x);
                let y = old.y.min(rect.y);
                Rect {
                    x,
                    y,
                    width: (old.x + old.width).max(rect.x + rect.width) - x,
                    height: (old.y + old.height).max(rect.y + rect.height) - y,
                }
            } else {
                rect
            });
        }
        bounds.unwrap_or(fallback)
    }
    fn padding_rect(&self, id: NodeId, rect: Rect) -> Rect {
        let border = self.styles[id].border_width.clamp(0., 1000.);
        Rect {
            x: rect.x + border,
            y: rect.y + border,
            width: (rect.width - 2. * border).max(0.),
            height: (rect.height - 2. * border).max(0.),
        }
    }
    fn finish_relative(
        &mut self,
        id: NodeId,
        items: usize,
        links: usize,
        fallback: Rect,
        parent_width: f32,
    ) -> Result<(), String> {
        let mut frame = self
            .positioned
            .pop()
            .ok_or("Missing relative containing block")?;
        let style = &self.styles[id];
        let dx = style.inset[3]
            .resolve(parent_width)
            .or_else(|| style.inset[1].resolve(parent_width).map(|n| -n))
            .unwrap_or(0.);
        // Percentage vertical relative offsets require a definite parent height.
        let parent_height = self.height_context.last().copied().flatten();
        let dy = resolve_height(style.inset[0], parent_height)
            .or_else(|| resolve_height(style.inset[2], parent_height).map(|n| -n))
            .unwrap_or(0.);
        let mut rect = self.element_rect(id, items, fallback);
        self.translate(items..self.items.len(), links..self.links.len(), dx, dy);
        rect.x += dx;
        rect.y += dy;
        for child in &mut frame.children {
            child.static_x += dx;
            child.static_y += dy;
        }
        self.paint_positioned_clipped(id, frame, rect)
    }
    fn paint_positioned_clipped(
        &mut self,
        id: NodeId,
        frame: PositionFrame,
        rect: Rect,
    ) -> Result<(), String> {
        let containing = self.padding_rect(id, rect);
        let clipped = self.styles[id].overflow_clip;
        let links = self.links.len();
        let clip_index = self.items.len();
        if clipped {
            self.push(DisplayItem::PushClip { rect: containing })?;
        }
        self.paint_positioned(frame, containing)?;
        if clipped {
            self.push(DisplayItem::PopClip)?;
            self.record_link_clip(id, clip_index, links)?;
        }
        Ok(())
    }
    fn paint_positioned(&mut self, frame: PositionFrame, containing: Rect) -> Result<(), String> {
        for child in frame.children {
            let id = child.node;
            let style = self.styles[id].clone();
            let left = style.inset[3].resolve(containing.width);
            let right = style.inset[1].resolve(containing.width);
            let top = style.inset[0].resolve(containing.height);
            let bottom = style.inset[2].resolve(containing.height);
            let margins = style.margin[1] + style.margin[3];
            let extra = if style.border_box {
                0.
            } else {
                style.padding[1] + style.padding[3] + 2. * style.border_width
            };
            let width = if let Some(width) = style.width.resolve(containing.width) {
                width + margins + extra
            } else if let (Some(left), Some(right)) = (left, right) {
                containing.width - left - right
            } else {
                self.intrinsic_width(id)?.min(containing.width)
            }
            .clamp(1., 16384.);
            let x = if let Some(left) = left {
                containing.x + left
            } else if let Some(right) = right {
                containing.x + containing.width - right - width
            } else {
                child.static_x
            };
            let y = top.map_or(child.static_y, |top| containing.y + top);
            let items = self.items.len();
            let links = self.links.len();
            self.positioned.push(PositionFrame::default());
            self.height_context.push(Some(containing.height));
            let height = self.box_child(id, x, y, width, child.link.as_deref())?;
            self.height_context.pop();
            let nested = self
                .positioned
                .pop()
                .ok_or("Missing absolute containing block")?;
            let rect = self.element_rect(
                id,
                items,
                Rect {
                    x,
                    y,
                    width,
                    height,
                },
            );
            self.paint_positioned_clipped(id, nested, rect)?;
            let dx = if left.is_none() {
                right.map_or(0., |right| {
                    containing.x + containing.width - right - rect.width - margins - x
                })
            } else {
                0.
            };
            let dy = if top.is_none() {
                bottom.map_or(0., |bottom| {
                    containing.y + containing.height - bottom - height - y
                })
            } else {
                0.
            };
            self.translate(items..self.items.len(), links..self.links.len(), dx, dy);
        }
        Ok(())
    }
    fn push(&mut self, item: DisplayItem) -> Result<(), String> {
        if self.items.len() >= MAX_ITEMS {
            return Err("Display list item limit exceeded".into());
        }
        match &item {
            DisplayItem::PushClip { .. } => {
                if self.clip_depth >= MAX_CLIP_DEPTH {
                    return Err("Layout clip nesting exceeded".into());
                }
                self.clip_depth += 1;
            }
            DisplayItem::PopClip => {
                self.clip_depth = self
                    .clip_depth
                    .checked_sub(1)
                    .ok_or("Unbalanced layout clip stack")?;
            }
            _ => {}
        }
        self.items.push(item);
        Ok(())
    }
    fn record_link_clip(&mut self, owner: NodeId, item: usize, start: usize) -> Result<(), String> {
        if self.link_clips.len() >= MAX_LINK_CLIPS {
            return Err("Layout link clip record limit exceeded".into());
        }
        if start > self.links.len() {
            return Err("Invalid layout link clip range".into());
        }
        self.link_clips.push(LinkClip {
            item,
            links: start..self.links.len(),
        });
        // Every index is also one bounded link-clip record, including empty
        // ranges. This bounds both owner entries and stored indices without
        // allocating a table proportional to an unchecked NodeId.
        self.owned_clips.entry(owner).or_default().push(item);
        Ok(())
    }
    fn flush(&mut self, flow: &mut Flow) {
        if matches!(flow.align, Alignment::Center | Alignment::End) && flow.x > flow.left {
            let delta = alignment_offset(flow.align, (flow.width - (flow.x - flow.left)).max(0.0));
            for item in &mut self.items[flow.item_start..] {
                match item {
                    DisplayItem::PushClip { rect } => rect.x += delta,
                    DisplayItem::PopClip => {}
                    DisplayItem::Rect { rect, .. }
                    | DisplayItem::Text { rect, .. }
                    | DisplayItem::Image { rect, .. }
                    | DisplayItem::Control { rect, .. } => rect.x += delta,
                }
            }
            for link in &mut self.links[flow.link_start..] {
                link.rect.x += delta;
            }
        }
        flow.y = (flow.y + flow.line_height).min(MAX_EXTENT);
        flow.x = flow.left;
        flow.line_height = 0.0;
        flow.space = false;
        flow.item_start = self.items.len();
        flow.link_start = self.links.len();
    }
    fn children(&mut self, id: NodeId, flow: &mut Flow, link: Option<&str>) -> Result<(), String> {
        let children = self.document.nodes[id].children.clone();
        for child in children {
            self.node(child, flow, link)?;
        }
        Ok(())
    }
    fn node(
        &mut self,
        id: NodeId,
        flow: &mut Flow,
        inherited_link: Option<&str>,
    ) -> Result<(), String> {
        if self.document.is_inactive_subtree(id) {
            return Ok(());
        }
        if self.document.element(id).is_some() && self.styles[id].display != Display::None {
            if self.styles[id].position == Position::Absolute {
                return self.defer_positioned(id, flow, inherited_link);
            }
            if self.styles[id].position == Position::Relative {
                let items = self.items.len();
                let links = self.links.len();
                let fallback = Rect {
                    x: flow.x,
                    y: flow.y,
                    width: flow.width,
                    height: 0.,
                };
                self.positioned.push(PositionFrame::default());
                self.node_in_flow(id, flow, inherited_link)?;
                return self.finish_relative(id, items, links, fallback, flow.width);
            }
        }
        self.node_in_flow(id, flow, inherited_link)
    }
    fn node_in_flow(
        &mut self,
        id: NodeId,
        flow: &mut Flow,
        inherited_link: Option<&str>,
    ) -> Result<(), String> {
        let style = &self.styles[id];
        if style.display == Display::None {
            return Ok(());
        }
        match &self.document.nodes[id].kind {
            NodeKind::Text(text) => {
                self.measure_budget = self
                    .measure_budget
                    .checked_sub(text.len())
                    .ok_or("Inline text copy work limit exceeded")?;
                let text = text.clone();
                let style = style.clone();
                self.text(&text, &style, flow, inherited_link)
            }
            NodeKind::Document => self.children(id, flow, inherited_link),
            NodeKind::Element(element) => {
                let href = if element.tag == "a" {
                    element.attr("href")
                } else {
                    None
                };
                let link = href.or(inherited_link).map(str::to_string);
                if matches!(
                    element.tag.as_str(),
                    "input" | "button" | "textarea" | "select"
                ) {
                    let block = style.display == Display::Block;
                    if block {
                        self.flush(flow);
                    }
                    self.control(id, flow, false)?;
                    if block {
                        self.flush(flow);
                    }
                    return Ok(());
                }
                if matches!(
                    style.display,
                    Display::Block | Display::Flex | Display::Grid
                ) {
                    self.flush(flow);
                    let height =
                        self.block(id, flow.left, flow.y, flow.width, link.as_deref(), false)?;
                    flow.y = (flow.y + height).min(MAX_EXTENT);
                    flow.item_start = self.items.len();
                    flow.link_start = self.links.len();
                    return Ok(());
                }
                if style.display == Display::InlineBlock {
                    let minimum = if element.tag == "img" { 0. } else { 1. };
                    let width = self.intrinsic_width(id)?.min(flow.width).max(minimum);
                    let space = if flow.space && flow.x > flow.left {
                        measure_text(" ", style.font_size)
                    } else {
                        0.
                    };
                    if !style.nowrap
                        && flow.x > flow.left
                        && flow.x + space + width > flow.left + flow.width
                    {
                        self.flush(flow);
                    } else {
                        flow.x += space;
                    }
                    flow.space = false;
                    let height = self.block(id, flow.x, flow.y, width, link.as_deref(), false)?;
                    flow.x += width;
                    flow.line_height = flow.line_height.max(height);
                    return Ok(());
                }
                if element.tag == "br" {
                    flow.line_height = flow.line_height.max(style.font_size * style.line_height);
                    self.flush(flow);
                    return Ok(());
                }
                if element.tag == "img" {
                    return self.image(id, flow, link.as_deref(), false);
                }
                self.children(id, flow, link.as_deref())
            }
        }
    }
    fn block(
        &mut self,
        id: NodeId,
        x: f32,
        y: f32,
        available: f32,
        link: Option<&str>,
        allocated: bool,
    ) -> Result<f32, String> {
        let style = self.styles[id].clone();
        let [mt, mr, mb, ml] = style.margin;
        let [pt, pr, pb, pl] = style.padding;
        let border = style.border_width.min(1000.0);
        let horizontal = pl + pr + border * 2.0;
        let is_image = self
            .document
            .element(id)
            .is_some_and(|element| element.tag == "img");
        let available_content = (available - ml - mr - horizontal).max(1.0);
        let image_width = if !allocated && is_image {
            let asset = self.image_asset(id)?;
            let element = self.document.element(id).expect("image element");
            let (width, _) = image_dimensions(
                asset,
                style
                    .width
                    .resolve(available)
                    .map(|width| {
                        if style.border_box {
                            (width - horizontal).max(0.)
                        } else {
                            width
                        }
                    })
                    .or_else(|| image_attribute(element.attr("width"))),
                resolve_height(style.height, self.height_context.last().copied().flatten())
                    .map(|height| {
                        if style.border_box {
                            (height - pt - pb - 2. * border).max(0.)
                        } else {
                            height
                        }
                    })
                    .or_else(|| image_attribute(element.attr("height"))),
                None,
            );
            Some(width)
        } else {
            None
        };
        let mut content_width = (if allocated { Length::Auto } else { style.width })
            .resolve(available)
            .map_or(image_width.unwrap_or(available_content), |n| {
                if style.border_box { n - horizontal } else { n }
            });
        if let Some(max) = style.max_width.resolve(available) {
            content_width = content_width.min(if style.border_box {
                max - horizontal
            } else {
                max
            });
        }
        if let Some(min) = style.min_width.resolve(available) {
            content_width = content_width.max(if style.border_box {
                min - horizontal
            } else {
                min
            });
        }
        content_width = content_width.clamp(if is_image { 0. } else { 1. }, 16384.0);
        let box_width = content_width + horizontal;
        let spare = (available - box_width - ml - mr).max(0.0);
        let left = if style.auto_margin[3] && style.auto_margin[1] {
            ml + spare / 2.0
        } else if style.auto_margin[3] {
            ml + spare
        } else {
            ml
        };
        let bx = (x + left).clamp(-MAX_EXTENT, MAX_EXTENT);
        let by = (y + mt).clamp(-MAX_EXTENT, MAX_EXTENT);
        let background_index = self.items.len();
        // Background placeholder precedes descendants in paint order.
        self.push(DisplayItem::Rect {
            rect: Rect {
                x: bx,
                y: by,
                width: box_width,
                height: 0.0,
            },
            color: if style.visible {
                style.background
            } else {
                Color(0, 0, 0, 0)
            },
        })?;
        let border_start = self.items.len();
        if border > 0.0 {
            for _ in 0..4 {
                self.push(DisplayItem::Rect {
                    rect: Rect::default(),
                    color: if style.visible {
                        style.border_color
                    } else {
                        Color(0, 0, 0, 0)
                    },
                })?;
            }
        }
        let clip_index = if style.overflow_clip {
            let index = self.items.len();
            self.push(DisplayItem::PushClip {
                rect: Rect {
                    x: bx + border,
                    y: by + border,
                    width: (box_width - 2. * border).max(0.),
                    height: 0.,
                },
            })?;
            Some(index)
        } else {
            None
        };
        let content_links = self.links.len();
        let mut inner = Flow::new(bx + border + pl, by + border + pt, content_width);
        if is_image {
            inner.width = content_width;
        }
        inner.item_start = self.items.len();
        inner.link_start = self.links.len();
        inner.align = style.text_align;
        let vertical = pt + pb + 2. * border;
        let content_height = |height: f32| {
            if style.border_box {
                (height - vertical).max(0.)
            } else {
                height
            }
        };
        let definite = resolve_height(style.height, self.height_context.last().copied().flatten())
            .map(content_height);
        self.height_context.push(definite);
        if style.display == Display::Block
            && self.document.element(id).is_some_and(|e| e.tag == "table")
        {
            self.table_children(id, &mut inner, link)?;
        } else if style.display == Display::Flex {
            self.flex_children(id, &mut inner, link)?;
        } else if style.display == Display::Grid {
            self.grid_children(id, &mut inner, link)?;
        } else if self
            .document
            .element(id)
            .is_some_and(|e| matches!(e.tag.as_str(), "input" | "button" | "textarea" | "select"))
        {
            self.control(id, &mut inner, true)?;
        } else if self.document.element(id).is_some_and(|e| e.tag == "img") {
            self.image(id, &mut inner, link, true)?;
        } else {
            self.children(id, &mut inner, link)?;
        }
        self.flush(&mut inner);
        self.height_context.pop();
        let natural = (inner.y - (by + border + pt)).max(0.0);
        let requested = definite
            .unwrap_or(natural)
            .max(
                resolve_height(
                    style.min_height,
                    self.height_context.last().copied().flatten(),
                )
                .map(content_height)
                .unwrap_or(0.),
            )
            .max(0.0);
        let box_height = (requested + vertical).min(MAX_EXTENT);
        self.boxes[id] = Some(BoxMetrics {
            width: box_width,
            height: box_height,
            border,
        });
        if let Some(index) = clip_index {
            let clip = Rect {
                x: bx + border,
                y: by + border,
                width: (box_width - 2. * border).max(0.),
                height: (box_height - 2. * border).max(0.),
            };
            if let DisplayItem::PushClip { rect } = &mut self.items[index] {
                *rect = clip;
            }
            self.push(DisplayItem::PopClip)?;
            self.record_link_clip(id, index, content_links)?;
        }
        if let DisplayItem::Rect { rect, .. } = &mut self.items[background_index] {
            rect.height = box_height;
        }
        if border > 0.0 {
            let rects = [
                Rect {
                    x: bx,
                    y: by,
                    width: box_width,
                    height: border,
                },
                Rect {
                    x: bx,
                    y: by + box_height - border,
                    width: box_width,
                    height: border,
                },
                Rect {
                    x: bx,
                    y: by,
                    width: border,
                    height: box_height,
                },
                Rect {
                    x: bx + box_width - border,
                    y: by,
                    width: border,
                    height: box_height,
                },
            ];
            for (offset, rect) in rects.into_iter().enumerate() {
                if let DisplayItem::Rect { rect: target, .. } =
                    &mut self.items[border_start + offset]
                {
                    *target = rect;
                }
            }
        }
        self.max_x = self.max_x.max(bx + box_width + mr);
        Ok((mt + box_height + mb).clamp(0.0, MAX_EXTENT))
    }
    fn image_asset(&mut self, id: NodeId) -> Result<Option<&'a ImageAsset>, String> {
        let src = self
            .document
            .element(id)
            .and_then(|e| e.attr("src"))
            .unwrap_or("");
        for asset in self.images {
            self.measure_budget = self
                .measure_budget
                .checked_sub(1 + src.len().min(asset.id.len()))
                .ok_or("Image lookup work limit exceeded")?;
            if asset.id == src {
                return Ok(browser_ipc::valid_natural_image_dimensions(
                    asset.natural_width,
                    asset.natural_height,
                )
                .then_some(asset));
            }
        }
        Ok(None)
    }
    fn intrinsic_width(&mut self, id: NodeId) -> Result<f32, String> {
        self.measure_budget = self
            .measure_budget
            .checked_sub(1)
            .ok_or("Intrinsic sizing work limit exceeded")?;
        let style = self.styles[id].clone();
        if self.document.is_inactive_subtree(id) || style.display == Display::None {
            return Ok(0.);
        }
        if self.document.element(id).is_some_and(|element| {
            element.tag == "input"
                && element
                    .attr("type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("hidden"))
        }) {
            return Ok(0.);
        }
        if let NodeKind::Text(text) = &self.document.nodes[id].kind {
            self.measure_budget = self
                .measure_budget
                .checked_sub(text.len().saturating_mul(2))
                .ok_or("Intrinsic text sizing work limit exceeded")?;
            return Ok(preferred_text_width(text, &style));
        }
        if self
            .document
            .element(id)
            .is_some_and(|element| element.tag == "img")
        {
            let asset = self.image_asset(id)?;
            let element = self.document.element(id).expect("image element");
            // Percentages have no definite containing width during measurement.
            let definite_width = |length: Length| match length {
                Length::Px(value) => Some(value),
                _ => None,
            };
            let horizontal = style.padding[1] + style.padding[3] + 2. * style.border_width;
            let vertical = style.padding[0] + style.padding[2] + 2. * style.border_width;
            let content_width = |width: f32| {
                if style.border_box {
                    (width - horizontal).max(0.)
                } else {
                    width
                }
            };
            let (width, _) = image_dimensions(
                asset,
                definite_width(style.width)
                    .map(content_width)
                    .or_else(|| image_attribute(element.attr("width"))),
                resolve_height(style.height, self.height_context.last().copied().flatten())
                    .map(|height| {
                        if style.border_box {
                            (height - vertical).max(0.)
                        } else {
                            height
                        }
                    })
                    .or_else(|| image_attribute(element.attr("height"))),
                definite_width(style.max_width).map(content_width),
            );
            return Ok((width + style.margin[1] + style.margin[3] + horizontal).clamp(0., 16384.));
        }
        if let Some(n) = style
            .width
            .resolve(0.)
            .filter(|_| !matches!(style.width, Length::Percent(_)))
        {
            return Ok((n
                + style.margin[1]
                + style.margin[3]
                + if style.border_box {
                    0.
                } else {
                    style.padding[1] + style.padding[3] + 2. * style.border_width
                })
            .clamp(1., 16384.));
        }
        if let Some(e) = self.document.element(id)
            && matches!(e.tag.as_str(), "input" | "button" | "textarea" | "select")
        {
            let (value, preferred) = self.control_preferred_width(id, &style)?;
            self.measure_budget = self
                .measure_budget
                .checked_sub(value.len())
                .ok_or("Intrinsic control sizing work limit exceeded")?;
            return Ok(preferred
                + style.margin[1]
                + style.margin[3]
                + style.padding[1]
                + style.padding[3]
                + 2. * style.border_width);
        }
        let mut inline = 0f32;
        let mut width = 0f32;
        for child in self.document.nodes[id].children.clone() {
            if self.document.element(child).is_some()
                && self.styles[child].position == Position::Absolute
            {
                continue;
            }
            let measured = self.intrinsic_width(child)?;
            if matches!(
                self.styles[child].display,
                Display::Block | Display::Flex | Display::Grid
            ) {
                width = width.max(inline).max(measured);
                inline = 0.;
            } else {
                inline += measured;
            }
        }
        Ok((width.max(inline)
            + style.padding[1]
            + style.padding[3]
            + 2. * style.border_width
            + style.margin[1]
            + style.margin[3])
            .clamp(1., 16384.))
    }
    fn control_preferred_width(&self, id: NodeId, style: &Style) -> Result<(String, f32), String> {
        let element = self
            .document
            .element(id)
            .ok_or("Control must be an element")?;
        let kind = element
            .attr("type")
            .unwrap_or(if element.tag == "button" {
                "submit"
            } else {
                "text"
            })
            .to_ascii_lowercase();
        if element.tag == "input" && kind == "hidden" {
            return Ok((String::new(), 0.));
        }
        let value = if element.tag == "button" {
            self.document.text_content(id)
        } else {
            element
                .attr("value")
                .unwrap_or(if kind == "submit" { "Enviar" } else { "" })
                .into()
        }
        .chars()
        .take(8192)
        .collect::<String>();
        let preferred = if element.tag == "button" || matches!(kind.as_str(), "submit" | "button") {
            measure_text(&value, style.font_size).max(1.)
        } else if kind == "checkbox" {
            22.
        } else if element.tag == "select" {
            160.
        } else {
            element
                .attr("size")
                .and_then(|size| size.parse::<f32>().ok())
                .filter(|n| n.is_finite())
                .map_or(260., |n| n.clamp(1., 80.) * 8.)
        };
        Ok((value, preferred))
    }
    fn option_text(&mut self, id: NodeId) -> Result<String, String> {
        let mut stack = vec![id];
        let mut text = String::new();
        let mut space = false;
        while let Some(node) = stack.pop() {
            self.measure_budget = self
                .measure_budget
                .checked_sub(1)
                .ok_or("Select option traversal work limit exceeded")?;
            if let NodeKind::Text(value) = &self.document.nodes[node].kind {
                for ch in value.chars() {
                    self.measure_budget = self
                        .measure_budget
                        .checked_sub(1)
                        .ok_or("Select option text work limit exceeded")?;
                    if ch.is_whitespace() {
                        space = !text.is_empty();
                        continue;
                    }
                    let needed = ch.len_utf8() + usize::from(space);
                    if text.len() + needed > 1024 {
                        return Ok(text);
                    }
                    if space {
                        text.push(' ');
                        space = false;
                    }
                    text.push(ch);
                }
            } else {
                let children = &self.document.nodes[node].children;
                self.measure_budget = self
                    .measure_budget
                    .checked_sub(children.len())
                    .ok_or("Select option traversal work limit exceeded")?;
                stack.extend(children.iter().rev().copied());
            }
        }
        Ok(text)
    }
    fn select_options(&mut self, id: NodeId) -> Result<(Vec<ControlOption>, String, bool), String> {
        fn bounded(value: &str, bytes: usize) -> String {
            value
                .char_indices()
                .take_while(|(offset, ch)| offset + ch.len_utf8() <= bytes)
                .map(|(_, ch)| ch)
                .collect()
        }
        let mut options = Vec::new();
        let mut selected = None;
        let mut fallback = None;
        let mut truncated = false;
        for option_id in 0..self.document.nodes.len() {
            self.measure_budget = self
                .measure_budget
                .checked_sub(1)
                .ok_or("Select option traversal work limit exceeded")?;
            if !self
                .document
                .element(option_id)
                .is_some_and(|element| element.tag == "option")
            {
                continue;
            }
            let mut parent = self.document.nodes[option_id].parent;
            let mut owner = None;
            let mut group_disabled = false;
            while let Some(ancestor) = parent {
                self.measure_budget = self
                    .measure_budget
                    .checked_sub(1)
                    .ok_or("Select option ancestor work limit exceeded")?;
                if let Some(element) = self.document.element(ancestor) {
                    if element.tag == "select" {
                        owner = Some(ancestor);
                        break;
                    }
                    if element.tag == "optgroup" && element.attributes.contains_key("disabled") {
                        group_disabled = true;
                    }
                }
                parent = self.document.nodes[ancestor].parent;
            }
            if owner != Some(id) {
                continue;
            }
            if options.len() >= 256 {
                truncated = true;
                break;
            }
            let element = self
                .document
                .element(option_id)
                .ok_or("Invalid option element")?;
            let label_attr = element.attr("label").map(|label| bounded(label, 256));
            let value_attr = element.attr("value").map(|value| bounded(value, 1024));
            let option_selected = element.attributes.contains_key("selected");
            let disabled = group_disabled || element.attributes.contains_key("disabled");
            let content = if label_attr.is_none() || value_attr.is_none() {
                self.option_text(option_id)?
            } else {
                String::new()
            };
            let label = label_attr.unwrap_or_else(|| bounded(&content, 256));
            let value = value_attr.unwrap_or(content);
            self.measure_budget = self
                .measure_budget
                .checked_sub(label.len() + value.len())
                .ok_or("Select option string work limit exceeded")?;
            if option_selected && selected.is_none() {
                selected = Some(value.clone());
            }
            if !disabled && fallback.is_none() {
                fallback = Some(value.clone());
            }
            options.push(ControlOption {
                value,
                label,
                disabled,
            });
        }
        let cleared = self
            .document
            .element(id)
            .is_some_and(|element| element.attributes.contains_key("data-noir-select-empty"));
        Ok((
            options,
            if cleared {
                String::new()
            } else {
                selected.or(fallback).unwrap_or_default()
            },
            truncated,
        ))
    }
    fn table_children(
        &mut self,
        id: NodeId,
        flow: &mut Flow,
        link: Option<&str>,
    ) -> Result<(), String> {
        let mut rows = Vec::new();
        for child in self.document.nodes[id].children.clone() {
            if self.styles[child].display == Display::None {
                continue;
            }
            match self.document.element(child).map(|e| e.tag.as_str()) {
                Some("tr") => rows.push(child),
                Some("thead" | "tbody" | "tfoot") => {
                    rows.extend(self.document.nodes[child].children.iter().copied().filter(
                        |node| {
                            self.styles[*node].display != Display::None
                                && self.document.element(*node).is_some_and(|e| e.tag == "tr")
                        },
                    ));
                }
                Some("caption") => {
                    self.node(child, flow, link)?;
                    self.flush(flow);
                }
                _ => {}
            }
        }
        if rows.is_empty() {
            return self.children(id, flow, link);
        }
        if rows.len() > 1024 {
            return Err("Table row count limit exceeded".into());
        }
        let mut cells = Vec::new();
        let mut columns = 0usize;
        for row in rows {
            let row_cells = self.document.nodes[row]
                .children
                .iter()
                .copied()
                .filter(|cell| {
                    self.styles[*cell].display != Display::None
                        && self
                            .document
                            .element(*cell)
                            .is_some_and(|e| matches!(e.tag.as_str(), "td" | "th"))
                })
                .collect::<Vec<_>>();
            columns = columns.max(row_cells.len());
            if columns > 32 {
                return Err("Table column count limit exceeded".into());
            }
            cells.push(row_cells);
        }
        if columns == 0 {
            return self.children(id, flow, link);
        }
        let style = self.styles[id].clone();
        let available = (flow.width - style.gap[1] * (columns + 1) as f32).max(1.);
        let mut widths = vec![0f32; columns];
        let mut specified = vec![false; columns];
        let mut natural = vec![0f32; columns];
        for row in &cells {
            for (column, cell) in row.iter().copied().enumerate() {
                if self.document.element(cell).is_some_and(|e| {
                    e.attr("colspan").is_some_and(|n| n != "1")
                        || e.attr("rowspan").is_some_and(|n| n != "1")
                }) && self.warnings.len() < 16
                {
                    self.warnings
                        .push("Table spans are not supported; cells use one track".into());
                }
                natural[column] = natural[column].max(self.intrinsic_width(cell)?);
                if let Some(width) = self.styles[cell].width.resolve(available) {
                    widths[column] = widths[column].max(width.max(1.));
                    specified[column] = true;
                }
            }
        }
        let fixed: f32 = widths.iter().sum();
        let auto: f32 = natural
            .iter()
            .zip(&specified)
            .filter_map(|(n, fixed)| (!fixed).then_some(*n))
            .sum();
        let auto_count = specified.iter().filter(|fixed| !**fixed).count();
        let remaining = (available - fixed).max(auto_count as f32);
        for column in 0..columns {
            if !specified[column] {
                widths[column] = if auto > 0. {
                    remaining * natural[column] / auto
                } else {
                    remaining / auto_count.max(1) as f32
                }
                .max(1.);
            }
        }
        // Honor explicit tracks without permitting runaway arithmetic or allocation.
        for width in &mut widths {
            *width = width.clamp(1., 16384.);
        }
        flow.y += style.gap[0];
        for row in cells {
            let mut x = flow.left + style.gap[1];
            let mut height = 0f32;
            for (column, cell) in row.into_iter().enumerate() {
                let cell_height = self.box_child(cell, x, flow.y, widths[column], link)?;
                height = height.max(cell_height);
                x += widths[column] + style.gap[1];
            }
            flow.y = (flow.y + height + style.gap[0]).min(MAX_EXTENT);
        }
        Ok(())
    }
    fn visible_children(
        &mut self,
        id: NodeId,
        flow: &Flow,
        link: Option<&str>,
    ) -> Result<Vec<NodeId>, String> {
        let mut children = Vec::new();
        for child in self.document.nodes[id].children.clone() {
            if self.styles[child].display == Display::None
                || matches!(&self.document.nodes[child].kind, NodeKind::Text(text) if text.trim().is_empty())
            {
                continue;
            }
            if self.document.element(child).is_some()
                && self.styles[child].position == Position::Absolute
            {
                self.defer_positioned(child, flow, link)?;
            } else {
                children.push(child);
            }
        }
        Ok(children)
    }
    fn box_child(
        &mut self,
        id: NodeId,
        x: f32,
        y: f32,
        width: f32,
        link: Option<&str>,
    ) -> Result<f32, String> {
        let href = self
            .document
            .element(id)
            .filter(|e| e.tag == "a")
            .and_then(|e| e.attr("href"))
            .map(str::to_owned);
        if matches!(self.document.nodes[id].kind, NodeKind::Element(_)) {
            let relative = self.styles[id].position == Position::Relative;
            let items = self.items.len();
            let links = self.links.len();
            if relative {
                self.positioned.push(PositionFrame::default());
            }
            let height = if self.document.element(id).is_some_and(|e| {
                matches!(e.tag.as_str(), "input" | "button" | "textarea" | "select")
            }) {
                let mut flow = Flow::new(x, y, width.max(1.));
                flow.item_start = items;
                flow.link_start = links;
                self.control(id, &mut flow, true)?;
                self.flush(&mut flow);
                flow.y - y
            } else {
                self.block(id, x, y, width.max(1.), href.as_deref().or(link), true)?
            };
            if relative {
                self.finish_relative(
                    id,
                    items,
                    links,
                    Rect {
                        x,
                        y,
                        width,
                        height,
                    },
                    width,
                )?;
            }
            Ok(height)
        } else {
            let mut flow = Flow::new(x, y, width.max(1.));
            flow.item_start = self.items.len();
            flow.link_start = self.links.len();
            self.node(id, &mut flow, link)?;
            self.flush(&mut flow);
            Ok(flow.y - y)
        }
    }
    fn translate(
        &mut self,
        items: std::ops::Range<usize>,
        links: std::ops::Range<usize>,
        dx: f32,
        dy: f32,
    ) {
        for item in &mut self.items[items] {
            let rect = match item {
                DisplayItem::PopClip => continue,
                DisplayItem::PushClip { rect } => rect,
                DisplayItem::Rect { rect, .. }
                | DisplayItem::Text { rect, .. }
                | DisplayItem::Image { rect, .. }
                | DisplayItem::Control { rect, .. } => rect,
            };
            rect.x = (rect.x + dx).clamp(-MAX_EXTENT, MAX_EXTENT);
            rect.y = (rect.y + dy).clamp(-MAX_EXTENT, MAX_EXTENT);
        }
        for link in &mut self.links[links] {
            link.rect.x += dx;
            link.rect.y += dy;
        }
    }
    fn stretch_box(&mut self, id: NodeId, start: usize, target: f32) {
        let style = &self.styles[id];
        if style.height != Length::Auto || self.document.element(id).is_none() {
            return;
        }
        if let Some(DisplayItem::Rect { rect, .. }) = self.items.get_mut(start) {
            let height = (target - style.margin[0] - style.margin[2]).max(rect.height);
            let delta = height - rect.height;
            rect.height = height;
            if let Some(metrics) = &mut self.boxes[id] {
                metrics.height = height;
            }
            if style.border_width > 0. {
                if let Some(DisplayItem::Rect { rect, .. }) = self.items.get_mut(start + 2) {
                    rect.y += delta;
                }
                for offset in [3, 4] {
                    if let Some(DisplayItem::Rect { rect, .. }) = self.items.get_mut(start + offset)
                    {
                        rect.height += delta;
                    }
                }
            }
            if style.overflow_clip {
                // Flow content and positioned children use separate markers,
                // both owned by this containing block. Descendant owners are
                // never visited when this box is stretched.
                if let Some(indices) = self.owned_clips.get(&id) {
                    for &index in indices {
                        if let Some(DisplayItem::PushClip { rect }) = self.items.get_mut(index) {
                            rect.height = (height - 2. * style.border_width.min(1000.)).max(0.);
                        }
                    }
                }
            }
        }
    }
    fn flex_children(
        &mut self,
        id: NodeId,
        flow: &mut Flow,
        link: Option<&str>,
    ) -> Result<(), String> {
        let style = self.styles[id].clone();
        let mut children = self.visible_children(id, flow, link)?;
        if style.flex_reverse {
            children.reverse();
        }
        if style.flex_column {
            let start_y = flow.y;
            let mut ranges = Vec::new();
            for (index, child) in children.iter().copied().enumerate() {
                if index > 0 {
                    flow.y += style.gap[0];
                }
                let width = if style.align_items == Alignment::Stretch {
                    flow.width
                } else {
                    self.intrinsic_width(child)?.min(flow.width)
                };
                let offset = alignment_offset(style.align_items, (flow.width - width).max(0.));
                let items = self.items.len();
                let links = self.links.len();
                let height = self.box_child(child, flow.left + offset, flow.y, width, link)?;
                ranges.push((items..self.items.len(), links..self.links.len()));
                flow.y = (flow.y + height).min(MAX_EXTENT);
            }
            let requested = self.height_context.last().copied().flatten().unwrap_or(0.);
            let spare = (requested - (flow.y - start_y)).max(0.);
            let (offset, gap) = distribution(style.justify_content, spare, ranges.len());
            for (index, (items, links)) in ranges.into_iter().enumerate() {
                self.translate(items, links, 0., offset + index as f32 * gap);
            }
            flow.y = flow.y.max(start_y + requested);
            return Ok(());
        }
        let mut bases = Vec::with_capacity(children.len());
        for child in &children {
            if matches!(self.document.nodes[*child].kind, NodeKind::Text(_)) {
                bases.push(self.intrinsic_width(*child)?);
                continue;
            }
            let child_style = &self.styles[*child];
            let size = child_style
                .flex_basis
                .resolve(flow.width)
                .or_else(|| child_style.width.resolve(flow.width));
            let extra = child_style.margin[1]
                + child_style.margin[3]
                + if child_style.border_box {
                    0.
                } else {
                    child_style.padding[1] + child_style.padding[3] + 2. * child_style.border_width
                };
            bases.push(if let Some(n) = size {
                (n + extra).clamp(1., 16384.)
            } else {
                self.intrinsic_width(*child)?
            });
        }
        let mut first = 0;
        while first < children.len() {
            let mut end = first;
            let mut used = 0.;
            while end < children.len() {
                let next = used + if end > first { style.gap[1] } else { 0. } + bases[end];
                if style.flex_wrap && end > first && next > flow.width {
                    break;
                }
                used = next;
                end += 1;
            }
            let spare = flow.width - used;
            let grow: f32 = children[first..end]
                .iter()
                .map(|child| self.styles[*child].flex_grow)
                .sum();
            let shrink: f32 = children[first..end]
                .iter()
                .zip(&bases[first..end])
                .map(|(child, basis)| self.styles[*child].flex_shrink * basis)
                .sum();
            let mut sizes = Vec::new();
            for index in first..end {
                let child_style = &self.styles[children[index]];
                let delta = if spare > 0. && grow > 0. {
                    spare * child_style.flex_grow / grow
                } else if spare < 0. && shrink > 0. {
                    spare * child_style.flex_shrink * bases[index] / shrink
                } else {
                    0.
                };
                sizes.push((bases[index] + delta).max(1.));
            }
            let remaining =
                (flow.width - sizes.iter().sum::<f32>() - style.gap[1] * (end - first - 1) as f32)
                    .max(0.);
            let (offset, added_gap) = distribution(style.justify_content, remaining, end - first);
            let mut x = flow.left + offset;
            let mut row_height = 0f32;
            let mut ranges = Vec::new();
            for (child, width) in children[first..end].iter().zip(sizes) {
                let items = self.items.len();
                let links = self.links.len();
                let height = self.box_child(*child, x, flow.y, width, link)?;
                ranges.push((
                    *child,
                    items..self.items.len(),
                    links..self.links.len(),
                    height,
                ));
                row_height = row_height.max(height);
                x += width + style.gap[1] + added_gap;
            }
            // A nowrap line uses the definite content height, even when items
            // overflow it. Wrapped lines retain their natural heights.
            if !style.flex_wrap
                && let Some(definite) = self.height_context.last().copied().flatten()
            {
                row_height = definite.clamp(0., MAX_EXTENT);
            }
            for (child, items, links, height) in ranges {
                if style.align_items == Alignment::Stretch {
                    self.stretch_box(child, items.start, row_height);
                }
                // The outer height includes margins; negative free space permits
                // center/end overflow instead of silently reverting to start.
                let dy = alignment_offset(style.align_items, row_height - height);
                self.translate(items, links, 0., dy);
            }
            flow.y = (flow.y
                + row_height
                + if end < children.len() {
                    style.gap[0]
                } else {
                    0.
                })
            .min(MAX_EXTENT);
            first = end;
        }
        Ok(())
    }
    fn grid_children(
        &mut self,
        id: NodeId,
        flow: &mut Flow,
        link: Option<&str>,
    ) -> Result<(), String> {
        let style = self.styles[id].clone();
        let tracks = if style.grid_columns.is_empty() {
            vec![GridTrack::Fraction(1.)]
        } else {
            style.grid_columns
        };
        let gaps = style.gap[1] * (tracks.len() - 1) as f32;
        let fixed: f32 = tracks
            .iter()
            .map(|track| match track {
                GridTrack::Fixed(n) => n.resolve(flow.width).unwrap_or(0.).max(0.),
                _ => 0.,
            })
            .sum();
        let fractions: f32 = tracks
            .iter()
            .map(|track| match track {
                GridTrack::Fraction(n) => *n,
                _ => 0.,
            })
            .sum();
        let free = (flow.width - gaps - fixed).max(0.);
        let widths = tracks
            .iter()
            .map(|track| match track {
                GridTrack::Fixed(n) => n.resolve(flow.width).unwrap_or(1.).max(1.),
                GridTrack::Fraction(n) => (free * n / fractions.max(0.001)).max(1.),
            })
            .collect::<Vec<_>>();
        let children = self.visible_children(id, flow, link)?;
        for (row, chunk) in children.chunks(widths.len()).enumerate() {
            if row > 0 {
                flow.y += style.gap[0];
            }
            let mut x = flow.left;
            let mut height = 0f32;
            let mut ranges = Vec::new();
            for (child, width) in chunk.iter().zip(&widths) {
                let items = self.items.len();
                let links = self.links.len();
                let child_height = self.box_child(*child, x, flow.y, *width, link)?;
                ranges.push((
                    *child,
                    items..self.items.len(),
                    links..self.links.len(),
                    child_height,
                ));
                height = height.max(child_height);
                x += width + style.gap[1];
            }
            for (child, items, links, child_height) in ranges {
                if style.align_items == Alignment::Stretch {
                    self.stretch_box(child, items.start, height);
                }
                self.translate(
                    items,
                    links,
                    0.,
                    alignment_offset(style.align_items, (height - child_height).max(0.)),
                );
            }
            flow.y = (flow.y + height).min(MAX_EXTENT);
        }
        Ok(())
    }
    fn text(
        &mut self,
        text: &str,
        style: &Style,
        flow: &mut Flow,
        link: Option<&str>,
    ) -> Result<(), String> {
        // Prepay the token scan, run measurement and possible emergency scalar
        // split. Collapsed whitespace never allocates per-character items.
        self.measure_budget = self
            .measure_budget
            .checked_sub(text.len().saturating_mul(3))
            .ok_or("Inline text work limit exceeded")?;
        for token in text_tokens(text, style) {
            match token {
                TextToken::Run(run) => self.word(run, style, flow, link)?,
                TextToken::Break => {
                    flow.line_height = flow.line_height.max(style.font_size * style.line_height);
                    self.flush(flow);
                }
                TextToken::Spaces(count) if style.pre => {
                    // Limited: tabs/form feeds occupy one space; preserved
                    // spaces do not themselves wrap. A collapsed pending gap
                    // uses the following font, and has no item or link target.
                    if flow.space && flow.x > flow.left {
                        flow.x += measure_text(" ", style.font_size);
                    }
                    flow.space = false;
                    let width = measure_text(" ", style.font_size);
                    for _ in 0..count {
                        self.fragment(" ", width, style, flow, link)?;
                    }
                }
                TextToken::Spaces(_) => flow.space = true,
            }
        }
        Ok(())
    }
    fn word(
        &mut self,
        word: &str,
        style: &Style,
        flow: &mut Flow,
        link: Option<&str>,
    ) -> Result<(), String> {
        let width = measure_text(word, style.font_size);
        // The collapsed gap crosses nodes using the following run font.
        // It has no independent background or link target, even across links.
        let space = if flow.space && flow.x > flow.left {
            measure_text(" ", style.font_size)
        } else {
            0.0
        };
        if !style.nowrap && flow.x > flow.left && flow.x + space + width > flow.left + flow.width {
            self.flush(flow);
        } else {
            flow.x += space;
        }
        flow.space = false;
        if width <= flow.width || style.nowrap {
            return self.fragment(word, width, style, flow, link);
        }
        // Split oversized tokens on scalar boundaries. No byte slicing of UTF-8.
        let mut chunk = String::new();
        let mut chunk_width = 0.0;
        let mut previous = None;
        for ch in word.chars() {
            let advance = font().metrics(ch, style.font_size).advance_width
                + previous
                    .and_then(|p| font().horizontal_kern(p, ch, style.font_size))
                    .unwrap_or(0.0);
            if !chunk.is_empty() && flow.x + chunk_width + advance > flow.left + flow.width {
                self.fragment(&chunk, chunk_width, style, flow, link)?;
                self.flush(flow);
                chunk.clear();
                chunk_width = 0.0;
                previous = None;
            }
            let kern = previous
                .and_then(|p| font().horizontal_kern(p, ch, style.font_size))
                .unwrap_or(0.0);
            chunk.push(ch);
            chunk_width += font().metrics(ch, style.font_size).advance_width + kern;
            previous = Some(ch);
        }
        if !chunk.is_empty() {
            self.fragment(&chunk, chunk_width, style, flow, link)?;
        }
        Ok(())
    }
    fn fragment(
        &mut self,
        text: &str,
        width: f32,
        style: &Style,
        flow: &mut Flow,
        link: Option<&str>,
    ) -> Result<(), String> {
        let height = style.font_size * style.line_height;
        let rect = Rect {
            x: flow.x,
            y: flow.y,
            width,
            height,
        };
        if style.visible && style.background.3 > 0 {
            self.push(DisplayItem::Rect {
                rect,
                color: style.background,
            })?;
        }
        if style.visible {
            self.push(DisplayItem::Text {
                rect,
                text: text.to_string(),
                size: style.font_size,
                color: style.color,
                bold: style.bold,
            })?;
        }
        if style.visible
            && let Some(href) = link
        {
            self.links.push(Link {
                rect,
                href: href.to_string(),
            });
        }
        flow.x += width;
        flow.line_height = flow.line_height.max(height);
        self.max_x = self.max_x.max(flow.x);
        Ok(())
    }
    fn control(&mut self, id: NodeId, flow: &mut Flow, allocated: bool) -> Result<(), String> {
        let element = self
            .document
            .element(id)
            .ok_or("Control must be an element")?;
        let style = self.styles[id].clone();
        let kind = match (
            element.tag.as_str(),
            element
                .attr("type")
                .unwrap_or("text")
                .to_ascii_lowercase()
                .as_str(),
        ) {
            ("textarea", _) => ControlKind::TextArea,
            ("select", _) => ControlKind::Select,
            ("button", _) => {
                if element.attr("type") == Some("button") {
                    ControlKind::Button
                } else {
                    ControlKind::Submit
                }
            }
            (_, "hidden") => ControlKind::Hidden,
            (_, "submit") => ControlKind::Submit,
            (_, "button") => ControlKind::Button,
            (_, "password") => ControlKind::Password,
            (_, "checkbox") => ControlKind::Checkbox,
            (_, "text" | "search" | "email" | "url" | "tel" | "number") => ControlKind::Text,
            _ => {
                if self.warnings.len() < 16 {
                    self.warnings
                        .push("Unsupported form control type was omitted".into());
                }
                return Ok(());
            }
        };
        if self.control_count >= 256 {
            return Err("Form control count limit exceeded".into());
        }
        self.control_count += 1;
        let mut ancestor = self.document.nodes[id].parent;
        let mut form_id = None;
        if let Some(external) = element.attr("form") {
            form_id = self
                .document
                .elements()
                .find(|(_, e)| e.tag == "form" && e.attr("id") == Some(external))
                .map(|(id, _)| id);
        } else {
            while let Some(parent) = ancestor {
                if self
                    .document
                    .element(parent)
                    .is_some_and(|e| e.tag == "form")
                {
                    form_id = Some(parent);
                    break;
                }
                ancestor = self.document.nodes[parent].parent;
            }
        }
        let form = form_id.and_then(|id| {
            self.document.element(id).map(|e| FormInfo {
                id: id as u64,
                action: element
                    .attr("formaction")
                    .or_else(|| e.attr("action"))
                    .unwrap_or("")
                    .chars()
                    .take(8192)
                    .collect(),
                method: element
                    .attr("formmethod")
                    .or_else(|| e.attr("method"))
                    .unwrap_or("get")
                    .to_ascii_lowercase(),
            })
        });
        let mut value = if element.tag == "textarea" || element.tag == "button" {
            self.document.text_content(id)
        } else {
            element
                .attr("value")
                .unwrap_or(if kind == ControlKind::Submit {
                    "Enviar"
                } else {
                    ""
                })
                .to_string()
        };
        if kind == ControlKind::Checkbox {
            value = if element.attributes.contains_key("checked") {
                element.attr("value").unwrap_or("on").to_string()
            } else {
                String::new()
            };
        }
        value = value.chars().take(8192).collect();
        let name = element
            .attr("name")
            .unwrap_or("")
            .chars()
            .take(512)
            .collect();
        let placeholder = element
            .attr("placeholder")
            .or_else(|| element.attr("aria-label"))
            .or_else(|| element.attr("title"))
            .unwrap_or("")
            .chars()
            .take(512)
            .collect();
        let hidden = kind == ControlKind::Hidden;
        let (_, preferred) = self.control_preferred_width(id, &style)?;
        let border = style.border_width.clamp(0., 128.);
        let [mt, mr, mb, ml] = style.margin;
        let horizontal = style.padding[1] + style.padding[3] + border * 2.;
        let vertical = style.padding[0] + style.padding[2] + border * 2.;
        let mut width = if hidden {
            0.
        } else if allocated {
            (flow.width - ml - mr).max(1.)
        } else {
            style
                .width
                .resolve(flow.width)
                .map_or(preferred + horizontal, |n| {
                    if style.border_box { n } else { n + horizontal }
                })
        };
        if !hidden {
            if let Some(max) = style.max_width.resolve(flow.width) {
                width = width.min(if style.border_box {
                    max
                } else {
                    max + horizontal
                });
            }
            if let Some(min) = style.min_width.resolve(flow.width) {
                width = width.max(if style.border_box {
                    min
                } else {
                    min + horizontal
                });
            }
            width = width.clamp(1., 16384.);
        }
        let height = if hidden {
            0.
        } else {
            let outer_height = |n: f32| {
                if style.border_box {
                    n.max(vertical)
                } else {
                    n + vertical
                }
            };
            resolve_height(style.height, self.height_context.last().copied().flatten())
                .map_or(
                    if kind == ControlKind::TextArea {
                        90.
                    } else {
                        style.font_size * style.line_height
                    } + vertical,
                    outer_height,
                )
                .max(
                    resolve_height(
                        style.min_height,
                        self.height_context.last().copied().flatten(),
                    )
                    .map(outer_height)
                    .unwrap_or(0.),
                )
                .clamp(1., 16384.)
        };
        let disabled = element.attributes.contains_key("disabled")
            || element.attributes.contains_key("readonly");
        let space = if !hidden && flow.space && flow.x > flow.left {
            measure_text(" ", style.font_size)
        } else {
            0.
        };
        if !style.nowrap
            && !hidden
            && flow.x > flow.left
            && flow.x + space + ml + width + mr > flow.left + flow.width
        {
            self.flush(flow);
        } else {
            flow.x += space;
        }
        if !hidden {
            flow.space = false;
        }
        let rect = Rect {
            x: flow.x + ml,
            y: flow.y + mt,
            width,
            height,
        };
        let multiple = element.attributes.contains_key("multiple");
        if !hidden && self.boxes[id].is_none() {
            self.boxes[id] = Some(BoxMetrics {
                width,
                height,
                border,
            });
        }
        let (options, selected, truncated) = if kind == ControlKind::Select {
            self.measure_budget = self
                .measure_budget
                .checked_sub(self.document.nodes.len())
                .ok_or("Select option traversal work limit exceeded")?;
            self.select_options(id)?
        } else {
            (Vec::new(), String::new(), false)
        };
        if truncated && self.warnings.len() < 16 {
            self.warnings
                .push("Select options beyond the first 256 were omitted".into());
        }
        if kind == ControlKind::Select {
            value = selected;
            if multiple && self.warnings.len() < 16 {
                self.warnings
                    .push("Multiple select is not supported; a single selection is exposed".into());
            }
        }
        if style.visible {
            self.push(DisplayItem::Control {
                rect,
                id: id as u64,
                kind,
                name,
                value,
                placeholder,
                form,
                disabled,
                appearance: ControlAppearance {
                    background: style.background,
                    border_color: style.border_color,
                    border_width: border,
                    color: style.color,
                    font_size: style.font_size,
                },
                options,
            })?;
        }
        if !hidden {
            flow.x += ml + width + mr;
            flow.line_height = flow.line_height.max(mt + height + mb);
            self.max_x = self.max_x.max(flow.x);
        }
        Ok(())
    }
    fn image(
        &mut self,
        id: NodeId,
        flow: &mut Flow,
        link: Option<&str>,
        allocated: bool,
    ) -> Result<(), String> {
        let asset = self.image_asset(id)?;
        let element = self.document.element(id).expect("image element");
        let style = self.styles[id].clone();
        let src = element.attr("src").unwrap_or("");
        let width = if allocated {
            Some(flow.width)
        } else {
            style
                .width
                .resolve(flow.width)
                .or_else(|| image_attribute(element.attr("width")))
        };
        // block() pushed this image's resolved content height, not its parent's
        // height. Auto remains None even inside a definite-height Flex/Grid.
        let height = (if allocated {
            self.height_context.last().copied().flatten()
        } else {
            resolve_height(style.height, self.height_context.last().copied().flatten())
        })
        .or_else(|| image_attribute(element.attr("height")));
        let (width, height) = image_dimensions(
            asset,
            width,
            height,
            // The block path already applied max-width to the content box.
            if allocated {
                None
            } else {
                style.max_width.resolve(flow.width)
            },
        );
        if !style.nowrap && flow.x > flow.left && flow.x + width > flow.left + flow.width {
            self.flush(flow);
        }
        let rect = Rect {
            x: flow.x,
            y: flow.y,
            width,
            height,
        };
        if self.boxes[id].is_none() {
            self.boxes[id] = Some(BoxMetrics {
                width,
                height,
                border: 0.,
            });
        }
        if !style.visible {
            // Keep formatting geometry, while exposing no hidden image or link.
        } else if let Some(asset) = asset {
            self.push(DisplayItem::Image {
                rect,
                asset_id: asset.id.clone(),
            })?;
        } else {
            self.push(DisplayItem::Rect {
                rect,
                color: Color(231, 234, 240, 255),
            })?;
            let alt = element
                .attr("alt")
                .unwrap_or("Imagem indisponível")
                .chars()
                .take(128)
                .collect::<String>();
            self.push(DisplayItem::Text {
                rect,
                text: alt,
                size: style.font_size,
                color: style.color,
                bold: false,
            })?;
            if self.warnings.len() < 16 {
                self.warnings.push(format!(
                    "Image not supplied by resource broker: {}",
                    src.chars().take(160).collect::<String>()
                ));
            }
        }
        if style.visible
            && let Some(href) = link
        {
            self.links.push(Link {
                rect,
                href: href.to_string(),
            });
        }
        flow.x += width;
        flow.line_height = flow.line_height.max(height);
        flow.space = false;
        self.max_x = self.max_x.max(flow.x);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reduced_image() -> ImageAsset {
        ImageAsset {
            id: "photo".into(),
            width: 1024,
            height: 512,
            natural_width: 2048.,
            natural_height: 1024.,
            rgba: vec![255; 1024 * 512 * 4],
        }
    }
    fn image_page(source: &str, images: &[ImageAsset]) -> (Document, LayoutResult) {
        let document =
            web_html::parse(&format!("<style>body{{margin:0}}</style>{source}")).unwrap();
        let styles = web_css::compute(&document).unwrap();
        let page = layout(
            &document,
            &styles.styles,
            images,
            Viewport {
                width: 4096.,
                height: 2048.,
            },
        )
        .unwrap();
        (document, page)
    }
    fn painted_images(page: &LayoutResult) -> Vec<Rect> {
        page.items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Image { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn image_natural_size_and_one_axis_use_metadata_not_reduced_texture() {
        let asset = reduced_image();
        for (attributes, expected) in [
            ("", (2048., 1024.)),
            ("width='400'", (400., 200.)),
            ("height='100'", (200., 100.)),
            ("width='400' height='100'", (400., 100.)),
            (
                "width='400' height='100' style='width:300px;height:75px'",
                (300., 75.),
            ),
            ("width='400' style='max-width:150px'", (150., 75.)),
            (
                "width='400' height='100' style='max-width:150px'",
                (150., 100.),
            ),
            ("height='100' style='width:auto;height:auto'", (200., 100.)),
        ] {
            let (_, page) = image_page(
                &format!("<img src='photo' {attributes}>"),
                std::slice::from_ref(&asset),
            );
            let rect = painted_images(&page)[0];
            assert_eq!((rect.width, rect.height), expected, "{attributes}");
        }
    }
    #[test]
    fn image_block_and_allocated_width_preserve_ratio_and_explicit_height() {
        let asset = reduced_image();
        for (source, expected) in [
            ("<img src=photo style='display:block'>", (2048., 1024.)),
            (
                "<img src=photo height=100 style='display:block'>",
                (200., 100.),
            ),
            (
                "<div style='display:flex;width:3000px;align-items:start'><img src=photo></div>",
                (2048., 1024.),
            ),
            (
                "<div style='display:flex;width:3000px;align-items:start'><img src=photo height=100></div>",
                (200., 100.),
            ),
            (
                "<div style='display:flex;width:300px;height:600px;align-items:start'><img src=photo></div>",
                (300., 150.),
            ),
            (
                "<div style='display:grid;width:500px;height:600px;align-items:start'><img src=photo></div>",
                (500., 250.),
            ),
            (
                "<div style='display:grid;width:500px;align-items:start'><img src=photo height=100></div>",
                (500., 100.),
            ),
            (
                "<div style='display:flex;width:3000px;align-items:start'><div style='display:grid'><img src=photo></div></div>",
                (2048., 1024.),
            ),
            (
                "<div style='display:grid;width:500px;align-items:start'><img src=photo style='max-width:50%'></div>",
                (250., 125.),
            ),
        ] {
            let (_, page) = image_page(source, std::slice::from_ref(&asset));
            let rect = painted_images(&page)[0];
            assert_eq!((rect.width, rect.height), expected, "{source}");
        }
    }
    #[test]
    fn image_border_box_clip_and_link_use_content_geometry() {
        let (_, page) = image_page(
            "<a href='/photo'><img src=photo style='display:block;box-sizing:border-box;width:240px;padding:10px;border:10px solid red;margin:5px;overflow:hidden'></a>",
            &[reduced_image()],
        );
        let rect = painted_images(&page)[0];
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (25., 25., 200., 100.)
        );
        assert_eq!(page.links.len(), 1);
        let link = page.links[0].rect;
        assert_eq!(
            (link.x, link.y, link.width, link.height),
            (25., 25., 200., 100.)
        );
        assert!(
            page.items
                .iter()
                .any(|item| matches!(item, DisplayItem::PushClip { rect }
            if rect.width == 220. && rect.height == 120.))
        );
    }
    #[test]
    fn image_fractional_svg_dimensions_and_shared_texture_are_independent() {
        let asset = ImageAsset {
            id: "photo".into(),
            width: 1,
            height: 1,
            natural_width: 0.5,
            natural_height: 0.25,
            rgba: vec![255; 4],
        };
        let (_, page) = image_page(
            "<img src=photo><img src=photo width=20><img src=photo style='display:block'><img src=photo style='display:inline-block'><img src=photo style='display:inline-block'>",
            std::slice::from_ref(&asset),
        );
        let sizes: Vec<_> = painted_images(&page)
            .iter()
            .map(|rect| (rect.width, rect.height))
            .collect();
        assert_eq!(
            sizes,
            vec![
                (0.5, 0.25),
                (20., 10.),
                (0.5, 0.25),
                (0.5, 0.25),
                (0.5, 0.25)
            ]
        );
        let rects = painted_images(&page);
        assert_eq!(rects[4].x - rects[3].x, 0.5);
        assert_eq!((asset.width, asset.height, asset.rgba.len()), (1, 1, 4));
        let thin = ImageAsset {
            natural_width: 4096.,
            natural_height: f32::MIN_POSITIVE,
            ..asset
        };
        assert_eq!(
            image_dimensions(Some(&thin), None, None, None),
            (4096., f32::MIN_POSITIVE)
        );
    }
    #[test]
    fn missing_image_keeps_synthetic_ratio_and_invalid_attributes_are_ignored() {
        assert_eq!(image_dimensions(None, None, None, None), (160., 48.));
        assert_eq!(image_dimensions(None, None, Some(96.), None), (320., 96.));
        for value in ["NaN", "inf", "-10", "0", "16385", "oops"] {
            assert_eq!(image_attribute(Some(value)), None);
        }
        let (document, page) = image_page("<img src=missing height=96>", &[]);
        let id = document
            .nodes
            .iter()
            .enumerate()
            .find_map(|(id, _)| document.element(id).filter(|e| e.tag == "img").map(|_| id))
            .unwrap();
        let metrics = page.boxes[id].unwrap();
        assert_eq!((metrics.width, metrics.height), (320., 96.));
        assert!(painted_images(&page).is_empty());
    }
    fn text_case(
        text: &str,
        style: Style,
        width: f32,
        budget: usize,
    ) -> Result<(Vec<DisplayItem>, Vec<Link>, Flow), String> {
        let document = web_html::parse("").unwrap();
        let mut engine = Layout {
            document: &document,
            styles: &[],
            images: &[],
            items: Vec::new(),
            links: Vec::new(),
            link_clips: Vec::new(),
            owned_clips: BTreeMap::new(),
            clip_depth: 0,
            warnings: Vec::new(),
            max_x: width,
            measure_budget: budget,
            positioned: vec![PositionFrame::default()],
            position_budget: 0,
            height_context: vec![None],
            control_count: 0,
            boxes: Vec::new(),
        };
        let mut flow = Flow::new(0., 0., width);
        engine.text(text, &style, &mut flow, Some("/text"))?;
        engine.flush(&mut flow);
        Ok((engine.items, engine.links, flow))
    }

    #[test]
    fn whitespace_runs_keep_unicode_spaces_and_raw_kerning() {
        let style = Style::default();
        let unicode = "A\u{a0}B\u{202f}C\u{2003}D";
        assert_eq!(
            text_tokens(unicode, &style).collect::<Vec<_>>(),
            vec![TextToken::Run(unicode)]
        );
        for run in ["AV", "To", unicode] {
            assert_eq!(
                preferred_text_width(run, &style),
                measure_text(run, style.font_size)
            );
            let (items, _, _) = text_case(run, style.clone(), 1000., 10000).unwrap();
            assert!(matches!(&items[0], DisplayItem::Text { text, rect, .. }
                if text == run && rect.width == measure_text(run, style.font_size)));
        }
        let expected = measure_text("AV", style.font_size)
            + measure_text(" ", style.font_size)
            + measure_text("To", style.font_size);
        assert!((preferred_text_width("  AV \t\r\n To  ", &style) - expected).abs() < 0.001);
    }

    #[test]
    fn preserved_breaks_and_spaces_have_independent_geometry() {
        for pre in [false, true] {
            let style = Style {
                pre,
                preserve_breaks: true,
                ..Style::default()
            };
            let (items, links, flow) = text_case("A   B\nC", style.clone(), 1000., 10000).unwrap();
            let texts: Vec<_> = items
                .iter()
                .filter_map(|item| match item {
                    DisplayItem::Text { text, rect, .. } => Some((text.as_str(), *rect)),
                    _ => None,
                })
                .collect();
            assert_eq!(texts.len(), if pre { 6 } else { 3 });
            let b = texts.iter().find(|(text, _)| *text == "B").unwrap().1;
            let expected = measure_text("A", style.font_size)
                + measure_text(" ", style.font_size) * if pre { 3. } else { 1. };
            assert!((b.x - expected).abs() < 0.001);
            let preferred = preferred_text_width("A   B\nC", &style);
            assert!(
                (preferred - (b.x + b.width).max(measure_text("C", style.font_size))).abs() < 0.001
            );
            assert_eq!(links.len(), texts.len());
            for ((_, rect), link) in texts.iter().zip(&links) {
                assert_eq!(
                    (rect.x, rect.y, rect.width, rect.height),
                    (link.rect.x, link.rect.y, link.rect.width, link.rect.height)
                );
            }
            assert!((flow.y - 2. * style.font_size * style.line_height).abs() < 0.001);
        }
        let style = Style {
            preserve_breaks: true,
            ..Style::default()
        };
        for break_text in ["\n", "\r\n", "\r"] {
            let source = format!(" \tA \t{break_text} \t{break_text}\tB \t{break_text}");
            let (items, links, flow) = text_case(&source, style.clone(), 1000., 10000).unwrap();
            assert_eq!(items.len(), 2);
            assert_eq!(links.len(), 2);
            // Three terminated lines, including one empty line. The final
            // break does not manufacture an additional fourth empty line.
            assert!((flow.y - 3. * style.font_size * style.line_height).abs() < 0.001);
            assert_eq!(
                preferred_text_width(&source, &style),
                measure_text("A", style.font_size).max(measure_text("B", style.font_size))
            );
        }
    }

    #[test]
    fn collapsed_gaps_cross_inline_nodes_and_disappear_after_wrap() {
        let (_, page) =
            flex_page("<div style='width:1000px'>A <span>  B</span><a href=/empty>   </a></div>");
        let texts: Vec<_> = page
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text {
                    text, rect, size, ..
                } => Some((text.as_str(), *rect, *size)),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 2);
        assert_eq!((texts[0].0, texts[1].0), ("A", "B"));
        assert!(
            (texts[1].1.x - texts[0].1.x - texts[0].1.width - measure_text(" ", texts[1].2)).abs()
                < 0.001
        );
        assert!(page.links.is_empty());
        let (_, page) = flex_page("<div style='width:20px'>A <a href=/b>  B</a></div>");
        let a = page
            .items
            .iter()
            .find_map(|item| match item {
                DisplayItem::Text { text, rect, .. } if text == "A" => Some(*rect),
                _ => None,
            })
            .unwrap();
        assert_eq!(page.links.len(), 1);
        assert_eq!(page.links[0].rect.x, a.x);
        assert!(page.links[0].rect.y > a.y);
    }

    #[test]
    fn all_five_whitespace_modes_have_distinct_wrap_geometry() {
        let size = 16.;
        let space = measure_text(" ", size);
        let narrow = ["A", "B", "C"]
            .into_iter()
            .map(|run| measure_text(run, size))
            .fold(0f32, f32::max)
            + 1.;
        for (mode, preserve_spaces, preserve_breaks, nowrap) in [
            ("normal", false, false, false),
            ("nowrap", false, false, true),
            ("pre-line", false, true, false),
            ("pre-wrap", true, true, false),
            ("pre", true, true, true),
        ] {
            for width in [1000., narrow] {
                let (_, page) = flex_page(&format!(
                    "<div style='font-size:16px;line-height:1.5;width:{width}px;white-space:{mode}'>A   B\nC</div>"
                ));
                let runs: Vec<_> = page
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        DisplayItem::Text { text, rect, .. } if text != " " => {
                            Some((text.as_str(), *rect))
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    runs.iter().map(|(text, _)| *text).collect::<Vec<_>>(),
                    ["A", "B", "C"],
                    "{mode}"
                );
                let (a, b, c) = (runs[0].1, runs[1].1, runs[2].1);
                let wraps = width == narrow && !nowrap;
                let expected_b_y = a.y + if wraps { 24. } else { 0. };
                let expected_c_y = expected_b_y + if wraps || preserve_breaks { 24. } else { 0. };
                assert!((b.y - expected_b_y).abs() < 0.001, "{mode}, width={width}");
                assert!((c.y - expected_c_y).abs() < 0.001, "{mode}, width={width}");
                let expected_b_x = if wraps {
                    a.x
                } else {
                    a.x + a.width + space * if preserve_spaces { 3. } else { 1. }
                };
                let expected_c_x = if wraps || preserve_breaks {
                    a.x
                } else {
                    b.x + b.width + space
                };
                assert!((b.x - expected_b_x).abs() < 0.001, "{mode}, width={width}");
                assert!((c.x - expected_c_x).abs() < 0.001, "{mode}, width={width}");
            }
        }
    }

    #[test]
    fn pending_gap_uses_following_font_without_cross_link_hit_target() {
        let narrow = measure_text("A", 12.).max(measure_text("B", 24.)) + 1.;
        for width in [1000., narrow] {
            let (_, page) = flex_page(&format!(
                "<div style='width:{width}px'><a href=/a style='font-size:12px'>A </a><a href=/empty style='font-size:40px'>  </a><a href=/b style='font-size:24px'> B</a></div>"
            ));
            let runs: Vec<_> = page
                .items
                .iter()
                .filter_map(|item| match item {
                    DisplayItem::Text {
                        text, rect, size, ..
                    } => Some((text.as_str(), *rect, *size)),
                    _ => None,
                })
                .collect();
            assert_eq!(runs.len(), 2);
            assert_eq!((runs[0].0, runs[1].0), ("A", "B"));
            assert_eq!((runs[0].2, runs[1].2), (12., 24.));
            assert_eq!(page.links.len(), 2);
            assert_eq!(
                (page.links[0].href.as_str(), page.links[1].href.as_str()),
                ("/a", "/b")
            );
            for ((_, rect, _), link) in runs.iter().zip(&page.links) {
                assert_eq!(
                    (rect.x, rect.y, rect.width, rect.height),
                    (link.rect.x, link.rect.y, link.rect.width, link.rect.height)
                );
            }
            let (a, b) = (runs[0].1, runs[1].1);
            if width == narrow {
                assert_eq!(b.x, a.x);
                assert!(b.y > a.y);
            } else {
                assert_eq!(a.y, b.y);
                let gap = b.x - a.x - a.width;
                assert!((gap - measure_text(" ", 24.)).abs() < 0.001);
                // The pending gap belongs to neither adjacent link, and the
                // whitespace-only link at the intervening font has no target.
                assert!(page.links[0].rect.x + page.links[0].rect.width < page.links[1].rect.x);
            }
        }
    }

    #[test]
    fn whitespace_work_and_item_limits_fail_before_unbounded_output() {
        for source in [" ".repeat(1000), "\n".repeat(1000), "\u{2003}".repeat(1000)] {
            let style = Style {
                preserve_breaks: true,
                ..Style::default()
            };
            let error = text_case(&source, style, 1000., 100).err().unwrap();
            assert!(error.contains("work limit"));
        }
        let (items, links, _) =
            text_case(&" ".repeat(10000), Style::default(), 1000., 40000).unwrap();
        assert!(items.is_empty() && links.is_empty());
        let style = Style {
            pre: true,
            preserve_breaks: true,
            ..Style::default()
        };
        assert!(text_case(&" ".repeat(MAX_ITEMS + 1), style, 1000., 4_000_000).is_err());
    }

    fn flex_page(source: &str) -> (Document, LayoutResult) {
        let document =
            web_html::parse(&format!("<style>body{{margin:0}}</style>{source}")).unwrap();
        let styles = web_css::compute(&document).unwrap();
        let page = layout(&document, &styles.styles, &[], Viewport::default()).unwrap();
        (document, page)
    }

    fn colored_rect(page: &LayoutResult, color: Color) -> Rect {
        page.items
            .iter()
            .find_map(|item| match item {
                DisplayItem::Rect {
                    rect,
                    color: actual,
                } if *actual == color => Some(*rect),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn nowrap_flex_uses_definite_content_height_in_both_row_directions() {
        for direction in ["row", "row-reverse"] {
            for (alignment, expected) in [("center", 40.), ("flex-end", 80.)] {
                let (_, page) = flex_page(&format!(
                    "<div style='display:flex;flex-direction:{direction};height:100px;align-items:{alignment}'><div style='width:20px;height:20px;background:#123456'></div><div style='width:30px;height:20px;background:#654321'></div></div>"
                ));
                let first = colored_rect(&page, Color(18, 52, 86, 255));
                let second = colored_rect(&page, Color(101, 67, 33, 255));
                assert_eq!(first.y, expected);
                assert_eq!(second.y, expected);
                assert_eq!(first.height, 20.);
                assert_eq!(first.x, if direction == "row" { 0. } else { 30. });
            }
        }
    }

    #[test]
    fn nowrap_flex_accounts_for_border_box_padding_margins_and_percent_height() {
        for (height, parent) in [("100px", ""), ("50%", "height:200px")] {
            let (_, page) = flex_page(&format!(
                "<section style='{parent}'><div style='display:flex;height:{height};box-sizing:border-box;padding:10px;border:2px solid black;align-items:center'><div style='width:20px;height:20px;margin:5px 0 15px;background:#123456'></div></div></section>"
            ));
            let rect = colored_rect(&page, Color(18, 52, 86, 255));
            // Content line 76, outer item 40, content origin 12, top margin 5.
            assert_eq!(rect.y, 35.);
            assert_eq!(rect.height, 20.);
        }
    }

    #[test]
    fn nowrap_flex_overflow_can_align_above_the_content_origin() {
        for overflow in ["visible", "hidden"] {
            for (alignment, expected) in [("center", -20.), ("flex-end", -40.)] {
                let (_, page) = flex_page(&format!(
                    "<div style='display:flex;height:100px;align-items:{alignment};overflow:{overflow}'><div style='height:140px;width:40px;background:#123456'><a href=target>Link</a></div></div>"
                ));
                let rect = colored_rect(&page, Color(18, 52, 86, 255));
                assert_eq!(rect.y, expected);
                assert_eq!(rect.height, 140.);
                if overflow == "hidden" {
                    assert!(page.links.iter().all(|link| link.rect.y >= 0. && link.rect.y + link.rect.height <= 100.));
                    assert!(page.items.iter().any(|item| matches!(item, DisplayItem::PushClip { rect } if rect.y == 0. && rect.height == 100.)));
                } else {
                    assert!(page.links.iter().any(|link| link.rect.y < 0.));
                }
            }
        }
    }

    #[test]
    fn nowrap_flex_translates_descendant_controls_links_and_absolute_boxes() {
        let content = "<div style='position:relative;height:20px;width:200px;background:#123456'><a href=target>Link</a><input name=field style='height:10px;padding:0;border:0'><div style='position:absolute;top:3px;left:7px;width:5px;height:6px;background:#654321'></div></div>";
        let (_, baseline) = flex_page(&format!(
            "<div style='display:flex;height:100px;align-items:flex-start'>{content}</div>"
        ));
        let (_, centered) = flex_page(&format!(
            "<div style='display:flex;height:100px;align-items:center'>{content}</div>"
        ));
        assert_eq!(baseline.items.len(), centered.items.len());
        assert_eq!(colored_rect(&centered, Color(101, 67, 33, 255)).y, 43.);
        assert_eq!(baseline.links.len(), centered.links.len());
        for (before, after) in baseline.links.iter().zip(&centered.links) {
            assert_eq!(after.rect.y, before.rect.y + 40.);
        }
        assert!(!centered.links.is_empty());
        let control_y = |page: &LayoutResult| {
            page.items
                .iter()
                .find_map(|item| match item {
                    DisplayItem::Control { rect, name, .. } if name == "field" => Some(rect.y),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(control_y(&centered), control_y(&baseline) + 40.);
    }

    #[test]
    fn nowrap_flex_stretch_updates_metrics_borders_and_only_owned_clips() {
        let (document, page) = flex_page(
            "<div style='display:flex;height:100px'><div id=item style='position:relative;width:100px;border:2px solid red;overflow:hidden;background:#123456'><div style='height:10px;overflow:hidden'><a href=inside>Inside</a></div><a href=absolute style='position:absolute;top:60px;left:0'>Absolute</a></div></div>",
        );
        let item = document
            .nodes
            .iter()
            .enumerate()
            .find_map(|(id, _)| {
                document
                    .element(id)
                    .filter(|e| e.attr("id") == Some("item"))
                    .map(|_| id)
            })
            .unwrap();
        assert_eq!(page.boxes[item].unwrap().height, 100.);
        assert_eq!(colored_rect(&page, Color(18, 52, 86, 255)).height, 100.);
        assert!(page.items.iter().any(|item| matches!(item, DisplayItem::Rect { rect, color } if *color == Color(255, 0, 0, 255) && rect.y == 98. && rect.height == 2.)));
        let heights: Vec<_> = page
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::PushClip { rect } => Some(rect.height),
                _ => None,
            })
            .collect();
        assert_eq!(heights.iter().filter(|&&height| height == 96.).count(), 2);
        assert!(heights.contains(&10.));
        assert!(page.links.iter().any(|link| link.href == "absolute"));
    }

    #[test]
    fn flex_indefinite_wrapped_column_and_grid_keep_existing_sizing() {
        for container in [
            "display:flex;align-items:center",
            "display:flex;align-items:center;min-height:100px",
            "display:flex;align-items:center;height:100px;flex-wrap:wrap",
            "display:flex;align-items:center;flex-direction:column;height:100px",
            "display:grid;align-items:center;height:100px",
        ] {
            let (_, page) = flex_page(&format!(
                "<div style='{container}'><div style='height:20px;width:20px;background:#123456'></div></div>"
            ));
            assert_eq!(
                colored_rect(&page, Color(18, 52, 86, 255)).y,
                0.,
                "{container}"
            );
        }
        let (_, page) = flex_page(
            "<div style='min-height:200px'><div style='display:flex;height:50%;align-items:center'><div style='height:20px;background:#123456'></div></div></div>",
        );
        assert_eq!(colored_rect(&page, Color(18, 52, 86, 255)).y, 0.);
    }

    #[test]
    fn nowrap_flex_keeps_min_height_and_shrinking_outside_the_alignment_contract() {
        let (_, page) = flex_page(
            "<div style='display:flex;height:40px;min-height:100px;align-items:center;background:#654321'><div style='height:20px;background:#123456'></div></div>",
        );
        assert_eq!(colored_rect(&page, Color(18, 52, 86, 255)).y, 10.);
        assert_eq!(colored_rect(&page, Color(101, 67, 33, 255)).height, 100.);
        let (_, page) = flex_page(
            "<div style='display:flex;height:20px'><div style='background:#123456'><div style='height:100px'></div></div></div>",
        );
        assert_eq!(colored_rect(&page, Color(18, 52, 86, 255)).height, 100.);
    }

    #[test]
    fn implicit_body_keeps_user_agent_margins_and_head_has_no_box() {
        let document = web_html::parse(
            "<title>Hidden metadata</title><div style='height:10px;background:#123456'></div>",
        )
        .unwrap();
        let styles = web_css::compute(&document).unwrap();
        let result = layout(
            &document,
            &styles.styles,
            &[],
            Viewport {
                width: 320.,
                height: 200.,
            },
        )
        .unwrap();
        let html = document.document_element().unwrap();
        let head = document.head().unwrap();
        let body = document.body().unwrap();
        assert!(result.boxes[head].is_none());
        assert_eq!(result.boxes[html].unwrap().width, 320.);
        assert_eq!(result.boxes[body].unwrap().width, 304.);
        let rect = result
            .items
            .iter()
            .find_map(|item| match item {
                DisplayItem::Rect { rect, color } if *color == Color(18, 52, 86, 255) => Some(rect),
                _ => None,
            })
            .unwrap();
        assert_eq!(rect.x, 8.);
        assert_eq!(rect.y, 8.);
        assert_eq!(rect.width, 304.);
        assert_eq!(rect.height, 10.);
        assert!(!result.items.iter().any(|item| matches!(
            item, DisplayItem::Text { text, .. } if text.contains("metadata")
        )));
    }
    #[test]
    fn ancestor_end_tags_do_not_paint_inert_links_controls_or_images() {
        for tag in ["template", "noscript"] {
            let source = format!(
                "<style>{tag}{{display:block!important}}</style><body><{tag}></body></html><a href=wrong>secret</a><input value=secret><img src=wrong alt=secret></{tag}><div>Live</div>"
            );
            let document = web_html::parse(&source).unwrap();
            let styles = web_css::compute(&document).unwrap();
            let result = layout(&document, &styles.styles, &[], Viewport::default()).unwrap();
            assert!(result.links.is_empty(), "{tag}");
            assert!(
                !result.items.iter().any(|item| matches!(
                    item,
                    DisplayItem::Control { .. } | DisplayItem::Image { .. }
                )),
                "{tag}"
            );
            assert!(
                !result.items.iter().any(|item| matches!(
                    item, DisplayItem::Text { text, .. } if text.contains("secret")
                )),
                "{tag}"
            );
            assert!(
                result.items.iter().any(|item| matches!(
                    item, DisplayItem::Text { text, .. } if text == "Live"
                )),
                "{tag}"
            );
            assert!(result.warnings.is_empty(), "{tag}: {:?}", result.warnings);
        }
    }
    #[test]
    fn inactive_subtrees_do_not_paint_even_with_author_display_overrides() {
        let document=web_html::parse("<style>template,noscript{display:block!important}</style><template><a href=wrong>Template secret</a></template><noscript><input value=wrong>Fallback secret</noscript><p>Live text</p>").unwrap();
        let styles = web_css::compute(&document).unwrap();
        let result = layout(&document, &styles.styles, &[], Viewport::default()).unwrap();
        assert!(result.links.is_empty());
        assert!(
            !result
                .items
                .iter()
                .any(|item| matches!(item, DisplayItem::Control { .. }))
        );
        assert!(
            !result
                .items
                .iter()
                .any(|item| matches!(item,DisplayItem::Text{text,..} if text.contains("secret")))
        );
        assert!(
            result
                .items
                .iter()
                .any(|item| matches!(item,DisplayItem::Text{text,..} if text.contains("Live")))
        );
    }
    #[test]
    fn font_metrics_are_proportional_and_finite() {
        assert!(measure_text("WWW", 16.0) > measure_text("iii", 16.0));
        assert!(measure_text("Ação", 16.0).is_finite());
    }
    #[test]
    fn minimum_height_uses_the_selected_box_sizing_for_blocks_and_controls() {
        for (sizing, expected) in [("border-box", 40.), ("content-box", 60.)] {
            let html = format!(
                "<style>.sized{{box-sizing:{sizing};min-height:40px;padding:10px;border:0;font:14px/18px sans-serif}}</style><a href=/login class=sized style='display:inline-block;background:#123456'>Entrar</a><input class=sized name=go type=submit value=Buscar>"
            );
            let dom = web_html::parse(&html).unwrap();
            let styles = web_css::compute(&dom).unwrap();
            let page = layout(&dom, &styles.styles, &[], Viewport::default()).unwrap();
            let block_height = page.items.iter().find_map(|item| match item {
                DisplayItem::Rect { rect, color } if *color == Color(18, 52, 86, 255) => {
                    Some(rect.height)
                }
                _ => None,
            });
            assert_eq!(block_height, Some(expected), "{sizing}");
            let control_height = page.items.iter().find_map(|item| match item {
                DisplayItem::Control { rect, name, .. } if name == "go" => Some(rect.height),
                _ => None,
            });
            assert_eq!(control_height, Some(expected), "{sizing}");
        }
    }
    #[test]
    fn border_box_height_never_produces_negative_content_space() {
        let dom = web_html::parse(
            "<style>.tiny{box-sizing:border-box;height:1px;min-height:1px;padding:10px;border:2px solid black}</style><div class=tiny style='background:#123456'></div><input class=tiny name=go type=submit value=Buscar><input class=tiny type=hidden name=secret>",
        )
        .unwrap();
        let styles = web_css::compute(&dom).unwrap();
        let page = layout(&dom, &styles.styles, &[], Viewport::default()).unwrap();
        assert!(page.items.iter().any(|item| matches!(item,
            DisplayItem::Rect {rect,color} if *color == Color(18,52,86,255) && rect.height == 24.
        )));
        assert!(page.items.iter().any(|item| matches!(item,
            DisplayItem::Control {rect,name,..} if name == "go" && rect.height == 24.
        )));
        assert!(page.items.iter().any(|item| matches!(item,
            DisplayItem::Control {rect,name,..} if name == "secret" && rect.height == 0. && rect.width == 0.
        )));
    }
    #[test]
    fn form_controls_preserve_ownership_values_and_hidden_fields() {
        let dom=web_html::parse("<form id=search action='/search' method=get><input type=hidden name=hl value=pt-BR><input name=q placeholder=Pesquisar><input type=checkbox name=safe checked><button name=go value=yes>Buscar</button></form><textarea form=search name=notes>Olá</textarea>").unwrap();
        let styles = web_css::compute(&dom).unwrap();
        let page = layout(&dom, &styles.styles, &[], Viewport::default()).unwrap();
        let controls = page
            .items
            .iter()
            .filter_map(|item| {
                if let DisplayItem::Control {
                    id,
                    kind,
                    form,
                    name,
                    value,
                    rect,
                    ..
                } = item
                {
                    Some((*id, *kind, form.as_ref(), name, value, rect))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(controls.len(), 5);
        let form_id = controls[0].2.unwrap().id;
        assert!(
            controls
                .iter()
                .all(|control| control.2.unwrap().id == form_id)
        );
        assert_eq!(controls[0].1, ControlKind::Hidden);
        assert_eq!(controls[0].4, "pt-BR");
        assert_eq!(controls[0].5.width, 0.);
        assert_eq!(controls[1].1, ControlKind::Text);
        assert_eq!(controls[2].4, "on");
        assert_eq!(controls[4].4, "Olá");
        assert!(controls[1].5.height >= 20.);
    }
    #[test]
    fn invalid_viewport_is_rejected() {
        let d = Document::default();
        assert!(
            layout(
                &d,
                &[Style::default()],
                &[],
                Viewport {
                    width: f32::NAN,
                    height: 50.0
                }
            )
            .is_err()
        );
    }
}
