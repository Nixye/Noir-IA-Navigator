//! Arena-owned document tree. IDs remain stable for a document's lifetime.
use std::collections::BTreeMap;

pub type NodeId = usize;
pub const MAX_NODES: usize = 20_000;
pub const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag: String,
    pub attributes: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn element(tag: &str) -> NodeKind {
        NodeKind::Element(Element {
            tag: tag.into(),
            attributes: BTreeMap::new(),
        })
    }
    #[test]
    fn structural_helpers_do_not_search_nested_or_later_roots() {
        let mut d = Document::default();
        let root = d.append(0, element("html")).unwrap();
        let div = d.append(root, element("div")).unwrap();
        d.append(div, element("head")).unwrap();
        let body = d.append(root, element("body")).unwrap();
        assert_eq!(d.document_element(), Some(root));
        assert_eq!(d.head(), None);
        assert_eq!(d.body(), Some(body));
        let other = d.append(0, element("main")).unwrap();
        d.nodes[0].children.swap(0, 1);
        assert_eq!(d.document_element(), Some(other));
        assert_eq!(d.body(), None);
    }

    #[test]
    fn title_uses_connected_tree_order_and_preserves_non_ascii_spaces() {
        let mut d = Document::default();
        assert_eq!(d.document_title(), "");
        assert!(!d.title().is_empty());
        let first = d.append(0, element("title")).unwrap();
        d.append(first, NodeKind::Text("Arena first".into()))
            .unwrap();
        let second = d.append(0, element("title")).unwrap();
        d.append(second, NodeKind::Text(" \tTree\nfirst\u{a0} \r".into()))
            .unwrap();
        d.nodes[0].children.swap(0, 1);
        assert_eq!(d.title_element(), Some(second));
        assert_eq!(d.document_title(), "Tree first\u{a0}");
        d.nodes[0].children.remove(0);
        d.nodes[second].parent = None;
        assert_eq!(d.title_element(), Some(first));
        let inert = d.append(0, element("noscript")).unwrap();
        let title = d.append(inert, element("title")).unwrap();
        d.append(title, NodeKind::Text("Inert".into())).unwrap();
        d.nodes[0].children.swap(0, 1);
        assert_eq!(d.title_element(), Some(first));
    }
    #[test]
    fn inactive_ancestors_are_bounded_and_do_not_supply_the_document_title() {
        let mut document = Document::default();
        let template = document.append(0, element("template")).unwrap();
        let inactive_title = document.append(template, element("title")).unwrap();
        document
            .append(inactive_title, NodeKind::Text("Inactive".into()))
            .unwrap();
        let title = document.append(0, element("title")).unwrap();
        document
            .append(title, NodeKind::Text("Live".into()))
            .unwrap();
        assert!(document.is_inactive_subtree(inactive_title));
        assert!(!document.is_inactive_subtree(title));
        assert_eq!(document.title(), "Live");
        assert!(document.is_inactive_subtree(usize::MAX));
        document.nodes[title].parent = Some(title);
        assert!(document.is_inactive_subtree(title));
    }
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub kind: NodeKind,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub nodes: Vec<Node>,
    /// Parser metadata only; the layout crate does not yet implement all quirks.
    pub mode: DocumentMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocumentMode {
    #[default]
    NoQuirks,
    LimitedQuirks,
    Quirks,
    /// A legacy declaration outside the classifier's documented subset.
    UnsupportedDoctype,
}

impl DocumentMode {
    pub fn compat_mode(self) -> Option<&'static str> {
        match self {
            Self::Quirks => Some("BackCompat"),
            Self::NoQuirks | Self::LimitedQuirks => Some("CSS1Compat"),
            Self::UnsupportedDoctype => None,
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self {
            mode: DocumentMode::NoQuirks,
            nodes: vec![Node {
                parent: None,
                children: Vec::new(),
                kind: NodeKind::Document,
            }],
        }
    }
}

