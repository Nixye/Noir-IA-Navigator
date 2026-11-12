//! Renderer-owned synchronous layout service for DOM measurement reads.
use browser_ipc::{ImageAsset, Rect, StylesheetAsset, Viewport};
use std::sync::Arc;
use web_bindings::{
    BoundingGeometry, GeometryResolver, GeometrySnapshot, NodeGeometry, UnsupportedGeometry,
};
use web_dom::{Document, DocumentMode, MAX_NODES, NodeKind};
use web_javascript::JsError;

#[derive(Debug)]
pub(super) struct PageGeometry {
    pub viewport: Viewport,
    pub images: Arc<Vec<ImageAsset>>,
    pub stylesheets: Arc<Vec<StylesheetAsset>>,
}

/// Keep NodeIds stable while excluding detached nodes from styles and paint.
pub(super) fn paint_document(document: &Document) -> Result<Document, String> {
    if document.nodes.is_empty() || document.nodes.len() > MAX_NODES {
        return Err("Invalid measurement DOM size".into());
    }
    let mut reachable = vec![false; document.nodes.len()];
    let mut pending = vec![0];
    while let Some(id) = pending.pop() {
        let Some(seen) = reachable.get_mut(id) else {
            return Err("Invalid DOM child in layout snapshot".into());
        };
        if *seen {
            return Err("Repeated DOM node in layout snapshot".into());
        }
        *seen = true;
        let children = &document.nodes[id].children;
        if pending.len().saturating_add(children.len()) > MAX_NODES {
            return Err("Layout traversal limit exceeded".into());
        }
        pending.extend(children.iter().copied());
    }
    let mut snapshot = document.clone();
    for (id, node) in snapshot.nodes.iter_mut().enumerate() {
        if !reachable[id] {
            node.kind = NodeKind::Text(String::new());
            node.parent = None;
            node.children.clear();
        }
    }
    Ok(snapshot)
}

impl GeometryResolver for PageGeometry {
    fn viewport(&self) -> Viewport {
        self.viewport
    }
    fn resolve(&self, document: &Document) -> Result<GeometrySnapshot, JsError> {
        let dom = paint_document(document).map_err(JsError::resource)?;
        let computed = web_css::compute_with_stylesheets(&dom, &self.stylesheets, self.viewport)
            .map_err(JsError::resource)?;
        let layout = web_layout::layout(&dom, &computed.styles, &self.images, self.viewport)
            .map_err(JsError::resource)?;
        let mut nodes = vec![NodeGeometry::default(); dom.nodes.len()];
        for (id, reason) in layout.unsupported_geometry.iter().enumerate() {
            if let Some(reason) = reason {
                nodes[id].bounding = BoundingGeometry::Unsupported(match reason {
                    web_layout::UnsupportedGeometry::Inline => UnsupportedGeometry::Inline,
                    web_layout::UnsupportedGeometry::TableStructure => {
                        UnsupportedGeometry::TableStructure
                    }
                    web_layout::UnsupportedGeometry::InlineImageDecoration => {
                        UnsupportedGeometry::InlineImageDecoration
                    }
                    web_layout::UnsupportedGeometry::AtomicContents => {
                        UnsupportedGeometry::AtomicContents
                    }
                });
            }
        }
        for (id, metrics) in layout.boxes.iter().enumerate() {
            let Some(metrics) = metrics else {
                continue;
            };
            let Some(element) = dom.element(id) else {
                continue;
            };
            let viewport_box = (element.tag == "html"
                && matches!(
                    dom.mode,
                    DocumentMode::NoQuirks | DocumentMode::LimitedQuirks
                ))
                || (element.tag == "body" && dom.mode == DocumentMode::Quirks);
            nodes[id] = NodeGeometry {
                client_available: !(dom.mode == DocumentMode::UnsupportedDoctype
                    && matches!(element.tag.as_str(), "html" | "body")),
                bounding: if layout.unsupported_geometry[id].is_some() {
                    nodes[id].bounding
                } else {
                    BoundingGeometry::Box(Rect {
                        x: metrics.x,
                        y: metrics.y,
                        width: metrics.width,
                        height: metrics.height,
                    })
                },
                client_width: if viewport_box {
                    self.viewport.width
                } else {
                    (metrics.width - 2. * metrics.border).max(0.)
                },
                client_height: if viewport_box {
                    self.viewport.height
                } else {
                    (metrics.height - 2. * metrics.border).max(0.)
                },
                client_left: metrics.border.min(metrics.width).max(0.),
                client_top: metrics.border.min(metrics.height).max(0.),
            };
        }
        Ok(GeometrySnapshot {
            nodes,
            document_width: layout.width,
            document_height: layout.height,
        })
    }
}

