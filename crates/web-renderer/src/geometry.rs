//! Renderer-owned synchronous layout service for DOM measurement reads.
use browser_ipc::{ImageAsset, StylesheetAsset, Viewport};
use std::sync::Arc;
use web_bindings::{GeometryResolver, GeometrySnapshot, NodeGeometry};
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
            if dom.mode == DocumentMode::UnsupportedDoctype
                && matches!(element.tag.as_str(), "html" | "body")
            {
                return Err(JsError::runtime(
                    "Client viewport dimensions for this document mode are not implemented",
                ));
            }
            nodes[id] = NodeGeometry {
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
        Ok(GeometrySnapshot { nodes })
    }
}
