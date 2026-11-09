//! Static CSS subset: compound/descendant/child selectors, cascade and inheritance.
use browser_ipc::{Color, StylesheetAsset, Viewport};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use web_dom::{Document, Element, NodeId};

pub mod selector_query;

const MAX_CSS_BYTES: usize = 256 * 1024;
const MAX_RULES: usize = 1024;
const MAX_CUSTOM_PROPERTY_BYTES: usize = 16 * 1024;
const MAX_MATERIALIZED_VARIABLE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CASCADE_COPY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Display {
    None,
    Block,
    Inline,
    InlineBlock,
    Flex,
    Grid,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Alignment {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Stretch,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridTrack {
    Fixed(Length),
    Fraction(f32),
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Auto,
    Px(f32),
    Percent(f32),
}
impl Length {
    pub fn resolve(self, available: f32) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(n) => Some(n),
            Self::Percent(n) => Some(available * n / 100.0),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Style {
    pub display: Display,
    pub position: Position,
    pub inset: [Length; 4],
    pub visible: bool,
    pub color: Color,
    pub background: Color,
    pub font_size: f32,
    pub bold: bool,
    pub line_height: f32,
    pub margin: [f32; 4],
    pub auto_margin: [bool; 4],
    pub padding: [f32; 4],
    pub width: Length,
    pub height: Length,
    pub max_width: Length,
    pub min_width: Length,
    pub min_height: Length,
    pub flex_column: bool,
    pub flex_reverse: bool,
    pub flex_wrap: bool,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Length,
    pub justify_content: Alignment,
    pub align_items: Alignment,
    pub gap: [f32; 2],
    pub grid_columns: Vec<GridTrack>,
    pub border_width: f32,
    pub border_color: Color,
    pub border_box: bool,
    pub text_align: Alignment,
    /// Preserve ASCII spaces/tabs (tabs remain single spaces in this subset).
    pub pre: bool,
    /// Preserve normalized LF/CRLF/CR breaks independently of spaces.
    pub preserve_breaks: bool,
    pub nowrap: bool,
    pub empty_clip: bool,
    pub overflow_clip: bool,
    pub variables: Arc<BTreeMap<String, String>>,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            position: Position::Static,
            inset: [Length::Auto; 4],
            visible: true,
            color: Color(32, 35, 42, 255),
            background: Color(0, 0, 0, 0),
            font_size: 16.0,
            bold: false,
            line_height: 1.5,
            margin: [0.0; 4],
            auto_margin: [false; 4],
            padding: [0.0; 4],
            width: Length::Auto,
            height: Length::Auto,
            max_width: Length::Auto,
            min_width: Length::Auto,
            min_height: Length::Auto,
            flex_column: false,
            flex_reverse: false,
            flex_wrap: false,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Length::Auto,
            justify_content: Alignment::Start,
            align_items: Alignment::Stretch,
            gap: [0.0; 2],
            grid_columns: Vec::new(),
            border_width: 0.0,
            border_color: Color(32, 35, 42, 255),
            border_box: false,
            text_align: Alignment::Start,
            pre: false,
            preserve_breaks: false,
            nowrap: false,
            empty_clip: false,
            overflow_clip: false,
            variables: Arc::new(BTreeMap::new()),
        }
    }
}

#[derive(Debug, Clone)]
struct Declaration {
    name: String,
    value: String,
    important: bool,
}
#[derive(Debug)]
struct Rule {
    selector: Selector,
    declarations: Arc<Vec<Declaration>>,
}
// Specificity is lexicographic: lower components never carry into IDs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Specificity {
    ids: u32,
    classes: u32,
    types: u32,
}
impl Specificity {
    const ID: Self = Self {
        ids: 1,
        classes: 0,
        types: 0,
    };
    const CLASS: Self = Self {
        ids: 0,
        classes: 1,
        types: 0,
    };
    const TYPE: Self = Self {
        ids: 0,
        classes: 0,
        types: 1,
    };

    // Overflow rejects the selector through parse_nested's Option, never wraps.
    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            ids: self.ids.checked_add(other.ids)?,
            classes: self.classes.checked_add(other.classes)?,
            types: self.types.checked_add(other.types)?,
        })
    }
}
// Importance precedes inline style; source order is a pair, not a packed integer.
type CascadePriority = (bool, bool, Specificity, (usize, usize));

#[derive(Debug)]
struct Selector {
    parts: Vec<Part>,
    specificity: Specificity,
}
#[derive(Debug)]
struct Part {
    compound: Compound,
    relation: Combinator,
}
#[derive(Debug, Clone, Copy)]
enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}
#[derive(Debug, Default)]
struct Compound {
    tag: Option<String>,
    ids: Vec<String>,
    classes: Vec<String>,
    root: bool,
    attributes: Vec<AttributeSelector>,
    pseudos: Vec<Pseudo>,
}
#[derive(Debug)]
enum Pseudo {
    Not(Vec<Selector>),
    Any(Vec<Selector>),
    Position {
        a: i64,
        b: i64,
        reverse: bool,
        of_type: bool,
    },
    Only {
        of_type: bool,
    },
    Empty,
    Checked,
    Disabled(bool),
    Link,
    Language(Vec<String>),
}
#[derive(Debug)]
struct AttributeSelector {
    name: String,
    operation: &'static str,
    value: String,
    ascii_insensitive: bool,
}

/// Constructed once for a cascade pass. Adjacent-sibling queries must not
/// repeatedly scan the parent's child list to locate their subject.
struct SiblingIndex {
    previous_element: Vec<Option<NodeId>>,
}
impl SiblingIndex {
    fn new(document: &Document, budget: &mut usize) -> Result<Self, String> {
        *budget = budget
            .checked_sub(document.nodes.len())
            .ok_or("CSS sibling index work limit exceeded")?;
        let mut previous_element = vec![None; document.nodes.len()];
        for parent in &document.nodes {
            let mut previous = None;
            for &child in &parent.children {
                consume_match_budget(budget)?;
                if document.element(child).is_some() {
                    previous_element[child] = previous;
                    previous = Some(child);
                }
            }
        }
        Ok(Self { previous_element })
    }
    fn previous(&self, id: NodeId) -> Option<NodeId> {
        self.previous_element.get(id).copied().flatten()
    }
}

/// The rightmost compound is a necessary condition for a selector match.
/// Indexing it avoids re-scanning every stylesheet rule for every DOM node.
struct RuleIndex<'a> {
    ids: HashMap<&'a str, Vec<usize>>,
    classes: HashMap<&'a str, Vec<usize>>,
    tags: HashMap<&'a str, Vec<usize>>,
    universal: Vec<usize>,
}
impl<'a> RuleIndex<'a> {
    fn new(rules: &'a [Rule], budget: &mut usize) -> Result<Self, String> {
        let mut index = Self {
            ids: HashMap::new(),
            classes: HashMap::new(),
            tags: HashMap::new(),
            universal: Vec::new(),
        };
        for (number, rule) in rules.iter().enumerate() {
            let compound = &rule
                .selector
                .parts
                .last()
                .expect("parsed selector has a compound")
                .compound;
            if let Some(id) = compound.ids.first().map(String::as_str) {
                charge_selector_bytes(budget, id.len())?;
                index.ids.entry(id).or_default().push(number);
            } else if let Some(class) = compound.classes.first() {
                charge_selector_bytes(budget, class.len())?;
                index.classes.entry(class).or_default().push(number);
            } else if let Some(tag) = compound.tag.as_deref() {
                charge_selector_bytes(budget, tag.len())?;
                index.tags.entry(tag).or_default().push(number);
            } else {
                index.universal.push(number);
            }
        }
        Ok(index)
    }
    fn candidates(
        &self,
        element: &Element,
        generation: usize,
        seen: &mut [usize],
        budget: &mut usize,
    ) -> Result<Vec<usize>, String> {
        let classes = element.attr("class").unwrap_or("");
        // Account for tokenization and hashing once per element, rather than
        // charging the entire class attribute for unrelated stylesheet rules.
        charge_selector_bytes(
            budget,
            element
                .tag
                .len()
                .saturating_add(element.attr("id").map_or(0, str::len))
                .saturating_add(classes.len().saturating_mul(2)),
        )?;
        let mut candidates = Vec::new();
        let mut add = |list: Option<&Vec<usize>>| -> Result<(), String> {
            if let Some(list) = list {
                for &number in list {
                    consume_match_budget(budget)?;
                    if seen[number] != generation {
                        seen[number] = generation;
                        candidates.push(number);
                    }
                }
            }
            Ok(())
        };
        add(Some(&self.universal))?;
        add(self.tags.get(element.tag.as_str()))?;
        if let Some(id) = element.attr("id") {
            add(self.ids.get(id))?;
        }
        for class in classes.split_ascii_whitespace() {
            add(self.classes.get(class))?;
        }
        candidates.sort_unstable();
        Ok(candidates)
    }
}
fn charge_selector_bytes(budget: &mut usize, bytes: usize) -> Result<(), String> {
    *budget = budget
        .checked_sub(bytes)
        .ok_or("CSS selector string work limit exceeded")?;
    Ok(())
}
fn selector_string_eq(left: &str, right: &str, budget: &mut usize) -> Result<bool, String> {
    // String equality rejects different lengths without inspecting payloads.
    charge_selector_bytes(
        budget,
        if left.len() == right.len() {
            left.len()
        } else {
            1
        },
    )?;
    Ok(left == right)
}

pub struct StyleSet {
    pub styles: Vec<Style>,
    pub warnings: Vec<String>,
}

pub fn compute(document: &Document) -> Result<StyleSet, String> {
    compute_for_viewport(document, Viewport::default())
}

pub fn compute_for_viewport(document: &Document, viewport: Viewport) -> Result<StyleSet, String> {
    compute_with_stylesheets(document, &[], viewport)
}

/// Parse stylesheet bytes already authorized by the broker, at their original
/// DOM positions. A link's href is an opaque lookup key, never a network request.
pub fn compute_with_stylesheets(
    document: &Document,
    stylesheets: &[StylesheetAsset],
    viewport: Viewport,
) -> Result<StyleSet, String> {
    if stylesheets.len() > browser_ipc::MAX_STYLESHEETS
        || stylesheets
            .iter()
            .any(|asset| asset.source.len() > browser_ipc::MAX_STYLESHEET_BYTES)
    {
        return Err("Authorized stylesheet asset limit exceeded".into());
    }
    let mut rules = Vec::new();
    let mut bytes: usize = 0;
    let mut warnings = Vec::new();
    for (id, e) in document.elements() {
        if document.is_inactive_subtree(id) || !matches!(e.tag.as_str(), "style" | "link") {
            continue;
        }
        if e.attr("type").is_some_and(|kind| {
            !kind.trim().is_empty() && !kind.trim().eq_ignore_ascii_case("text/css")
        }) || e.attributes.contains_key("disabled")
            || e.attr("media").is_some_and(|media| {
                !media.trim().is_empty() && (media.len() > 1024 || !media_matches(media, viewport))
            })
        {
            continue;
        }
        let css = if e.tag == "style" {
            document.text_content(id)
        } else {
            let rel = e
                .attr("rel")
                .unwrap_or("")
                .split_ascii_whitespace()
                .collect::<Vec<_>>();
            if !rel
                .iter()
                .any(|token| token.eq_ignore_ascii_case("stylesheet"))
                || rel
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("alternate"))
            {
                continue;
            }
            let href = e.attr("href").unwrap_or("");
            let Some(asset) = stylesheets.iter().find(|asset| asset.href == href) else {
                if warnings.len() < 64 {
                    warnings.push(format!(
                        "Stylesheet not supplied by resource broker: {}",
                        href.chars().take(160).collect::<String>()
                    ));
                }
                continue;
            };
            if asset.source.len() > MAX_CSS_BYTES.saturating_sub(bytes) {
                css_warning(
                    &mut warnings,
                    "Stylesheet omitted: cumulative CSS byte limit reached",
                );
                continue;
            }
            asset.source.clone()
        };
        if css.len() > MAX_CSS_BYTES.saturating_sub(bytes) {
            css_warning(
                &mut warnings,
                "Stylesheet omitted: cumulative CSS byte limit reached",
            );
            continue;
        }
        bytes += css.len();
        if let Err(error) = parse_stylesheet(&css, &mut rules, &mut warnings, viewport, 0) {
            // Stop at the existing structural budget. Retain the bounded prefix
            // instead of discarding an otherwise readable HTML document.
            if error == "CSS rule limit exceeded"
                || error == "CSS conditional nesting limit exceeded"
            {
                css_warning(
                    &mut warnings,
                    "Remaining CSS omitted: rule or conditional nesting limit reached",
                );
                break;
            }
            return Err(error);
        }
    }
    let mut styles = vec![Style::default(); document.nodes.len()];
    let mut match_budget = 4_000_000usize;
    let rule_index = RuleIndex::new(&rules, &mut match_budget)?;
    let siblings = SiblingIndex::new(document, &mut match_budget)?;
    let mut candidate_seen = vec![0usize; rules.len()];
    let mut cascade_budget = MAX_CASCADE_COPY_BYTES;
    let mut resolution_budget = MAX_CASCADE_COPY_BYTES;
    let mut variables_budget = MAX_MATERIALIZED_VARIABLE_BYTES;
    let mut root_font = 16.;
    for (id, node) in document.nodes.iter().enumerate() {
        if document.is_inactive_subtree(id) {
            styles[id].display = Display::None;
            continue;
        }
        let parent = node.parent.map(|n| styles[n].clone()).unwrap_or_default();
        let Some(element) = document.element(id) else {
            styles[id] = parent;
            continue;
        };
        let mut style = Style {
            color: parent.color,
            font_size: parent.font_size,
            bold: parent.bold,
            line_height: parent.line_height,
            text_align: parent.text_align,
            visible: parent.visible,
            pre: parent.pre,
            preserve_breaks: parent.preserve_breaks,
            nowrap: parent.nowrap,
            variables: parent.variables.clone(),
            ..Style::default()
        };
        user_agent(element, &mut style);
        let mut winners: BTreeMap<String, (CascadePriority, String)> = BTreeMap::new();
        for order in
            rule_index.candidates(element, id + 1, &mut candidate_seen, &mut match_budget)?
        {
            let rule = &rules[order];
            if rule
                .selector
                .matches(document, id, &siblings, &mut match_budget)?
            {
                for (declaration_index, declaration) in rule.declarations.iter().enumerate() {
                    cascade_budget = cascade_budget
                        .checked_sub(declaration.name.len() + declaration.value.len())
                        .ok_or("CSS cascade copy budget exceeded")?;
                    let key = (
                        declaration.important,
                        false,
                        rule.selector.specificity,
                        (order, declaration_index),
                    );
                    let entry = winners
                        .entry(declaration.name.clone())
                        .or_insert((key, declaration.value.clone()));
                    if key >= entry.0 {
                        *entry = (key, declaration.value.clone());
                    }
                }
            }
        }
        if let Some(inline) = element.attr("style") {
            for (declaration_index, declaration) in declarations(inline).into_iter().enumerate() {
                cascade_budget = cascade_budget
                    .checked_sub(declaration.name.len() + declaration.value.len())
                    .ok_or("CSS cascade copy budget exceeded")?;
                let key = (
                    declaration.important,
                    true,
                    Specificity::default(),
                    (0, declaration_index),
                );
                let entry = winners
                    .entry(declaration.name)
                    .or_insert((key, declaration.value.clone()));
                if key >= entry.0 {
                    *entry = (key, declaration.value);
                }
            }
        }
        if winners.keys().any(|name| name.starts_with("--")) {
            // Inherited maps share allocation. Bound all copy-on-write materialization
            // before cloning so chains of custom-property overrides cannot amplify memory.
            let mut materialized_bytes: usize =
                style.variables.iter().map(|(k, v)| k.len() + v.len()).sum();
            for (name, (_, value)) in &winners {
                if name.starts_with("--") {
                    if value.len() > MAX_CUSTOM_PROPERTY_BYTES {
                        return Err("CSS custom property exceeds 16 KiB".into());
                    }
                    if let Some(previous) = style.variables.get(name) {
                        materialized_bytes -= name.len() + previous.len();
                    }
                    materialized_bytes += name.len() + value.len();
                }
            }
            variables_budget = variables_budget
                .checked_sub(materialized_bytes)
                .ok_or("CSS custom property materialization budget exceeded")?;
            let variables = Arc::make_mut(&mut style.variables);
            for (name, (_, value)) in &winners {
                if name.starts_with("--") {
                    variables.insert(name.clone(), value.clone());
                }
            }
        }
        // Font size must be computed before lengths expressed in em.
        if let Some((_, value)) = winners.get("font-size") {
            let value = resolve_vars_budgeted(value, &style.variables, &mut resolution_budget)?;
            let value = resolve_relative_units(&value, viewport, root_font);
            if let Some(n) =
                length(&value, parent.font_size).and_then(|n| n.resolve(parent.font_size))
            {
                style.font_size = n.clamp(6.0, 256.0);
            }
        }
        // Expand shorthands in cascade priority order, preserving longhand precedence.
        let mut ordered = winners.into_iter().collect::<Vec<_>>();
        ordered.sort_by_key(|(_, (priority, _))| *priority);
        for (name, (_, value)) in ordered {
            if name.starts_with("--") || !supported_property(&name) {
                continue;
            }
            let value = resolve_vars_budgeted(&value, &style.variables, &mut resolution_budget)?;
            let value = resolve_relative_units(&value, viewport, root_font);
            resolution_budget = resolution_budget
                .checked_sub(value.len())
                .ok_or("CSS relative length resolution budget exceeded")?;
            apply(&mut style, &name, &value, parent.font_size);
        }
        if style.position == Position::Absolute && style.empty_clip {
            style.visible = false;
        }
        if element.tag == "html" {
            root_font = style.font_size;
        }
        styles[id] = style;
    }
    Ok(StyleSet { styles, warnings })
}
fn css_warning(warnings: &mut Vec<String>, message: &str) {
    if warnings.iter().any(|warning| warning == message) {
        return;
    }
    if warnings.len() >= 64 {
        warnings[63] = message.into();
    } else {
        warnings.push(message.into());
    }
}

