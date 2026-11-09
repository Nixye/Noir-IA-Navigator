//! Bounded HTML tokenizer and tree builder for the documented static subset.
//! This is not yet the WHATWG tree-construction algorithm (tables and adoption
//! agency are intentionally unsupported).
use std::collections::BTreeMap;
use web_dom::{Document, DocumentMode, Element, MAX_DEPTH, NodeKind};

/// Parse a document with one html/head/body skeleton. Repeated html/body
/// start tags add missing attributes only; repeated head tags are ignored.
pub fn parse(source: &str) -> Result<Document, String> {
    parse_tree(source, false)
}

/// Detached children live directly under node 0, without document wrappers.
/// Context names are ASCII-case-insensitive. With scripting enabled,
/// script/style/noscript consume the entire input literally; title/textarea
/// consume it as RCDATA (including apparent closing tags). Table/select/foreign
/// contexts are rejected. The temporary mode must not replace receiver metadata.
pub fn parse_fragment(source: &str, context: &str) -> Result<Document, String> {
    check_size(source)?;
    let context = context.to_ascii_lowercase();
    if matches!(
        context.as_str(),
        "table"
            | "tbody"
            | "thead"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "caption"
            | "colgroup"
            | "select"
            | "optgroup"
            | "option"
            | "svg"
            | "math"
            | "foreignobject"
    ) {
        return Err("Unsupported HTML fragment context".into());
    }
    if is_raw_context(&context) {
        let mut document = Document::default();
        if !source.is_empty() {
            document.append(0, NodeKind::Text(context_text(source, &context)))?;
        }
        return Ok(document);
    }
    parse_tree(source, true)
}

fn check_size(source: &str) -> Result<(), String> {
    if source.len() > browser_ipc::MAX_HTML_BYTES {
        Err("HTML input exceeds 1 MiB".into())
    } else {
        Ok(())
    }
}

fn is_raw_context(tag: &str) -> bool {
    matches!(tag, "script" | "style" | "noscript" | "textarea" | "title")
}

