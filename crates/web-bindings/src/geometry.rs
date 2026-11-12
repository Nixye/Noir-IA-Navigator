//! Synchronous measurements supplied by the renderer, never guessed by bindings.
use super::{DocumentSession, DomHost, NODE, WINDOW};
use browser_ipc::{Rect, ViewContext, Viewport};
use std::{fmt::Debug, sync::Arc};
use web_dom::{Document, MAX_NODES};
use web_javascript::{JsError, Runtime, Value};

pub(super) const BOUNDING_METHOD: u64 = 120_040;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedGeometry {
    Inline,
    TableStructure,
    InlineImageDecoration,
    AtomicContents,
}
#[derive(Clone, Copy, Debug, Default)]
pub enum BoundingGeometry {
    #[default]
    NoBox,
    Box(Rect),
    Unsupported(UnsupportedGeometry),
}
#[derive(Clone, Copy, Debug)]
pub struct NodeGeometry {
    pub client_width: f32,
    pub client_height: f32,
    pub client_left: f32,
    pub client_top: f32,
    pub client_available: bool,
    pub bounding: BoundingGeometry,
}
impl Default for NodeGeometry {
    fn default() -> Self {
        Self {
            client_width: 0.,
            client_height: 0.,
            client_left: 0.,
            client_top: 0.,
            client_available: true,
            bounding: BoundingGeometry::NoBox,
        }
    }
}
#[derive(Clone, Debug)]
pub struct GeometrySnapshot {
    pub nodes: Vec<NodeGeometry>,
    pub document_width: f32,
    pub document_height: f32,
}
/// Data-only engine callback. Implementations must not perform page-directed I/O.
pub trait GeometryResolver: Debug + Send + Sync {
    fn resolve(&self, document: &Document) -> Result<GeometrySnapshot, JsError>;
    fn viewport(&self) -> Viewport;
}
#[derive(Clone, Debug, Default)]
pub(super) struct GeometryState {
    resolver: Option<Arc<dyn GeometryResolver>>,
    snapshot: Option<Arc<GeometrySnapshot>>,
    mutations: usize,
    resolutions: usize,
    context: Option<ViewContext>,
}
impl DocumentSession {
    pub fn set_geometry_resolver(&mut self, resolver: Arc<dyn GeometryResolver>) {
        self.state.geometry.resolver = Some(resolver);
        self.state.geometry.snapshot = None;
        // Replacement never renews the lifetime work allowance.
    }
    pub fn set_view_context(&mut self, context: Option<ViewContext>) -> Result<(), JsError> {
        self.state.geometry.context = None;
        browser_ipc::validate_view_context(context).map_err(JsError::runtime)?;
        self.state.geometry.context = context;
        Ok(())
    }
}
fn supported(property: &str) -> bool {
    matches!(
        property,
        "clientWidth" | "clientHeight" | "clientLeft" | "clientTop"
    )
}
impl DomHost<'_> {
    pub(super) fn geometry_global(&self, property: &str) -> Option<Value> {
        if !matches!(property, "innerWidth" | "innerHeight") {
            return None;
        }
        let viewport = self.state.geometry.resolver.as_ref()?.viewport();
        let value = if property == "innerWidth" {
            viewport.width
        } else {
            viewport.height
        };
        (value.is_finite() && (1. ..=16384.).contains(&value))
            .then_some(Value::Number(f64::from(value)))
    }
    pub(super) fn geometry_has(&self, object: u64, property: &str) -> Option<bool> {
        if object == BOUNDING_METHOD {
            return Some(matches!(property, "length" | "name"));
        }
        if object == WINDOW && matches!(property, "innerWidth" | "innerHeight") {
            return Some(self.state.geometry.resolver.is_some());
        }
        if !supported(property) && property != "getBoundingClientRect" {
            return None;
        }
        let id = object.checked_sub(NODE)?;
        if id >= MAX_NODES as u64 {
            return None;
        }
        Some(self.document.element(id as usize).is_some())
    }
    fn resolve_geometry(&mut self) -> Result<(), JsError> {
        if self.state.geometry.snapshot.is_some()
            && self.state.geometry.mutations == self.state.mutations
        {
            return Ok(());
        }
        let resolver =
            self.state.geometry.resolver.clone().ok_or_else(|| {
                JsError::runtime("Layout measurements are unavailable in this host")
            })?;
        if self.state.geometry.resolutions >= 64 {
            return Err(JsError::resource(
                "Synchronous layout resolution budget exceeded",
            ));
        }
        self.charge_tree_read(self.document.nodes.len())?;
        self.state.geometry.resolutions += 1;
        let snapshot = resolver.resolve(self.document)?;
        if snapshot.nodes.len() != self.document.nodes.len() || snapshot.nodes.len() > MAX_NODES {
            return Err(JsError::resource("Invalid layout measurement count"));
        }
        if [snapshot.document_width, snapshot.document_height]
            .into_iter()
            .any(|v| !v.is_finite() || !(0. ..=1_000_000.).contains(&v))
        {
            return Err(JsError::resource("Invalid layout document extent"));
        }
        for node in &snapshot.nodes {
            if [
                node.client_width,
                node.client_height,
                node.client_left,
                node.client_top,
            ]
            .into_iter()
            .any(|v| !v.is_finite() || !(0. ..=1_000_000.).contains(&v))
            {
                return Err(JsError::resource("Invalid layout measurement geometry"));
            }
            if let BoundingGeometry::Box(rect) = node.bounding
                && ([rect.x, rect.y]
                    .into_iter()
                    .any(|v| !v.is_finite() || !(-1_000_000. ..=1_000_000.).contains(&v))
                    || [rect.width, rect.height]
                        .into_iter()
                        .any(|v| !v.is_finite() || !(0. ..=1_000_000.).contains(&v)))
            {
                return Err(JsError::resource("Invalid bounding box geometry"));
            }
        }
        self.state.geometry.snapshot = Some(Arc::new(snapshot));
        self.state.geometry.mutations = self.state.mutations;
        Ok(())
    }
    pub(super) fn geometry_get(
        &mut self,
        object: u64,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if object == BOUNDING_METHOD {
            return Ok(Some(match property {
                "name" => Value::string("getBoundingClientRect"),
                "length" => Value::Number(0.),
                _ => Value::Undefined,
            }));
        }
        if object == WINDOW && matches!(property, "innerWidth" | "innerHeight") {
            self.charge_tree_read(1)?;
            return self.geometry_global(property).map(Some).ok_or_else(|| {
                JsError::runtime("Viewport measurements are unavailable in this host")
            });
        }
        if self.geometry_has(object, property) != Some(true) {
            return Ok(None);
        }
        self.charge_tree_read(1)?;
        if property == "getBoundingClientRect" {
            return Ok(Some(Value::Host(BOUNDING_METHOD)));
        }
        let id = (object - NODE) as usize;
        self.resolve_geometry()?;
        let node = self
            .state
            .geometry
            .snapshot
            .as_ref()
            .and_then(|s| s.nodes.get(id))
            .ok_or_else(|| JsError::runtime("Missing layout measurement"))?;
        if !node.client_available {
            return Err(JsError::runtime(
                "Client viewport dimensions for this document mode are not implemented",
            ));
        }
        let value = match property {
            "clientWidth" => node.client_width,
            "clientHeight" => node.client_height,
            "clientLeft" => node.client_left,
            "clientTop" => node.client_top,
            _ => unreachable!(),
        };
        Ok(Some(Value::Number(f64::from(value.round()))))
    }
    pub(super) fn bounding_call(
        &mut self,
        object: u64,
        method: &str,
        this: &Value,
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        let receiver = if object == BOUNDING_METHOD {
            if method != "call" {
                return Err(JsError::runtime("Invalid bounding method invocation"));
            }
            this.as_host()
                .ok_or_else(|| JsError::runtime("Element receiver required"))?
        } else {
            if method != "getBoundingClientRect" {
                return Err(JsError::runtime("Invalid bounding method invocation"));
            }
            object
        };
        let id = self.node(receiver)?;
        if self.document.element(id).is_none() {
            return Err(JsError::runtime("Element receiver required"));
        }
        self.charge_tree_read(1)?;
        self.resolve_geometry()?;
        let snapshot = self
            .state
            .geometry
            .snapshot
            .as_ref()
            .ok_or_else(|| JsError::runtime("Missing layout measurement"))?;
        let rect = match snapshot.nodes[id].bounding {
            BoundingGeometry::NoBox => Rect {
                x: 0.,
                y: 0.,
                width: 0.,
                height: 0.,
            },
            BoundingGeometry::Unsupported(reason) => {
                return Err(JsError::runtime(format!(
                    "Bounding geometry is not implemented for {reason:?}"
                )));
            }
            BoundingGeometry::Box(mut rect) => {
                let (x, y) = match self.state.geometry.context {
                    None => return Err(JsError::runtime("CSS viewport origin is unknown")),
                    Some(ViewContext::Origin) => (0., 0.),
                    Some(ViewContext::Sampled {
                        scroll_x,
                        scroll_y,
                        clip_width,
                        clip_height,
                    }) => (
                        scroll_x.clamp(
                            0.,
                            (snapshot.document_width.min(1_000_000.) - clip_width).max(0.),
                        ),
                        scroll_y.clamp(
                            0.,
                            (snapshot.document_height.min(1_000_000.) - clip_height).max(0.),
                        ),
                    ),
                };
                rect.x -= x;
                rect.y -= y;
                rect
            }
        };
        let x = f64::from(rect.x);
        let y = f64::from(rect.y);
        let width = f64::from(rect.width);
        let height = f64::from(rect.height);
        runtime.numeric_record(&[
            ("x", x),
            ("y", y),
            ("width", width),
            ("height", height),
            ("left", x),
            ("top", y),
            ("right", x + width),
            ("bottom", y + height),
        ])
    }
}
#[cfg(test)]
mod recovery_r5_tests;
