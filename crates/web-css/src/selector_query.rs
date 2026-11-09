//! Strict, bounded DOM queries backed by the stylesheet selector implementation.
//!
//! This module does not enumerate candidates or impose a query root. Callers
//! choose candidates and inactive-subtree policy; matching can inspect ancestors
//! and siblings outside that root. Keep one context for the entire operation,
//! including `closest` ancestor walks, and account for `work_used` on errors too.
use super::{Selector, SiblingIndex, css_whitespace, visit_selector_list};
use web_dom::{Document, NodeId};

pub const MAX_QUERY_BYTES: usize = 16 * 1024;
pub const MAX_QUERY_SELECTORS: usize = 64;
const MAX_SELECTOR_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorQueryError {
    /// Malformed syntax or a selector outside the bounded CSS subset.
    SyntaxOrUnsupported,
    /// Query bytes/count or the operation's shared work budget was exhausted.
    Resource,
}

impl std::fmt::Display for SelectorQueryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::SyntaxOrUnsupported => "Invalid or unsupported CSS selector",
            Self::Resource => "CSS selector query resource limit exceeded",
        })
    }
}
impl std::error::Error for SelectorQueryError {}

/// Opaque, all-or-nothing selector list. No stylesheet recovery is performed.
#[derive(Debug)]
pub struct SelectorList {
    selectors: Vec<Selector>,
}

impl SelectorList {
    pub fn parse(source: &str) -> Result<Self, SelectorQueryError> {
        if source.len() > MAX_QUERY_BYTES {
            return Err(SelectorQueryError::Resource);
        }
        // Count and bound before allocating either the list or selector ASTs.
        let mut count = 0usize;
        let mut resource = false;
        let valid = visit_selector_list(source, |branch| {
            count += 1;
            resource = count > MAX_QUERY_SELECTORS
                || branch.trim_matches(css_whitespace).len() > MAX_SELECTOR_BYTES;
            !resource
        });
        if resource {
            return Err(SelectorQueryError::Resource);
        }
        valid.ok_or(SelectorQueryError::SyntaxOrUnsupported)?;
        let mut selectors = Vec::with_capacity(count);
        visit_selector_list(source, |branch| {
            if let Some(selector) = Selector::parse(branch.trim_matches(css_whitespace)) {
                selectors.push(selector);
                true
            } else {
                false
            }
        })
        .ok_or(SelectorQueryError::SyntaxOrUnsupported)?;
        Ok(Self { selectors })
    }
}

/// One immutable document and one non-renewing budget per DOM operation.
///
/// Construction does not allocate an index. The first `matches` builds it once,
/// charging the arena size before allocation plus each inspected child edge.
/// Failed work debits the entire remaining allowance and cannot be retried with
/// a fresh allowance on this context. This conservative accounting is exposed
/// even if index construction or a comparison fails.
pub struct SelectorMatchContext<'a> {
    document: &'a Document,
    siblings: Option<SiblingIndex>,
    initial_work: usize,
    remaining_work: usize,
    exhausted: bool,
}

impl<'a> SelectorMatchContext<'a> {
    pub fn new(document: &'a Document, work_budget: usize) -> Self {
        Self {
            document,
            siblings: None,
            initial_work: work_budget,
            remaining_work: work_budget,
            exhausted: false,
        }
    }

    pub fn work_used(&self) -> usize {
        self.initial_work - self.remaining_work
    }

    pub fn remaining_work(&self) -> usize {
        self.remaining_work
    }

    /// Charge caller-side candidate/ancestor traversal to the same allowance.
    pub fn charge_work(&mut self, work: usize) -> Result<(), SelectorQueryError> {
        if self.exhausted {
            return Err(SelectorQueryError::Resource);
        }
        let Some(remaining) = self.remaining_work.checked_sub(work) else {
            return Err(self.exhaust());
        };
        self.remaining_work = remaining;
        Ok(())
    }