fn context_text(source: &str, tag: &str) -> String {
    if matches!(tag, "textarea" | "title") {
        decode_entities(source)
    } else {
        source.to_string()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    AfterBody,
}

struct TreeBuilder {
    document: Document,
    stack: Vec<usize>,
    mode: Mode,
    fragment: bool,
    html: Option<usize>,
    head: Option<usize>,
    body: Option<usize>,
}

impl TreeBuilder {
    fn new(fragment: bool) -> Self {
        Self {
            document: Document {
                mode: if fragment {
                    DocumentMode::NoQuirks
                } else {
                    DocumentMode::Quirks
                },
                ..Default::default()
            },
            stack: vec![0],
            mode: if fragment {
                Mode::InBody
            } else {
                Mode::BeforeHtml
            },
            fragment,
            html: None,
            head: None,
            body: None,
        }
    }

    fn in_template(&self) -> bool {
        self.stack.iter().any(|&id| {
            self.document
                .element(id)
                .is_some_and(|e| e.tag == "template")
        })
    }

    fn append(&mut self, kind: NodeKind) -> Result<usize, String> {
        // The same depth budget counts document, wrappers, elements and text.
        if self.stack.len() >= MAX_DEPTH {
            return Err("HTML nesting limit exceeded".into());
        }
        self.document.append(*self.stack.last().unwrap_or(&0), kind)
    }

    fn insert(
        &mut self,
        tag: String,
        attributes: BTreeMap<String, String>,
    ) -> Result<usize, String> {
        let is_void = matches!(
            tag.as_str(),
            "area"
                | "base"
                | "br"
                | "col"
                | "embed"
                | "hr"
                | "img"
                | "input"
                | "link"
                | "meta"
                | "param"
                | "source"
                | "track"
                | "wbr"
        );
        let id = self.append(NodeKind::Element(Element { tag, attributes }))?;
        if !is_void {
            self.stack.push(id);
        }
        Ok(id)
    }

    fn merge_attributes(&mut self, id: Option<usize>, attributes: BTreeMap<String, String>) {
        if let Some(id) = id
            && let NodeKind::Element(element) = &mut self.document.nodes[id].kind
        {
            for (name, value) in attributes {
                element.attributes.entry(name).or_insert(value);
            }
        }
    }

    // Initial transitions are monotonic. Reprocessing is bounded by six modes.
    fn prepare(&mut self, tag: Option<&str>, whitespace: bool) -> Result<bool, String> {
        if self.fragment || self.in_template() {
            return Ok(true);
        }
        for _ in 0..6 {
            match self.mode {
                Mode::BeforeHtml => {
                    if whitespace {
                        return Ok(false);
                    }
                    self.html = Some(self.insert("html".into(), BTreeMap::new())?);
                    self.mode = Mode::BeforeHead;
                }
                Mode::BeforeHead => {
                    if whitespace {
                        return Ok(false);
                    }
                    self.head = Some(self.insert("head".into(), BTreeMap::new())?);
                    self.mode = Mode::InHead;
                }
                Mode::InHead => {
                    if whitespace
                        || tag.is_some_and(|tag| {
                            matches!(
                                tag,
                                "base"
                                    | "link"
                                    | "meta"
                                    | "title"
                                    | "style"
                                    | "script"
                                    | "noscript"
                                    | "template"
                            )
                        })
                    {
                        return Ok(true);
                    }
                    self.stack.truncate(2);
                    self.mode = Mode::AfterHead;
                }
                Mode::AfterHead => {
                    if whitespace {
                        return Ok(false);
                    }
                    if tag.is_some_and(|tag| {
                        matches!(
                            tag,
                            "base"
                                | "link"
                                | "meta"
                                | "title"
                                | "style"
                                | "script"
                                | "noscript"
                                | "template"
                        )
                    }) {
                        // A closed head may still receive initial metadata until
                        // body starts. Insert there directly, never move nodes.
                        if let Some(head) = self.head {
                            self.stack.push(head);
                        }
                        self.mode = Mode::InHead;
                        return Ok(true);
                    }
                    self.body = Some(self.insert("body".into(), BTreeMap::new())?);
                    self.mode = Mode::InBody;
                }
                Mode::AfterBody => self.mode = Mode::InBody,
                Mode::InBody => return Ok(true),
            }
        }
        Err("HTML insertion mode transition limit exceeded".into())
    }

    fn text(&mut self, text: String) -> Result<(), String> {
        if !text.is_empty() && self.prepare(None, text.chars().all(|c| c.is_ascii_whitespace()))? {
            self.append(NodeKind::Text(text))?;
        }
        Ok(())
    }

    fn start(&mut self, tag: String, attributes: BTreeMap<String, String>) -> Result<(), String> {
        if matches!(tag.as_str(), "html" | "head" | "body") {
            // Never reset a template stack or create fragment wrappers.
            if self.fragment || self.in_template() {
                return Ok(());
            }
            match tag.as_str() {
                "html" => {
                    if self.html.is_none() {
                        self.html = Some(self.insert(tag, attributes)?);
                        self.mode = Mode::BeforeHead;
                    } else {
                        self.merge_attributes(self.html, attributes);
                    }
                }
                "head" => {
                    if self.mode == Mode::BeforeHtml {
                        self.html = Some(self.insert("html".into(), BTreeMap::new())?);
                        self.mode = Mode::BeforeHead;
                    }
                    if self.mode == Mode::BeforeHead {
                        self.head = Some(self.insert(tag, attributes)?);
                        self.mode = Mode::InHead;
                    }
                }
                "body" => {
                    self.prepare(Some("body"), false)?;
                    self.merge_attributes(self.body, attributes);
                }
                _ => {}
            }
            return Ok(());
        }
        self.prepare(Some(&tag), false)?;
        if matches!(tag.as_str(), "option" | "optgroup") {
            for index in (1..self.stack.len()).rev() {
                let Some(open) = self.document.element(self.stack[index]) else {
                    continue;
                };
                if matches!(open.tag.as_str(), "select" | "datalist" | "template") {
                    break;
                }
                if open.tag == "option" || (tag == "optgroup" && open.tag == "optgroup") {
                    self.stack.truncate(index);
                    if tag == "option" {
                        break;
                    }
                } else if open.tag == "optgroup" && tag == "option" {
                    break;
                }
            }
        }
        if let Some(current) = self.stack.last().and_then(|&id| self.document.element(id))
            && ((current.tag == "p" && is_block(&tag))
                || (current.tag == tag && matches!(tag.as_str(), "li" | "dt" | "dd" | "p")))
        {
            self.stack.pop();
        }
        self.insert(tag, attributes)?;
        Ok(())
    }

    fn end(&mut self, tag: &str) -> Result<(), String> {
        if matches!(tag, "html" | "head" | "body") {
            if self.fragment || self.in_template() {
                return Ok(());
            }
            if tag == "head" {
                if matches!(self.mode, Mode::BeforeHtml | Mode::BeforeHead) {
                    self.prepare(Some("meta"), false)?;
                }
                if self.mode == Mode::InHead {
                    self.stack.truncate(2);
                    self.mode = Mode::AfterHead;
                }
            } else {
                self.prepare(None, false)?;
                // Keep body and descendants open; trailing content is in-body.
                self.mode = Mode::AfterBody;
            }
            return Ok(());
        }
        for index in (1..self.stack.len()).rev() {
            let Some(open) = self.document.element(self.stack[index]) else {
                continue;
            };
            if open.tag == tag {
                self.stack.truncate(index);
                break;
            }
            if open.tag == "template" {
                break;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Document, String> {
        if !self.fragment && self.body.is_none() {
            // EOF finishes only the missing skeleton. Existing head/template
            // descendants stay where they were built; no subtree is moved.
            if self.html.is_none() {
                self.html = Some(self.insert("html".into(), BTreeMap::new())?);
            }
            self.stack = vec![0, self.html.unwrap()];
            if self.head.is_none() {
                self.head = Some(self.insert("head".into(), BTreeMap::new())?);
                self.stack.truncate(2);
            }
            self.body = Some(self.insert("body".into(), BTreeMap::new())?);
        }
        Ok(self.document)
    }
}

fn parse_tree(source: &str, fragment: bool) -> Result<Document, String> {
    check_size(source)?;
    let mut builder = TreeBuilder::new(fragment);
    let mut initial = true;
    let mut pos = 0;
    while pos < source.len() {
        let parent = *builder.stack.last().unwrap_or(&0);
        if let Some(element) = builder.document.element(parent)
            && is_raw_context(&element.tag)
        {
            let tag = element.tag.clone();
            let end = find_end_tag(&source[pos..], &tag)
                .map(|n| pos + n)
                .filter(|&candidate| tag_end(source, candidate + 1).is_some())
                .unwrap_or(source.len());
            if end > pos {
                builder.append(NodeKind::Text(context_text(&source[pos..end], &tag)))?;
            }
            if end == source.len() {
                break;
            }
            pos = end;
        }
        if source[pos..].starts_with("<!--") {
            pos = source[pos + 4..]
                .find("-->")
                .map_or(source.len(), |n| pos + 4 + n + 3);
            continue;
        }
        if source.as_bytes()[pos] != b'<' {
            let end = source[pos..].find('<').map_or(source.len(), |n| pos + n);
            if !source[pos..end].chars().all(|ch| ch.is_ascii_whitespace()) {
                initial = false;
            }
            builder.text(decode_entities(&source[pos..end]))?;
            pos = end;
            continue;
        }
        if source[pos..]
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<!doctype"))
        {
            let end = tag_end(source, pos + 1).unwrap_or(source.len());
            if initial && !fragment {
                builder.document.mode = if end < source.len() {
                    doctype_mode(&source[pos + 9..end])
                } else {
                    DocumentMode::Quirks
                };
                initial = false;
            }
            pos = (end + 1).min(source.len());
            continue;
        }
        if source[pos..].starts_with("<!") || source[pos..].starts_with("<?") {
            pos = source[pos..]
                .find('>')
                .map_or(source.len(), |n| pos + n + 1);
            continue;
        }
        let Some(end) = tag_end(source, pos + 1) else {
            builder.text(decode_entities(&source[pos..]))?;
            break;
        };
        initial = false;
        let inside = &source[pos + 1..end];
        if let Some(closing) = inside.strip_prefix('/') {
            let tag = closing
                .split_ascii_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('/')
                .to_ascii_lowercase();
            builder.end(&tag)?;
        } else {
            let (tag, attributes) = parse_tag(inside);
            if tag.is_empty() {
                builder.text("<".into())?;
                pos += 1;
                continue;
            }
            builder.start(tag, attributes)?;
        }
        pos = end + 1;
    }
    builder.finish()
}

fn doctype_mode(declaration: &str) -> DocumentMode {
    // Do not allocate a copy of an attacker-sized public/system identifier.
    // Other HTML budgets still apply; unsupported metadata is not guessed.
    if declaration.len() > 1024 {
        return DocumentMode::UnsupportedDoctype;
    }
    let trim = trim_doctype;
    let declaration = trim(declaration);
    let name_end = declaration
        .find(|ch: char| ch.is_ascii_whitespace())
        .unwrap_or(declaration.len());
    if !declaration[..name_end].eq_ignore_ascii_case("html") {
        return DocumentMode::Quirks;
    }
    let rest = trim(&declaration[name_end..]);
    if rest.is_empty() {
        return DocumentMode::NoQuirks;
    }
    let keyword_end = if rest.get(..6).is_some_and(|prefix| {
        prefix.eq_ignore_ascii_case("public") || prefix.eq_ignore_ascii_case("system")
    }) && rest
        .as_bytes()
        .get(6)
        .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'\'' | b'"'))
    {
        6
    } else {
        rest.find(|ch: char| ch.is_ascii_whitespace())
            .unwrap_or(rest.len())
    };
    let keyword = &rest[..keyword_end];
    let Some((identifier, rest)) = quoted_doctype_id(trim(&rest[keyword_end..])) else {
        return DocumentMode::Quirks;
    };
    let rest = trim(rest);
    if keyword.eq_ignore_ascii_case("system") {
        if !rest.is_empty() {
            return DocumentMode::Quirks;
        }
        return if identifier
            .eq_ignore_ascii_case("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
        {
            DocumentMode::Quirks
        } else {
            DocumentMode::NoQuirks
        };
    }
    if !keyword.eq_ignore_ascii_case("public") {
        return DocumentMode::Quirks;
    }
    let system = if rest.is_empty() {
        None
    } else {
        let Some((system, trailing)) = quoted_doctype_id(rest) else {
            return DocumentMode::Quirks;
        };
        if !trim(trailing).is_empty() {
            return DocumentMode::Quirks;
        }
        Some(system)
    };
    if system.is_some_and(|value| {
        value.eq_ignore_ascii_case("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
    }) {
        return DocumentMode::Quirks;
    }
    let public = identifier.to_ascii_lowercase();
    if public.starts_with("-//w3c//dtd xhtml 1.0 transitional//")
        || public.starts_with("-//w3c//dtd xhtml 1.0 frameset//")
    {
        return DocumentMode::LimitedQuirks;
    }
    if public.starts_with("-//w3c//dtd html 4.01 transitional//")
        || public.starts_with("-//w3c//dtd html 4.01 frameset//")
    {
        return if system.is_some_and(|value| !value.is_empty()) {
            DocumentMode::LimitedQuirks
        } else {
            DocumentMode::Quirks
        };
    }
    if matches!(
        public.as_str(),
        "-//w3c//dtd html 4.01//en"
            | "-//w3c//dtd xhtml 1.0 strict//en"
            | "-//w3c//dtd xhtml 1.1//en"
    ) {
        return DocumentMode::NoQuirks;
    }
    if [
        "-//w3c//dtd html 3 1995-03-24//",
        "-//w3c//dtd html 3.2 draft//",
        "-//w3c//dtd html 3.2 final//",
        "-//w3c//dtd html 3.2//",
        "-//w3c//dtd html 3.2s draft//",
    ]
    .iter()
    .any(|prefix| public.starts_with(prefix))
        || public.starts_with("-//w3c//dtd html 4.0 transitional//")
        || public.starts_with("-//w3c//dtd html 4.0 frameset//")
    {
        return DocumentMode::Quirks;
    }
    DocumentMode::UnsupportedDoctype
}

fn trim_doctype(source: &str) -> &str {
    source.trim_matches(|ch| matches!(ch, '\t' | '\n' | '\u{c}' | '\r' | ' '))
}

fn quoted_doctype_id(source: &str) -> Option<(&str, &str)> {
    let quote = *source.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let end = source[1..].find(char::from(quote))? + 1;
    Some((&source[1..end], &source[end + 1..]))
}

fn tag_end(source: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, ch) in source[start..].char_indices() {
        match (quote, ch) {
            (Some(q), c) if q == c => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, '>') => return Some(start + offset),
            _ => {}
        }
    }
    None
}

fn parse_tag(inside: &str) -> (String, BTreeMap<String, String>) {
    let bytes = inside.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'-') {
        pos += 1;
    }
    let tag = inside[..pos].to_ascii_lowercase();
    if !tag.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
        return (String::new(), BTreeMap::new());
    }
    let mut attrs = BTreeMap::new();
    while pos < bytes.len() {
        while pos < bytes.len() && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b'/') {
            pos += 1;
        }
        let start = pos;
        while pos < bytes.len()
            && !bytes[pos].is_ascii_whitespace()
            && !matches!(bytes[pos], b'=' | b'/' | b'>' | b'\'' | b'"')
        {
            pos += 1;
        }
        if start == pos {
            pos += 1;
            continue;
        }
        let name = inside[start..pos].to_ascii_lowercase();
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let mut value = String::new();
        if pos < bytes.len() && bytes[pos] == b'=' {
            pos += 1;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos < bytes.len() && matches!(bytes[pos], b'\'' | b'"') {
                let quote = bytes[pos];
                pos += 1;
                let value_start = pos;
                while pos < bytes.len() && bytes[pos] != quote {
                    pos += 1;
                }
                value = decode_entities(&inside[value_start..pos]);
                if pos < bytes.len() {
                    pos += 1;
                }
            } else {
                let value_start = pos;
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                value = decode_entities(&inside[value_start..pos]);
            }
        }
        // HTML keeps the first duplicate attribute.
        attrs.entry(name).or_insert(value);
    }
    (tag, attrs)
}