fn user_agent(e: &Element, s: &mut Style) {
    if web_html::is_block(&e.tag) {
        s.display = Display::Block;
    }
    match e.tag.as_str() {
        "center" => {
            s.display = Display::Block;
            s.text_align = Alignment::Center;
        }
        "th" => s.bold = true,
        "input" | "button" | "textarea" | "select" => {
            s.border_width = 1.;
            s.border_color = Color(155, 165, 177, 255);
            s.padding = [2., 4., 2., 4.];
            let button = e.tag == "button"
                || (e.tag == "input"
                    && e.attr("type").is_some_and(|kind| {
                        matches!(kind.to_ascii_lowercase().as_str(), "submit" | "button")
                    }));
            s.background = if button {
                Color(242, 243, 245, 255)
            } else {
                Color(255, 255, 255, 255)
            };
            if button {
                s.padding[1] = 8.;
                s.padding[3] = 8.;
            }
            if button || e.tag == "select" {
                s.border_box = true;
            }
            if e.tag == "input"
                && e.attr("type").is_some_and(|kind| {
                    matches!(kind.to_ascii_lowercase().as_str(), "checkbox" | "hidden")
                })
            {
                s.padding = [0.; 4];
                s.border_width = 0.;
                s.background = Color(0, 0, 0, 0);
            }
        }
        "table" => {
            s.gap = [2.; 2];
            if let Some(spacing) = e
                .attr("cellspacing")
                .and_then(|n| n.parse::<f32>().ok())
                .filter(|n| n.is_finite())
            {
                s.gap = [spacing.clamp(0., 256.); 2];
            }
            if s.text_align == Alignment::Center {
                s.auto_margin[1] = true;
                s.auto_margin[3] = true;
            }
        }
        "head" | "style" | "script" | "title" | "meta" | "link" | "base" | "template"
        | "option" | "optgroup" | "noscript" | "datalist" => s.display = Display::None,
        "dialog" if !e.attributes.contains_key("open") => s.display = Display::None,
        "body" => s.margin = [8.0; 4],
        "p" | "blockquote" | "ul" | "ol" | "figure" | "pre" => {
            s.margin[0] = s.font_size;
            s.margin[2] = s.font_size;
        }
        "h1" => {
            s.font_size = 32.0;
            s.bold = true;
            s.margin = [21.0, 0.0, 21.0, 0.0];
        }
        "h2" => {
            s.font_size = 24.0;
            s.bold = true;
            s.margin = [20.0, 0.0, 20.0, 0.0];
        }
        "h3" => {
            s.font_size = 20.0;
            s.bold = true;
            s.margin = [16.0, 0.0, 16.0, 0.0];
        }
        "h4" | "h5" | "h6" | "b" | "strong" => s.bold = true,
        "small" => s.font_size *= 0.83,
        "a" => s.color = Color(63, 91, 207, 255),
        "hr" => {
            s.height = Length::Px(1.0);
            s.background = Color(205, 208, 216, 255);
            s.margin = [8.0, 0.0, 8.0, 0.0];
        }
        _ => {}
    }
    if let Some(align) = e.attr("align") {
        match align.to_ascii_lowercase().as_str() {
            "center" => {
                s.text_align = Alignment::Center;
                if matches!(e.tag.as_str(), "table" | "div") {
                    s.auto_margin[1] = true;
                    s.auto_margin[3] = true;
                }
            }
            "right" => s.text_align = Alignment::End,
            "left" => s.text_align = Alignment::Start,
            _ => {}
        }
    }
    if e.attributes.contains_key("nowrap") {
        s.nowrap = true;
    }
    if matches!(e.tag.as_str(), "table" | "td" | "th")
        && let Some(width) = e.attr("width")
    {
        s.width = length(width, s.font_size)
            .or_else(|| {
                width
                    .parse::<f32>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(|n| Length::Px(n.clamp(0., 16384.)))
            })
            .unwrap_or(Length::Auto);
    }
    if e.tag == "pre" {
        s.pre = true;
        s.preserve_breaks = true;
        s.nowrap = true;
    }
    if matches!(e.tag.as_str(), "ul" | "ol" | "blockquote") {
        s.padding[3] = 24.0;
    }
    if e.attributes.contains_key("hidden") {
        s.display = Display::None;
    }
}

fn supported_property(name: &str) -> bool {
    matches!(
        name,
        "position"
            | "inset"
            | "top"
            | "right"
            | "bottom"
            | "left"
            | "visibility"
            | "clip"
            | "display"
            | "color"
            | "background-color"
            | "background"
            | "font-size"
            | "font-weight"
            | "font"
            | "line-height"
            | "width"
            | "height"
            | "max-width"
            | "min-width"
            | "min-height"
            | "flex-direction"
            | "flex-wrap"
            | "flex-grow"
            | "flex-shrink"
            | "flex-basis"
            | "flex"
            | "justify-content"
            | "align-items"
            | "gap"
            | "border-spacing"
            | "row-gap"
            | "column-gap"
            | "grid-template-columns"
            | "margin"
            | "padding"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
            | "border"
            | "border-width"
            | "border-color"
            | "border-style"
            | "box-sizing"
            | "text-align"
            | "white-space"
            | "overflow"
    )
}

fn apply(s: &mut Style, name: &str, value: &str, parent_font: f32) {
    let v = value.trim();
    let em = s.font_size;
    match name {
        "position" => match v {
            "static" => s.position = Position::Static,
            "relative" => s.position = Position::Relative,
            "absolute" => s.position = Position::Absolute,
            _ => {}
        },
        "inset" => {
            if let Some(values) = edges(v, em) {
                s.inset = values;
            }
        }
        "top" | "right" | "bottom" | "left" => {
            if let Some(value) = length(v, em) {
                s.inset[edge_index(name)] = value;
            }
        }
        "visibility" => match v {
            "visible" => s.visible = true,
            "hidden" | "collapse" => s.visible = false,
            _ => {}
        },
        "clip" => {
            if v == "auto" {
                s.empty_clip = false;
            } else if let Some(inner) = v.strip_prefix("rect(").and_then(|v| v.strip_suffix(')')) {
                let values = inner
                    .replace(',', " ")
                    .split_whitespace()
                    .map(|part| length(part, em).and_then(|n| n.resolve(0.)))
                    .collect::<Option<Vec<_>>>();
                if let Some(values) = values
                    && let [top, right, bottom, left] = values.as_slice()
                {
                    s.empty_clip = right <= left || bottom <= top;
                }
            }
        }
        "display" => match v {
            "none" => s.display = Display::None,
            "block" => s.display = Display::Block,
            "inline" => s.display = Display::Inline,
            "inline-block" => s.display = Display::InlineBlock,
            "flex" | "inline-flex" => s.display = Display::Flex,
            "grid" | "inline-grid" => s.display = Display::Grid,
            _ => {}
        },
        "color" => {
            if let Some(c) = color(v) {
                s.color = c;
            }
        }
        "background-color" => {
            if let Some(c) = color(v) {
                s.background = c;
            }
        }
        "background" => {
            // The shorthand resets color even when its image is unsupported.
            s.background = color(v)
                .or_else(|| v.split_whitespace().filter_map(color).next_back())
                .unwrap_or(Color(0, 0, 0, 0));
        }
        "font-size" => {
            if let Some(n) = length(v, parent_font).and_then(|n| n.resolve(parent_font)) {
                s.font_size = n.clamp(6.0, 256.0);
            }
        }
        "font-weight" => {
            s.bold = matches!(v, "bold" | "bolder") || v.parse::<u32>().is_ok_and(|n| n >= 600)
        }
        "font" => {
            if v == "inherit" {
                s.font_size = parent_font;
            } else {
                let parts = v.split_whitespace().collect::<Vec<_>>();
                for (index, part) in parts.iter().enumerate() {
                    let mut pieces = part.splitn(2, '/');
                    let size = pieces.next().unwrap_or("");
                    if let Some(size) = length(size, parent_font)
                        .and_then(|n| n.resolve(parent_font))
                        .filter(|n| *n > 0.)
                    {
                        s.font_size = size.clamp(6., 256.);
                        s.bold = parts[..index].iter().any(|n| {
                            matches!(*n, "bold" | "bolder")
                                || n.parse::<u32>().is_ok_and(|n| n >= 600)
                        });
                        s.line_height = 1.5;
                        if let Some(line) = pieces.next() {
                            if let Ok(number) = line.parse::<f32>() {
                                if number.is_finite() {
                                    s.line_height = number.clamp(0.5, 8.);
                                }
                            } else if let Some(height) =
                                length(line, size).and_then(|n| n.resolve(size))
                            {
                                s.line_height = (height / size).clamp(0.5, 8.);
                            }
                        }
                        break;
                    }
                }
            }
        }
        "line-height" => {
            if let Ok(n) = v.parse::<f32>() {
                if n.is_finite() {
                    s.line_height = n.clamp(0.5, 8.0);
                }
            } else if let Some(n) = length(v, em).and_then(|n| n.resolve(em)) {
                s.line_height = (n / em).clamp(0.5, 8.0);
            }
        }
        "width" => {
            if let Some(n) = length(v, em) {
                s.width = n;
            }
        }
        "height" => {
            if let Some(n) = length(v, em) {
                s.height = n;
            }
        }
        "max-width" => {
            if let Some(n) = length(v, em) {
                s.max_width = n;
            }
        }
        "min-width" => {
            if let Some(n) = length(v, em) {
                s.min_width = n;
            }
        }
        "min-height" => {
            if let Some(n) = length(v, em) {
                s.min_height = n;
            }
        }
        "flex-direction" => {
            if matches!(v, "row" | "row-reverse" | "column" | "column-reverse") {
                s.flex_column = v.starts_with("column");
                s.flex_reverse = v.ends_with("reverse");
            }
        }
        "flex-wrap" => s.flex_wrap = v == "wrap",
        "flex-grow" => {
            if let Some(n) = flex_number(v) {
                s.flex_grow = n;
            }
        }
        "flex-shrink" => {
            if let Some(n) = flex_number(v) {
                s.flex_shrink = n;
            }
        }
        "flex-basis" => {
            if let Some(n) = length(v, em) {
                s.flex_basis = n;
            }
        }
        "flex" => {
            if v == "none" {
                s.flex_grow = 0.;
                s.flex_shrink = 0.;
                s.flex_basis = Length::Auto;
            } else if v == "auto" {
                s.flex_grow = 1.;
                s.flex_shrink = 1.;
                s.flex_basis = Length::Auto;
            } else {
                let parts = v.split_whitespace().collect::<Vec<_>>();
                if let Some(n) = parts.first().and_then(|n| flex_number(n)) {
                    s.flex_grow = n;
                    s.flex_shrink = 1.;
                    s.flex_basis = Length::Px(0.);
                    if let Some(n) = parts.get(1).and_then(|n| flex_number(n)) {
                        s.flex_shrink = n;
                    }
                    if let Some(n) = parts.last().and_then(|n| length(n, em)) {
                        s.flex_basis = n;
                    }
                }
            }
        }
        "justify-content" => {
            if let Some(n) = alignment(v) {
                s.justify_content = n;
            }
        }
        "align-items" => {
            if let Some(n) = alignment(v) {
                s.align_items = n;
            }
        }
        "gap" | "border-spacing" => {
            let values = v
                .split_whitespace()
                .map(|n| length(n, em).and_then(|n| n.resolve(0.)))
                .collect::<Option<Vec<_>>>();
            if let Some(values) = values {
                match values.as_slice() {
                    [a] => s.gap = [a.clamp(0., 16384.); 2],
                    [a, b] => s.gap = [a.clamp(0., 16384.), b.clamp(0., 16384.)],
                    _ => {}
                }
            }
        }
        "row-gap" | "column-gap" => {
            if let Some(n) = length(v, em).and_then(|n| n.resolve(0.)) {
                s.gap[usize::from(name == "column-gap")] = n.clamp(0., 16384.);
            }
        }
        "grid-template-columns" => {
            if let Some(tracks) = grid_tracks(v, em) {
                s.grid_columns = tracks;
            }
        }
        "margin" => {
            let values = edges(v, em);
            if let Some(values) = values {
                s.margin = values.map(|n| n.resolve(0.0).unwrap_or(0.0));
                s.auto_margin = values.map(|n| n == Length::Auto);
            }
        }
        "padding" => {
            if let Some(values) = edges(v, em) {
                s.padding = values.map(|n| n.resolve(0.0).unwrap_or(0.0).max(0.0));
            }
        }
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            if let Some(n) = length(v, em) {
                let i = edge_index(name);
                s.margin[i] = n.resolve(0.0).unwrap_or(0.0);
                s.auto_margin[i] = n == Length::Auto;
            }
        }
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            if let Some(n) = length(v, em).and_then(|n| n.resolve(0.0)) {
                s.padding[edge_index(name)] = n.max(0.0);
            }
        }
        "border" => {
            if v.split_whitespace()
                .any(|part| matches!(part, "none" | "hidden"))
            {
                s.border_width = 0.;
            }
            for part in v.split_whitespace() {
                if let Some(c) = color(part) {
                    s.border_color = c;
                } else if let Some(Length::Px(n)) = length(part, em) {
                    s.border_width = n.max(0.0);
                }
            }
        }
        "border-width" => {
            if let Some(Length::Px(n)) = length(v, em) {
                s.border_width = n.max(0.0);
            }
        }
        "border-color" => {
            if let Some(c) = color(v).or_else(|| v.split_whitespace().next().and_then(color)) {
                s.border_color = c;
            }
        }
        "border-style" if matches!(v, "none" | "hidden") => s.border_width = 0.,
        "box-sizing" => s.border_box = v == "border-box",
        "text-align" => match v {
            "left" | "start" => s.text_align = Alignment::Start,
            "center" => s.text_align = Alignment::Center,
            "right" | "end" => s.text_align = Alignment::End,
            _ => {}
        },
        "overflow" => match v {
            "hidden" | "clip" => s.overflow_clip = true,
            "visible" => s.overflow_clip = false,
            _ => {}
        },
        "white-space" => match v {
            "normal" | "pre-line" => {
                s.pre = false;
                s.preserve_breaks = v == "pre-line";
                s.nowrap = false;
            }
            "pre" | "pre-wrap" => {
                s.pre = true;
                s.preserve_breaks = true;
                s.nowrap = v == "pre";
            }
            "nowrap" => {
                s.pre = false;
                s.preserve_breaks = false;
                s.nowrap = true;
            }
            _ => {}
        },
        _ => {}
    }
}

