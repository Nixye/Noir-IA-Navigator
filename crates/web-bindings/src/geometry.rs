//! Synchronous measurements supplied by the renderer, never guessed by bindings.
use super::{DocumentSession, DomHost, NODE, WINDOW};
use browser_ipc::Viewport;
use std::{fmt::Debug, sync::Arc};
use web_dom::{Document, MAX_NODES};
use web_javascript::{JsError, Value};

#[derive(Clone, Copy, Debug, Default)]
pub struct NodeGeometry {
    pub client_width: f32,
    pub client_height: f32,
    pub client_left: f32,
    pub client_top: f32,
}

#[derive(Clone, Debug)]
pub struct GeometrySnapshot {
    pub nodes: Vec<NodeGeometry>,
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
}

impl DocumentSession {
    pub fn set_geometry_resolver(&mut self, resolver: Arc<dyn GeometryResolver>) {
        self.state.geometry.resolver = Some(resolver);
        self.state.geometry.snapshot = None;
        // Replacing viewport data never renews the lifetime work allowance.
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
        if object == WINDOW && matches!(property, "innerWidth" | "innerHeight") {
            return Some(self.state.geometry.resolver.is_some());
        }
        if !supported(property) {
            return None;
        }
        let id = object.checked_sub(NODE)?;
        if id >= MAX_NODES as u64 {
            return None;
        }
        Some(self.document.element(id as usize).is_some())
    }

    pub(super) fn geometry_get(
        &mut self,
        object: u64,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
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
        let id = (object - NODE) as usize;
        if self.state.geometry.snapshot.is_none()
            || self.state.geometry.mutations != self.state.mutations
        {
            let resolver = self.state.geometry.resolver.clone().ok_or_else(|| {
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
            if snapshot.nodes.len() != self.document.nodes.len() || snapshot.nodes.len() > MAX_NODES
            {
                return Err(JsError::resource("Invalid layout measurement count"));
            }
            for node in &snapshot.nodes {
                if [
                    node.client_width,
                    node.client_height,
                    node.client_left,
                    node.client_top,
                ]
                .into_iter()
                .any(|value| !value.is_finite() || !(0. ..=1_000_000.).contains(&value))
                {
                    return Err(JsError::resource("Invalid layout measurement geometry"));
                }
            }
            self.state.geometry.snapshot = Some(Arc::new(snapshot));
            self.state.geometry.mutations = self.state.mutations;
        }
        let node = self
            .state
            .geometry
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.nodes.get(id))
            .ok_or_else(|| JsError::runtime("Missing layout measurement"))?;
        let number = match property {
            "clientWidth" => node.client_width,
            "clientHeight" => node.client_height,
            "clientLeft" => node.client_left,
            "clientTop" => node.client_top,
            _ => unreachable!(),
        };
        Ok(Some(Value::Number(f64::from(number.round()))))
    }
}