#[cfg(test)]
mod recovery_r5_tests {
    use super::*;
    fn resolve(source: &str) -> (Document, GeometrySnapshot) {
        let d = web_html::parse(&format!("<style>body{{margin:0}}</style>{source}")).unwrap();
        let provider = PageGeometry {
            viewport: Viewport::default(),
            images: Arc::new(vec![]),
            stylesheets: Arc::new(vec![]),
        };
        let result = provider.resolve(&d).unwrap();
        (d, result)
    }
    fn node(d: &Document, name: &str) -> usize {
        d.elements()
            .find(|(_, e)| e.attr("id") == Some(name))
            .unwrap()
            .0
    }
    #[test]
    fn provider_transports_document_coordinates_not_client_dimensions() {
        let (d, s) = resolve(
            "<div id=x style='position:relative;left:12.5px;top:9.25px;width:20px;height:10px;padding:2px;border:1px solid red'></div>",
        );
        let g = &s.nodes[node(&d, "x")];
        let BoundingGeometry::Box(r) = g.bounding else {
            panic!("expected principal box")
        };
        assert_eq!((r.x, r.y, r.width, r.height), (12.5, 9.25, 26., 16.));
        assert_eq!(
            (g.client_width, g.client_height, g.client_left),
            (24., 14., 1.)
        );
        assert!(s.document_width >= Viewport::default().width);
        assert!(s.document_height >= Viewport::default().height);
    }
    #[test]
    fn provider_distinguishes_unsupported_and_no_box() {
        let (d, s) = resolve(
            "<span id=x>text</span><span id=n style='display:none'>text</span><button><span id=a>text</span></button>",
        );
        assert!(matches!(
            s.nodes[node(&d, "x")].bounding,
            BoundingGeometry::Unsupported(UnsupportedGeometry::Inline)
        ));
        assert!(matches!(
            s.nodes[node(&d, "n")].bounding,
            BoundingGeometry::NoBox
        ));
        assert!(matches!(
            s.nodes[node(&d, "a")].bounding,
            BoundingGeometry::Unsupported(UnsupportedGeometry::AtomicContents)
        ));
    }
    #[test]
    fn detached_element_is_not_measured_or_classified_as_inline() {
        let mut d = web_html::parse("<span id=x>text</span>").unwrap();
        let n = node(&d, "x");
        let parent = d.nodes[n].parent.unwrap();
        d.nodes[parent].children.retain(|child| *child != n);
        d.nodes[n].parent = None;
        let provider = PageGeometry {
            viewport: Viewport::default(),
            images: Arc::new(vec![]),
            stylesheets: Arc::new(vec![]),
        };
        let s = provider.resolve(&d).unwrap();
        assert!(matches!(s.nodes[n].bounding, BoundingGeometry::NoBox));
    }
    #[test]
    fn unsupported_document_mode_disables_only_client_viewport_exception() {
        let mut d = web_html::parse("<body><div id=x style='width:20px;height:10px'></div></body>")
            .unwrap();
        d.mode = DocumentMode::UnsupportedDoctype;
        let provider = PageGeometry {
            viewport: Viewport::default(),
            images: Arc::new(vec![]),
            stylesheets: Arc::new(vec![]),
        };
        let s = provider.resolve(&d).unwrap();
        assert!(!s.nodes[d.document_element().unwrap()].client_available);
        assert!(matches!(
            s.nodes[d.body().unwrap()].bounding,
            BoundingGeometry::Box(_)
        ));
        assert!(s.nodes[node(&d, "x")].client_available);
    }
}