impl Document {
    /// Content of templates and scripting-disabled fallbacks is inert while
    /// this browser's scripting mode is enabled. Consumers must not activate
    /// subordinate scripts, styles or resource requests from these subtrees.
    pub fn is_inactive_subtree(&self, id: NodeId) -> bool {
        let mut current = Some(id);
        for _ in 0..=MAX_DEPTH {
            let Some(id) = current else {
                return false;
            };
            let Some(node) = self.nodes.get(id) else {
                return true;
            };
            if matches!(&node.kind, NodeKind::Element(element) if matches!(element.tag.as_str(), "template" | "noscript"))
            {
                return true;
            }
            current = node.parent;
        }
        true
    }
    pub fn append(&mut self, parent: NodeId, kind: NodeKind) -> Result<NodeId, String> {
        if self.nodes.len() >= MAX_NODES {
            return Err("HTML node limit exceeded".into());
        }
        let id = self.nodes.len();
        self.nodes.push(Node {
            parent: Some(parent),
            children: Vec::new(),
            kind,
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.nodes.get(id)?.kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn elements(&self) -> impl Iterator<Item = (NodeId, &Element)> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| match &node.kind {
                NodeKind::Element(element) => Some((id, element)),
                _ => None,
            })
    }

    pub fn text_content(&self, root: NodeId) -> String {
        let mut output = String::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if let NodeKind::Text(text) = &self.nodes[id].kind {
                output.push_str(text);
            }
            stack.extend(self.nodes[id].children.iter().rev().copied());
        }
        output
    }

    /// The first element directly parented by the document root.
    pub fn document_element(&self) -> Option<NodeId> {
        self.nodes.first()?.children.iter().copied().find(|&id| {
            self.nodes
                .get(id)
                .is_some_and(|node| node.parent == Some(0))
                && self.element(id).is_some()
        })
    }

    /// Structural head/body lookup never searches nested or detached subtrees.
    pub fn head(&self) -> Option<NodeId> {
        let root = self.document_element()?;
        (self.element(root)?.tag == "html")
            .then(|| self.structural_child(root, "head"))
            .flatten()
    }

    pub fn body(&self) -> Option<NodeId> {
        let root = self.document_element()?;
        (self.element(root)?.tag == "html")
            .then(|| self.structural_child(root, "body"))
            .flatten()
    }

    fn structural_child(&self, parent: NodeId, tag: &str) -> Option<NodeId> {
        self.nodes
            .get(parent)?
            .children
            .iter()
            .copied()
            .find(|&id| {
                self.nodes
                    .get(id)
                    .is_some_and(|node| node.parent == Some(parent))
                    && self.element(id).is_some_and(|element| element.tag == tag)
            })
    }

    /// First connected, active title in tree order, independent of arena order.
    pub fn title_element(&self) -> Option<NodeId> {
        let mut stack = vec![(0, 0)];
        let mut visited = vec![false; self.nodes.len()];
        while let Some((id, depth)) = stack.pop() {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            if depth >= MAX_DEPTH || visited[id] {
                continue;
            }
            visited[id] = true;
            if let NodeKind::Element(element) = &node.kind {
                if matches!(element.tag.as_str(), "template" | "noscript") {
                    continue;
                }
                if element.tag == "title" {
                    return Some(id);
                }
            }
            for &child in node.children.iter().rev() {
                if self
                    .nodes
                    .get(child)
                    .is_some_and(|node| node.parent == Some(id))
                {
                    stack.push((child, depth + 1));
                }
            }
        }
        None
    }

    /// DOM title without the window's display fallback; only ASCII whitespace
    /// is collapsed, so nonbreaking spaces remain part of the actual title.
    pub fn document_title(&self) -> String {
        self.title_element()
            .map(|id| {
                self.text_content(id)
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    }

    pub fn title(&self) -> String {
        Some(self.document_title())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Documento sem título".into())
    }
}