    pub fn matches(
        &mut self,
        selectors: &SelectorList,
        candidate: NodeId,
    ) -> Result<bool, SelectorQueryError> {
        if self.exhausted {
            return Err(SelectorQueryError::Resource);
        }
        if self.siblings.is_none() {
            match SiblingIndex::new(self.document, &mut self.remaining_work) {
                Ok(siblings) => self.siblings = Some(siblings),
                Err(_) => return Err(self.exhaust()),
            }
        }
        let siblings = self.siblings.as_ref().expect("index initialized above");
        for selector in &selectors.selectors {
            match selector.matches(self.document, candidate, siblings, &mut self.remaining_work) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(_) => return Err(self.exhaust()),
            }
        }
        Ok(false)
    }

    fn exhaust(&mut self) -> SelectorQueryError {
        self.remaining_work = 0;
        self.exhausted = true;
        SelectorQueryError::Resource
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(document: &Document, name: &str) -> NodeId {
        document
            .nodes
            .iter()
            .enumerate()
            .find_map(|(id, _)| {
                document
                    .element(id)
                    .filter(|element| element.attr("id") == Some(name))
                    .map(|_| id)
            })
            .unwrap()
    }

    #[test]
    fn compounds_lists_combinators_attributes_and_escapes_share_css_matching() {
        let document = web_html::parse(concat!(
            "<main class=outer lang=pt-BR><section><p id=first></p>",
            "text<span></span><p id='target' class='card x:y' ",
            "data-label='A,B' data-kind='VALUE' data-tags='one two'></p>",
            "</section></main>"
        ))
        .unwrap();
        let id = target(&document, "target");
        for source in [
            "p#target.card",
            "aside, main > section > p.card",
            "main .card",
            "span + p",
            "#first ~ p",
            "[data-label='A,B']",
            "[data-kind=value i]",
            "[data-tags~=two]",
            r".x\:y",
            r"#t\61 rget",
            ":is(.missing, .card):not([hidden])",
            "p:nth-child(3)",
            "p:last-child",
            ":lang(pt)",
        ] {
            let list = SelectorList::parse(source).unwrap();
            let mut context = SelectorMatchContext::new(&document, 100_000);
            assert!(context.matches(&list, id).unwrap(), "{source}");
            let mut budget = 100_000;
            let siblings = SiblingIndex::new(&document, &mut budget).unwrap();
            let expected = super::super::split_selector_list(source)
                .unwrap()
                .iter()
                .any(|branch| {
                    Selector::parse(branch.trim())
                        .unwrap()
                        .matches(&document, id, &siblings, &mut budget)
                        .unwrap()
                });
            assert_eq!(context.matches(&list, id).unwrap(), expected, "{source}");
        }
        let list = SelectorList::parse("p[data-kind=other], #first").unwrap();
        assert!(
            !SelectorMatchContext::new(&document, 100_000)
                .matches(&list, id)
                .unwrap()
        );
    }

    #[test]
    fn identifier_starts_are_strict_but_escaped_digits_match_real_nodes() {
        for source in [
            "#123",
            ".123",
            "123",
            "#-1",
            ".-1",
            "-1",
            "#-",
            ".-",
            "-",
            "p, #123",
            ":is(p, #-1)",
            ".\\\ninvalid",
        ] {
            assert_eq!(
                SelectorList::parse(source).unwrap_err(),
                SelectorQueryError::SyntaxOrUnsupported,
                "{source:?}"
            );
            assert!(
                super::super::Selector::parse(source).is_none(),
                "{source:?}"
            );
        }
        let document =
            web_html::parse("<p id='123' class='123 -1 -- --1 -valid _valid x1 \u{1f408}'></p>")
                .unwrap();
        let id = target(&document, "123");
        for source in [
            r"#\31 23",
            r".\31 23",
            r".-\31",
            ".--",
            ".--1",
            ".-valid",
            "._valid",
            ".x1",
            ".\u{1f408}",
        ] {
            let list = SelectorList::parse(source).unwrap();
            assert!(
                SelectorMatchContext::new(&document, 100_000)
                    .matches(&list, id)
                    .unwrap(),
                "{source:?}"
            );
        }
    }

    #[test]
    fn invalid_branch_rejects_the_entire_list_without_recovery() {
        for source in [
            "",
            " ",
            ",p",
            "p,",
            "p,,span",
            "p, :unknown",
            "p, div >",
            "p, [broken",
            "p, :is(.ok,)",
            "p, :has(span)",
            "p, :scope",
            "p, svg|rect",
            "p, ::before",
            "p, [x='unterminated]",
            "p, \\",
        ] {
            assert_eq!(
                SelectorList::parse(source).unwrap_err(),
                SelectorQueryError::SyntaxOrUnsupported,
                "{source}"
            );
        }
        // Commas inside strings, functions, or escapes are not outer branches.
        for source in ["[x=',']", ":is(p,span)", r".comma\,name"] {
            assert!(SelectorList::parse(source).is_ok(), "{source}");
        }
    }

    #[test]
    fn query_limits_are_checked_before_materializing_the_list() {
        assert_eq!(
            SelectorList::parse(&" ".repeat(MAX_QUERY_BYTES + 1)).unwrap_err(),
            SelectorQueryError::Resource
        );
        assert_eq!(
            SelectorList::parse(&"p,".repeat(MAX_QUERY_SELECTORS)).unwrap_err(),
            SelectorQueryError::Resource
        );
        assert_eq!(
            SelectorList::parse(&format!(".{}", "a".repeat(MAX_SELECTOR_BYTES))).unwrap_err(),
            SelectorQueryError::Resource
        );
        assert!(SelectorList::parse(&["p"; MAX_QUERY_SELECTORS].join(",")).is_ok());
        assert!(SelectorList::parse(&["p"; 33].join(" ")).is_err());
        let deeply_nested = format!("{}p{}", ":is(".repeat(9), ")".repeat(9));
        assert!(SelectorList::parse(&deeply_nested).is_err());
    }

    #[test]
    fn index_is_built_once_and_context_budget_is_not_renewed_per_candidate() {
        let document = web_html::parse("<main><p id=a></p><p id=b></p></main>").unwrap();
        let list = SelectorList::parse("*").unwrap();
        let indexing = document.nodes.len()
            + document
                .nodes
                .iter()
                .map(|node| node.children.len())
                .sum::<usize>();
        let mut context = SelectorMatchContext::new(&document, indexing + 2);
        assert_eq!(context.work_used(), 0);
        assert!(context.siblings.is_none());
        assert!(context.matches(&list, target(&document, "a")).unwrap());
        assert_eq!(context.work_used(), indexing + 1);
        assert!(context.matches(&list, target(&document, "b")).unwrap());
        assert_eq!(context.work_used(), indexing + 2);
        assert_eq!(context.matches(&list, 0), Err(SelectorQueryError::Resource));
        assert_eq!(context.remaining_work(), 0);
    }

    #[test]
    fn exhausted_index_and_string_work_remain_accounted_on_error() {
        let document = web_html::parse("<p id=target class=longclassname></p>").unwrap();
        let list = SelectorList::parse(".longclassname").unwrap();
        let budget = document.nodes.len() - 1;
        let mut context = SelectorMatchContext::new(&document, budget);
        assert_eq!(
            context.matches(&list, target(&document, "target")),
            Err(SelectorQueryError::Resource)
        );
        assert!(context.siblings.is_none());
        assert_eq!(context.work_used(), budget);
        assert_eq!(context.charge_work(0), Err(SelectorQueryError::Resource));
        let indexing = document.nodes.len()
            + document
                .nodes
                .iter()
                .map(|node| node.children.len())
                .sum::<usize>();
        let mut context = SelectorMatchContext::new(&document, indexing + 3);
        assert_eq!(
            context.matches(&list, target(&document, "target")),
            Err(SelectorQueryError::Resource)
        );
        assert_eq!(context.work_used(), indexing + 3);
    }

    #[test]
    fn whitespace_root_scans_and_of_type_strings_consume_work() {
        let document =
            web_html::parse(&format!("<p id=target class='{}'></p>", " ".repeat(512))).unwrap();
        let indexing = document.nodes.len()
            + document
                .nodes
                .iter()
                .map(|node| node.children.len())
                .sum::<usize>();
        let mut context = SelectorMatchContext::new(&document, indexing + 10);
        assert_eq!(
            context.matches(
                &SelectorList::parse(".x").unwrap(),
                target(&document, "target")
            ),
            Err(SelectorQueryError::Resource)
        );
        assert_eq!(context.work_used(), indexing + 10);

        let mut document = Document::default();
        for _ in 0..16 {
            document
                .append(0, web_dom::NodeKind::Text(" ".into()))
                .unwrap();
        }
        let root = document
            .append(
                0,
                web_dom::NodeKind::Element(web_dom::Element {
                    tag: "longtagname".repeat(8),
                    attributes: Default::default(),
                }),
            )
            .unwrap();
        let indexing = document.nodes.len()
            + document
                .nodes
                .iter()
                .map(|node| node.children.len())
                .sum::<usize>();
        for source in [":root", ":first-of-type"] {
            let list = SelectorList::parse(source).unwrap();
            let mut context = SelectorMatchContext::new(&document, indexing + 5);
            assert_eq!(
                context.matches(&list, root),
                Err(SelectorQueryError::Resource),
                "{source}"
            );
            assert_eq!(context.work_used(), indexing + 5);
            assert!(
                SelectorMatchContext::new(&document, 100_000)
                    .matches(&list, root)
                    .unwrap()
            );
        }
        // Isolate tag comparison from traversal: one child, a long tag string.
        document.nodes[0].children = vec![root];
        let indexing = document.nodes.len() + 1;
        let mut context = SelectorMatchContext::new(&document, indexing + 5);
        assert_eq!(
            context.matches(&SelectorList::parse(":first-of-type").unwrap(), root),
            Err(SelectorQueryError::Resource)
        );
    }
    #[test]
    fn caller_traversal_is_charged_without_restricting_matching_ancestors() {
        let document =
            web_html::parse("<main class=outside><section><p id=target></p></section></main>")
                .unwrap();
        let list = SelectorList::parse("main.outside p").unwrap();
        let mut context = SelectorMatchContext::new(&document, 100_000);
        context.charge_work(7).unwrap();
        assert_eq!(context.work_used(), 7);
        // A caller can enumerate only section's descendants; ancestors remain visible.
        assert!(context.matches(&list, target(&document, "target")).unwrap());
        let remaining = context.remaining_work();
        assert_eq!(
            context.charge_work(remaining + 1),
            Err(SelectorQueryError::Resource)
        );
        assert_eq!(context.work_used(), 100_000);
    }
}