fn find_end_tag(haystack: &str, tag: &str) -> Option<usize> {
    let needle = format!("</{tag}");
    haystack
        .as_bytes()
        .windows(needle.len())
        .enumerate()
        .find_map(|(index, window)| {
            let following = haystack.as_bytes().get(index + needle.len());
            (window.eq_ignore_ascii_case(needle.as_bytes())
                && following.is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'>')))
            .then_some(index)
        })
}

pub fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "html"
            | "body"
            | "main"
            | "article"
            | "aside"
            | "section"
            | "nav"
            | "header"
            | "footer"
            | "div"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "blockquote"
            | "pre"
            | "hr"
            | "figure"
            | "figcaption"
            | "dl"
            | "dt"
            | "dd"
            | "table"
            | "form"
    )
}

pub fn decode_entities(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut pos = 0;
    while pos < input.len() {
        if input.as_bytes()[pos] == b'&'
            && let Some(end) = input[pos + 1..].find(';').filter(|n| *n <= 32)
        {
            let entity = &input[pos + 1..pos + 1 + end];
            let decoded = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "copy" => Some('©'),
                "reg" => Some('®'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "hellip" => Some('…'),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|s| u32::from_str_radix(s, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|s| s.parse().ok()))
                    .map(|n| {
                        char::from_u32(n)
                            .filter(|c| *c != '\0')
                            .unwrap_or('\u{fffd}')
                    }),
            };
            if let Some(ch) = decoded {
                result.push(ch);
                pos += end + 2;
                continue;
            }
        }
        let ch = input[pos..].chars().next().unwrap();
        result.push(ch);
        pos += ch.len_utf8();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_skeleton_and_initial_metadata_are_constructed_in_place() {
        for source in [
            "",
            "text",
            "<p>text",
            "<html><head></head><body>text</body></html>",
        ] {
            let d = parse(source).unwrap();
            let html = d.document_element().unwrap();
            assert_eq!(d.nodes[0].children, vec![html]);
            assert_eq!(
                d.nodes[html].children,
                vec![d.head().unwrap(), d.body().unwrap()]
            );
            for tag in ["html", "head", "body"] {
                assert_eq!(d.elements().filter(|(_, e)| e.tag == tag).count(), 1);
            }
        }
        let d = parse("<title>Title</title><meta name=x><p>Body<style>p{}</style>").unwrap();
        let title = d.title_element().unwrap();
        assert_eq!(d.nodes[title].parent, d.head());
        let style = d.elements().find(|(_, e)| e.tag == "style").unwrap().0;
        assert_ne!(d.nodes[style].parent, d.head());
        let d = parse("text<html lang=en><head id=ignored><body class=one><html lang=pt id=root><body class=two id=body>").unwrap();
        assert_eq!(
            d.element(d.document_element().unwrap())
                .unwrap()
                .attr("lang"),
            Some("en")
        );
        assert_eq!(
            d.element(d.document_element().unwrap()).unwrap().attr("id"),
            Some("root")
        );
        assert_eq!(
            d.element(d.body().unwrap()).unwrap().attr("class"),
            Some("one")
        );
        assert_eq!(
            d.element(d.body().unwrap()).unwrap().attr("id"),
            Some("body")
        );
        assert_eq!(d.element(d.head().unwrap()).unwrap().attr("id"), None);
        assert!(d.text_content(d.body().unwrap()).starts_with("text"));
    }

    #[test]
    fn fragment_contexts_do_not_create_document_structure_or_change_modes() {
        let d = parse_fragment("<!doctype svg><html><body><b>A&amp;B</b>", "div").unwrap();
        assert_eq!(d.mode, DocumentMode::NoQuirks);
        assert_eq!(d.elements().count(), 1);
        assert_eq!(d.element(d.nodes[0].children[0]).unwrap().tag, "b");
        for context in ["script", "style", "noscript", "SCRIPT"] {
            let d = parse_fragment("a&amp;</script><img>", context).unwrap();
            assert_eq!(d.elements().count(), 0);
            assert_eq!(d.text_content(0), "a&amp;</script><img>");
        }
        for context in ["title", "textarea"] {
            let d = parse_fragment("a&amp;</title><b>", context).unwrap();
            assert_eq!(d.elements().count(), 0);
            assert_eq!(d.text_content(0), "a&</title><b>");
        }
        for context in ["table", "select", "td", "svg", "math"] {
            assert!(parse_fragment("x", context).is_err());
        }
        assert!(
            parse_fragment("", "div").unwrap().nodes[0]
                .children
                .is_empty()
        );
    }

    #[test]
    fn template_scope_and_scripting_enabled_noscript_remain_inert() {
        for source in [
            "<body><template></body><script>bad()</script><link href=x><img src=x></template>",
            "<body><div><template></div><html><head><body><script>bad()</script></template>",
            "<select><option><template><option><script>bad()</script></template>",
            "<select><optgroup><template><optgroup><style>bad{}</style></template>",
            "<template><body><script>bad()</script></template><p>live",
        ] {
            let d = parse(source).unwrap();
            for (id, e) in d
                .elements()
                .filter(|(_, e)| matches!(e.tag.as_str(), "script" | "style" | "link" | "img"))
            {
                assert!(d.is_inactive_subtree(id), "{} from {source}", e.tag);
            }
            assert_eq!(d.elements().filter(|(_, e)| e.tag == "html").count(), 1);
            assert_eq!(d.elements().filter(|(_, e)| e.tag == "body").count(), 1);
        }
        let d = parse("<body><noscript></body><script>bad()</script><img src=x></noscript><p>live")
            .unwrap();
        assert!(
            !d.elements()
                .any(|(_, e)| matches!(e.tag.as_str(), "script" | "img"))
        );
        let n = d.elements().find(|(_, e)| e.tag == "noscript").unwrap().0;
        assert_eq!(
            d.text_content(n),
            "</body><script>bad()</script><img src=x>"
        );
        assert_eq!(d.nodes[n].parent, d.body());
    }

    #[test]
    fn wrapper_and_text_nodes_consume_the_same_budgets() {
        let d = parse(&"<br>".repeat(web_dom::MAX_NODES - 4)).unwrap();
        assert_eq!(d.nodes.len(), web_dom::MAX_NODES);
        assert!(parse(&"<br>".repeat(web_dom::MAX_NODES - 3)).is_err());
        assert!(parse(&"<div>".repeat(MAX_DEPTH - 3)).is_ok());
        assert!(parse(&"<div>".repeat(MAX_DEPTH - 2)).is_err());
        assert!(parse(&format!("{}x", "<div>".repeat(MAX_DEPTH - 3))).is_err());
        assert!(parse_fragment(&"x".repeat(browser_ipc::MAX_HTML_BYTES + 1), "script").is_err());
    }
    #[test]
    fn doctype_metadata_tracks_supported_modes_and_initial_position() {
        for (source, expected) in [
            ("<!doctype html><p>live", DocumentMode::NoQuirks),
            ("<!DOCTYPEhtml><p>live", DocumentMode::NoQuirks),
            (
                " \n<!--lead--><!DOCTYPE HTML><p>live",
                DocumentMode::NoQuirks,
            ),
            (
                "<!DOCTYPE html SYSTEM 'about:legacy-compat'>",
                DocumentMode::NoQuirks,
            ),
            (
                "<!doctype html PUBLIC '-//W3C//DTD HTML 4.01//EN' 'strict.dtd'>",
                DocumentMode::NoQuirks,
            ),
            (
                "<!doctype html PUBLIC '-//W3C//DTD XHTML 1.0 Transitional//EN' 'xhtml.dtd'>",
                DocumentMode::LimitedQuirks,
            ),
            (
                "<!doctype html PUBLIC '-//W3C//DTD HTML 4.01 Transitional//EN' 'loose.dtd'>",
                DocumentMode::LimitedQuirks,
            ),
            (
                "<!doctype html PUBLIC '-//W3C//DTD HTML 4.01 Transitional//EN'>",
                DocumentMode::Quirks,
            ),
            (
                "<!doctype html PUBLIC '-//W3C//DTD HTML 4.01 Transitional//EN' ''>",
                DocumentMode::Quirks,
            ),
            ("<p>no declaration", DocumentMode::Quirks),
            ("text<!doctype html>", DocumentMode::Quirks),
            ("<html><!doctype html>", DocumentMode::Quirks),
            ("<!doctype svg>", DocumentMode::Quirks),
            ("<!doctype><!doctype html>", DocumentMode::Quirks),
            ("<!doctype html PUBLIC unquoted>", DocumentMode::Quirks),
            ("<!doctype html PUBLIC 'unterminated>", DocumentMode::Quirks),
            (
                "<!doctype html PUBLIC 'unclassified legacy'>",
                DocumentMode::UnsupportedDoctype,
            ),
            (
                "<!doctype html PUBLIC '-//W3C//DTD HTML 3.14 Fake//EN'>",
                DocumentMode::UnsupportedDoctype,
            ),
        ] {
            assert_eq!(parse(source).unwrap().mode, expected, "{source}");
        }
        assert_eq!(
            parse(&format!("<!doctype html PUBLIC '{}'>", "x".repeat(1025)))
                .unwrap()
                .mode,
            DocumentMode::UnsupportedDoctype
        );
        assert_eq!(DocumentMode::NoQuirks.compat_mode(), Some("CSS1Compat"));
        assert_eq!(
            DocumentMode::LimitedQuirks.compat_mode(),
            Some("CSS1Compat")
        );
        assert_eq!(DocumentMode::Quirks.compat_mode(), Some("BackCompat"));
        assert_eq!(DocumentMode::UnsupportedDoctype.compat_mode(), None);
    }
    #[test]
    fn omitted_option_end_tags_keep_choices_in_their_own_groups() {
        let document = parse("<select><optgroup label=A><option value=a>Alpha<option value=b>Beta<optgroup label=B><option value=c>Gamma</select>").unwrap();
        let options = document
            .elements()
            .filter(|(_, element)| element.tag == "option")
            .collect::<Vec<_>>();
        assert_eq!(options.len(), 3);
        assert_eq!(document.text_content(options[0].0), "Alpha");
        assert_eq!(document.text_content(options[1].0), "Beta");
        assert_eq!(document.text_content(options[2].0), "Gamma");
        assert_eq!(
            document.nodes[options[0].0].parent,
            document.nodes[options[1].0].parent
        );
        assert_ne!(
            document.nodes[options[1].0].parent,
            document.nodes[options[2].0].parent
        );
    }
    #[test]
    fn recovers_markup_and_preserves_utf8() {
        let d = parse("<TITLE>Ação &amp; &#x1f319;</TiTlE><p>Olá<b> mundo</p><p>fim<img src='a>b.png' SRC=ignored>").unwrap();
        assert_eq!(d.title(), "Ação & 🌙");
        assert_eq!(d.elements().filter(|(_, e)| e.tag == "p").count(), 2);
        assert!(d.elements().any(|(_, e)| e.attr("src") == Some("a>b.png")));
        assert!(d.text_content(0).contains("Olá mundo"));
    }
    #[test]
    fn incomplete_raw_end_tags_keep_the_entire_suffix_in_the_original_element() {
        for tag in ["noscript", "script", "style", "title", "textarea"] {
            for prefix in ["", "<body>"] {
                let source = format!("{prefix}<{tag}>a</{tag} x='");
                let d = parse(&source).unwrap();
                let id = d.elements().find(|(_, e)| e.tag == tag).unwrap().0;
                assert_eq!(d.text_content(id), format!("a</{tag} x='"));
                assert_eq!(d.nodes[id].children.len(), 1);
                let expected_parent = if prefix.is_empty() && tag != "textarea" {
                    d.head().unwrap()
                } else {
                    d.body().unwrap()
                };
                assert_eq!(d.nodes[id].parent, Some(expected_parent));
                assert_eq!(d.nodes[expected_parent].children, vec![id]);
                if expected_parent == d.head().unwrap() {
                    assert!(d.nodes[d.body().unwrap()].children.is_empty());
                }
            }
        }
    }
    #[test]
    fn raw_text_and_limits() {
        let d = parse(
            "<!--x--><style>a > b { color:red }</style><script>if (a < b) x()</script><p>safe</p>",
        )
        .unwrap();
        assert_eq!(d.elements().count(), 6);
        assert!(parse(&"<div>".repeat(130)).is_err());
        assert!(parse(&"a".repeat(browser_ipc::MAX_HTML_BYTES + 1)).is_err());
        let d = parse("<script>a </scripture> b</script><p>safe</p>").unwrap();
        let script = d.elements().find(|(_, e)| e.tag == "script").unwrap().0;
        assert_eq!(d.text_content(script), "a </scripture> b");
    }
}