// Convert viewport units only within numeric tokens; quoted CSS strings remain
// untouched. Unsupported functions such as calc still fail their normal parser.
fn resolve_relative_units(value: &str, viewport: Viewport, root_font: f32) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.char_indices().peekable();
    let mut quote = None;
    while let Some((start, ch)) = chars.next() {
        if let Some(delimiter) = quote {
            out.push(ch);
            if ch == '\\' {
                if let Some((_, next)) = chars.next() {
                    out.push(next);
                }
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            out.push(ch);
            continue;
        }
        if ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.') {
            let mut end = start + ch.len_utf8();
            while let Some(&(offset, next)) = chars.peek() {
                if next.is_ascii_alphanumeric() || matches!(next, '.' | '-' | '+' | '%') {
                    end = offset + next.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let token = &value[start..end];
            let unit = ["vmin", "vmax", "vw", "vh", "rem"]
                .into_iter()
                .find(|unit| token.ends_with(unit));
            if let Some(unit) = unit
                && let Ok(number) = token[..token.len() - unit.len()].parse::<f32>()
                && number.is_finite()
                && number.abs() <= 100_000.
            {
                let reference = match unit {
                    "vw" => viewport.width / 100.,
                    "vh" => viewport.height / 100.,
                    "vmin" => viewport.width.min(viewport.height) / 100.,
                    "vmax" => viewport.width.max(viewport.height) / 100.,
                    _ => root_font,
                };
                let pixels = (number * reference).clamp(-16384., 16384.);
                out.push_str(&format!("{pixels}px"));
            } else {
                out.push_str(token);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn flex_number(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.)
        .map(|n| n.min(1000.))
}
fn alignment(value: &str) -> Option<Alignment> {
    Some(match value {
        "start" | "flex-start" | "normal" => Alignment::Start,
        "center" => Alignment::Center,
        "end" | "flex-end" => Alignment::End,
        "space-between" => Alignment::SpaceBetween,
        "space-around" => Alignment::SpaceAround,
        "space-evenly" => Alignment::SpaceEvenly,
        "stretch" => Alignment::Stretch,
        _ => return None,
    })
}
fn grid_tracks(value: &str, em: f32) -> Option<Vec<GridTrack>> {
    grid_tracks_inner(value, em, 0)
}
fn grid_tracks_inner(value: &str, em: f32, depth: usize) -> Option<Vec<GridTrack>> {
    if depth > 8 {
        return None;
    }
    let value = value.trim();
    if let Some(inner) = value
        .strip_prefix("repeat(")
        .and_then(|n| n.strip_suffix(')'))
    {
        let (count, value) = inner.split_once(',')?;
        let count = count
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=32).contains(n))?;
        let tracks = grid_tracks_inner(value, em, depth + 1)?;
        if tracks.len() * count > 32 {
            return None;
        }
        return Some((0..count).flat_map(|_| tracks.iter().copied()).collect());
    }
    let mut tracks = Vec::new();
    for part in value.split_whitespace() {
        if tracks.len() == 32 {
            return None;
        }
        let track = if let Some(n) = part.strip_suffix("fr") {
            GridTrack::Fraction(flex_number(n)?.max(0.001))
        } else if part == "auto" {
            GridTrack::Fraction(1.)
        } else {
            GridTrack::Fixed(length(part, em)?)
        };
        tracks.push(track);
    }
    (!tracks.is_empty()).then_some(tracks)
}

fn edge_index(name: &str) -> usize {
    if name.ends_with("top") {
        0
    } else if name.ends_with("right") {
        1
    } else if name.ends_with("bottom") {
        2
    } else {
        3
    }
}
fn edges(value: &str, em: f32) -> Option<[Length; 4]> {
    let v = value
        .split_whitespace()
        .map(|n| length(n, em))
        .collect::<Option<Vec<_>>>()?;
    Some(match v.as_slice() {
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return None,
    })
}
fn length(value: &str, em: f32) -> Option<Length> {
    let value = value.trim();
    if value == "auto" {
        return Some(Length::Auto);
    }
    let (number, unit) = if let Some(v) = value.strip_suffix("rem") {
        (v, "rem")
    } else if let Some(v) = value.strip_suffix("px") {
        (v, "px")
    } else if let Some(v) = value.strip_suffix("em") {
        (v, "em")
    } else if let Some(v) = value.strip_suffix('%') {
        (v, "%")
    } else if let Some(v) = value.strip_suffix("pt") {
        (v, "pt")
    } else if let Some(v) = value.strip_suffix("pc") {
        (v, "pc")
    } else if let Some(v) = value.strip_suffix("in") {
        (v, "in")
    } else if let Some(v) = value.strip_suffix("cm") {
        (v, "cm")
    } else if let Some(v) = value.strip_suffix("mm") {
        (v, "mm")
    } else {
        (value, "")
    };
    let n = number.parse::<f32>().ok()?;
    if !n.is_finite() || n.abs() > 100_000.0 {
        return None;
    }
    Some(match unit {
        "%" => Length::Percent(n.clamp(-10000.0, 10000.0)),
        "em" => Length::Px((n * em).clamp(-16384.0, 16384.0)),
        "rem" => Length::Px((n * 16.0).clamp(-16384.0, 16384.0)),
        "px" => Length::Px(n.clamp(-16384.0, 16384.0)),
        "pt" => Length::Px((n * 96. / 72.).clamp(-16384., 16384.)),
        "pc" => Length::Px((n * 16.).clamp(-16384., 16384.)),
        "in" => Length::Px((n * 96.).clamp(-16384., 16384.)),
        "cm" => Length::Px((n * 96. / 2.54).clamp(-16384., 16384.)),
        "mm" => Length::Px((n * 96. / 25.4).clamp(-16384., 16384.)),
        "" if n == 0.0 => Length::Px(0.0),
        _ => return None,
    })
}

pub fn color(value: &str) -> Option<Color> {
    let lower = value.trim().to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        if !hex.is_ascii() {
            return None;
        }
        let byte = |a: usize, b: usize| u8::from_str_radix(&hex[a..b], 16).ok();
        return match hex.len() {
            3 => Some(Color(
                byte(0, 1)? * 17,
                byte(1, 2)? * 17,
                byte(2, 3)? * 17,
                255,
            )),
            4 => Some(Color(
                byte(0, 1)? * 17,
                byte(1, 2)? * 17,
                byte(2, 3)? * 17,
                byte(3, 4)? * 17,
            )),
            6 => Some(Color(byte(0, 2)?, byte(2, 4)?, byte(4, 6)?, 255)),
            8 => Some(Color(byte(0, 2)?, byte(2, 4)?, byte(4, 6)?, byte(6, 8)?)),
            _ => None,
        };
    }
    let named = match lower.as_str() {
        "transparent" => Color(0, 0, 0, 0),
        "black" => Color(0, 0, 0, 255),
        "white" => Color(255, 255, 255, 255),
        "red" => Color(255, 0, 0, 255),
        "green" => Color(0, 128, 0, 255),
        "blue" => Color(0, 0, 255, 255),
        "gray" | "grey" => Color(128, 128, 128, 255),
        "silver" => Color(192, 192, 192, 255),
        "yellow" => Color(255, 255, 0, 255),
        "orange" => Color(255, 165, 0, 255),
        "purple" => Color(128, 0, 128, 255),
        "navy" => Color(0, 0, 128, 255),
        "teal" => Color(0, 128, 128, 255),
        _ => {
            let args = lower
                .strip_prefix("rgba(")
                .or_else(|| lower.strip_prefix("rgb("))?
                .strip_suffix(')')?;
            let parts = args.split(',').map(str::trim).collect::<Vec<_>>();
            if !(3..=4).contains(&parts.len()) {
                return None;
            }
            let mut rgba = [255u8; 4];
            for i in 0..3 {
                let n = parts[i].parse::<f32>().ok()?;
                if !n.is_finite() {
                    return None;
                }
                rgba[i] = n.clamp(0.0, 255.0) as u8;
            }
            if parts.len() == 4 {
                let a = parts[3].parse::<f32>().ok()?;
                if !a.is_finite() {
                    return None;
                }
                rgba[3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
            }
            return Some(Color(rgba[0], rgba[1], rgba[2], rgba[3]));
        }
    };
    Some(named)
}

fn resolve_vars(value: &str, variables: &BTreeMap<String, String>) -> String {
    let mut result = value.to_string();
    for _ in 0..8 {
        let Some(start) = result.find("var(") else {
            break;
        };
        let Some(end) = result[start + 4..].find(')').map(|n| start + 4 + n) else {
            break;
        };
        let inside = &result[start + 4..end];
        let (name, fallback) = inside.split_once(',').map_or((inside, ""), |(a, b)| (a, b));
        let replacement = variables
            .get(name.trim())
            .map_or(fallback.trim(), String::as_str)
            .to_string();
        if result.len() + replacement.len() > MAX_CSS_BYTES {
            return String::new();
        }
        result.replace_range(start..=end, &replacement);
    }
    result
}

fn resolve_vars_budgeted(
    value: &str,
    variables: &BTreeMap<String, String>,
    budget: &mut usize,
) -> Result<String, String> {
    let resolved = resolve_vars(value, variables);
    *budget = budget
        .checked_sub(resolved.len())
        .ok_or("CSS variable resolution budget exceeded")?;
    Ok(resolved)
}

fn parse_stylesheet(
    source: &str,
    rules: &mut Vec<Rule>,
    warnings: &mut Vec<String>,
    viewport: Viewport,
    depth: usize,
) -> Result<(), String> {
    if depth > 8 {
        return Err("CSS conditional nesting limit exceeded".into());
    }
    let cleaned = strip_comments(source);
    let mut pos = 0;
    let mut statements = 0usize;
    while pos < cleaned.len() {
        while let Some(ch) = cleaned[pos..].chars().next() {
            if css_whitespace(ch) || (pos == 0 && ch == '\u{feff}') {
                pos += ch.len_utf8();
            } else {
                break;
            }
        }
        if pos == cleaned.len() {
            break;
        }
        statements += 1;
        if statements > 8192 {
            return Err("CSS rule limit exceeded".into());
        }
        let Some((open, delimiter)) = rule_prelude_end(&cleaned, pos) else {
            break;
        };
        let selector = cleaned[pos..open].trim_matches(css_whitespace);
        let at_rule = selector.strip_prefix('@').map(|header| {
            let end = header
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
                .unwrap_or(header.len());
            (
                header[..end]
                    .chars()
                    .take(40)
                    .collect::<String>()
                    .to_ascii_lowercase(),
                header[end..].trim_matches(css_whitespace),
            )
        });
        if delimiter == ';' {
            // Statement at-rules end here, never at the next style rule's brace.
            // Only UTF-8 metadata at the beginning of the decoded sheet is known.
            let charset = at_rule.as_ref().is_some_and(|(name, value)| {
                depth == 0
                    && (pos == 0 || (pos == 3 && cleaned.starts_with('\u{feff}')))
                    && name == "charset"
                    && value.eq_ignore_ascii_case("\"utf-8\"")
            });
            if !charset && !selector.is_empty() {
                if let Some((name, _)) = &at_rule {
                    unsupported_at_rule_warning(warnings, name);
                } else {
                    css_warning(warnings, "Unsupported CSS statement was ignored");
                }
            }
            pos = open + 1;
            continue;
        }
        let Some(close) = closing_brace(&cleaned, open) else {
            break;
        };
        if at_rule.is_none() {
            let declarations = Arc::new(declarations(&cleaned[open + 1..close]));
            let Some(selectors) = split_selector_list(selector) else {
                css_warning(warnings, "Unsupported CSS selectors were ignored");
                pos = close + 1;
                continue;
            };
            for selector in selectors {
                if let Some(selector) = Selector::parse(selector.trim()) {
                    if rules.len() >= MAX_RULES {
                        return Err("CSS rule limit exceeded".into());
                    }
                    rules.push(Rule {
                        selector,
                        declarations: declarations.clone(),
                    });
                } else if !warnings
                    .iter()
                    .any(|s| s == "Unsupported CSS selectors were ignored")
                {
                    warnings.push("Unsupported CSS selectors were ignored".into());
                }
            }
        } else if let Some((name, condition)) = at_rule {
            let active = match name.as_str() {
                "media" => media_matches(condition, viewport),
                "supports" => supports(condition),
                _ => {
                    unsupported_at_rule_warning(warnings, &name);
                    false
                }
            };
            if active {
                parse_stylesheet(
                    &cleaned[open + 1..close],
                    rules,
                    warnings,
                    viewport,
                    depth + 1,
                )?;
            }
        }
        pos = close + 1;
    }
    Ok(())
}

fn unsupported_at_rule_warning(warnings: &mut Vec<String>, name: &str) {
    if warnings.len() >= 32 {
        return;
    }
    let name = if name.is_empty() {
        "(invalid)".into()
    } else {
        name.chars().take(40).collect::<String>()
    };
    css_warning(
        warnings,
        &format!("Unsupported CSS at-rule @{name} was ignored"),
    );
}

// Scan a rule prelude as CSS component values. Quoted/escaped punctuation and
// delimiters inside functions or attribute selectors cannot terminate a rule.
fn rule_prelude_end(source: &str, start: usize) -> Option<(usize, char)> {
    let mut quote = None;
    let mut skip_until = 0;
    let mut stack = Vec::new();
    for (offset, ch) in source[start..].char_indices() {
        let index = start + offset;
        if index < skip_until {
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == '\\' {
                let next = source.get(index + 1..)?.chars().next()?;
                if matches!(next, '\n' | '\r' | '\u{c}') {
                    skip_until = index + 2;
                    if next == '\r' && source[skip_until..].starts_with('\n') {
                        skip_until += 1;
                    }
                } else {
                    skip_until = css_escape(source, index)?.0;
                }
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        if ch == '\\' {
            skip_until = css_escape(source, index)?.0;
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' | '[' => {
                if stack.len() >= 32 {
                    return None;
                }
                stack.push(ch);
            }
            ')' | ']' => {
                if stack.pop()? != if ch == ')' { '(' } else { '[' } {
                    return None;
                }
            }
            '{' | ';' if stack.is_empty() => return Some((index, ch)),
            '}' if stack.is_empty() => return None,
            _ => {}
        }
    }
    None
}

/// Evaluate the bounded capability-query grammar used by @supports. This is an
/// engine capability query, never a script, resource lookup or layout operation.
pub fn supports(condition: &str) -> bool {
    if condition.len() > 4096 {
        return false;
    }
    supports_condition(&strip_comments(condition), 0).unwrap_or(false)
}
fn supports_condition(source: &str, depth: usize) -> Option<bool> {
    if depth > 8 {
        return None;
    }
    let source = source.trim_matches(css_whitespace);
    if source
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("not"))
        && source[3..].starts_with(css_whitespace)
    {
        let (value, end) =
            supports_operand(source[3..].trim_start_matches(css_whitespace), depth + 1)?;
        let rest = source[3..].trim_start_matches(css_whitespace);
        return rest[end..]
            .trim_matches(css_whitespace)
            .is_empty()
            .then_some(!value);
    }
    let (mut value, mut end) = supports_operand(source, depth + 1)?;
    let mut operation = None;
    loop {
        let rest = &source[end..];
        if rest.is_empty() {
            return Some(value);
        }
        if !rest.starts_with(css_whitespace) {
            return None;
        }
        let rest = rest.trim_start_matches(css_whitespace);
        if rest.is_empty() {
            return Some(value);
        }
        let word_end = rest.find(css_whitespace)?;
        let word = &rest[..word_end];
        let and = if word.eq_ignore_ascii_case("and") {
            true
        } else if word.eq_ignore_ascii_case("or") {
            false
        } else {
            return None;
        };
        if operation.is_some_and(|previous| previous != and) {
            return None;
        }
        operation = Some(and);
        let operand = rest[word_end..].trim_start_matches(css_whitespace);
        let (right, consumed) = supports_operand(operand, depth + 1)?;
        value = if and { value && right } else { value || right };
        end = source.len() - operand.len() + consumed;
    }
}
fn supports_operand(source: &str, depth: usize) -> Option<(bool, usize)> {
    if depth > 8 {
        return None;
    }
    if source.starts_with('(') {
        let close = matching_paren(source, 0)?;
        let inside = source[1..close].trim_matches(css_whitespace);
        let value = if let Some((property, value)) = inside.split_once(':') {
            let property = property.trim_matches(css_whitespace);
            if property
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            {
                supports_declaration(property, value)
            } else {
                supports_condition(inside, depth).unwrap_or(false)
            }
        } else {
            supports_condition(inside, depth).unwrap_or(false)
        };
        return Some((value, close + 1));
    }
    let open = source.find('(')?;
    let name = &source[..open];
    if name.is_empty() || !name.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '-') {
        return None;
    }
    let close = matching_paren(source, open)?;
    let value = name.eq_ignore_ascii_case("selector")
        && Selector::parse(source[open + 1..close].trim_matches(css_whitespace)).is_some();
    Some((value, close + 1))
}
fn supports_length(value: &str, allow_auto: bool, nonnegative: bool, allow_percent: bool) -> bool {
    let value = resolve_relative_units(value, Viewport::default(), 16.);
    match length(&value, 16.) {
        Some(Length::Auto) => allow_auto,
        Some(Length::Px(n)) => !nonnegative || n >= 0.,
        Some(Length::Percent(n)) => allow_percent && (!nonnegative || n >= 0.),
        None => false,
    }
}
fn supports_declaration(property: &str, value: &str) -> bool {
    let property = property.to_ascii_lowercase();
    let value = value.trim_matches(css_whitespace);
    let value = if value.to_ascii_lowercase().ends_with("!important") {
        value[..value.len() - 10].trim_end_matches(css_whitespace)
    } else {
        value
    };
    if value.is_empty() || value.contains([';', '{', '}']) {
        return false;
    }
    if property.starts_with("--") {
        return property.len() > 2 && property.len() <= 128 && balanced_css_value(value);
    }
    if !supported_property(&property) {
        return false;
    }
    let length = |auto, nonnegative, percent| supports_length(value, auto, nonnegative, percent);
    match property.as_str() {
        "display" => matches!(
            value,
            "none" | "block" | "inline" | "inline-block" | "flex" | "grid"
        ),
        "position" => matches!(value, "static" | "relative" | "absolute"),
        "visibility" => matches!(value, "visible" | "hidden"),
        "overflow" => matches!(value, "visible" | "hidden" | "clip"),
        "color" | "background" | "background-color" | "border-color" => color(value).is_some(),
        "width" | "height" | "max-width" | "min-width" | "min-height" | "flex-basis" => {
            length(true, true, true)
        }
        "top" | "right" | "bottom" | "left" | "margin-top" | "margin-right" | "margin-bottom"
        | "margin-left" => length(true, false, true),
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            length(false, true, true)
        }
        "font-size" => length(false, true, true),
        "border-width" => length(false, true, false),
        "line-height" => {
            value.parse::<f32>().is_ok_and(|n| n.is_finite() && n >= 0.)
                || length(false, true, true)
        }
        "font-weight" => {
            matches!(value, "normal" | "bold" | "bolder" | "lighter")
                || value.parse::<u32>().is_ok_and(|n| (1..=1000).contains(&n))
        }
        "flex-direction" => matches!(value, "row" | "row-reverse" | "column" | "column-reverse"),
        "flex-wrap" => matches!(value, "nowrap" | "wrap"),
        "flex-grow" | "flex-shrink" => flex_number(value).is_some(),
        "justify-content" => matches!(
            value,
            "start"
                | "flex-start"
                | "center"
                | "end"
                | "flex-end"
                | "space-between"
                | "space-around"
                | "space-evenly"
        ),
        "align-items" => matches!(
            value,
            "start" | "flex-start" | "center" | "end" | "flex-end" | "stretch"
        ),
        "box-sizing" => matches!(value, "content-box" | "border-box"),
        "text-align" => matches!(value, "left" | "start" | "center" | "right" | "end"),
        "white-space" => matches!(value, "normal" | "pre" | "pre-wrap" | "pre-line" | "nowrap"),
        "border-style" => value == "none" || value == "hidden",
        "margin" | "inset" | "padding" => {
            let parts = value.split_whitespace().collect::<Vec<_>>();
            (1..=4).contains(&parts.len())
                && parts.iter().all(|part| {
                    supports_length(part, property != "padding", property == "padding", true)
                })
        }
        "gap" | "row-gap" | "column-gap" | "border-spacing" => {
            let parts = value.split_whitespace().collect::<Vec<_>>();
            let max = if matches!(property.as_str(), "row-gap" | "column-gap") {
                1
            } else {
                2
            };
            (1..=max).contains(&parts.len())
                && parts
                    .iter()
                    .all(|part| supports_length(part, false, true, false))
        }
        // More complex shorthands are left false until their complete accepted
        // grammar can be shared with the actual declaration parser.
        _ => false,
    }
}
fn balanced_css_value(source: &str) -> bool {
    // A synthetic trailing semicolon must be the first top-level terminator.
    let mut terminated = String::with_capacity(source.len() + 1);
    terminated.push_str(source);
    terminated.push(';');
    rule_prelude_end(&terminated, 0) == Some((source.len(), ';'))
}

// Deliberately conservative: unknown features do not match. Each comma branch
// supports screen/all plus parenthesized width/height/orientation conjunctions.
fn media_matches(condition: &str, viewport: Viewport) -> bool {
    condition.split(',').any(|branch| {
        let lower = branch.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return false;
        }
        let mut remaining = lower.as_str();
        if let Some(rest) = remaining.strip_prefix("only ") {
            remaining = rest.trim();
        }
        if let Some(rest) = remaining
            .strip_prefix("screen")
            .or_else(|| remaining.strip_prefix("all"))
        {
            remaining = rest.trim();
        }
        while !remaining.is_empty() {
            if let Some(rest) = remaining.strip_prefix("and") {
                remaining = rest.trim();
            }
            let Some(rest) = remaining.strip_prefix('(') else {
                return false;
            };
            let Some((feature, tail)) = rest.split_once(')') else {
                return false;
            };
            let Some((name, value)) = feature.split_once(':') else {
                return false;
            };
            let name = name.trim();
            let value = value.trim();
            let matches = if name == "orientation" {
                match value {
                    "portrait" => viewport.height >= viewport.width,
                    "landscape" => viewport.width > viewport.height,
                    _ => false,
                }
            } else if let Some(Length::Px(n)) = length(value, 16.) {
                match name {
                    "min-width" => viewport.width >= n,
                    "max-width" => viewport.width <= n,
                    "min-height" => viewport.height >= n,
                    "max-height" => viewport.height <= n,
                    "width" => viewport.width == n,
                    "height" => viewport.height == n,
                    _ => false,
                }
            } else {
                false
            };
            if !matches {
                return false;
            }
            remaining = tail.trim();
        }
        true
    })
}

fn closing_brace(source: &str, open: usize) -> Option<usize> {
    let mut depth = 1;
    let mut quote = None;
    let mut escape = false;
    for (i, c) in source[open + 1..].char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(open + 1 + i);
            }
        }
    }
    None
}
fn strip_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut pos = 0;
    let mut quote = None;
    while pos < source.len() {
        let ch = source[pos..].chars().next().unwrap();
        if let Some(delimiter) = quote {
            output.push(ch);
            pos += ch.len_utf8();
            if ch == '\\' {
                if let Some(next) = source[pos..].chars().next() {
                    output.push(next);
                    pos += next.len_utf8();
                }
            } else if ch == delimiter {
                quote = None;
            }
        } else if source[pos..].starts_with("/*") {
            output.push(' ');
            pos = source[pos + 2..]
                .find("*/")
                .map_or(source.len(), |n| pos + 2 + n + 2);
        } else {
            if matches!(ch, '\'' | '"') {
                quote = Some(ch);
            }
            output.push(ch);
            pos += ch.len_utf8();
        }
    }
    output
}
fn declarations(source: &str) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut depth = 0;
    let mut escape = false;
    for (i, c) in source
        .char_indices()
        .chain(std::iter::once((source.len(), ';')))
    {
        if escape {
            escape = false;
            continue;
        }
        if let Some(q) = quote {
            if c == '\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            ';' if depth == 0 => {
                if let Some((name, value)) = source[start..i].split_once(':') {
                    let name = name.trim().to_ascii_lowercase();
                    let value = value.trim();
                    let important = value.to_ascii_lowercase().ends_with("!important");
                    let value = if important {
                        value[..value.len() - 10].trim()
                    } else {
                        value
                    };
                    if !name.is_empty() && !value.is_empty() {
                        declarations.push(Declaration {
                            name,
                            value: value.to_string(),
                            important,
                        });
                    }
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    declarations
}

impl Selector {
    fn parse(input: &str) -> Option<Self> {
        Self::parse_nested(input, 0)
    }
    fn parse_nested(input: &str, depth: usize) -> Option<Self> {
        if input.len() > 1024 || depth > 8 {
            return None;
        }
        let tokens = selector_tokens(input)?;
        let mut parts = Vec::new();
        let mut relation = None;
        let mut specificity = Specificity::default();
        for token in &tokens {
            if matches!(token.as_str(), ">" | "+" | "~") {
                if parts.is_empty() || relation.is_some() {
                    return None;
                }
                relation = Some(match token.as_str() {
                    ">" => Combinator::Child,
                    "+" => Combinator::NextSibling,
                    _ => Combinator::SubsequentSibling,
                });
                continue;
            }
            let mut compound = Compound::default();
            let mut pos = 0;
            let chars = token.char_indices().collect::<Vec<_>>();
            while pos < chars.len() {
                let (start, ch) = chars[pos];
                if ch == '*' {
                    if start != 0 {
                        return None;
                    }
                    pos += 1;
                    continue;
                }
                if ch == '[' {
                    let mut quote = None;
                    let mut escaped = false;
                    let mut end = None;
                    for (index, (_, ch)) in chars.iter().enumerate().skip(pos + 1) {
                        if escaped {
                            escaped = false;
                        } else if *ch == '\\' {
                            escaped = true;
                        } else if let Some(delimiter) = quote {
                            if *ch == delimiter {
                                quote = None;
                            }
                        } else if matches!(*ch, '\'' | '"') {
                            quote = Some(*ch);
                        } else if *ch == ']' {
                            end = Some(index);
                            break;
                        }
                    }
                    let end = end?;
                    let inside = &token[start + 1..chars[end].0];
                    compound.attributes.push(parse_attribute_selector(inside)?);
                    specificity = specificity.checked_add(Specificity::CLASS)?;
                    pos = end + 1;
                    continue;
                }
                if ch == ':' {
                    pos += 1;
                    let name_start = chars.get(pos)?.0;
                    while pos < chars.len()
                        && (chars[pos].1.is_ascii_alphabetic() || chars[pos].1 == '-')
                    {
                        pos += 1;
                    }
                    let name_end = chars.get(pos).map_or(token.len(), |ch| ch.0);
                    let name = token[name_start..name_end].to_ascii_lowercase();
                    if chars.get(pos).is_some_and(|ch| ch.1 == '(') {
                        let open = chars[pos].0;
                        let close = matching_paren(token, open)?;
                        let args = &token[open + 1..close];
                        pos = chars
                            .iter()
                            .position(|(offset, _)| *offset > close)
                            .unwrap_or(chars.len());
                        match name.as_str() {
                            "lang" => {
                                let branches = split_selector_list(args)?;
                                if branches.is_empty() || branches.len() > 16 {
                                    return None;
                                }
                                let ranges = branches
                                    .into_iter()
                                    .map(|branch| {
                                        let branch = branch.trim_matches(css_whitespace);
                                        let (range, end) = if branch.starts_with(['\'', '"']) {
                                            css_quoted_string(branch)?
                                        } else {
                                            css_identifier_value(branch, false, true)?
                                        };
                                        if end != branch.len()
                                            || range.is_empty()
                                            || range.len() > 128
                                            || !basic_language_tag(&range)
                                        {
                                            return None;
                                        }
                                        Some(range.to_ascii_lowercase())
                                    })
                                    .collect::<Option<Vec<_>>>()?;
                                specificity = specificity.checked_add(Specificity::CLASS)?;
                                compound.pseudos.push(Pseudo::Language(ranges));
                            }
                            "not" | "is" | "where" => {
                                let branches = split_selector_list(args)?;
                                if branches.is_empty() || branches.len() > 16 {
                                    return None;
                                }
                                let selectors = branches
                                    .into_iter()
                                    .map(|branch| Self::parse_nested(branch.trim(), depth + 1))
                                    .collect::<Option<Vec<_>>>()?;
                                if name != "where" {
                                    specificity = specificity.checked_add(
                                        selectors
                                            .iter()
                                            .map(|selector| selector.specificity)
                                            .max()
                                            .unwrap_or_default(),
                                    )?;
                                }
                                compound.pseudos.push(if name == "not" {
                                    Pseudo::Not(selectors)
                                } else {
                                    Pseudo::Any(selectors)
                                });
                            }
                            "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                                let (a, b) = nth_expression(args)?;
                                specificity = specificity.checked_add(Specificity::CLASS)?;
                                compound.pseudos.push(Pseudo::Position {
                                    a,
                                    b,
                                    reverse: name.starts_with("nth-last"),
                                    of_type: name.ends_with("of-type"),
                                });
                            }
                            _ => return None,
                        }
                    } else {
                        specificity = specificity.checked_add(Specificity::CLASS)?;
                        match name.as_str() {
                            "root" => compound.root = true,
                            "first-child" | "last-child" | "first-of-type" | "last-of-type" => {
                                compound.pseudos.push(Pseudo::Position {
                                    a: 0,
                                    b: 1,
                                    reverse: name.starts_with("last"),
                                    of_type: name.ends_with("of-type"),
                                })
                            }
                            "only-child" | "only-of-type" => compound.pseudos.push(Pseudo::Only {
                                of_type: name.ends_with("of-type"),
                            }),
                            "empty" => compound.pseudos.push(Pseudo::Empty),
                            "checked" => compound.pseudos.push(Pseudo::Checked),
                            "disabled" => compound.pseudos.push(Pseudo::Disabled(true)),
                            "enabled" => compound.pseudos.push(Pseudo::Disabled(false)),
                            "link" | "any-link" => compound.pseudos.push(Pseudo::Link),
                            _ => return None,
                        }
                    }
                    continue;
                }
                let kind = if ch == '#' || ch == '.' {
                    pos += 1;
                    ch
                } else if start == 0 {
                    't'
                } else {
                    return None;
                };
                let value = selector_identifier(token, &chars, &mut pos)?;
                match kind {
                    '#' => {
                        compound.ids.push(value);
                        specificity = specificity.checked_add(Specificity::ID)?;
                    }
                    '.' => {
                        compound.classes.push(value);
                        specificity = specificity.checked_add(Specificity::CLASS)?;
                    }
                    _ => {
                        compound.tag = Some(value.to_ascii_lowercase());
                        specificity = specificity.checked_add(Specificity::TYPE)?;
                    }
                }
            }
            parts.push(Part {
                compound,
                relation: relation.take().unwrap_or(Combinator::Descendant),
            });
        }
        if parts.is_empty() || relation.is_some() || parts.len() > 32 {
            return None;
        }
        Some(Self { parts, specificity })
    }
    fn matches(
        &self,
        document: &Document,
        id: NodeId,
        siblings: &SiblingIndex,
        budget: &mut usize,
    ) -> Result<bool, String> {
        self.matches_at(document, id, self.parts.len() - 1, siblings, budget)
    }
    fn matches_at(
        &self,
        document: &Document,
        id: NodeId,
        index: usize,
        siblings: &SiblingIndex,
        budget: &mut usize,
    ) -> Result<bool, String> {
        consume_match_budget(budget)?;
        if !self.parts[index]
            .compound
            .matches(document, id, siblings, budget)?
        {
            return Ok(false);
        }
        if index == 0 {
            return Ok(true);
        }
        let parent = document.nodes.get(id).and_then(|node| node.parent);
        match self.parts[index].relation {
            Combinator::Child => parent.map_or(Ok(false), |parent| {
                self.matches_at(document, parent, index - 1, siblings, budget)
            }),
            Combinator::Descendant => {
                let mut ancestor = parent;
                while let Some(candidate) = ancestor {
                    consume_match_budget(budget)?;
                    // Try the entire remaining selector, not just the nearest
                    // compound. Mixed chains require bounded backtracking.
                    if self.matches_at(document, candidate, index - 1, siblings, budget)? {
                        return Ok(true);
                    }
                    ancestor = document.nodes.get(candidate).and_then(|node| node.parent);
                }
                Ok(false)
            }
            relation @ (Combinator::NextSibling | Combinator::SubsequentSibling) => {
                let mut previous = siblings.previous(id);
                while let Some(sibling) = previous {
                    consume_match_budget(budget)?;
                    if self.matches_at(document, sibling, index - 1, siblings, budget)? {
                        return Ok(true);
                    }
                    if matches!(relation, Combinator::NextSibling) {
                        return Ok(false);
                    }
                    previous = siblings.previous(sibling);
                }
                Ok(false)
            }
        }
    }
}

fn consume_match_budget(budget: &mut usize) -> Result<(), String> {
    *budget = budget
        .checked_sub(1)
        .ok_or("CSS selector matching work limit exceeded")?;
    Ok(())
}
impl Compound {
    fn matches(
        &self,
        document: &Document,
        id: NodeId,
        siblings: &SiblingIndex,
        budget: &mut usize,
    ) -> Result<bool, String> {
        let Some(e) = document.element(id) else {
            return Ok(false);
        };
        if let Some(tag) = &self.tag
            && !selector_string_eq(tag, &e.tag, budget)?
        {
            return Ok(false);
        }
        for id in &self.ids {
            let Some(value) = e.attr("id") else {
                return Ok(false);
            };
            if !selector_string_eq(id, value, budget)? {
                return Ok(false);
            }
        }
        if self.root {
            // Bound document_element's root-child scan before traversing it.
            charge_selector_bytes(
                budget,
                document.nodes.first().map_or(0, |node| node.children.len()),
            )?;
            if document.document_element() != Some(id) {
                return Ok(false);
            }
        }
        for class in &self.classes {
            let Some(value) = e.attr("class") else {
                return Ok(false);
            };
            // Tokenization also visits whitespace, even when there are no tokens.
            charge_selector_bytes(budget, value.len())?;
            let mut matched = false;
            for token in value.split_ascii_whitespace() {
                charge_selector_bytes(budget, token.len().saturating_add(1))?;
                if selector_string_eq(class, token, budget)? {
                    matched = true;
                    break;
                }
            }
            if !matched {
                return Ok(false);
            }
        }
        for attribute in &self.attributes {
            let work = e
                .attr(&attribute.name)
                .map_or(1, |value| value.len().saturating_add(attribute.value.len()));
            *budget = budget
                .checked_sub(work)
                .ok_or("CSS attribute matching work limit exceeded")?;
        }
        let base_matches = self.attributes.iter().all(|attribute| {
            e.attr(&attribute.name)
                .is_some_and(|value| attribute.matches(value))
        });
        if !base_matches {
            return Ok(false);
        }
        for pseudo in &self.pseudos {
            if !pseudo.matches(document, id, siblings, budget)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl AttributeSelector {
    fn matches(&self, value: &str) -> bool {
        let (value, expected) = if self.ascii_insensitive {
            (
                std::borrow::Cow::Owned(value.to_ascii_lowercase()),
                std::borrow::Cow::Owned(self.value.to_ascii_lowercase()),
            )
        } else {
            (
                std::borrow::Cow::Borrowed(value),
                std::borrow::Cow::Borrowed(self.value.as_str()),
            )
        };
        match self.operation {
            "exists" => true,
            "=" => value == expected,
            "~=" => value.split_ascii_whitespace().any(|part| part == expected),
            "|=" => {
                value == expected
                    || value
                        .strip_prefix(expected.as_ref())
                        .is_some_and(|rest| rest.starts_with('-'))
            }
            "^=" => !expected.is_empty() && value.starts_with(expected.as_ref()),
            "$=" => !expected.is_empty() && value.ends_with(expected.as_ref()),
            "*=" => !expected.is_empty() && value.contains(expected.as_ref()),
            _ => false,
        }
    }
}

impl Pseudo {
    fn matches(
        &self,
        document: &Document,
        id: NodeId,
        siblings: &SiblingIndex,
        budget: &mut usize,
    ) -> Result<bool, String> {
        consume_match_budget(budget)?;
        let Some(element) = document.element(id) else {
            return Ok(false);
        };
        match self {
            Self::Not(selectors) => {
                for selector in selectors {
                    if selector.matches(document, id, siblings, budget)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Any(selectors) => {
                for selector in selectors {
                    if selector.matches(document, id, siblings, budget)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Self::Position {
                a,
                b,
                reverse,
                of_type,
            } => {
                let Some((position, total)) = sibling_position(document, id, *of_type, budget)?
                else {
                    return Ok(false);
                };
                let position = if *reverse {
                    total - position + 1
                } else {
                    position
                };
                let delta = position - *b;
                Ok(if *a == 0 {
                    delta == 0
                } else {
                    delta % *a == 0 && delta / *a >= 0
                })
            }
            Self::Only { of_type } => Ok(sibling_position(document, id, *of_type, budget)?
                .is_some_and(|(_, total)| total == 1)),
            Self::Empty => {
                for child in &document.nodes[id].children {
                    consume_match_budget(budget)?;
                    if matches!(&document.nodes[*child].kind, web_dom::NodeKind::Element(_))
                        || matches!(&document.nodes[*child].kind,web_dom::NodeKind::Text(text) if !text.is_empty())
                    {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Checked => Ok((element.tag == "option"
                && element.attributes.contains_key("selected"))
                || (element.tag == "input"
                    && element.attr("type").is_some_and(|kind| {
                        kind.eq_ignore_ascii_case("checkbox") || kind.eq_ignore_ascii_case("radio")
                    })
                    && element.attributes.contains_key("checked"))),
            Self::Disabled(disabled) => Ok(matches!(
                element.tag.as_str(),
                "input" | "textarea" | "button" | "select" | "option" | "optgroup" | "fieldset"
            ) && element.attributes.contains_key("disabled")
                == *disabled),
            Self::Link => Ok(matches!(element.tag.as_str(), "a" | "area")
                && element.attributes.contains_key("href")),
            Self::Language(ranges) => {
                let mut current = Some(id);
                while let Some(node) = current {
                    consume_match_budget(budget)?;
                    if let Some(language) = document
                        .element(node)
                        .and_then(|element| element.attr("lang"))
                    {
                        charge_selector_bytes(budget, language.len())?;
                        // An explicitly empty/invalid language blocks inheritance.
                        if !basic_language_tag(language) {
                            return Ok(false);
                        }
                        for range in ranges {
                            charge_selector_bytes(budget, range.len())?;
                            if language.eq_ignore_ascii_case(range)
                                || (language
                                    .get(..range.len())
                                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(range))
                                    && language.as_bytes().get(range.len()) == Some(&b'-'))
                            {
                                return Ok(true);
                            }
                        }
                        return Ok(false);
                    }
                    current = document.nodes.get(node).and_then(|node| node.parent);
                }
                Ok(false)
            }
        }
    }
}
fn basic_language_tag(value: &str) -> bool {
    let mut subtags = value.split('-');
    let Some(first) = subtags.next() else {
        return false;
    };
    (1..=8).contains(&first.len())
        && first.chars().all(|ch| ch.is_ascii_alphabetic())
        && subtags.all(|part| {
            (1..=8).contains(&part.len()) && part.chars().all(|ch| ch.is_ascii_alphanumeric())
        })
}
fn sibling_position(
    document: &Document,
    id: NodeId,
    of_type: bool,
    budget: &mut usize,
) -> Result<Option<(i64, i64)>, String> {
    let Some(parent) = document.nodes[id].parent else {
        return Ok(None);
    };
    let Some(element) = document.element(id) else {
        return Ok(None);
    };
    let mut total = 0;
    let mut position = 0;
    for child in &document.nodes[parent].children {
        consume_match_budget(budget)?;
        let Some(sibling) = document.element(*child) else {
            continue;
        };
        if !of_type || selector_string_eq(&sibling.tag, &element.tag, budget)? {
            total += 1;
            if *child == id {
                position = total;
            }
        }
    }
    Ok((position > 0).then_some((position, total)))
}

fn parse_attribute_selector(source: &str) -> Option<AttributeSelector> {
    let source = source.trim_start_matches(css_whitespace);
    let (name, end) = css_identifier_value(source, true, false)?;
    let name = name.to_ascii_lowercase();
    let rest = source[end..].trim_start_matches(css_whitespace);
    if rest.is_empty() {
        return Some(AttributeSelector {
            name,
            operation: "exists",
            value: String::new(),
            ascii_insensitive: false,
        });
    }
    let operation = ["~=", "|=", "^=", "$=", "*=", "="]
        .into_iter()
        .find(|operation| rest.starts_with(operation))?;
    let rest = rest[operation.len()..].trim_start_matches(css_whitespace);
    let first = rest.chars().next()?;
    let (value, flags) = if matches!(first, '\'' | '"') {
        let (value, end) = css_quoted_string(rest)?;
        let tail = &rest[end..];
        if !tail.is_empty() && !tail.starts_with(css_whitespace) {
            return None;
        }
        (value, tail.trim_matches(css_whitespace))
    } else {
        let (value, end) = css_identifier_value(rest, false, true)?;
        let tail = &rest[end..];
        if !tail.is_empty() && !tail.starts_with(css_whitespace) {
            return None;
        }
        (value, tail.trim_matches(css_whitespace))
    };
    let ascii_insensitive = match flags {
        "" | "s" | "S" => false,
        "i" | "I" => true,
        _ => return None,
    };
    Some(AttributeSelector {
        name,
        operation,
        value,
        ascii_insensitive,
    })
}
fn css_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}
// CSS escapes consume up to six hex digits and one optional whitespace unit.
// Return the byte immediately after the escape, so token separators inside an
// escaped identifier cannot become combinators or selector-list delimiters.
fn css_escape(source: &str, slash: usize) -> Option<(usize, char)> {
    let mut chars = source.get(slash + 1..)?.char_indices().peekable();
    let (_, first) = *chars.peek()?;
    if matches!(first, '\n' | '\r' | '\u{c}') {
        return None;
    }
    if !first.is_ascii_hexdigit() {
        return Some((
            slash + 1 + first.len_utf8(),
            if first == '\0' { '\u{fffd}' } else { first },
        ));
    }
    let mut scalar = 0u32;
    let mut end = slash + 1;
    for _ in 0..6 {
        let Some(&(offset, ch)) = chars.peek() else {
            break;
        };
        let Some(digit) = ch.to_digit(16).filter(|_| ch.is_ascii_hexdigit()) else {
            break;
        };
        scalar = scalar * 16 + digit;
        end = slash + 1 + offset + ch.len_utf8();
        chars.next();
    }
    if let Some(&(offset, ch)) = chars.peek().filter(|(_, ch)| css_whitespace(*ch)) {
        end = slash + 1 + offset + ch.len_utf8();
        chars.next();
        if ch == '\r'
            && let Some(&(offset, '\n')) = chars.peek()
        {
            end = slash + 1 + offset + 1;
        }
    }
    Some((
        end,
        char::from_u32(scalar)
            .filter(|ch| *ch != '\0')
            .unwrap_or('\u{fffd}'),
    ))
}
fn selector_identifier(source: &str, chars: &[(usize, char)], pos: &mut usize) -> Option<String> {
    // CSS identifiers cannot begin with an unescaped digit or a lone hyphen.
    // Validate the source spelling before decoding: escaped digits are valid
    // starts, and an escaped separator must remain part of the identifier.
    let starts_name = |(offset, ch): (usize, char)| {
        ch.is_ascii_alphabetic()
            || ch == '_'
            || !ch.is_ascii()
            || (ch == '\\' && css_escape(source, offset).is_some())
    };
    let first = *chars.get(*pos)?;
    if !starts_name(first)
        && !(first.1 == '-'
            && chars
                .get(*pos + 1)
                .is_some_and(|next| next.1 == '-' || starts_name(*next)))
    {
        return None;
    }
    let mut value = String::new();
    while let Some(&(offset, ch)) = chars.get(*pos) {
        if ch == '\\' {
            let (end, decoded) = css_escape(source, offset)?;
            value.push(decoded);
            while chars.get(*pos).is_some_and(|(offset, _)| *offset < end) {
                *pos += 1;
            }
        } else if ch.is_alphanumeric() || matches!(ch, '-' | '_') || !ch.is_ascii() {
            value.push(ch);
            *pos += 1;
        } else {
            break;
        }
    }
    (!value.is_empty()).then_some(value)
}
fn css_identifier_value(
    source: &str,
    allow_colon: bool,
    require_start: bool,
) -> Option<(String, usize)> {
    if require_start {
        let mut chars = source.chars();
        let first = chars.next()?;
        let start = |ch: char| ch.is_alphabetic() || matches!(ch, '_' | '\\');
        if !start(first) && !(first == '-' && chars.next().is_some_and(|ch| start(ch) || ch == '-'))
        {
            return None;
        }
    }
    let mut value = String::new();
    let mut pos = 0;
    while let Some(ch) = source[pos..].chars().next() {
        if ch == '\\' {
            let (end, decoded) = css_escape(source, pos)?;
            value.push(decoded);
            pos = end;
        } else if ch.is_alphanumeric() || matches!(ch, '-' | '_') || (allow_colon && ch == ':') {
            value.push(ch);
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    (!value.is_empty()).then_some((value, pos))
}
fn css_quoted_string(source: &str) -> Option<(String, usize)> {
    let delimiter = source.chars().next()?;
    if !matches!(delimiter, '\'' | '"') {
        return None;
    }
    let mut value = String::new();
    let mut pos = 1;
    while let Some(ch) = source[pos..].chars().next() {
        if ch == delimiter {
            return Some((value, pos + 1));
        }
        if matches!(ch, '\n' | '\r' | '\u{c}') {
            return None;
        }
        if ch == '\\' {
            let next = source.get(pos + 1..)?.chars().next()?;
            if matches!(next, '\n' | '\r' | '\u{c}') {
                pos += 2;
                if next == '\r' && source[pos..].starts_with('\n') {
                    pos += 1;
                }
            } else {
                let (end, decoded) = css_escape(source, pos)?;
                value.push(decoded);
                pos = end;
            }
        } else {
            value.push(if ch == '\0' { '\u{fffd}' } else { ch });
            pos += ch.len_utf8();
        }
    }
    None
}
fn matching_paren(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escape = false;
    let mut skip_until = 0;
    for (offset, ch) in source[open..].char_indices() {
        if open + offset < skip_until {
            continue;
        }
        if escape {
            escape = false;
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == '\\' {
                escape = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        if ch == '\\' {
            skip_until = css_escape(source, open + offset)?.0;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            continue;
        }
        if ch == '(' {
            depth += 1;
            if depth > 8 {
                return None;
            }
        }
        if ch == ')' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(open + offset);
            }
        }
    }
    None
}
fn split_selector_list(source: &str) -> Option<Vec<&str>> {
    let mut result = Vec::new();
    visit_selector_list(source, |branch| {
        result.push(branch);
        true
    })?;
    Some(result)
}

// Share escape/delimiter handling with bounded DOM query compilation. The
// visitor can count/reject branches without first materializing a Vec.
fn visit_selector_list<'a>(source: &'a str, mut visit: impl FnMut(&'a str) -> bool) -> Option<()> {
    let mut start = 0;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut quote = None;
    let mut escape = false;
    let mut skip_until = 0;
    for (offset, ch) in source.char_indices() {
        if offset < skip_until {
            continue;
        }
        if escape {
            escape = false;
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == '\\' {
                escape = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        if ch == '\\' {
            skip_until = css_escape(source, offset)?.0;
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => {
                parens += 1;
                if parens > 8 {
                    return None;
                }
            }
            ')' => parens = parens.checked_sub(1)?,
            '[' => {
                brackets += 1;
                if brackets > 1 {
                    return None;
                }
            }
            ']' => brackets = brackets.checked_sub(1)?,
            ',' if parens == 0 && brackets == 0 => {
                if !visit(&source[start..offset]) {
                    return None;
                }
                start = offset + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || parens != 0 || brackets != 0 {
        return None;
    }
    visit(&source[start..]).then_some(())
}
fn nth_expression(source: &str) -> Option<(i64, i64)> {
    let expression = source
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let result: (i64, i64) = match expression.as_str() {
        "odd" => (2, 1),
        "even" => (2, 0),
        _ => {
            if let Some((a, b)) = expression.split_once('n') {
                let a = match a {
                    "" | "+" => 1,
                    "-" => -1,
                    _ => a.parse().ok()?,
                };
                let b = if b.is_empty() { 0 } else { b.parse().ok()? };
                (a, b)
            } else {
                (0, expression.parse().ok()?)
            }
        }
    };
    ((-1_000_000..=1_000_000).contains(&result.0) && (-1_000_000..=1_000_000).contains(&result.1))
        .then_some(result)
}
fn selector_tokens(input: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut bracket = false;
    let mut parens = 0usize;
    let mut escape = false;
    let mut skip_until = 0;
    for (offset, ch) in input.char_indices() {
        if offset < skip_until {
            continue;
        }
        if escape {
            token.push(ch);
            escape = false;
            continue;
        }
        if let Some(delimiter) = quote {
            token.push(ch);
            if ch == '\\' {
                escape = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        if ch == '\\' {
            skip_until = css_escape(input, offset)?.0;
            token.push_str(&input[offset..skip_until]);
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            token.push(ch);
            continue;
        }
        match ch {
            '[' => {
                if bracket {
                    return None;
                }
                bracket = true;
            }
            ']' => {
                if !bracket {
                    return None;
                }
                bracket = false;
            }
            '(' if !bracket => {
                parens += 1;
                if parens > 8 {
                    return None;
                }
            }
            ')' if !bracket => parens = parens.checked_sub(1)?,
            _ => {}
        }
        if !bracket && parens == 0 && (css_whitespace(ch) || matches!(ch, '>' | '+' | '~')) {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
            if matches!(ch, '>' | '+' | '~') {
                tokens.push(ch.to_string());
            }
        } else {
            token.push(ch);
        }
    }
    if quote.is_some() || bracket || parens != 0 {
        return None;
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whitespace_modes_inherit_and_override_pre_defaults() {
        for (value, expected) in [
            ("normal", (false, false, false)),
            ("nowrap", (false, false, true)),
            ("pre-line", (false, true, false)),
            ("pre-wrap", (true, true, false)),
            ("pre", (true, true, true)),
        ] {
            let source = format!("<pre style='white-space:{value}'><span id=target>A</span></pre>");
            let style = specificity_style("", &source);
            assert_eq!(
                (style.pre, style.preserve_breaks, style.nowrap),
                expected,
                "{value}"
            );
            let source = format!("<pre><span id=target style='white-space:{value}'>A</span></pre>");
            let style = specificity_style("", &source);
            assert_eq!(
                (style.pre, style.preserve_breaks, style.nowrap),
                expected,
                "{value}"
            );
        }
        let style = specificity_style("", "<pre><span id=target>A</span></pre>");
        assert_eq!(
            (style.pre, style.preserve_breaks, style.nowrap),
            (true, true, true)
        );
    }

    fn specificity_style(css: &str, markup: &str) -> Style {
        let document = web_html::parse(&format!("<style>{css}</style>{markup}")).unwrap();
        let target = document
            .elements()
            .find(|(_, e)| e.attr("id") == Some("target"))
            .unwrap()
            .0;
        compute(&document).unwrap().styles[target].clone()
    }
    #[test]
    fn specificity_components_do_not_carry_and_source_order_breaks_only_ties() {
        for count in [10, 11] {
            let classes = ".x".repeat(count);
            let types = format!("{}p", "div ".repeat(count - 1));
            let nested = format!(
                "{}<p id=target class=x>x</p>{}",
                "<div>".repeat(count - 1),
                "</div>".repeat(count - 1)
            );
            for (strong, weak, markup) in [
                ("#target", classes.as_str(), "<p id=target class=x>x</p>"),
                (".x", types.as_str(), nested.as_str()),
            ] {
                let first = format!("{strong}{{color:red}}{weak}{{color:blue}}");
                let second = format!("{weak}{{color:blue}}{strong}{{color:red}}");
                for css in [first, second] {
                    assert_eq!(specificity_style(&css, markup).color, Color(255, 0, 0, 255));
                }
            }
        }
        for css in [".x{color:red}.x{color:blue}", ".x{color:red;color:blue}"] {
            assert_eq!(
                specificity_style(css, "<p id=target class=x>x</p>").color,
                Color(0, 0, 255, 255)
            );
        }
    }
    #[test]
    fn inline_priority_has_its_own_axis_below_importance() {
        for (sheet, inline, expected) in [
            ("color:red", "color:blue", Color(0, 0, 255, 255)),
            ("color:red!important", "color:blue", Color(255, 0, 0, 255)),
            (
                "color:red!important",
                "color:blue!important",
                Color(0, 0, 255, 255),
            ),
            ("color:red", "color:blue!important", Color(0, 0, 255, 255)),
        ] {
            let css = format!("{}{{{sheet}}}", "#target".repeat(100));
            let markup = format!("<p id=target style='{inline}'>x</p>");
            assert_eq!(specificity_style(&css, &markup).color, expected);
        }
        let style = specificity_style(
            "#target{margin:8px!important}",
            "<p id=target style='margin-left:2px'>x</p>",
        );
        assert_eq!(style.margin, [8.; 4]);
    }
    #[test]
    fn logical_specificity_uses_all_branches_and_where_is_zero() {
        // A top-level list contributes only the branches that actually match.
        assert_eq!(
            specificity_style(
                "#absent,.x{color:red}.x.x{color:blue}",
                "<p id=target class=x>x</p>"
            )
            .color,
            Color(0, 0, 255, 255)
        );
        assert_eq!(
            specificity_style(
                ".x.x{color:blue}#absent,.x{color:red}",
                "<p id=target class=x>x</p>"
            )
            .color,
            Color(0, 0, 255, 255)
        );
        for pseudo in ["is", "not"] {
            let argument = if pseudo == "is" { ".x" } else { ".missing" };
            let selector = format!(":{pseudo}(#absent,{argument})");
            assert_eq!(
                Selector::parse(&selector).unwrap().specificity,
                Specificity::ID
            );
            let css = format!("{selector}{{color:red}}{}{{color:blue}}", ".x".repeat(11));
            assert_eq!(
                specificity_style(&css, "<p id=target class=x>x</p>").color,
                Color(255, 0, 0, 255)
            );
        }
        for selector in [
            ":where(#target.x)",
            ":where(:is(#target,:not(.a)))",
            ":is(:where(#target),:where(.x))",
        ] {
            assert_eq!(
                Selector::parse(selector).unwrap().specificity,
                Specificity::default()
            );
        }
        assert_eq!(
            Selector::parse(":is(:where(#target),.x):not(:where(#absent))")
                .unwrap()
                .specificity,
            Specificity::CLASS
        );
        assert_eq!(
            Selector::parse(":is(#a,.x.x.x.x.x.x.x.x.x.x.x)")
                .unwrap()
                .specificity,
            Specificity::ID
        );
        assert_eq!(
            specificity_style(
                ":where(:is(#target)){color:red}p{color:blue}",
                "<p id=target>x</p>"
            )
            .color,
            Color(0, 0, 255, 255)
        );
    }
    #[test]
    fn repeated_ids_are_all_required_and_each_occurrence_counts() {
        let document = web_html::parse("<p id=a>a</p><p id=b>b</p>").unwrap();
        assert_eq!(matching_ids(&document, "#a#a"), ["a"]);
        assert!(matching_ids(&document, "#a#b").is_empty());
        assert!(matching_ids(&document, "#b#a").is_empty());
        assert_eq!(matching_ids(&document, r"#\61 #a"), ["a"]);
        assert_eq!(
            Selector::parse(r"#\61 #a").unwrap().specificity,
            Specificity {
                ids: 2,
                classes: 0,
                types: 0
            }
        );
        assert_eq!(
            Selector::parse(".x.x[root]:root:root").unwrap().specificity,
            Specificity {
                ids: 0,
                classes: 5,
                types: 0
            }
        );
        let siblings = SiblingIndex::new(&document, &mut 1000).unwrap();
        let a = document
            .elements()
            .find(|(_, e)| e.attr("id") == Some("a"))
            .unwrap()
            .0;
        let one = Selector::parse("#a").unwrap();
        let many = Selector::parse(&"#a".repeat(100)).unwrap();
        let mut one_budget = 1000;
        let mut many_budget = 1000;
        assert!(
            one.matches(&document, a, &siblings, &mut one_budget)
                .unwrap()
        );
        assert!(
            many.matches(&document, a, &siblings, &mut many_budget)
                .unwrap()
        );
        assert_eq!(one_budget - many_budget, 99);
        assert!(
            many.matches(&document, a, &siblings, &mut 50)
                .unwrap_err()
                .contains("work limit")
        );
        let style = specificity_style(
            "#target#target{color:red}#target{color:blue}#absent#target{color:green}",
            "<p id=target>x</p>",
        );
        assert_eq!(style.color, Color(255, 0, 0, 255));
    }
    #[test]
    fn specificity_limits_and_checked_addition_remain_bounded() {
        assert_eq!(
            Selector::parse(&"#a".repeat(512)).unwrap().specificity.ids,
            512
        );
        assert!(Selector::parse(&"#a".repeat(513)).is_none());
        assert!(Selector::parse(&vec!["p"; 32].join(" ")).is_some());
        assert!(Selector::parse(&vec!["p"; 33].join(" ")).is_none());
        assert!(Selector::parse(&format!(":is({})", vec!["#a"; 16].join(","))).is_some());
        assert!(Selector::parse(&format!(":is({})", vec!["#a"; 17].join(","))).is_none());
        assert!(Selector::parse(&format!("{}#a{}", ":is(".repeat(8), ")".repeat(8))).is_some());
        assert!(Selector::parse(&format!("{}#a{}", ":is(".repeat(9), ")".repeat(9))).is_none());
        for (limit, increment) in [
            (
                Specificity {
                    ids: u32::MAX,
                    classes: 0,
                    types: 0,
                },
                Specificity::ID,
            ),
            (
                Specificity {
                    ids: 0,
                    classes: u32::MAX,
                    types: 0,
                },
                Specificity::CLASS,
            ),
            (
                Specificity {
                    ids: 0,
                    classes: 0,
                    types: u32::MAX,
                },
                Specificity::TYPE,
            ),
        ] {
            assert!(limit.checked_add(increment).is_none());
            assert_eq!(limit.checked_add(Specificity::default()), Some(limit));
        }
        assert!(
            Specificity::ID
                > Specificity {
                    ids: 0,
                    classes: u32::MAX,
                    types: u32::MAX
                }
        );
        assert!(
            Specificity::CLASS
                > Specificity {
                    ids: 0,
                    classes: 0,
                    types: u32::MAX
                }
        );
    }
    #[test]
    fn lexicographic_priority_is_preserved_for_shorthands_and_longhands() {
        for css in [
            "#target{margin:7px}.x.x.x.x.x.x.x.x.x.x.x{margin-left:2px}",
            ".x.x.x.x.x.x.x.x.x.x.x{margin-left:2px}#target{margin:7px}",
        ] {
            assert_eq!(
                specificity_style(css, "<p id=target class=x>x</p>").margin,
                [7.; 4]
            );
        }
        assert_eq!(
            specificity_style(
                ".x.x.x.x.x.x.x.x.x.x.x{margin:7px}#target{margin-left:2px}",
                "<p id=target class=x>x</p>"
            )
            .margin,
            [7., 7., 7., 2.]
        );
    }
    fn matching_ids(document: &Document, source: &str) -> Vec<String> {
        let selector = Selector::parse(source).expect("supported selector");
        let mut budget = 100_000;
        let siblings = SiblingIndex::new(document, &mut budget).unwrap();
        document
            .elements()
            .filter(|(id, _)| {
                selector
                    .matches(document, *id, &siblings, &mut budget)
                    .unwrap()
            })
            .map(|(_, element)| element.attr("id").unwrap_or("").to_owned())
            .collect()
    }
    #[test]
    fn sibling_combinators_skip_text_and_backtrack_mixed_chains() {
        let document = web_html::parse(
            "<main><h2 id=heading></h2>text<!-- comment --><span id=after></span><p id=one></p><p id=two></p><section><p id=nested></p></section></main><section class=a><div class=b><div class=b><p id=deep></p></div></div></section>",
        )
        .unwrap();
        assert_eq!(matching_ids(&document, "h2+span"), ["after"]);
        assert!(matching_ids(&document, "h2+p").is_empty());
        assert_eq!(matching_ids(&document, "h2 ~ p"), ["one", "two"]);
        assert_eq!(matching_ids(&document, "h2~span+p"), ["one"]);
        assert_eq!(matching_ids(&document, "h2+span~p"), ["one", "two"]);
        assert_eq!(matching_ids(&document, ":is(h2 + span,.absent)+p"), ["one"]);
        assert_eq!(matching_ids(&document, ".a > .b p"), ["deep"]);
        assert!(matching_ids(&document, "h2~section>p+span").is_empty());
        for invalid in ["+p", "p+", "p~~p", "p > + p", "p+~p", "p||p"] {
            assert!(Selector::parse(invalid).is_none(), "{invalid}");
        }
    }
    #[test]
    fn attribute_flags_are_ascii_only_and_quoted_values_keep_delimiters() {
        let document = web_html::parse(
            "<p id=one data-word='AbC-Z' data-list='One TWO' data-symbol='x~=y]z+q' data-unicode='Å'></p><p id=two data-word='abc-z' data-list='one two' data-unicode='å'></p>",
        )
        .unwrap();
        for selector in [
            "[data-word='ABC-Z' i]",
            "[data-word^='AB' I]",
            "[data-word$='C-Z' i]",
            "[data-word*='BC-' i]",
            "[data-word|='ABC' i]",
            "[data-list~=two i]",
        ] {
            assert_eq!(
                matching_ids(&document, selector),
                ["one", "two"],
                "{selector}"
            );
        }
        assert_eq!(matching_ids(&document, "[data-word='abc-z' s]"), ["two"]);
        assert_eq!(matching_ids(&document, "[data-word='AbC-Z' S]"), ["one"]);
        assert_eq!(
            matching_ids(&document, r"[data\2d word='\41 bC-Z' S]"),
            ["one"]
        );
        assert_eq!(matching_ids(&document, "[data-unicode='Å' i]"), ["one"]);
        assert_eq!(matching_ids(&document, "[data-symbol='x~=y]z+q']"), ["one"]);
        assert!(matching_ids(&document, "[data-word^='' i]").is_empty());
        for whitespace in [' ', '\t', '\n', '\r', '\u{c}'] {
            assert_eq!(
                matching_ids(&document, &format!("[data-word='ABC-Z'{whitespace}i]")),
                ["one", "two"]
            );
        }
        for invalid in [
            "[data-word='x'i]",
            "[data-word='x' z]",
            "[data-word='x' i s]",
            "[data-word i]",
            "[data-word=1]",
            r"[data-word='a\']",
            "[data-word='x'\u{a0}i]",
            "[data-word='x' \u{a0}i]",
            "[data-word=x\u{a0}i]",
        ] {
            assert!(Selector::parse(invalid).is_none(), "{invalid}");
        }
    }
    #[test]
    fn sibling_traversal_and_attribute_folding_use_existing_work_budget() {
        let document = web_html::parse(&format!(
            "<main>{}<p id=last data-long='{}'></p></main>",
            "<p></p>".repeat(200),
            "A".repeat(512)
        ))
        .unwrap();
        let id = document
            .elements()
            .find(|(_, element)| element.attr("id") == Some("last"))
            .unwrap()
            .0;
        let siblings = SiblingIndex::new(&document, &mut 100_000).unwrap();
        assert!(
            Selector::parse(".absent~p")
                .unwrap()
                .matches(&document, id, &siblings, &mut 100)
                .unwrap_err()
                .contains("work")
        );
        assert!(
            Selector::parse("[data-long=a i]")
                .unwrap()
                .matches(&document, id, &siblings, &mut 100)
                .unwrap_err()
                .contains("work")
        );
    }
    #[test]
    fn common_sibling_rules_share_one_index_for_three_thousand_rows() {
        let document = web_html::parse(&format!(
            "<style>.row+.row{{color:red}}.row~.row{{padding-left:7px}}:is(.row+.row){{font-weight:bold}}</style><main>{}</main>",
            "<p class=row>x</p>".repeat(3000)
        ))
        .unwrap();
        let adjacent = Selector::parse(".row+.row").unwrap();
        let subsequent = Selector::parse(".row~.row").unwrap();
        // A quadratic search for each subject's position cannot complete with
        // this budget; both common rules should need only local sibling links.
        let mut budget = 200_000;
        let siblings = SiblingIndex::new(&document, &mut budget).unwrap();
        let mut adjacent_count = 0;
        let mut subsequent_count = 0;
        for (id, element) in document.elements() {
            if element.tag != "p" {
                continue;
            }
            adjacent_count += usize::from(
                adjacent
                    .matches(&document, id, &siblings, &mut budget)
                    .unwrap(),
            );
            subsequent_count += usize::from(
                subsequent
                    .matches(&document, id, &siblings, &mut budget)
                    .unwrap(),
            );
        }
        assert_eq!(adjacent_count, 2999);
        assert_eq!(subsequent_count, 2999);
        let computed = compute(&document).unwrap();
        let rows = document
            .elements()
            .filter(|(_, element)| element.tag == "p")
            .map(|(id, _)| &computed.styles[id])
            .collect::<Vec<_>>();
        assert_eq!(rows[0].padding[3], 0.);
        assert!(!rows[0].bold);
        assert!(rows[1..].iter().all(|style| {
            style.color == Color(255, 0, 0, 255) && style.padding[3] == 7. && style.bold
        }));
    }
    #[test]
    fn sibling_index_construction_also_consumes_the_shared_work_budget() {
        let document = web_html::parse("<main><p>one</p><p>two</p></main>").unwrap();
        let mut too_small = document.nodes.len() - 1;
        assert!(SiblingIndex::new(&document, &mut too_small).is_err());
        let mut node_slots_only = document.nodes.len();
        assert!(SiblingIndex::new(&document, &mut node_slots_only).is_err());
        let mut budget = 100;
        let siblings = SiblingIndex::new(&document, &mut budget).unwrap();
        assert!(budget < 100 - document.nodes.len());
        let rows = document
            .elements()
            .filter(|(_, element)| element.tag == "p")
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        assert_eq!(siblings.previous(rows[0]), None);
        assert_eq!(siblings.previous(rows[1]), Some(rows[0]));
    }
    #[test]
    fn indexed_candidates_match_full_scan_and_keep_cascade_order() {
        let css = "*{color:black}.a{color:red}.a.b{color:blue}#target.a{color:green}#target#target{color:red}#absent#target{color:blue}#target#absent{color:green}main > p{padding:2px}:is(.b,#target){margin:3px}:not([hidden]){font-weight:bold}[data-x]{background:#123456}p+p{color:purple}.a~.b{padding:7px}[data-x=YES i]{margin:9px}";
        let mut rules = Vec::new();
        parse_stylesheet(css, &mut rules, &mut Vec::new(), Viewport::default(), 0).unwrap();
        let document=web_html::parse("<main><p id=target class='a a b' data-x=yes>One</p><p class=b>Two</p><div hidden>Three</div></main>").unwrap();
        let index = RuleIndex::new(&rules, &mut 100_000).unwrap();
        let siblings = SiblingIndex::new(&document, &mut 100_000).unwrap();
        let mut seen = vec![0; rules.len()];
        for (id, element) in document.elements() {
            let mut budget = 100_000;
            let candidates = index
                .candidates(element, id + 1, &mut seen, &mut budget)
                .unwrap();
            let indexed = candidates
                .into_iter()
                .filter(|&rule| {
                    rules[rule]
                        .selector
                        .matches(&document, id, &siblings, &mut budget)
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let mut budget = 100_000;
            let scanned = rules
                .iter()
                .enumerate()
                .filter_map(|(rule, value)| {
                    value
                        .selector
                        .matches(&document, id, &siblings, &mut budget)
                        .unwrap()
                        .then_some(rule)
                })
                .collect::<Vec<_>>();
            assert_eq!(indexed, scanned);
        }
    }
    #[test]
    fn unrelated_rules_do_not_repeatedly_scan_long_class_attributes() {
        let mut css = (0..600)
            .map(|n| format!(".unrelated-{n}{{color:red}}"))
            .collect::<String>();
        css.push_str(".matched{color:blue}");
        let source = format!(
            "<style>{css}</style>{}",
            format!("<p class='matched {}'>text</p>", "x".repeat(300)).repeat(1200)
        );
        let document = web_html::parse(&source).unwrap();
        let computed = compute(&document).unwrap();
        assert!(
            document
                .elements()
                .filter(|(_, element)| element.tag == "p")
                .all(|(id, _)| computed.styles[id].color == Color(0, 0, 255, 255))
        );
    }
    #[test]
    fn duplicate_class_candidates_remain_bounded_before_matching() {
        let mut rules = Vec::new();
        parse_stylesheet(
            &".a{color:red}".repeat(256),
            &mut rules,
            &mut Vec::new(),
            Viewport::default(),
            0,
        )
        .unwrap();
        let index = RuleIndex::new(&rules, &mut 100_000).unwrap();
        let element = Element {
            tag: "p".into(),
            attributes: BTreeMap::from([("class".into(), "a a a a".into())]),
        };
        assert!(
            index
                .candidates(&element, 1, &mut vec![0; rules.len()], &mut 100)
                .unwrap_err()
                .contains("work limit")
        );
        let candidates = index
            .candidates(&element, 2, &mut vec![0; rules.len()], &mut 10_000)
            .unwrap();
        assert_eq!(candidates.len(), rules.len());
    }
    #[test]
    fn normalized_root_inherits_without_selecting_body_as_root() {
        let document = web_html::parse(concat!(
            "<style>:root{color:#123456;padding:3px;--accent:#abcdef}",
            "p{background:var(--accent)}</style><p>Live</p>"
        ))
        .unwrap();
        let result = compute(&document).unwrap();
        let html = document.document_element().unwrap();
        let head = document.head().unwrap();
        let body = document.body().unwrap();
        let paragraph = document.elements().find(|(_, e)| e.tag == "p").unwrap().0;
        assert_eq!(result.styles[html].padding, [3.; 4]);
        assert_eq!(result.styles[body].padding, [0.; 4]);
        assert_eq!(result.styles[body].margin, [8.; 4]);
        assert_eq!(result.styles[head].display, Display::None);
        assert_eq!(result.styles[paragraph].color, Color(18, 52, 86, 255));
        assert_eq!(
            result.styles[paragraph].background,
            Color(171, 205, 239, 255)
        );
        let siblings = SiblingIndex::new(&document, &mut 100_000).unwrap();
        let selector = Selector::parse(":root").unwrap();
        for (id, _) in document.elements() {
            assert_eq!(
                selector
                    .matches(&document, id, &siblings, &mut 1000)
                    .unwrap(),
                id == html
            );
        }
    }
    #[test]
    fn root_selector_rejects_nested_html_and_extra_document_children() {
        let mut document = web_html::parse("<p>Live</p>").unwrap();
        let html = document.document_element().unwrap();
        let body = document.body().unwrap();
        for parent in [body, 0] {
            document
                .append(
                    parent,
                    web_dom::NodeKind::Element(Element {
                        tag: "html".into(),
                        attributes: BTreeMap::new(),
                    }),
                )
                .unwrap();
        }
        let siblings = SiblingIndex::new(&document, &mut 100_000).unwrap();
        let selector = Selector::parse(":root").unwrap();
        for (id, _) in document.elements() {
            assert_eq!(
                selector
                    .matches(&document, id, &siblings, &mut 1000)
                    .unwrap(),
                id == html
            );
        }
    }
    #[test]
    fn ancestor_end_tags_do_not_activate_inert_styles() {
        for tag in ["template", "noscript"] {
            let source = format!(
                "<style>p{{color:blue}}</style><body><{tag}></body></html><style>p{{color:red!important}}</style><link rel=stylesheet href=inactive.css></{tag}><p id=live>Live</p>"
            );
            let document = web_html::parse(&source).unwrap();
            let result = compute_with_stylesheets(
                &document,
                &[StylesheetAsset {
                    href: "inactive.css".into(),
                    source: "p{display:none!important}".into(),
                }],
                Viewport::default(),
            )
            .unwrap();
            let live = document
                .elements()
                .find(|(_, e)| e.attr("id") == Some("live"))
                .unwrap()
                .0;
            assert_eq!(result.styles[live].color, Color(0, 0, 255, 255), "{tag}");
            assert_ne!(result.styles[live].display, Display::None, "{tag}");
            assert!(!document.is_inactive_subtree(live), "{tag}");
        }
    }
    #[test]
    fn inactive_styles_and_links_do_not_change_live_content() {
        let document=web_html::parse("<style>p{color:blue}</style><noscript><style>p{display:none!important}</style></noscript><template style='display:block'><style>p{color:red!important}</style><link rel=stylesheet href=inactive.css></template><p id=live>Visible</p><dialog id=closed>Hidden dialog</dialog><dialog id=open open>Open dialog</dialog>").unwrap();
        let result = compute_with_stylesheets(
            &document,
            &[StylesheetAsset {
                href: "inactive.css".into(),
                source: "p{display:none!important}".into(),
            }],
            Viewport::default(),
        )
        .unwrap();
        let find = |wanted: &str| {
            document
                .elements()
                .find(|(_, e)| e.attr("id") == Some(wanted))
                .unwrap()
                .0
        };
        assert_eq!(result.styles[find("live")].color, Color(0, 0, 255, 255));
        assert_ne!(result.styles[find("live")].display, Display::None);
        assert_eq!(result.styles[find("closed")].display, Display::None);
        assert_ne!(result.styles[find("open")].display, Display::None);
    }
    #[test]
    fn logical_selectors_preserve_specificity_and_structure() {
        let document=web_html::parse("<style>:is(#target,.other){color:red}:where(#target){color:green}p:not(.excluded,[hidden]){background:#123456}main > p:first-child{font-weight:bold}p:nth-child(2n+1){padding-left:7px}p:nth-last-child(2){margin-right:9px}p:empty{display:none}input:checked{color:blue}input:disabled{display:none}</style><main><p id=target>First</p><p class=excluded>Second</p><p id=empty></p><p id=last>Last</p></main><input id=checked type=checkbox checked><input id=disabled disabled>").unwrap();
        let result = compute(&document).unwrap();
        let find = |wanted: &str| {
            document
                .elements()
                .find(|(_, e)| e.attr("id") == Some(wanted))
                .unwrap()
                .0
        };
        let target = &result.styles[find("target")];
        let siblings = SiblingIndex::new(&document, &mut 100_000).unwrap();
        assert_eq!(target.color, Color(255, 0, 0, 255));
        assert_eq!(target.background, Color(18, 52, 86, 255));
        assert!(target.bold);
        assert_eq!(target.padding[3], 7.);
        assert_eq!(result.styles[find("empty")].margin[1], 9.);
        assert_eq!(result.styles[find("empty")].display, Display::None);
        assert_eq!(result.styles[find("checked")].color, Color(0, 0, 255, 255));
        assert_eq!(result.styles[find("disabled")].display, Display::None);
        assert!(
            Selector::parse(":is(main > p, .other)")
                .unwrap()
                .matches(&document, find("target"), &siblings, &mut 1000)
                .unwrap()
        );
        assert!(
            Selector::parse("p:not(:is(.excluded,[hidden]))")
                .unwrap()
                .matches(&document, find("target"), &siblings, &mut 1000)
                .unwrap()
        );
    }
    #[test]
    fn structural_selectors_are_bounded_and_css_comments_keep_quoted_data() {
        assert_eq!(
            strip_comments("a{background:url(\"https://x/a/*literal*/.png\")}/*remove*/"),
            "a{background:url(\"https://x/a/*literal*/.png\")} "
        );
        assert!(Selector::parse(&format!("{}p{}", ":not(".repeat(9), ")".repeat(9))).is_none());
        assert!(Selector::parse("p:nth-child(9223372036854775808n)").is_none());
        assert!(Selector::parse("p:nth-child(-9223372036854775808n)").is_none());
        let document =
            web_html::parse(&format!("<main>{}</main>", "<p>x</p>".repeat(100))).unwrap();
        let last = document
            .elements()
            .filter(|(_, e)| e.tag == "p")
            .last()
            .unwrap()
            .0;
        let siblings = SiblingIndex::new(&document, &mut 100_000).unwrap();
        let error = Selector::parse("p:nth-child(1)")
            .unwrap()
            .matches(&document, last, &siblings, &mut 10)
            .unwrap_err();
        assert!(error.contains("work limit"));
    }
    #[test]
    fn cascade_inheritance_selectors_and_variables() {
        let d=web_html::parse("<style>:root{--accent:#123456} div{color:red} .x{color:blue!important} #a{color:green} main > div{padding:4px 8px} p{color:var(--accent)}</style><main><div id=a class=x style='color:white'><b>bold</b></div><p>ink</p></main>").unwrap();
        let computed = compute(&d).unwrap();
        let a = d
            .elements()
            .find(|(_, e)| e.attr("id") == Some("a"))
            .unwrap()
            .0;
        assert_eq!(computed.styles[a].color, Color(0, 0, 255, 255));
        assert_eq!(computed.styles[a].padding, [4.0, 8.0, 4.0, 8.0]);
        let b = d.elements().find(|(_, e)| e.tag == "b").unwrap().0;
        assert_eq!(computed.styles[b].color, computed.styles[a].color);
        assert!(computed.styles[b].bold);
    }
    #[test]
    fn rejects_nonfinite_lengths_and_bad_colors() {
        assert!(length("NaNpx", 16.0).is_none());
        assert!(color("#ééé").is_none());
        assert!(color("#éa").is_none());
        assert_eq!(color("#abc"), Some(Color(170, 187, 204, 255)));
        assert_eq!(color("rgba(1,2,3,0.5)"), Some(Color(1, 2, 3, 127)));
    }
    #[test]
    fn shorthand_respects_declaration_order() {
        let d=web_html::parse("<p style='margin-left:2px;margin:4px'>one</p><p style='margin:4px;margin-left:2px'>two</p>").unwrap();
        let computed = compute(&d).unwrap();
        let ids = d
            .elements()
            .filter(|(_, e)| e.tag == "p")
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        assert_eq!(computed.styles[ids[0]].margin, [4.0; 4]);
        assert_eq!(computed.styles[ids[1]].margin[3], 2.0);
    }
    #[test]
    fn inherited_custom_properties_share_storage() {
        let source = format!(
            "<html><style>html{{--large:{}}}</style><body>{}</body></html>",
            "x".repeat(8192),
            "<span>text</span>".repeat(4000)
        );
        let document = web_html::parse(&source).unwrap();
        let styles = compute(&document).unwrap().styles;
        let html = document
            .elements()
            .find(|(_, e)| e.tag == "html")
            .unwrap()
            .0;
        let span = document
            .elements()
            .find(|(_, e)| e.tag == "span")
            .unwrap()
            .0;
        assert!(Arc::ptr_eq(
            &styles[html].variables,
            &styles[span].variables
        ));
    }
    #[test]
    fn custom_property_overrides_cannot_amplify_memory() {
        let source = format!(
            "<html><style>html{{--large:{}}}</style><body>{}</body></html>",
            "x".repeat(16 * 1024),
            "<div style='--small:y'></div>".repeat(300)
        );
        let document = web_html::parse(&source).unwrap();
        let error = compute(&document)
            .err()
            .expect("must enforce allocation budget");
        assert!(error.contains("materialization"));
    }
    #[test]
    fn selector_lists_share_declarations_and_large_cascades_are_bounded() {
        let selectors = (0..128)
            .map(|n| format!(".x{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let css = format!("{selectors}{{--large:{}}}", "v".repeat(16000));
        let mut rules = Vec::new();
        parse_stylesheet(&css, &mut rules, &mut Vec::new(), Viewport::default(), 0).unwrap();
        assert!(Arc::ptr_eq(
            &rules[0].declarations,
            &rules[127].declarations
        ));
        let source = format!(
            "<style>div{{unsupported:{}}}</style>{}",
            "v".repeat(16000),
            "<div></div>".repeat(1200)
        );
        let error = compute(&web_html::parse(&source).unwrap())
            .err()
            .expect("must enforce cascade budget");
        assert!(error.contains("cascade copy"));
    }
    #[test]
    fn repeated_variable_expansion_has_a_work_budget() {
        let source = format!(
            "<html><style>html{{--large:{}}}span{{color:var(--large)}}</style><body>{}</body></html>",
            "x".repeat(16000),
            "<span>x</span>".repeat(1200)
        );
        let error = compute(&web_html::parse(&source).unwrap())
            .err()
            .expect("must bound variable expansion");
        assert!(error.contains("resolution budget"));
    }
}
