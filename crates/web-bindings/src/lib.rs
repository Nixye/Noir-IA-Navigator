//! Data-only DOM bindings for Noir's original JavaScript VM.
//! Sessions retain callbacks and timers in their renderer. No filesystem,
//! network or privileged browser services exist in this host contract.
use browser_ipc::ScriptAsset;
use std::collections::BTreeMap;
use web_dom::{Document, Element, MAX_DEPTH, MAX_NODES, Node, NodeId, NodeKind};
use web_javascript::{ErrorKind, Host, JsError, Runtime, Value};
mod animation;
#[cfg(test)]
mod array_from_tests;
#[cfg(test)]
mod crypto_context_tests;
#[cfg(test)]
mod promise_host_tests;
pub use animation::MAX_ANIMATION_FRAME_TIMESTAMP_MS;
mod diagnostics;
mod events;
mod geometry;
mod lifecycle;
mod media;
#[cfg(test)]
mod selector_query_tests;
mod storage;
#[cfg(test)]
mod wasm_i64_tests;
pub use geometry::{GeometryResolver, GeometrySnapshot, NodeGeometry};
use lifecycle::ReadyState;

const DOC: u64 = 1;
const WINDOW: u64 = 2;
const CONSOLE: u64 = 3;
const MATH: u64 = 4;
const NAVIGATOR: u64 = 5;
const PLUGINS: u64 = 6;
const MIME_TYPES: u64 = 7;
const LOCATION: u64 = 8;
const NAVIGATOR_METHOD: u64 = 70;
const NAVIGATOR_METHODS: [&str; 2] = ["sendBeacon", "javaEnabled"];
const LOCATION_STRING: u64 = 72;
const LOCATION_PROPERTIES: [&str; 10] = [
    "href", "origin", "protocol", "host", "hostname", "port", "pathname", "search", "hash",
    "toString",
];
const PLUGIN_METHOD: u64 = 80;
const PLUGIN_METHODS: [&str; 3] = ["item", "namedItem", "refresh"];
const SET_TIMEOUT: u64 = 10;
const CLEAR_TIMEOUT: u64 = 11;
const ADD_WINDOW_LISTENER: u64 = 12;
const REMOVE_WINDOW_LISTENER: u64 = 13;
const EVENT_CONSTRUCTOR: u64 = 14;
const CUSTOM_EVENT_CONSTRUCTOR: u64 = 15;
const DISPATCH_WINDOW_EVENT: u64 = 16;
const MAX_DISPATCH_DEPTH: usize = 32;
const DOCUMENT_METHOD: u64 = 40;
const DOCUMENT_METHODS: [&str; 14] = [
    "getElementById",
    "querySelector",
    "querySelectorAll",
    "createElement",
    "createTextNode",
    "addEventListener",
    "removeEventListener",
    "insertBefore",
    "appendChild",
    "removeChild",
    "replaceChild",
    "hasChildNodes",
    "contains",
    "dispatchEvent",
];
const EVENT_METHOD: u64 = 21;
const EVENT_METHODS: [&str; 3] = [
    "preventDefault",
    "stopPropagation",
    "stopImmediatePropagation",
];
const EVENT_BASE: u64 = 5_000_000;
const MAX_EVENTS: usize = 256;
const CLASS_METHOD: u64 = 120_000;
const CLASS_METHODS: [&str; 4] = ["contains", "add", "remove", "toggle"];
const CONSOLE_METHOD: u64 = 120_010;
const CONSOLE_METHODS: [&str; 5] = ["log", "info", "warn", "error", "debug"];
const NODE: u64 = 100;
const NODE_METHOD: u64 = 100_000;
const NODE_METHODS: [&str; 19] = [
    "appendChild",
    "insertBefore",
    "removeChild",
    "replaceChild",
    "remove",
    "hasChildNodes",
    "contains",
    "getAttribute",
    "setAttribute",
    "hasAttribute",
    "removeAttribute",
    "matches",
    "closest",
    "querySelector",
    "querySelectorAll",
    "addEventListener",
    "removeEventListener",
    "cloneNode",
    "dispatchEvent",
];
const STYLE: u64 = 1_000_000;
const CLASS: u64 = 2_000_000;
const LIST: u64 = 3_000_000;

fn navigator_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "Win32"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "Linux x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "Linux aarch64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "MacIntel"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "MacARM"
    } else {
        std::env::consts::OS
    }
}
#[derive(Debug, Clone)]
struct Listener {
    id: u64,
    target: u64,
    kind: String,
    callback: Value,
    property: bool,
    options: ListenerOptions,
}
#[derive(Debug, Clone, Copy, Default)]
struct ListenerOptions {
    capture: bool,
    once: bool,
    passive: bool,
}
#[derive(Debug, Clone)]
struct DomEvent {
    kind: String,
    target: Option<u64>,
    detail: Value,
    custom: bool,
    trusted: bool,
    dispatching: bool,
    current: Option<u64>,
    phase: u8,
    bubbles: bool,
    cancelable: bool,
    canceled: bool,
    stopped: bool,
    immediate: bool,
    passive: bool,
    timestamp: u64,
}
#[derive(Debug, Clone)]
struct Timer {
    id: u64,
    due: u64,
    callback: Value,
    args: Vec<Value>,
}
#[derive(Debug, Clone, Default)]
struct State {
    listeners: Vec<Listener>,
    timers: Vec<Timer>,
    animation: animation::AnimationState,
    lists: Vec<Vec<NodeId>>,
    next_timer: u64,
    clock: u64,
    mutations: usize,
    string_bytes: usize,
    events: Vec<DomEvent>,
    dispatch_depth: usize,
    next_listener: u64,
    geometry: geometry::GeometryState,
    logs: Vec<String>,
    read_work: usize,
    ready_state: ReadyState,
    document_url: Option<url::Url>,
    hardware_concurrency: Option<usize>,
    storage: storage::StorageState,
    media: media::MediaState,
}
#[derive(Debug, Default)]
pub struct ScriptReport {
    pub executed: usize,
    pub warnings: Vec<String>,
    pub pending_event_handlers: usize,
    pub pending_timers: usize,
}

/// Owns a DOM and its VM together; callback IDs never cross a process boundary.
pub struct DocumentSession {
    pub document: Document,
    runtime: Runtime,
    state: State,
    scripts_disabled: bool,
    last_animation_frame_timestamp_ms: Option<f64>,
    initial_started: bool,
    // Trust metadata must never be reopened by DOM/VM transaction rollback.
    document_url_frozen: bool,
    storage_started: bool,
    media_started: bool,
}
impl DocumentSession {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            runtime: Runtime::default(),
            state: State {
                hardware_concurrency: std::thread::available_parallelism()
                    .ok()
                    .map(|count| count.get().min(64)),
                ..State::default()
            },
            scripts_disabled: false,
            last_animation_frame_timestamp_ms: None,
            initial_started: false,
            document_url_frozen: false,
            storage_started: false,
            media_started: false,
        }
    }
    /// Supply the actual broker-authorized document address before executing
    /// scripts. This only parses data; the host never opens or navigates it.
    /// Any execution attempt freezes this metadata, even if it fails or rolls
    /// back. Initial lifecycle, event delivery and timer polling also freeze it.
    pub fn set_document_url(&mut self, address: &str) -> Result<(), JsError> {
        if self.document_url_frozen || self.initial_started {
            return Err(JsError::runtime(
                "Document URL metadata is frozen after execution starts",
            ));
        }
        if address.len() > 8192 {
            return Err(JsError::resource("Document URL exceeds 8192 bytes"));
        }
        let parsed = url::Url::parse(address)
            .map_err(|_| JsError::runtime("Invalid document URL metadata"))?;
        if parsed.as_str().len() > 8192 {
            return Err(JsError::resource(
                "Serialized document URL exceeds 8192 bytes",
            ));
        }
        self.state.document_url = Some(parsed);
        Ok(())
    }
    pub fn execute(&mut self, source: &str) -> Result<Value, JsError> {
        self.document_url_frozen = true;
        self.storage_started = true;
        self.media_started = true;
        if self.scripts_disabled {
            return Err(JsError::resource(
                "JavaScript disabled after resource exhaustion",
            ));
        }
        let old_document = self.document.clone();
        let old_runtime = self.runtime.clone();
        let old_state = self.state.clone();
        let mut result = self.runtime.execute(
            source,
            &mut DomHost {
                document: &mut self.document,
                state: &mut self.state,
            },
        );
        if let Err(error) = &result
            && error.kind != ErrorKind::Resource
            && let Err(handler_error) = self.notify_window_error(error)
        {
            result = Err(handler_error);
        }
        if result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
        {
            self.document = old_document;
            self.state = old_state;
            self.runtime.rollback_to(old_runtime);
            self.disable_scripts();
        }
        result
    }
    fn notify_window_error(&mut self, error: &JsError) -> Result<(), JsError> {
        let callback = self
            .state
            .listeners
            .iter()
            .find(|listener| {
                listener.target == WINDOW && listener.kind == "error" && listener.property
            })
            .map(|listener| listener.callback.clone());
        let Some(callback) = callback else {
            return Ok(());
        };
        // Filename/line are unknown in this host contract. Do not fabricate a
        // stack or evaluate source text to obtain an Error object.
        let args = vec![
            Value::string(error.message.chars().take(1024).collect::<String>()),
            Value::string(""),
            Value::Number(0.),
            Value::Number(0.),
            error.thrown.clone().unwrap_or(Value::Null),
        ];
        match self.runtime.invoke_with_this(
            callback,
            args,
            Value::Host(WINDOW),
            &mut DomHost {
                document: &mut self.document,
                state: &mut self.state,
            },
        ) {
            Ok(_) => Ok(()),
            Err(error) if error.kind == ErrorKind::Resource => Err(error),
            Err(error) => {
                // An error handler cannot recursively report its own failure.
                if self.state.logs.len() < 32 {
                    self.state.logs.push(
                        format!("window.onerror: {error}")
                            .chars()
                            .take(1024)
                            .collect(),
                    );
                }
                Ok(())
            }
        }
    }
    fn disable_scripts(&mut self) {
        self.scripts_disabled = true;
        self.state.timers.clear();
        self.state.animation.pending.clear();
        self.state.listeners.clear();
    }
    /// Applies a native editing event as data, never as evaluated JavaScript.
    /// Only connected input/textarea/select nodes are accepted.
    pub fn update_control_value(&mut self, node: NodeId, value: &str) -> Result<(), JsError> {
        if value.len() > 32 * 1024 || !connected(&self.document, 0).contains(&node) {
            return Err(JsError::runtime(
                "Invalid or oversized native control value",
            ));
        }
        let element = self
            .document
            .element(node)
            .ok_or_else(|| JsError::runtime("Control target is not an element"))?;
        if !matches!(element.tag.as_str(), "input" | "textarea" | "select") {
            return Err(JsError::runtime(
                "Only input/textarea/select values may be updated",
            ));
        }
        if element.attributes.contains_key("disabled") {
            return Err(JsError::runtime(
                "Disabled controls cannot receive native edits",
            ));
        }
        let checkbox = element.tag == "input"
            && element
                .attr("type")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("checkbox"));
        let textarea = element.tag == "textarea";
        let select = element.tag == "select";
        let mut host = DomHost {
            document: &mut self.document,
            state: &mut self.state,
        };
        if select {
            host.set_select_value(node, value, true)?;
        } else if checkbox {
            host.set(
                NODE + node as u64,
                "checked",
                Value::Bool(!value.is_empty()),
            )?;
            if !value.is_empty() {
                host.set_attribute(node, "value".into(), value.into())?;
            }
        } else {
            host.set_attribute(node, "value".into(), value.into())?;
            if textarea {
                host.replace_text(node, value.into())?;
            }
        }
        Ok(())
    }
    fn require_module_origin(&self, url: &url::Url, base: &url::Url) -> Result<(), JsError> {
        let allowed = if matches!(base.scheme(), "http" | "https") {
            matches!(url.scheme(), "http" | "https") && url.origin() == base.origin()
        } else {
            base.scheme() == "file" && url.scheme() == "file"
        };
        if !allowed {
            return Err(JsError::runtime(
                "Cross-origin modules require CORS support and are not enabled",
            ));
        }
        Ok(())
    }
    fn module_asset<'a>(
        &self,
        url: &url::Url,
        sources: &'a [ScriptAsset],
        _base: &url::Url,
    ) -> Option<&'a str> {
        // Only the broker's explicitly authorized module assets may enter the
        // module map. A classic script can have the same URL but weaker policy.
        let key = format!("noir-module:{}", url.as_str());
        sources
            .iter()
            .find(|asset| asset.src == key)
            .map(|asset| asset.source.as_str())
    }
    fn register_module_graph(
        &mut self,
        root: &str,
        source_url: &str,
        source: String,
        sources: &[ScriptAsset],
        base: &url::Url,
        bytes: &mut usize,
    ) -> Result<(), JsError> {
        let mut pending =
            std::collections::VecDeque::from([(root.to_owned(), source_url.to_owned(), source)]);
        let mut seen = std::collections::BTreeSet::new();
        while let Some((id, source_url, source)) = pending.pop_front() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if seen.len() > 64 {
                return Err(JsError::resource("Module graph exceeds 64 records"));
            }
            let requests = web_javascript::module_requests(&source)
                .map_err(|error| diagnostics::module_error(error, &source_url, Some(&source)))?;
            self.runtime
                .register_module_with_source_url(&id, &source, &source_url)
                .map_err(|error| diagnostics::module_error(error, &source_url, None))?;
            for request in requests {
                let resolved = web_javascript::resolve_module_specifier(&source_url, &request)?;
                if seen.contains(&resolved) || pending.iter().any(|(id, _, _)| id == &resolved) {
                    continue;
                }
                let url = url::Url::parse(&resolved)
                    .map_err(|_| JsError::runtime("Invalid module dependency URL"))?;
                self.require_module_origin(&url, base)?;
                let source = self.module_asset(&url, sources, base).ok_or_else(|| {
                    JsError::runtime(format!(
                        "Module dependency was not supplied by broker: {}",
                        diagnostics::source_label(&resolved, None)
                    ))
                })?;
                if pending.len() >= 64 {
                    return Err(JsError::resource(
                        "Module dependency queue exceeds 64 records",
                    ));
                }
                lifecycle::charge_source(bytes, source.len())?;
                pending.push_back((resolved.clone(), resolved, source.to_owned()));
            }
        }
        Ok(())
    }
    fn execute_registered_module(&mut self, id: &str) -> Result<Value, JsError> {
        self.document_url_frozen = true;
        self.media_started = true;
        if self.scripts_disabled {
            return Err(JsError::resource(
                "JavaScript disabled after resource exhaustion",
            ));
        }
        let old_document = self.document.clone();
        let old_runtime = self.runtime.clone();
        let old_state = self.state.clone();
        let mut result = self.runtime.execute_module(
            id,
            &mut DomHost {
                document: &mut self.document,
                state: &mut self.state,
            },
        );
        if let Err(error) = &result
            && error.kind != ErrorKind::Resource
            && let Err(handler_error) = self.notify_window_error(error)
        {
            result = Err(handler_error);
        }
        if result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
        {
            self.document = old_document;
            self.state = old_state;
            self.runtime.rollback_to(old_runtime);
            self.disable_scripts();
        }
        result
    }
    pub fn dispatch_event(&mut self, node: NodeId, kind: &str) -> Result<(), JsError> {
        self.document_url_frozen = true;
        self.media_started = true;
        if node >= self.document.nodes.len() || !connected(&self.document, 0).contains(&node) {
            return Err(JsError::runtime("Event target is not connected"));
        }
        let target = DomHost::node_value(node).as_host().expect("node host");
        let mut targets = vec![target];
        let mut parent = self.document.nodes[node].parent;
        let mut depth = 0;
        while let Some(id) = parent {
            if depth >= MAX_DEPTH {
                return Err(JsError::resource("Event path depth exceeded"));
            }
            let next = DomHost::node_value(id).as_host().expect("node host");
            if targets.last() != Some(&next) {
                targets.push(next);
            }
            parent = self.document.nodes[id].parent;
            depth += 1;
        }
        if targets.last() != Some(&DOC) {
            targets.push(DOC);
        }
        targets.push(WINDOW);
        self.dispatch_path(targets, kind)
    }
    fn dispatch_target(&mut self, target: u64, kind: &str) -> Result<(), JsError> {
        self.dispatch_path(
            if target == DOC {
                vec![DOC, WINDOW]
            } else {
                vec![target]
            },
            kind,
        )
    }
    fn dispatch_path(&mut self, path: Vec<u64>, kind: &str) -> Result<(), JsError> {
        self.document_url_frozen = true;
        self.media_started = true;
        if self.scripts_disabled {
            return Ok(());
        }
        if kind.len() > 64 || self.state.events.len() >= MAX_EVENTS {
            self.disable_scripts();
            return Err(JsError::resource("DOM event count/type budget exceeded"));
        }
        let event = self.state.events.len();
        self.state.events.push(DomEvent {
            kind: kind.into(),
            target: Some(path[0]),
            detail: Value::Null,
            custom: false,
            trusted: true,
            dispatching: false,
            current: None,
            phase: 0,
            bubbles: !matches!(
                kind,
                "load" | "error" | "focus" | "blur" | "readystatechange"
            ),
            cancelable: matches!(
                kind,
                "click"
                    | "submit"
                    | "keydown"
                    | "keyup"
                    | "mousedown"
                    | "mouseup"
                    | "wheel"
                    | "touchstart"
                    | "touchmove"
            ),
            canceled: false,
            stopped: false,
            immediate: false,
            passive: false,
            timestamp: self.state.clock,
        });
        let result = DomHost {
            document: &mut self.document,
            state: &mut self.state,
        }
        .dispatch_existing(&mut self.runtime, event, path, true);
        if result
            .as_ref()
            .is_err_and(|error| error.kind == ErrorKind::Resource)
        {
            self.disable_scripts();
        }
        result
    }
    /// The caller supplies elapsed time; this API creates no host timers/threads.
    pub fn run_due_timers(&mut self, elapsed_ms: u64) -> Result<(), JsError> {
        self.document_url_frozen = true;
        self.media_started = true;
        if self.scripts_disabled {
            return Ok(());
        }
        self.state.clock = self.state.clock.saturating_add(elapsed_ms);
        // One bounded task batch prevents recursively scheduled timers from
        // starving layout. Newly registered tasks wait for the next dispatch.
        let ready = self
            .state
            .timers
            .iter()
            .filter(|t| t.due <= self.state.clock)
            .take(32)
            .cloned()
            .collect::<Vec<_>>();
        let mut first_error = None;
        for timer in ready {
            if !self
                .state
                .timers
                .iter()
                .any(|pending| pending.id == timer.id)
            {
                continue;
            }
            self.state.timers.retain(|t| t.id != timer.id);
            let old_document = self.document.clone();
            let old_state = self.state.clone();
            let old_runtime = self.runtime.clone();
            let mut result = self.runtime.invoke_with_this(
                timer.callback,
                timer.args,
                Value::Host(WINDOW),
                &mut DomHost {
                    document: &mut self.document,
                    state: &mut self.state,
                },
            );
            if let Err(error) = &result
                && error.kind != ErrorKind::Resource
                && let Err(handler_error) = self.notify_window_error(error)
            {
                result = Err(handler_error);
            }
            if let Err(error) = result {
                if error.kind == ErrorKind::Resource {
                    self.document = old_document;
                    self.state = old_state;
                    self.runtime.rollback_to(old_runtime);
                    self.disable_scripts();
                    return Err(error);
                }
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
    pub fn next_timer_ms(&self) -> Option<u64> {
        self.state
            .timers
            .iter()
            .map(|timer| timer.due.saturating_sub(self.state.clock))
            .min()
    }
    pub fn query_selector(&self, selector: &str) -> Result<Option<NodeId>, JsError> {
        query(&self.document, 0, selector, &mut 0, true).map(|nodes| nodes.first().copied())
    }
    pub fn console_messages(&self) -> &[String] {
        &self.state.logs
    }
}

pub fn execute_scripts(document: &mut Document, sources: &[ScriptAsset]) -> ScriptReport {
    let mut session = DocumentSession::new(std::mem::take(document));
    let report = session.run_initial(sources);
    // The snapshot renderer no longer retains VM node handles. Compact away
    // detached nodes so removed style/script elements cannot affect CSS cascade.
    *document = compact(&session.document);
    report
}

struct DomHost<'a> {
    document: &'a mut Document,
    state: &'a mut State,
}
impl DomHost<'_> {
    fn metadata_read(&mut self, property: &str) -> Result<(), JsError> {
        if property.len() > 1024 {
            return Err(JsError::resource(
                "Browser metadata property name exceeds 1024 bytes",
            ));
        }
        self.charge_tree_read(property.len().saturating_add(1))
    }
    fn navigator_property(&mut self, property: &str) -> Result<Value, JsError> {
        self.metadata_read(property)?;
        if let Some(index) = NAVIGATOR_METHODS.iter().position(|name| *name == property) {
            return Ok(Value::Host(NAVIGATOR_METHOD + index as u64));
        }
        Ok(match property {
            "userAgent" => Value::string(concat!(
                "Noir/",
                env!("CARGO_PKG_VERSION"),
                " (Independent Web Engine)"
            )),
            "appVersion" => Value::string(concat!(
                env!("CARGO_PKG_VERSION"),
                " (Independent Web Engine)"
            )),
            "appName" | "appCodeName" | "product" | "vendor" => Value::string("Noir"),
            "language" => Value::string("pt-BR"),
            "platform" => Value::string(navigator_platform()),
            "hardwareConcurrency" => self
                .state
                .hardware_concurrency
                .map_or(Value::Undefined, |count| Value::Number(count as f64)),
            "cookieEnabled" => Value::Bool(false),
            "plugins" => Value::Host(PLUGINS),
            "mimeTypes" => Value::Host(MIME_TYPES),
            _ => Value::Undefined,
        })
    }
    fn location_property(&mut self, property: &str) -> Result<Value, JsError> {
        self.metadata_read(property)?;
        if property == "toString" {
            return Ok(Value::Host(LOCATION_STRING));
        }
        if !LOCATION_PROPERTIES.contains(&property) {
            return Ok(Value::Undefined);
        }
        let address = self
            .state
            .document_url
            .as_ref()
            .ok_or_else(|| JsError::runtime("Document URL metadata is unavailable"))?;
        let value = match property {
            "href" => address.as_str().to_owned(),
            "origin" => address.origin().ascii_serialization(),
            "protocol" => format!("{}:", address.scheme()),
            "hostname" => address.host_str().unwrap_or("").to_owned(),
            "host" => {
                let hostname = match address.host() {
                    Some(url::Host::Ipv6(host)) => format!("[{host}]"),
                    Some(host) => host.to_string(),
                    None => String::new(),
                };
                address
                    .port()
                    .map_or(hostname.clone(), |port| format!("{hostname}:{port}"))
            }
            "port" => address
                .port()
                .map_or_else(String::new, |port| port.to_string()),
            "pathname" => address.path().to_owned(),
            "search" => address
                .query()
                .filter(|query| !query.is_empty())
                .map_or_else(String::new, |query| format!("?{query}")),
            "hash" => address
                .fragment()
                .filter(|fragment| !fragment.is_empty())
                .map_or_else(String::new, |fragment| format!("#{fragment}")),
            _ => return Ok(Value::Undefined),
        };
        self.charge_tree_read(value.len())?;
        Ok(Value::string(value))
    }
    fn navigator_call(&mut self, method: &str, args: &[Value]) -> Result<Value, JsError> {
        self.metadata_read(method)?;
        match method {
            "javaEnabled" => Ok(Value::Bool(false)),
            "sendBeacon" => {
                let address = args
                    .first()
                    .ok_or_else(|| JsError::runtime("sendBeacon requires a URL"))?
                    .primitive_text()?;
                if address.len() > 8192 {
                    return Err(JsError::resource("Beacon URL exceeds 8192 bytes"));
                }
                self.charge_tree_read(address.len())?;
                let parsed = match &self.state.document_url {
                    Some(base) => base.join(&address),
                    None => url::Url::parse(&address),
                }
                .map_err(|_| JsError::runtime("Invalid beacon URL"))?;
                if parsed.as_str().len() > 8192 {
                    return Err(JsError::resource("Resolved beacon URL exceeds 8192 bytes"));
                }
                if !matches!(parsed.scheme(), "http" | "https") {
                    return Err(JsError::runtime("Beacon URL must use HTTP or HTTPS"));
                }
                if let Some(body) = args.get(1) {
                    match body {
                        Value::String(body) => {
                            self.charge_tree_read(body.len())?;
                            if body.len() > 65_536 {
                                return Ok(Value::Bool(false));
                            }
                        }
                        Value::Null | Value::Undefined | Value::Bool(_) | Value::Number(_) => {}
                        _ => return Err(JsError::runtime("Beacon body type is not implemented")),
                    }
                }
                // No request was queued. False is the actual result, never a
                // simulation of successful delivery or a direct network call.
                Ok(Value::Bool(false))
            }
            _ => Err(JsError::runtime(format!(
                "Unsupported Navigator method {method}"
            ))),
        }
    }
    fn select_options(&mut self, id: NodeId) -> Result<Vec<NodeId>, JsError> {
        let mut options = Vec::new();
        let mut stack = self.document.nodes[id]
            .children
            .iter()
            .rev()
            .copied()
            .collect::<Vec<_>>();
        while let Some(node) = stack.pop() {
            self.state.read_work = self.state.read_work.saturating_add(1);
            if self.state.read_work > 2_000_000 {
                return Err(JsError::resource("DOM read/traversal work budget exceeded"));
            }
            if let Some(element) = self.document.element(node) {
                if element.tag == "select" {
                    continue;
                }
                if element.tag == "option" {
                    if options.len() >= 256 {
                        return Err(JsError::resource("Select option count exceeds 256"));
                    }
                    options.push(node);
                    continue;
                }
            }
            stack.extend(self.document.nodes[node].children.iter().rev().copied());
        }
        Ok(options)
    }
    fn option_value(&mut self, id: NodeId) -> Result<String, JsError> {
        if let Some(value) = self
            .document
            .element(id)
            .and_then(|element| element.attr("value"))
        {
            if value.len() > 1024 {
                return Err(JsError::resource("Option value exceeds 1024 bytes"));
            }
            return Ok(value.into());
        }
        let mut value = String::new();
        let mut space = false;
        let mut stack = vec![id];
        while let Some(node) = stack.pop() {
            self.state.read_work = self.state.read_work.saturating_add(1);
            if self.state.read_work > 2_000_000 {
                return Err(JsError::resource("DOM read/traversal work budget exceeded"));
            }
            if let NodeKind::Text(text) = &self.document.nodes[node].kind {
                for character in text.chars() {
                    self.state.read_work = self.state.read_work.saturating_add(1);
                    if self.state.read_work > 2_000_000 {
                        return Err(JsError::resource("DOM option text work budget exceeded"));
                    }
                    if matches!(character, '\t' | '\n' | '\r' | '\u{c}' | ' ') {
                        space = !value.is_empty();
                    } else {
                        let additional = usize::from(space) + character.len_utf8();
                        if value.len() + additional > 1024 {
                            return Err(JsError::resource("Option text value exceeds 1024 bytes"));
                        }
                        if space {
                            value.push(' ');
                            space = false;
                        }
                        value.push(character);
                    }
                }
            }
            stack.extend(self.document.nodes[node].children.iter().rev().copied());
        }
        Ok(value)
    }
    fn option_disabled(&self, id: NodeId, select: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(node) = current {
            if node == select {
                break;
            }
            if self.document.element(node).is_some_and(|element| {
                element.attributes.contains_key("disabled")
                    && matches!(element.tag.as_str(), "option" | "optgroup")
            }) {
                return true;
            }
            current = self.document.nodes[node].parent;
        }
        false
    }
    fn select_value(&mut self, id: NodeId) -> Result<String, JsError> {
        let options = self.select_options(id)?;
        let element = self
            .document
            .element(id)
            .ok_or_else(|| JsError::runtime("Select target must be an element"))?;
        if element.attributes.contains_key("data-noir-select-empty") {
            return Ok(String::new());
        }
        let multiple = element.attributes.contains_key("multiple");
        let selected = if multiple {
            options.iter().find(|&&option| {
                self.document
                    .element(option)
                    .is_some_and(|e| e.attributes.contains_key("selected"))
            })
        } else {
            options.iter().rev().find(|&&option| {
                self.document
                    .element(option)
                    .is_some_and(|e| e.attributes.contains_key("selected"))
            })
        };
        let selected = selected.copied().or_else(|| {
            (!multiple
                && element
                    .attr("size")
                    .and_then(|size| size.parse::<usize>().ok())
                    .unwrap_or(1)
                    <= 1)
                .then(|| {
                    options
                        .iter()
                        .find(|&&option| !self.option_disabled(option, id))
                        .copied()
                })
                .flatten()
        });
        selected.map_or(Ok(String::new()), |option| self.option_value(option))
    }
    fn set_select_value(&mut self, id: NodeId, value: &str, native: bool) -> Result<(), JsError> {
        if value.len() > 1024 {
            return Err(JsError::resource("Select value exceeds 1024 bytes"));
        }
        let options = self.select_options(id)?;
        let mut chosen = None;
        for &option in &options {
            if (!native || !self.option_disabled(option, id)) && self.option_value(option)? == value
            {
                chosen = Some(option);
                break;
            }
        }
        if native && chosen.is_none() {
            return Err(JsError::runtime(
                "Native select value is not an enabled option",
            ));
        }
        let changes = options
            .into_iter()
            .filter(|&option| {
                let selected = self
                    .document
                    .element(option)
                    .is_some_and(|element| element.attributes.contains_key("selected"));
                selected != (chosen == Some(option))
            })
            .collect::<Vec<_>>();
        // Reserve the entire mutation budget before changing any option. Native
        // control synchronization must not leave a partially updated select.
        for _ in &changes {
            self.mutate(0)?;
        }
        self.mutate(0)?;
        for option in changes {
            if let NodeKind::Element(element) = &mut self.document.nodes[option].kind {
                if chosen == Some(option) {
                    element.attributes.insert("selected".into(), String::new());
                } else {
                    element.attributes.remove("selected");
                }
            }
        }
        if let NodeKind::Element(element) = &mut self.document.nodes[id].kind {
            if chosen.is_none() {
                element
                    .attributes
                    .insert("data-noir-select-empty".into(), String::new());
            } else {
                element.attributes.remove("data-noir-select-empty");
            }
        }
        Ok(())
    }
    fn read_cost(&mut self) -> Result<(), JsError> {
        self.state.read_work = self
            .state
            .read_work
            .saturating_add(self.document.nodes.len());
        if self.state.read_work > 2_000_000 {
            return Err(JsError::resource("DOM read/traversal work budget exceeded"));
        }
        Ok(())
    }
    fn mutate(&mut self, bytes: usize) -> Result<(), JsError> {
        self.state.mutations += 1;
        self.state.string_bytes = self.state.string_bytes.saturating_add(bytes);
        if self.state.mutations > 2048 || self.state.string_bytes > 256 * 1024 {
            return Err(JsError::resource("DOM mutation budget exceeded"));
        }
        Ok(())
    }
    fn node(&self, object: u64) -> Result<NodeId, JsError> {
        let node = object
            .checked_sub(NODE)
            .and_then(|id| usize::try_from(id).ok())
            .filter(|&id| id < self.document.nodes.len())
            .ok_or_else(|| JsError::runtime("Invalid DOM node handle"))?;
        Ok(node)
    }
    fn list(&mut self, nodes: Vec<NodeId>) -> Result<Value, JsError> {
        if nodes.len() > 4096 || self.state.lists.len() >= 64 {
            return Err(JsError::resource("DOM node list budget exceeded"));
        }
        let id = self.state.lists.len();
        self.state.lists.push(nodes);
        Ok(Value::Host(LIST + id as u64))
    }
    fn create(&mut self, kind: NodeKind) -> Result<NodeId, JsError> {
        if self.document.nodes.len() >= MAX_NODES {
            return Err(JsError::resource("DOM node limit exceeded"));
        }
        let id = self.document.nodes.len();
        self.document.nodes.push(Node {
            kind,
            parent: None,
            children: vec![],
        });
        Ok(id)
    }
    fn detach(&mut self, id: NodeId) {
        if let Some(parent) = self.document.nodes[id].parent.take() {
            self.document.nodes[parent]
                .children
                .retain(|&child| child != id);
        }
    }
    fn charge_tree_read(&mut self, work: usize) -> Result<(), JsError> {
        self.state.read_work = self.state.read_work.saturating_add(work);
        if self.state.read_work > 2_000_000 {
            return Err(JsError::resource("DOM tree traversal budget exceeded"));
        }
        Ok(())
    }
    fn node_value(id: NodeId) -> Value {
        if id == 0 {
            Value::Host(DOC)
        } else {
            Value::Host(NODE + id as u64)
        }
    }
    fn node_argument(&self, value: Option<&Value>) -> Result<NodeId, JsError> {
        match value.and_then(Value::as_host) {
            Some(DOC) => Ok(0),
            Some(handle) => self.node(handle),
            None => Err(JsError::runtime("Tree operation requires a Node")),
        }
    }
    fn supports_node_method(&self, id: NodeId, method: &str) -> bool {
        match method {
            "appendChild"
            | "insertBefore"
            | "removeChild"
            | "replaceChild"
            | "hasChildNodes"
            | "contains"
            | "addEventListener"
            | "removeEventListener"
            | "dispatchEvent" => true,
            "cloneNode" | "remove" => !matches!(self.document.nodes[id].kind, NodeKind::Document),
            "getAttribute" | "setAttribute" | "hasAttribute" | "removeAttribute" | "matches"
            | "closest" | "querySelector" | "querySelectorAll" => {
                self.document.element(id).is_some()
            }
            _ => false,
        }
    }
    fn validate_insertion(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: Option<NodeId>,
        strict_document: bool,
    ) -> Result<(), JsError> {
        if !matches!(
            self.document.nodes[parent].kind,
            NodeKind::Document | NodeKind::Element(_)
        ) {
            return Err(JsError::runtime("A text node cannot own children"));
        }
        if matches!(self.document.nodes[child].kind, NodeKind::Document) {
            return Err(JsError::runtime("A Document cannot be inserted as a child"));
        }
        if let Some(reference) = reference
            && (self.document.nodes[reference].parent != Some(parent)
                || !self.document.nodes[parent].children.contains(&reference))
        {
            return Err(JsError::runtime(
                "Reference node is not a child of this parent",
            ));
        }
        let mut cursor = Some(parent);
        let mut depth = 0;
        while let Some(id) = cursor {
            self.charge_tree_read(1)?;
            if id == child {
                return Err(JsError::runtime("DOM insertion would create a cycle"));
            }
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(JsError::resource("DOM depth limit exceeded"));
            }
            cursor = self.document.nodes[id].parent;
        }
        let mut pending = vec![(child, depth)];
        while let Some((id, level)) = pending.pop() {
            self.charge_tree_read(self.document.nodes[id].children.len().saturating_add(1))?;
            if level >= MAX_DEPTH {
                return Err(JsError::resource("DOM depth limit exceeded"));
            }
            pending.extend(
                self.document.nodes[id]
                    .children
                    .iter()
                    .map(|&node| (node, level + 1)),
            );
        }
        if strict_document && parent == 0 {
            if matches!(self.document.nodes[child].kind, NodeKind::Text(_)) {
                return Err(JsError::runtime(
                    "Text nodes cannot be inserted directly into a Document",
                ));
            }
            self.charge_tree_read(self.document.nodes[parent].children.len())?;
            let roots = self.document.nodes[parent]
                .children
                .iter()
                .filter(|&&node| {
                    node != child
                        && Some(node) != reference
                        && self.document.element(node).is_some()
                })
                .count();
            // A reference is not removed by insertBefore. replaceChild checks
            // document hierarchy separately using its replaced node exemption.
            let referenced_root = reference
                .is_some_and(|node| node != child && self.document.element(node).is_some());
            if roots + usize::from(referenced_root) > 0 {
                return Err(JsError::runtime("Document already has a document element"));
            }
        }
        Ok(())
    }
    fn commit_insertion(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        if reference == Some(child) {
            return;
        }
        self.detach(child);
        let index = reference
            .and_then(|reference| {
                self.document.nodes[parent]
                    .children
                    .iter()
                    .position(|&node| node == reference)
            })
            .unwrap_or(self.document.nodes[parent].children.len());
        self.document.nodes[parent].children.insert(index, child);
        self.document.nodes[child].parent = Some(parent);
    }
    fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: Option<NodeId>,
    ) -> Result<(), JsError> {
        if reference == Some(child) {
            if self.document.nodes[child].parent != Some(parent) {
                return Err(JsError::runtime(
                    "Reference node is not a child of this parent",
                ));
            }
            return Ok(());
        }
        self.validate_insertion(parent, child, reference, true)?;
        self.mutate(0)?;
        self.commit_insertion(parent, child, reference);
        Ok(())
    }
    fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), JsError> {
        if self.document.nodes[child].parent != Some(parent)
            || !self.document.nodes[parent].children.contains(&child)
        {
            return Err(JsError::runtime("Node is not a child of this parent"));
        }
        self.mutate(0)?;
        self.detach(child);
        Ok(())
    }
    fn replace_child(&mut self, parent: NodeId, child: NodeId, old: NodeId) -> Result<(), JsError> {
        if self.document.nodes[old].parent != Some(parent)
            || !self.document.nodes[parent].children.contains(&old)
        {
            return Err(JsError::runtime(
                "Replacement target is not a child of this parent",
            ));
        }
        if child == old {
            return Ok(());
        }
        self.validate_insertion(parent, child, Some(old), false)?;
        if parent == 0 {
            self.charge_tree_read(self.document.nodes[parent].children.len())?;
            if matches!(self.document.nodes[child].kind, NodeKind::Text(_))
                || self.document.nodes[parent].children.iter().any(|&node| {
                    node != old && node != child && self.document.element(node).is_some()
                })
            {
                return Err(JsError::runtime("Replacement violates Document hierarchy"));
            }
        }
        // Reserve both changes before altering links or sibling order.
        self.mutate(0)?;
        self.mutate(0)?;
        self.commit_insertion(parent, child, Some(old));
        self.detach(old);
        Ok(())
    }
    fn append(&mut self, parent: NodeId, child: NodeId) -> Result<(), JsError> {
        self.validate_insertion(parent, child, None, false)?;
        self.commit_insertion(parent, child, None);
        Ok(())
    }
    fn clone_node(&mut self, root: NodeId, deep: bool) -> Result<NodeId, JsError> {
        if matches!(self.document.nodes[root].kind, NodeKind::Document) {
            return Err(JsError::runtime("Cloning Document is not implemented"));
        }
        if self.document.nodes.len() >= MAX_NODES {
            return Err(JsError::resource("DOM node limit exceeded while cloning"));
        }
        let mut plan = vec![(root, None)];
        let mut index = 0;
        while index < plan.len() {
            let id = plan[index].0;
            self.charge_tree_read(1)?;
            if deep {
                let child_count = self.document.nodes[id].children.len();
                self.charge_tree_read(child_count)?;
                if self
                    .document
                    .nodes
                    .len()
                    .saturating_add(plan.len())
                    .saturating_add(child_count)
                    > MAX_NODES
                {
                    return Err(JsError::resource("DOM node limit exceeded while cloning"));
                }
                plan.extend(
                    self.document.nodes[id]
                        .children
                        .iter()
                        .map(|&child| (child, Some(index))),
                );
            }
            if self.document.nodes.len().saturating_add(plan.len()) > MAX_NODES {
                return Err(JsError::resource("DOM node limit exceeded while cloning"));
            }
            index += 1;
        }
        for &(id, _) in &plan {
            let bytes = match &self.document.nodes[id].kind {
                NodeKind::Text(text) => text.len(),
                NodeKind::Element(element) => element
                    .attributes
                    .iter()
                    .fold(element.tag.len(), |sum, (key, value)| {
                        sum.saturating_add(key.len()).saturating_add(value.len())
                    }),
                NodeKind::Document => 0,
            };
            self.charge_tree_read(bytes)?;
            self.mutate(bytes)?;
        }
        let mut clones = Vec::with_capacity(plan.len());
        for (id, parent) in plan {
            let cloned = self.create(self.document.nodes[id].kind.clone())?;
            if let Some(parent) = parent {
                self.append(clones[parent], cloned)?;
            }
            clones.push(cloned);
        }
        Ok(clones[0])
    }
    /// Import a temporary parser arena without changing links until every
    /// count, final depth, traversal, mutation and string budget check succeeds.
    /// IDs in the fragment never escape: all links are remapped to this arena.
    fn import_fragment(
        &mut self,
        target: NodeId,
        fragment: &Document,
        replace: bool,
    ) -> Result<(), JsError> {
        let count = fragment.nodes.len().saturating_sub(1);
        if self.document.nodes.len().saturating_add(count) > MAX_NODES {
            return Err(JsError::resource(
                "DOM node limit exceeded importing fragment",
            ));
        }
        let mut depth = 0;
        let mut cursor = Some(target);
        while let Some(node) = cursor {
            self.charge_tree_read(1)?;
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(JsError::resource("DOM depth limit exceeded"));
            }
            cursor = self.document.nodes[node].parent;
        }
        self.charge_tree_read(fragment.nodes[0].children.len())?;
        let mut plan = fragment.nodes[0]
            .children
            .iter()
            .map(|&node| (node, depth))
            .collect::<Vec<_>>();
        let mut index = 0;
        while index < plan.len() {
            let (source, level) = plan[index];
            let node = &fragment.nodes[source];
            self.charge_tree_read(node.children.len().saturating_add(1))?;
            if level >= MAX_DEPTH {
                return Err(JsError::resource(
                    "DOM depth limit exceeded importing fragment",
                ));
            }
            let bytes = match &node.kind {
                NodeKind::Text(text) => text.len(),
                NodeKind::Element(element) => element
                    .attributes
                    .iter()
                    .fold(element.tag.len(), |sum, (key, value)| {
                        sum.saturating_add(key.len()).saturating_add(value.len())
                    }),
                NodeKind::Document => return Err(JsError::runtime("Nested Document in fragment")),
            };
            self.charge_tree_read(bytes)?;
            self.mutate(bytes)?;
            plan.extend(node.children.iter().map(|&child| (child, level + 1)));
            index += 1;
        }
        self.charge_tree_read(self.document.nodes[target].children.len())?;
        self.mutate(0)?;
        // No fallible operation follows. Stage allocations and remapped links
        // before detaching old children. The document mode stays intact.
        let base = self.document.nodes.len();
        let mut remap = vec![target; fragment.nodes.len()];
        for (index, &(source, _)) in plan.iter().enumerate() {
            remap[source] = base + index;
        }
        let roots = fragment.nodes[0]
            .children
            .iter()
            .map(|&child| remap[child])
            .collect::<Vec<_>>();
        let staged = plan
            .into_iter()
            .map(|(source, _)| {
                let node = &fragment.nodes[source];
                Node {
                    kind: node.kind.clone(),
                    parent: Some(remap[node.parent.unwrap_or(0)]),
                    children: node.children.iter().map(|&child| remap[child]).collect(),
                }
            })
            .collect::<Vec<_>>();
        self.document.nodes.extend(staged);
        if replace {
            let old = std::mem::take(&mut self.document.nodes[target].children);
            for child in old {
                self.document.nodes[child].parent = None;
            }
        }
        self.document.nodes[target].children.extend(roots);
        Ok(())
    }
    fn related_node(&mut self, id: NodeId, property: &str) -> Result<Value, JsError> {
        let elements = property.contains("Element");
        let result = if property.starts_with("first") || property.starts_with("last") {
            self.charge_tree_read(self.document.nodes[id].children.len())?;
            let children = &self.document.nodes[id].children;
            if property.starts_with("last") {
                children
                    .iter()
                    .rev()
                    .copied()
                    .find(|&child| !elements || self.document.element(child).is_some())
            } else {
                children
                    .iter()
                    .copied()
                    .find(|&child| !elements || self.document.element(child).is_some())
            }
        } else if let Some(parent) = self.document.nodes[id].parent {
            self.charge_tree_read(self.document.nodes[parent].children.len())?;
            let siblings = &self.document.nodes[parent].children;
            let position = siblings
                .iter()
                .position(|&child| child == id)
                .ok_or_else(|| JsError::runtime("DOM parent/child links are inconsistent"))?;
            if property.starts_with("previous") {
                siblings[..position]
                    .iter()
                    .rev()
                    .copied()
                    .find(|&child| !elements || self.document.element(child).is_some())
            } else {
                siblings[position + 1..]
                    .iter()
                    .copied()
                    .find(|&child| !elements || self.document.element(child).is_some())
            }
        } else {
            None
        };
        Ok(result.map_or(Value::Null, Self::node_value))
    }
    fn connected_node(&mut self, id: NodeId) -> Result<bool, JsError> {
        let mut current = Some(id);
        for _ in 0..=MAX_DEPTH {
            self.charge_tree_read(1)?;
            let Some(node) = current else {
                return Ok(false);
            };
            if node == 0 {
                return Ok(true);
            }
            current = self.document.nodes[node].parent;
        }
        Err(JsError::resource("DOM ancestor traversal depth exceeded"))
    }
    fn replace_text(&mut self, id: NodeId, text: String) -> Result<(), JsError> {
        if matches!(self.document.nodes[id].kind, NodeKind::Text(_)) {
            self.mutate(text.len())?;
            let NodeKind::Text(current) = &mut self.document.nodes[id].kind else {
                unreachable!()
            };
            *current = text;
            return Ok(());
        }
        self.charge_tree_read(self.document.nodes[id].children.len())?;
        if !text.is_empty() {
            if self.document.nodes.len() >= MAX_NODES {
                return Err(JsError::resource("DOM node limit exceeded"));
            }
            let mut current = Some(id);
            let mut depth = 0;
            while let Some(node) = current {
                self.charge_tree_read(1)?;
                depth += 1;
                if depth >= MAX_DEPTH {
                    return Err(JsError::resource("DOM depth limit exceeded"));
                }
                current = self.document.nodes[node].parent;
            }
        }
        self.mutate(text.len())?;
        // All fallible budget/shape checks precede changing the old children.
        let child = if text.is_empty() {
            None
        } else {
            Some(self.create(NodeKind::Text(text))?)
        };
        let old = std::mem::take(&mut self.document.nodes[id].children);
        for child in old {
            self.document.nodes[child].parent = None;
        }
        if let Some(child) = child {
            self.document.nodes[child].parent = Some(id);
            self.document.nodes[id].children.push(child);
        }
        Ok(())
    }
    fn set_attribute(&mut self, id: NodeId, key: String, value: String) -> Result<(), JsError> {
        if key.len() > 128
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
        {
            return Err(JsError::runtime("Unsupported attribute name"));
        }
        self.mutate(key.len() + value.len())?;
        let NodeKind::Element(element) = &mut self.document.nodes[id].kind else {
            return Err(JsError::runtime("Attributes require an element"));
        };
        if element.attributes.len() >= 128 && !element.attributes.contains_key(&key) {
            return Err(JsError::resource("Element attribute count exceeded"));
        }
        if key.eq_ignore_ascii_case("src") {
            self.state.media.nodes.remove(&id);
        }
        element.attributes.insert(key, value);
        Ok(())
    }
    fn listener(
        &mut self,
        target: u64,
        kind: String,
        callback: Value,
        options: ListenerOptions,
    ) -> Result<(), JsError> {
        if matches!(callback, Value::Null | Value::Undefined) {
            return Ok(());
        }
        if !matches!(callback, Value::Function(_) | Value::Native(_))
            && !callback.as_host().is_some_and(|id| self.is_callable(id))
        {
            return Err(JsError::runtime(
                "Event listener must be a JavaScript function",
            ));
        }
        if self.state.listeners.iter().any(|listener| {
            !listener.property
                && listener.target == target
                && listener.kind == kind
                && listener.callback == callback
                && listener.options.capture == options.capture
        }) {
            return Ok(());
        }
        if self.state.listeners.len() >= 128 || kind.len() > 64 {
            return Err(JsError::resource("DOM listener budget exceeded"));
        }
        self.state.next_listener = self
            .state
            .next_listener
            .checked_add(1)
            .ok_or_else(|| JsError::resource("DOM listener identity exhausted"))?;
        self.state.listeners.push(Listener {
            id: self.state.next_listener,
            target,
            kind,
            callback,
            property: false,
            options,
        });
        Ok(())
    }
    fn property_listener(
        &mut self,
        target: u64,
        kind: &str,
        callback: Value,
    ) -> Result<(), JsError> {
        if !matches!(
            callback,
            Value::Function(_) | Value::Native(_) | Value::Null | Value::Undefined
        ) && !callback.as_host().is_some_and(|id| self.is_callable(id))
        {
            return Err(JsError::runtime(
                "Event handler property must be a function or null",
            ));
        }
        if matches!(callback, Value::Null | Value::Undefined) {
            self.state.listeners.retain(|listener| {
                !(listener.target == target && listener.kind == kind && listener.property)
            });
            return Ok(());
        }
        if let Some(listener) = self.state.listeners.iter_mut().find(|listener| {
            listener.target == target && listener.kind == kind && listener.property
        }) {
            listener.callback = callback;
            return Ok(());
        }
        if self.state.listeners.len() >= 128 {
            return Err(JsError::resource("DOM listener budget exceeded"));
        }
        self.state.next_listener = self
            .state
            .next_listener
            .checked_add(1)
            .ok_or_else(|| JsError::resource("DOM listener identity exhausted"))?;
        self.state.listeners.push(Listener {
            id: self.state.next_listener,
            target,
            kind: kind.into(),
            callback,
            property: true,
            options: ListenerOptions::default(),
        });
        Ok(())
    }
    fn event(&self, object: u64) -> Result<&DomEvent, JsError> {
        object
            .checked_sub(EVENT_BASE)
            .and_then(|index| self.state.events.get(index as usize))
            .ok_or_else(|| JsError::runtime("Invalid Event receiver"))
    }
    fn event_mut(&mut self, object: u64) -> Result<&mut DomEvent, JsError> {
        object
            .checked_sub(EVENT_BASE)
            .and_then(|index| self.state.events.get_mut(index as usize))
            .ok_or_else(|| JsError::runtime("Invalid Event receiver"))
    }
    fn listener_call(
        &mut self,
        target: u64,
        method: &str,
        args: &[Value],
        options: ListenerOptions,
    ) -> Result<Value, JsError> {
        let kind = args
            .first()
            .ok_or_else(|| JsError::runtime("Listener type required"))?
            .primitive_text()?;
        if kind.len() > 64 {
            return Err(JsError::resource("DOM listener type exceeds 64 bytes"));
        }
        let callback = args
            .get(1)
            .cloned()
            .ok_or_else(|| JsError::runtime("Listener callback required"))?;
        if method == "addEventListener" {
            self.listener(target, kind, callback, options)?;
        } else {
            self.state.listeners.retain(|listener| {
                listener.property
                    || listener.target != target
                    || listener.kind != kind
                    || listener.callback != callback
                    || listener.options.capture != options.capture
            });
        }
        Ok(Value::Undefined)
    }
}
fn event_handler_kind(property: &str) -> Option<&str> {
    property.strip_prefix("on").filter(|kind| {
        matches!(
            *kind,
            "click"
                | "load"
                | "error"
                | "input"
                | "change"
                | "submit"
                | "focus"
                | "blur"
                | "readystatechange"
        )
    })
}
impl Host for DomHost<'_> {
    fn allows_subtle_crypto(&self) -> bool {
        // Conservative top-level policy, not a complete secure-context model.
        // Only loader-supplied metadata grants this capability; JS location,
        // document properties, and script source URLs cannot grant it.
        self.state.document_url.as_ref().is_some_and(|url| {
            url.scheme() == "https" && url.host_str().is_some_and(|host| !host.is_empty())
        })
    }
    fn report_microtask_error(&mut self, error: &JsError) {
        if self.state.logs.len() < 32 {
            self.state.logs.push(
                format!("queueMicrotask: {error}")
                    .chars()
                    .take(1024)
                    .collect(),
            );
        }
    }
    fn utc_now_millis(&self) -> Option<i64> {
        let duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        i64::try_from(duration.as_millis()).ok()
    }
    fn trace_roots(&self) -> Vec<Value> {
        let mut roots = self
            .state
            .listeners
            .iter()
            .map(|listener| listener.callback.clone())
            .collect::<Vec<_>>();
        for timer in &self.state.timers {
            roots.push(timer.callback.clone());
            roots.extend(timer.args.iter().cloned());
        }
        roots.extend(
            self.state
                .animation
                .pending
                .iter()
                .map(|frame| frame.callback.clone()),
        );
        roots.extend(self.state.events.iter().map(|event| event.detail.clone()));
        roots
    }
    fn global_object(&self) -> Option<u64> {
        Some(WINDOW)
    }
    fn global_accessor(&self, property: &str) -> bool {
        event_handler_kind(property).is_some()
            || matches!(
                property,
                "navigator" | "location" | "sessionStorage" | "innerWidth" | "innerHeight"
            )
    }
    fn is_callable(&self, object: u64) -> bool {
        (DOCUMENT_METHOD..DOCUMENT_METHOD + DOCUMENT_METHODS.len() as u64).contains(&object)
            || (NODE_METHOD..NODE_METHOD + NODE_METHODS.len() as u64).contains(&object)
            || (NAVIGATOR_METHOD..NAVIGATOR_METHOD + NAVIGATOR_METHODS.len() as u64)
                .contains(&object)
            || (PLUGIN_METHOD..PLUGIN_METHOD + PLUGIN_METHODS.len() as u64).contains(&object)
            || (storage::METHOD..storage::METHOD + storage::METHODS.len() as u64).contains(&object)
            || object == LOCATION_STRING
            || (EVENT_METHOD..EVENT_METHOD + EVENT_METHODS.len() as u64).contains(&object)
            || (CLASS_METHOD..CLASS_METHOD + CLASS_METHODS.len() as u64).contains(&object)
            || (CONSOLE_METHOD..CONSOLE_METHOD + CONSOLE_METHODS.len() as u64).contains(&object)
            || matches!(
                object,
                SET_TIMEOUT
                    | CLEAR_TIMEOUT
                    | animation::REQUEST
                    | animation::CANCEL
                    | ADD_WINDOW_LISTENER
                    | REMOVE_WINDOW_LISTENER
                    | DISPATCH_WINDOW_EVENT
                    | EVENT_CONSTRUCTOR
                    | CUSTOM_EVENT_CONSTRUCTOR
            )
    }
    fn global(&mut self, name: &str) -> Option<Value> {
        if let Some(value) = self.geometry_global(name) {
            return Some(value);
        }
        if let Some(kind) = event_handler_kind(name) {
            return Some(
                self.state
                    .listeners
                    .iter()
                    .find(|listener| {
                        listener.target == WINDOW && listener.kind == kind && listener.property
                    })
                    .map_or(Value::Null, |listener| listener.callback.clone()),
            );
        }
        Some(Value::Host(match name {
            "Event" => EVENT_CONSTRUCTOR,
            "CustomEvent" => CUSTOM_EVENT_CONSTRUCTOR,
            "dispatchEvent" => DISPATCH_WINDOW_EVENT,
            "document" => DOC,
            "window" | "self" | "globalThis" | "this" => WINDOW,
            "console" => CONSOLE,
            "Math" => MATH,
            "navigator" => NAVIGATOR,
            "location" => LOCATION,
            "sessionStorage" => storage::OBJECT,
            "setTimeout" => SET_TIMEOUT,
            "clearTimeout" => CLEAR_TIMEOUT,
            "requestAnimationFrame" => animation::REQUEST,
            "cancelAnimationFrame" => animation::CANCEL,
            "addEventListener" => ADD_WINDOW_LISTENER,
            "removeEventListener" => REMOVE_WINDOW_LISTENER,
            _ => return None,
        }))
    }
    fn has_property(&mut self, object: u64, property: &str) -> Result<bool, JsError> {
        if matches!(object, animation::REQUEST | animation::CANCEL) {
            return Ok(matches!(property, "name" | "length"));
        }
        if object == WINDOW && property == "crypto" {
            return Ok(self.allows_subtle_crypto());
        }
        if object == storage::OBJECT {
            self.storage_available()?;
            return Ok(property == "length" || storage::METHODS.contains(&property));
        }
        if object == WINDOW && property == "sessionStorage" {
            return Ok(true);
        }
        if matches!(object, EVENT_CONSTRUCTOR | CUSTOM_EVENT_CONSTRUCTOR) {
            if property == "prototype" {
                return Err(JsError::runtime("Event prototypes are not implemented"));
            }
            return Ok(matches!(property, "name" | "length"));
        }
        if let Some(value) = self.geometry_has(object, property) {
            return Ok(value);
        }
        if event_handler_kind(property).is_some()
            && (object == WINDOW || object == DOC || self.node(object).is_ok())
        {
            return Ok(true);
        }
        if (EVENT_BASE..EVENT_BASE + MAX_EVENTS as u64).contains(&object) {
            self.event(object)?;
            return Ok((property == "detail" && self.event(object)?.custom)
                || EVENT_METHODS.contains(&property)
                || matches!(
                    property,
                    "type"
                        | "target"
                        | "currentTarget"
                        | "eventPhase"
                        | "bubbles"
                        | "cancelable"
                        | "defaultPrevented"
                        | "cancelBubble"
                        | "returnValue"
                        | "timeStamp"
                        | "isTrusted"
                        | "composed"
                ));
        }
        if matches!(object, NAVIGATOR | PLUGINS | MIME_TYPES | LOCATION) {
            self.metadata_read(property)?;
            return Ok(match object {
                NAVIGATOR => {
                    NAVIGATOR_METHODS.contains(&property)
                        || matches!(
                            property,
                            "userAgent"
                                | "appVersion"
                                | "appName"
                                | "appCodeName"
                                | "product"
                                | "vendor"
                                | "language"
                                | "platform"
                                | "hardwareConcurrency"
                                | "cookieEnabled"
                                | "plugins"
                                | "mimeTypes"
                        )
                }
                PLUGINS | MIME_TYPES => {
                    matches!(property, "length" | "item" | "namedItem")
                        || object == PLUGINS && property == "refresh"
                }
                LOCATION => LOCATION_PROPERTIES.contains(&property),
                _ => false,
            });
        }
        if object == DOC || (NODE..LIST).contains(&object) {
            self.read_cost()?;
        }
        if object == WINDOW {
            return Ok(matches!(
                property,
                "Event"
                    | "CustomEvent"
                    | "dispatchEvent"
                    | "document"
                    | "window"
                    | "self"
                    | "globalThis"
                    | "console"
                    | "setTimeout"
                    | "clearTimeout"
                    | "requestAnimationFrame"
                    | "cancelAnimationFrame"
                    | "addEventListener"
                    | "removeEventListener"
                    | "Math"
                    | "onload"
                    | "onerror"
                    | "navigator"
                    | "location"
            ));
        }
        if object == DOC {
            return Ok(DOCUMENT_METHODS.contains(&property)
                || matches!(
                    property,
                    "title"
                        | "readyState"
                        | "body"
                        | "head"
                        | "documentElement"
                        | "compatMode"
                        | "prerendering"
                        | "nodeType"
                        | "nodeName"
                        | "ownerDocument"
                        | "parentNode"
                        | "parentElement"
                        | "contains"
                        | "firstChild"
                        | "lastChild"
                        | "firstElementChild"
                        | "lastElementChild"
                        | "isConnected"
                        | "childElementCount"
                        | "children"
                        | "childNodes"
                        | "URL"
                        | "documentURI"
                        | "location"
                ));
        }
        if object == CONSOLE {
            return Ok(matches!(
                property,
                "log" | "info" | "warn" | "error" | "debug"
            ));
        }
        if object == MATH {
            return Ok(matches!(
                property,
                "PI" | "E" | "abs" | "floor" | "ceil" | "round" | "sqrt" | "pow" | "min" | "max"
            ));
        }
        if object >= LIST {
            let values = self
                .state
                .lists
                .get((object - LIST) as usize)
                .ok_or_else(|| JsError::runtime("Invalid NodeList"))?;
            return Ok(property == "length"
                || property
                    .parse::<usize>()
                    .ok()
                    .is_some_and(|index| index.to_string() == property && index < values.len()));
        }
        if (CLASS..LIST).contains(&object) {
            self.node(NODE + object - CLASS)?;
            return Ok(matches!(property, "contains" | "add" | "remove" | "toggle"));
        }
        if (STYLE..CLASS).contains(&object) {
            self.node(NODE + object - STYLE)?;
            return Err(JsError::runtime(
                "CSS style property existence is not implemented",
            ));
        }
        if (NODE..STYLE).contains(&object) {
            let id = self.node(object)?;
            if NODE_METHODS.contains(&property) && self.supports_node_method(id, property) {
                return Ok(true);
            }
            if matches!(
                property,
                "textContent"
                    | "innerText"
                    | "nodeType"
                    | "nodeName"
                    | "ownerDocument"
                    | "parentNode"
                    | "parentElement"
                    | "childNodes"
                    | "firstChild"
                    | "lastChild"
                    | "previousSibling"
                    | "nextSibling"
                    | "previousElementSibling"
                    | "nextElementSibling"
                    | "isConnected"
                    | "appendChild"
                    | "contains"
                    | "addEventListener"
                    | "removeEventListener"
            ) || property.strip_prefix("on").is_some_and(|kind| {
                matches!(
                    kind,
                    "click" | "load" | "input" | "change" | "submit" | "focus" | "blur"
                )
            }) {
                return Ok(true);
            }
            let Some(element) = self.document.element(id) else {
                return Ok(false);
            };
            if property == "duration" {
                return Ok(element.tag == "audio");
            }
            return Ok(matches!(
                property,
                "id" | "className"
                    | "style"
                    | "classList"
                    | "tagName"
                    | "children"
                    | "firstElementChild"
                    | "lastElementChild"
                    | "previousElementSibling"
                    | "nextElementSibling"
                    | "childElementCount"
                    | "getAttribute"
                    | "setAttribute"
                    | "hasAttribute"
                    | "removeAttribute"
                    | "querySelector"
                    | "querySelectorAll"
                    | "matches"
                    | "closest"
                    | "remove"
            ) || property == "value"
                && matches!(
                    element.tag.as_str(),
                    "input" | "textarea" | "select" | "button" | "option"
                )
                || property == "disabled"
                    && matches!(
                        element.tag.as_str(),
                        "input"
                            | "textarea"
                            | "select"
                            | "button"
                            | "option"
                            | "optgroup"
                            | "fieldset"
                    )
                || property == "checked" && element.tag == "input");
        }
        Err(JsError::runtime(
            "Host property existence is not implemented for this object",
        ))
    }
    fn get(&mut self, object: u64, property: &str) -> Result<Value, JsError> {
        if matches!(object, animation::REQUEST | animation::CANCEL) {
            return Ok(match property {
                "length" => Value::Number(1.),
                "name" => Value::string(if object == animation::REQUEST {
                    "requestAnimationFrame"
                } else {
                    "cancelAnimationFrame"
                }),
                _ => Value::Undefined,
            });
        }
        if object == storage::OBJECT || object == WINDOW && property == "sessionStorage" {
            return self.storage_get(object, property);
        }
        if matches!(object, EVENT_CONSTRUCTOR | CUSTOM_EVENT_CONSTRUCTOR) {
            return match property {
                "name" => Ok(Value::string(if object == EVENT_CONSTRUCTOR {
                    "Event"
                } else {
                    "CustomEvent"
                })),
                "length" => Ok(Value::Number(1.)),
                "prototype" => Err(JsError::runtime("Event prototypes are not implemented")),
                _ => Ok(Value::Undefined),
            };
        }
        if let Some(value) = self.geometry_get(object, property)? {
            return Ok(value);
        }
        if let Some(kind) = event_handler_kind(property)
            && (object == WINDOW || object == DOC || self.node(object).is_ok())
        {
            return Ok(self
                .state
                .listeners
                .iter()
                .find(|listener| {
                    listener.target == object && listener.kind == kind && listener.property
                })
                .map_or(Value::Null, |listener| listener.callback.clone()));
        }
        if (EVENT_BASE..EVENT_BASE + MAX_EVENTS as u64).contains(&object) {
            let event = self.event(object)?;
            if let Some(index) = EVENT_METHODS.iter().position(|method| *method == property) {
                return Ok(Value::Host(EVENT_METHOD + index as u64));
            }
            return Ok(match property {
                "type" => Value::string(event.kind.clone()),
                "target" => event.target.map_or(Value::Null, Value::Host),
                "isTrusted" => Value::Bool(event.trusted),
                "composed" => Value::Bool(false),
                "detail" if event.custom => event.detail.clone(),
                "currentTarget" => event.current.map_or(Value::Null, Value::Host),
                "eventPhase" => Value::Number(f64::from(event.phase)),
                "bubbles" => Value::Bool(event.bubbles),
                "cancelable" => Value::Bool(event.cancelable),
                "defaultPrevented" => Value::Bool(event.canceled),
                "cancelBubble" => Value::Bool(event.stopped),
                "returnValue" => Value::Bool(!event.canceled),
                "timeStamp" => Value::Number(event.timestamp as f64),
                _ => Value::Undefined,
            });
        }
        if object == CONSOLE
            && let Some(index) = CONSOLE_METHODS
                .iter()
                .position(|method| *method == property)
        {
            return Ok(Value::Host(CONSOLE_METHOD + index as u64));
        }
        if object == NAVIGATOR {
            return self.navigator_property(property);
        }
        if object == LOCATION {
            return self.location_property(property);
        }
        if matches!(object, PLUGINS | MIME_TYPES) {
            self.metadata_read(property)?;
            return Ok(if property == "length" {
                Value::Number(0.)
            } else if let Some(index) = PLUGIN_METHODS
                .iter()
                .position(|name| *name == property && (*name != "refresh" || object == PLUGINS))
            {
                Value::Host(PLUGIN_METHOD + index as u64)
            } else {
                Value::Undefined
            });
        }
        if object == DOC || (NODE..LIST).contains(&object) {
            self.read_cost()?;
        }
        if object == WINDOW {
            return match property {
                "Event" => Ok(Value::Host(EVENT_CONSTRUCTOR)),
                "CustomEvent" => Ok(Value::Host(CUSTOM_EVENT_CONSTRUCTOR)),
                "dispatchEvent" => Ok(Value::Host(DISPATCH_WINDOW_EVENT)),
                "document" => Ok(Value::Host(DOC)),
                "window" | "self" | "globalThis" => Ok(Value::Host(WINDOW)),
                "console" => Ok(Value::Host(CONSOLE)),
                "setTimeout" => Ok(Value::Host(SET_TIMEOUT)),
                "clearTimeout" => Ok(Value::Host(CLEAR_TIMEOUT)),
                "requestAnimationFrame" => Ok(Value::Host(animation::REQUEST)),
                "cancelAnimationFrame" => Ok(Value::Host(animation::CANCEL)),
                "addEventListener" => Ok(Value::Host(ADD_WINDOW_LISTENER)),
                "removeEventListener" => Ok(Value::Host(REMOVE_WINDOW_LISTENER)),
                "Math" => Ok(Value::Host(MATH)),
                "navigator" => Ok(Value::Host(NAVIGATOR)),
                "location" => Ok(Value::Host(LOCATION)),
                "onload" | "onerror" => Ok(self
                    .state
                    .listeners
                    .iter()
                    .find(|listener| {
                        listener.target == WINDOW
                            && listener.kind
                                == (if property == "onload" {
                                    "load"
                                } else {
                                    "error"
                                })
                            && listener.property
                    })
                    .map_or(Value::Null, |listener| listener.callback.clone())),
                _ => Ok(Value::Undefined),
            };
        }
        if object == DOC {
            if property == "URL" || property == "documentURI" {
                return self.location_property("href");
            }
            if property == "location" {
                return Ok(Value::Host(LOCATION));
            }
            if let Some(method) = NODE_METHODS.iter().position(|method| *method == property)
                && self.supports_node_method(0, property)
            {
                return Ok(Value::Host(NODE_METHOD + method as u64));
            }
            if let Some(index) = DOCUMENT_METHODS
                .iter()
                .position(|method| *method == property)
            {
                return Ok(Value::Host(DOCUMENT_METHOD + index as u64));
            }
            if matches!(
                property,
                "firstChild" | "lastChild" | "firstElementChild" | "lastElementChild"
            ) {
                return self.related_node(0, property);
            }
            if property == "children" || property == "childNodes" {
                let nodes = self.document.nodes[0]
                    .children
                    .iter()
                    .copied()
                    .filter(|&child| {
                        property == "childNodes" || self.document.element(child).is_some()
                    })
                    .collect();
                return self.list(nodes);
            }
            return match property {
                // Promise resolution probes this absent property. Keep other
                // unsupported document APIs explicit instead of hiding errors.
                "then" => Ok(Value::Undefined),
                "isConnected" => Ok(Value::Bool(true)),
                "childElementCount" => Ok(Value::Number(
                    self.document.nodes[0]
                        .children
                        .iter()
                        .filter(|&&child| self.document.element(child).is_some())
                        .count() as f64,
                )),
                "compatMode" => self
                    .document
                    .mode
                    .compat_mode()
                    .map(Value::string)
                    .ok_or_else(|| {
                        JsError::runtime(
                            "Legacy DOCTYPE compatibility classification is not implemented",
                        )
                    }),
                // No speculative/prerender pipeline exists. Visibility, unlike
                // prerendering, depends on native tab state and is not guessed.
                "prerendering" => Ok(Value::Bool(false)),
                "nodeType" => Ok(Value::Number(9.)),
                "nodeName" => Ok(Value::string("#document")),
                "ownerDocument" | "parentNode" | "parentElement" => Ok(Value::Null),
                "title" => Ok(Value::string(self.document.document_title())),
                "readyState" => Ok(Value::string(self.state.ready_state.as_str())),
                "body" | "head" | "documentElement" => {
                    let node = match property {
                        "documentElement" => self.document.document_element(),
                        "head" => self.document.head(),
                        _ => self.document.body(),
                    };
                    Ok(node.map_or(Value::Null, Self::node_value))
                }
                _ => Err(JsError::runtime(format!(
                    "Unsupported document property {property}"
                ))),
            };
        }
        if object == MATH {
            return match property {
                "PI" => Ok(Value::Number(std::f64::consts::PI)),
                "E" => Ok(Value::Number(std::f64::consts::E)),
                _ => Err(JsError::runtime(format!(
                    "Unsupported Math property {property}"
                ))),
            };
        }
        if object >= LIST {
            let values = self
                .state
                .lists
                .get((object - LIST) as usize)
                .ok_or_else(|| JsError::runtime("Invalid NodeList"))?;
            return if property == "length" {
                Ok(Value::Number(values.len() as f64))
            } else {
                Ok(property
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| values.get(i))
                    .map_or(Value::Undefined, |&id| Value::Host(NODE + id as u64)))
            };
        }
        if object >= CLASS {
            self.node(NODE + object - CLASS)?;
            if let Some(index) = CLASS_METHODS.iter().position(|method| *method == property) {
                return Ok(Value::Host(CLASS_METHOD + index as u64));
            }
            return Err(JsError::runtime("Read classList with contains()"));
        }
        if object >= STYLE {
            let id = self.node(NODE + object - STYLE)?;
            let key = css_property(property)?;
            let style = self
                .document
                .element(id)
                .and_then(|e| e.attr("style"))
                .unwrap_or("");
            let value = style
                .split(';')
                .filter_map(|d| d.split_once(':'))
                .filter(|(name, _)| name.trim() == key)
                .map(|(_, value)| value.trim())
                .next_back()
                .unwrap_or("");
            return Ok(Value::string(value));
        }
        let id = self.node(object)?;
        if property == "duration" && self.document.element(id).is_some_and(|e| e.tag == "audio") {
            return self.audio_duration(id);
        }
        if let Some(method) = NODE_METHODS.iter().position(|method| *method == property)
            && self.supports_node_method(id, property)
        {
            return Ok(Value::Host(NODE_METHOD + method as u64));
        }
        if matches!(
            property,
            "firstChild"
                | "lastChild"
                | "previousSibling"
                | "nextSibling"
                | "firstElementChild"
                | "lastElementChild"
                | "previousElementSibling"
                | "nextElementSibling"
        ) {
            if matches!(property, "firstElementChild" | "lastElementChild")
                && self.document.element(id).is_none()
            {
                return Err(JsError::runtime("Element relationship requires an element"));
            }
            return self.related_node(id, property);
        }
        if property == "value"
            && self
                .document
                .element(id)
                .is_some_and(|element| element.tag == "textarea")
        {
            return Ok(Value::string(self.document.text_content(id)));
        }
        if property == "value"
            && self
                .document
                .element(id)
                .is_some_and(|element| element.tag == "select")
        {
            return Ok(Value::string(self.select_value(id)?));
        }
        if let Some(kind) = property.strip_prefix("on")
            && matches!(
                kind,
                "click" | "load" | "input" | "change" | "submit" | "focus" | "blur"
            )
        {
            return Ok(self
                .state
                .listeners
                .iter()
                .find(|listener| {
                    listener.target == object && listener.kind == kind && listener.property
                })
                .map_or(Value::Null, |listener| listener.callback.clone()));
        }
        match property {
            // Reached only after node-handle validation and the normal read
            // charge, so assimilation cannot bypass either boundary.
            "then" => Ok(Value::Undefined),
            "isConnected" => Ok(Value::Bool(self.connected_node(id)?)),
            "childElementCount" => {
                if self.document.element(id).is_none() {
                    return Err(JsError::runtime("childElementCount requires an element"));
                }
                Ok(Value::Number(
                    self.document.nodes[id]
                        .children
                        .iter()
                        .filter(|&&child| self.document.element(child).is_some())
                        .count() as f64,
                ))
            }
            "textContent" | "innerText" => Ok(Value::string(self.document.text_content(id))),
            "id" | "className" | "value" => Ok(Value::string(
                self.document
                    .element(id)
                    .and_then(|e| {
                        e.attr(if property == "className" {
                            "class"
                        } else {
                            property
                        })
                    })
                    .unwrap_or(""),
            )),
            "tagName" => Ok(self.document.element(id).map_or(Value::Undefined, |e| {
                Value::string(e.tag.to_ascii_uppercase())
            })),
            "style" => Ok(Value::Host(STYLE + id as u64)),
            "classList" => Ok(Value::Host(CLASS + id as u64)),
            "nodeType" => Ok(Value::Number(match self.document.nodes[id].kind {
                NodeKind::Document => 9.,
                NodeKind::Element(_) => 1.,
                NodeKind::Text(_) => 3.,
            })),
            "nodeName" => Ok(Value::string(match &self.document.nodes[id].kind {
                NodeKind::Document => "#document".into(),
                NodeKind::Text(_) => "#text".into(),
                NodeKind::Element(element) => element.tag.to_ascii_uppercase(),
            })),
            "ownerDocument" => Ok(
                if matches!(self.document.nodes[id].kind, NodeKind::Document) {
                    Value::Null
                } else {
                    Value::Host(DOC)
                },
            ),
            "parentNode" => Ok(self.document.nodes[id].parent.map_or(Value::Null, |id| {
                if id == 0 {
                    Value::Host(DOC)
                } else {
                    Value::Host(NODE + id as u64)
                }
            })),
            "parentElement" => Ok(self.document.nodes[id]
                .parent
                .filter(|&parent| self.document.element(parent).is_some())
                .map_or(Value::Null, |parent| Value::Host(NODE + parent as u64))),
            "children" | "childNodes" => self.list(
                self.document.nodes[id]
                    .children
                    .iter()
                    .filter(|&&child| {
                        property == "childNodes" || self.document.element(child).is_some()
                    })
                    .copied()
                    .collect(),
            ),
            "disabled" | "checked" => Ok(Value::Bool(
                self.document
                    .element(id)
                    .is_some_and(|e| e.attributes.contains_key(property)),
            )),
            _ => Err(diagnostics::unsupported_dom(
                self.document,
                id,
                property,
                None,
            )),
        }
    }
    fn set(&mut self, object: u64, property: &str, value: Value) -> Result<(), JsError> {
        if object == storage::OBJECT || object == WINDOW && property == "sessionStorage" {
            return Err(JsError::runtime(
                "Session storage properties are read-only; use storage methods",
            ));
        }
        if let Some(kind) = event_handler_kind(property)
            && (object == WINDOW || object == DOC || self.node(object).is_ok())
        {
            return self.property_listener(object, kind, value);
        }
        if (EVENT_BASE..EVENT_BASE + MAX_EVENTS as u64).contains(&object) {
            let event = self.event_mut(object)?;
            return match property {
                "cancelBubble" => {
                    if value.truthy() {
                        event.stopped = true;
                    }
                    Ok(())
                }
                "returnValue" => {
                    if !value.truthy() && event.cancelable && !event.passive {
                        event.canceled = true;
                    }
                    Ok(())
                }
                _ => Err(JsError::runtime(
                    "Event property is read-only or unsupported",
                )),
            };
        }
        if matches!(object, NAVIGATOR | PLUGINS | MIME_TYPES | LOCATION)
            || object == WINDOW && matches!(property, "navigator" | "location")
            || object == DOC && matches!(property, "URL" | "documentURI" | "location")
        {
            self.metadata_read(property)?;
            return Err(JsError::runtime(
                "Browser metadata is read-only; script navigation is not implemented",
            ));
        }
        if object == DOC && property == "title" {
            let text = value.primitive_text()?;
            self.read_cost()?;
            if let Some(title) = self.document.title_element() {
                return self.replace_text(title, text);
            }
            let Some(head) = self.document.head() else {
                // A removed head is not silently recreated by the title setter.
                return Ok(());
            };
            self.mutate(text.len().saturating_add(5))?;
            let mut fragment = Document::default();
            let title = fragment
                .append(
                    0,
                    NodeKind::Element(Element {
                        tag: "title".into(),
                        attributes: BTreeMap::new(),
                    }),
                )
                .map_err(JsError::runtime)?;
            if !text.is_empty() {
                fragment
                    .append(title, NodeKind::Text(text))
                    .map_err(JsError::runtime)?;
            }
            return self.import_fragment(head, &fragment, false);
        }
        if (STYLE..CLASS).contains(&object) {
            let id = self.node(NODE + object - STYLE)?;
            let key = css_property(property)?;
            let value = value.primitive_text()?;
            if value.contains([';', '{', '}']) {
                return Err(JsError::runtime(
                    "Style property values must be a single declaration",
                ));
            }
            let current = self
                .document
                .element(id)
                .and_then(|e| e.attr("style"))
                .unwrap_or("");
            let mut style = current
                .split(';')
                .filter(|d| {
                    !d.split_once(':')
                        .is_some_and(|(name, _)| name.trim() == key)
                })
                .filter(|d| !d.trim().is_empty())
                .collect::<Vec<_>>()
                .join(";");
            style.push_str(&format!(";{key}:{value}"));
            return self.set_attribute(id, "style".into(), style);
        }
        let id = self.node(object)?;
        if property == "duration" && self.document.element(id).is_some_and(|e| e.tag == "audio") {
            return Err(JsError::runtime("audio.duration is read-only"));
        }
        if property == "value"
            && self
                .document
                .element(id)
                .is_some_and(|element| element.tag == "textarea")
        {
            let text = value.primitive_text()?;
            self.set_attribute(id, "value".into(), text.clone())?;
            return self.replace_text(id, text);
        }
        if property == "value"
            && self
                .document
                .element(id)
                .is_some_and(|element| element.tag == "select")
        {
            return self.set_select_value(id, &value.primitive_text()?, false);
        }
        match property {
            "textContent" | "innerText" => self.replace_text(
                id,
                if matches!(value, Value::Null) {
                    String::new()
                } else {
                    value.primitive_text()?
                },
            ),
            "id" | "className" | "value" => self.set_attribute(
                id,
                if property == "className" {
                    "class"
                } else {
                    property
                }
                .into(),
                value.primitive_text()?,
            ),
            "innerHTML" => {
                if self.document.element(id).is_none() {
                    return Err(JsError::runtime("innerHTML requires an element"));
                }
                let html = value.primitive_text()?;
                self.mutate(html.len())?;
                if html.len() > 65_536 {
                    return Err(JsError::resource("innerHTML exceeds 64 KiB"));
                }
                self.charge_tree_read(html.len())?;
                let context = &self.document.element(id).unwrap().tag;
                // Unsupported table/select/foreign contexts fail explicitly.
                // Raw text and RCDATA use the target's context.
                let fragment = web_html::parse_fragment(&html, context).map_err(|message| {
                    // Parser quotas must retain session-level resource rollback.
                    // Unsupported contexts remain ordinary, catchable failures.
                    if matches!(
                        message.as_str(),
                        "HTML input exceeds 1 MiB"
                            | "HTML node limit exceeded"
                            | "HTML nesting limit exceeded"
                            | "HTML insertion mode transition limit exceeded"
                    ) {
                        JsError::resource(message)
                    } else {
                        JsError::runtime(message)
                    }
                })?;
                self.import_fragment(id, &fragment, true)
            }
            "disabled" | "checked" => {
                self.mutate(0)?;
                let NodeKind::Element(element) = &mut self.document.nodes[id].kind else {
                    return Err(JsError::runtime("Not an element"));
                };
                if value.truthy() {
                    element.attributes.insert(property.into(), String::new());
                } else {
                    element.attributes.remove(property);
                }
                Ok(())
            }
            _ => Err(diagnostics::unsupported_dom(
                self.document,
                id,
                property,
                Some(&value),
            )),
        }
    }
    fn is_constructor(&self, object: u64) -> bool {
        matches!(object, EVENT_CONSTRUCTOR | CUSTOM_EVENT_CONSTRUCTOR)
    }
    fn construct_with_runtime(
        &mut self,
        object: u64,
        args: &[Value],
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        self.construct_event(object, args, runtime)
    }
    fn call_with_runtime(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
        this: &Value,
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        if matches!(object, animation::REQUEST | animation::CANCEL) {
            return self.animation_call(object, method, args, this, runtime);
        }
        if object == storage::OBJECT
            || (storage::METHOD..storage::METHOD + storage::METHODS.len() as u64).contains(&object)
        {
            return self.storage_call(object, method, args, this, runtime);
        }
        if matches!(object, EVENT_CONSTRUCTOR | CUSTOM_EVENT_CONSTRUCTOR) {
            return Err(JsError::runtime("Event constructors require new"));
        }
        if object == DISPATCH_WINDOW_EVENT && method == "call" {
            if !matches!(this, Value::Undefined | Value::Null) && *this != Value::Host(WINDOW) {
                return Err(JsError::runtime("Illegal Window receiver"));
            }
            return self.dispatch_programmatic(runtime, WINDOW, args);
        }
        let alias = if method == "call"
            && (DOCUMENT_METHOD..DOCUMENT_METHOD + DOCUMENT_METHODS.len() as u64).contains(&object)
        {
            if *this != Value::Host(DOC) {
                return Err(JsError::runtime("Illegal Document receiver"));
            }
            Some((DOC, DOCUMENT_METHODS[(object - DOCUMENT_METHOD) as usize]))
        } else if method == "call"
            && (NODE_METHOD..NODE_METHOD + NODE_METHODS.len() as u64).contains(&object)
        {
            let id = self.node_argument(Some(this))?;
            Some((
                if id == 0 { DOC } else { NODE + id as u64 },
                NODE_METHODS[(object - NODE_METHOD) as usize],
            ))
        } else if method == "call" && matches!(object, ADD_WINDOW_LISTENER | REMOVE_WINDOW_LISTENER)
        {
            if !matches!(this, Value::Undefined | Value::Null) && *this != Value::Host(WINDOW) {
                return Err(JsError::runtime("Illegal Window receiver"));
            }
            Some((
                WINDOW,
                if object == ADD_WINDOW_LISTENER {
                    "addEventListener"
                } else {
                    "removeEventListener"
                },
            ))
        } else {
            None
        };
        let (target, name) = alias.unwrap_or((object, method));
        if name == "dispatchEvent"
            && (target == WINDOW || target == DOC || self.node(target).is_ok())
        {
            return self.dispatch_programmatic(runtime, target, args);
        }
        if matches!(name, "addEventListener" | "removeEventListener")
            && (target == WINDOW || target == DOC || self.node(target).is_ok())
        {
            // Convert the event type before evaluating the options getters.
            let kind = args
                .first()
                .ok_or_else(|| JsError::runtime("Listener type required"))?
                .primitive_text()?;
            if kind.len() > 64 {
                return Err(JsError::resource("DOM listener type exceeds 64 bytes"));
            }
            let mut options = ListenerOptions::default();
            if let Some(value) = args.get(2) {
                match value {
                    Value::Undefined | Value::Null => {}
                    Value::Bool(capture) => options.capture = *capture,
                    Value::Object(_)
                    | Value::Array(_)
                    | Value::Function(_)
                    | Value::Native(_)
                    | Value::Host(_) => {
                        options.capture = runtime
                            .read_property(value.clone(), "capture", self)?
                            .truthy();
                        if name == "addEventListener" {
                            options.once =
                                runtime.read_property(value.clone(), "once", self)?.truthy();
                            options.passive = runtime
                                .read_property(value.clone(), "passive", self)?
                                .truthy();
                            let signal = runtime.read_property(value.clone(), "signal", self)?;
                            if !matches!(signal, Value::Null | Value::Undefined) {
                                return Err(JsError::runtime(
                                    "AbortSignal listener option is not implemented",
                                ));
                            }
                        }
                    }
                    _ => options.capture = value.truthy(),
                }
            }
            if name == "addEventListener"
                && let Some(callback) = args.get(1)
                && !matches!(callback, Value::Null | Value::Undefined)
                && !runtime.is_callable_value(callback, self)
            {
                return Err(JsError::runtime(
                    "Listener objects/handleEvent are not implemented; callable callback required",
                ));
            }
            let callback = args
                .get(1)
                .cloned()
                .ok_or_else(|| JsError::runtime("Listener callback required"))?;
            return self.listener_call(target, name, &[Value::string(kind), callback], options);
        }
        self.call_with_this(object, method, args, this)
    }
    fn call_with_this(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
        this: &Value,
    ) -> Result<Value, JsError> {
        if (EVENT_METHOD..EVENT_METHOD + EVENT_METHODS.len() as u64).contains(&object) {
            let receiver = this
                .as_host()
                .ok_or_else(|| JsError::runtime("Event method requires Event receiver"))?;
            if method != "call" {
                return Err(JsError::runtime("Invalid Event invocation"));
            }
            self.event(receiver)?;
            return self.call(
                receiver,
                EVENT_METHODS[(object - EVENT_METHOD) as usize],
                args,
            );
        }
        if (CLASS_METHOD..CLASS_METHOD + CLASS_METHODS.len() as u64).contains(&object) {
            let receiver = this
                .as_host()
                .filter(|id| (CLASS..LIST).contains(id))
                .ok_or_else(|| {
                    JsError::runtime("classList method requires DOMTokenList receiver")
                })?;
            if method != "call" {
                return Err(JsError::runtime("Invalid classList invocation"));
            }
            return self.call(
                receiver,
                CLASS_METHODS[(object - CLASS_METHOD) as usize],
                args,
            );
        }
        if (CONSOLE_METHOD..CONSOLE_METHOD + CONSOLE_METHODS.len() as u64).contains(&object) {
            if method != "call" {
                return Err(JsError::runtime("Invalid console invocation"));
            }
            return self.call(
                CONSOLE,
                CONSOLE_METHODS[(object - CONSOLE_METHOD) as usize],
                args,
            );
        }
        if (NAVIGATOR_METHOD..NAVIGATOR_METHOD + NAVIGATOR_METHODS.len() as u64).contains(&object) {
            if method != "call" || *this != Value::Host(NAVIGATOR) {
                return Err(JsError::runtime(
                    "Illegal invocation: Navigator method requires navigator receiver",
                ));
            }
            return self.navigator_call(
                NAVIGATOR_METHODS[(object - NAVIGATOR_METHOD) as usize],
                args,
            );
        }
        if object == LOCATION_STRING {
            if method != "call" || *this != Value::Host(LOCATION) {
                return Err(JsError::runtime(
                    "Illegal invocation: Location method requires location receiver",
                ));
            }
            return self.location_property("href");
        }
        if (PLUGIN_METHOD..PLUGIN_METHOD + PLUGIN_METHODS.len() as u64).contains(&object) {
            let receiver = this
                .as_host()
                .filter(|receiver| matches!(*receiver, PLUGINS | MIME_TYPES))
                .ok_or_else(|| {
                    JsError::runtime("Illegal invocation: plugin collection receiver required")
                })?;
            if method != "call" {
                return Err(JsError::runtime(
                    "Invalid plugin collection method invocation",
                ));
            }
            return self.call(
                receiver,
                PLUGIN_METHODS[(object - PLUGIN_METHOD) as usize],
                args,
            );
        }
        if (NODE_METHOD..NODE_METHOD + NODE_METHODS.len() as u64).contains(&object) {
            let name = NODE_METHODS[(object - NODE_METHOD) as usize];
            let id = self.node_argument(Some(this))?;
            if method != "call" || !self.supports_node_method(id, name) {
                return Err(JsError::runtime(
                    "Illegal invocation: node method requires a compatible Node receiver",
                ));
            }
            return self.call(if id == 0 { DOC } else { NODE + id as u64 }, name, args);
        }
        if (DOCUMENT_METHOD..DOCUMENT_METHOD + DOCUMENT_METHODS.len() as u64).contains(&object) {
            if method != "call" || *this != Value::Host(DOC) {
                return Err(JsError::runtime(
                    "Illegal invocation: document method requires document receiver",
                ));
            }
            return self.call(
                DOC,
                DOCUMENT_METHODS[(object - DOCUMENT_METHOD) as usize],
                args,
            );
        }
        self.call(object, method, args)
    }
    fn call(&mut self, object: u64, method: &str, args: &[Value]) -> Result<Value, JsError> {
        if (EVENT_BASE..EVENT_BASE + MAX_EVENTS as u64).contains(&object) {
            let event = self.event_mut(object)?;
            return match method {
                "preventDefault" => {
                    if event.cancelable && !event.passive {
                        event.canceled = true;
                    }
                    Ok(Value::Undefined)
                }
                "stopPropagation" => {
                    event.stopped = true;
                    Ok(Value::Undefined)
                }
                "stopImmediatePropagation" => {
                    event.stopped = true;
                    event.immediate = true;
                    Ok(Value::Undefined)
                }
                _ => Err(JsError::runtime("Unsupported Event method")),
            };
        }
        if object == NAVIGATOR {
            return self.navigator_call(method, args);
        }
        if object == LOCATION {
            if method == "toString" {
                return self.location_property("href");
            }
            return Err(JsError::runtime(
                "Location navigation methods are not implemented",
            ));
        }
        if matches!(object, PLUGINS | MIME_TYPES) {
            self.metadata_read(method)?;
            return match method {
                "refresh" if object == PLUGINS => Ok(Value::Undefined),
                "item" | "namedItem" => {
                    let argument = args
                        .first()
                        .ok_or_else(|| JsError::runtime("Plugin lookup requires an argument"))?;
                    let value = argument.primitive_text()?;
                    if value.len() > 1024 {
                        return Err(JsError::resource("Plugin lookup name exceeds 1024 bytes"));
                    }
                    self.charge_tree_read(value.len())?;
                    Ok(Value::Null)
                }
                _ => Err(JsError::runtime("Unsupported plugin collection method")),
            };
        }
        if object == ADD_WINDOW_LISTENER && method == "call" {
            return self.call(WINDOW, "addEventListener", args);
        }
        if object == REMOVE_WINDOW_LISTENER && method == "call" {
            return self.call(WINDOW, "removeEventListener", args);
        }
        if object == DOC || (NODE..LIST).contains(&object) {
            self.read_cost()?;
        }
        let text = |index: usize| {
            args.get(index)
                .ok_or_else(|| JsError::runtime("Missing DOM argument"))?
                .primitive_text()
        };
        if object == CONSOLE && matches!(method, "log" | "info" | "warn" | "error" | "debug") {
            if self.state.logs.len() < 32 {
                let line = args
                    .iter()
                    .map(Value::primitive_text)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(" ");
                self.state.logs.push(line.chars().take(1024).collect());
            }
            return Ok(Value::Undefined);
        }
        if object == MATH {
            let first = args.first().map_or(f64::NAN, number);
            return Ok(Value::Number(match method {
                "abs" => first.abs(),
                "floor" => first.floor(),
                "ceil" => first.ceil(),
                "round" => (first + 0.5).floor(),
                "sqrt" => first.sqrt(),
                "pow" => first.powf(args.get(1).map_or(f64::NAN, number)),
                "min" => args.iter().map(number).fold(f64::INFINITY, |a, b| {
                    if a.is_nan() || b.is_nan() {
                        f64::NAN
                    } else {
                        a.min(b)
                    }
                }),
                "max" => args.iter().map(number).fold(f64::NEG_INFINITY, |a, b| {
                    if a.is_nan() || b.is_nan() {
                        f64::NAN
                    } else {
                        a.max(b)
                    }
                }),
                _ => {
                    return Err(JsError::runtime(format!(
                        "Unsupported Math method {method}"
                    )));
                }
            }));
        }
        if object == SET_TIMEOUT && method == "call" {
            let callback = args
                .first()
                .cloned()
                .ok_or_else(|| JsError::runtime("Timer callback required"))?;
            if !matches!(callback, Value::Function(_) | Value::Native(_))
                && !callback.as_host().is_some_and(|id| self.is_callable(id))
            {
                return Err(JsError::runtime("Timer strings/eval are not supported"));
            }
            if self.state.timers.len() >= 128 {
                return Err(JsError::resource("Timer budget exceeded"));
            }
            let delay = args.get(1).map_or(0., number).clamp(0., 86_400_000.) as u64;
            self.state.next_timer += 1;
            let id = self.state.next_timer;
            self.state.timers.push(Timer {
                id,
                due: self.state.clock.saturating_add(delay),
                callback,
                args: args.iter().skip(2).cloned().collect(),
            });
            return Ok(Value::Number(id as f64));
        }
        if object == CLEAR_TIMEOUT && method == "call" {
            if let Some(value) = args.first() {
                let id = number(value) as u64;
                self.state.timers.retain(|timer| timer.id != id);
            }
            return Ok(Value::Undefined);
        }
        if matches!(
            method,
            "appendChild" | "insertBefore" | "removeChild" | "replaceChild" | "hasChildNodes"
        ) && (object == DOC || self.node(object).is_ok())
        {
            let parent = if object == DOC { 0 } else { self.node(object)? };
            if method == "hasChildNodes" {
                return Ok(Value::Bool(
                    !self.document.nodes[parent].children.is_empty(),
                ));
            }
            let child = self.node_argument(args.first())?;
            match method {
                "appendChild" => self.insert_before(parent, child, None)?,
                "insertBefore" => {
                    let reference = match args.get(1) {
                        Some(Value::Null | Value::Undefined) => None,
                        Some(value) => Some(self.node_argument(Some(value))?),
                        None => {
                            return Err(JsError::runtime(
                                "insertBefore requires a reference Node or null",
                            ));
                        }
                    };
                    self.insert_before(parent, child, reference)?;
                }
                "removeChild" => self.remove_child(parent, child)?,
                "replaceChild" => {
                    let old = self.node_argument(args.get(1))?;
                    self.replace_child(parent, child, old)?;
                    return Ok(Self::node_value(old));
                }
                _ => unreachable!(),
            }
            return Ok(Self::node_value(child));
        }
        if method == "contains" && (object == DOC || self.node(object).is_ok()) {
            let ancestor = if object == DOC { 0 } else { self.node(object)? };
            let target = args
                .first()
                .ok_or_else(|| JsError::runtime("contains requires a Node or null"))?;
            if matches!(target, Value::Null | Value::Undefined) {
                return Ok(Value::Bool(false));
            }
            let target = target
                .as_host()
                .ok_or_else(|| JsError::runtime("contains requires a Node or null"))?;
            let mut node = Some(if target == DOC { 0 } else { self.node(target)? });
            for _ in 0..=MAX_DEPTH {
                self.state.read_work = self.state.read_work.saturating_add(1);
                if self.state.read_work > 2_000_000 {
                    return Err(JsError::resource("DOM contains work budget exceeded"));
                }
                let Some(id) = node else {
                    return Ok(Value::Bool(false));
                };
                if id == ancestor {
                    return Ok(Value::Bool(true));
                }
                node = self.document.nodes[id].parent;
            }
            return Err(JsError::resource("DOM ancestor traversal depth exceeded"));
        }
        if method == "addEventListener"
            && (object == DOC || object == WINDOW || self.node(object).is_ok())
        {
            let value = args.get(2);
            if value.is_some_and(|value| {
                matches!(
                    value,
                    Value::Object(_)
                        | Value::Array(_)
                        | Value::Function(_)
                        | Value::Native(_)
                        | Value::Host(_)
                )
            }) {
                return Err(JsError::runtime(
                    "Listener dictionary requires the VM bridge",
                ));
            }
            return self.listener_call(
                object,
                method,
                args,
                ListenerOptions {
                    capture: value.is_some_and(Value::truthy),
                    ..ListenerOptions::default()
                },
            );
        }
        if method == "removeEventListener"
            && (object == DOC || object == WINDOW || self.node(object).is_ok())
        {
            let value = args.get(2);
            if value.is_some_and(|value| {
                matches!(
                    value,
                    Value::Object(_)
                        | Value::Array(_)
                        | Value::Function(_)
                        | Value::Native(_)
                        | Value::Host(_)
                )
            }) {
                return Err(JsError::runtime(
                    "Listener dictionary requires the VM bridge",
                ));
            }
            return self.listener_call(
                object,
                method,
                args,
                ListenerOptions {
                    capture: value.is_some_and(Value::truthy),
                    ..ListenerOptions::default()
                },
            );
        }
        if object == DOC {
            return match method {
                "getElementById" => {
                    let wanted = text(0)?;
                    Ok(connected(self.document, 0)
                        .into_iter()
                        .find(|&id| {
                            self.document
                                .element(id)
                                .is_some_and(|e| e.attr("id") == Some(wanted.as_str()))
                        })
                        .map_or(Value::Null, |id| Value::Host(NODE + id as u64)))
                }
                "querySelector" => {
                    Ok(
                        query(self.document, 0, &text(0)?, &mut self.state.read_work, true)?
                            .first()
                            .map_or(Value::Null, |&id| Value::Host(NODE + id as u64)),
                    )
                }
                "querySelectorAll" => {
                    let nodes = query(
                        self.document,
                        0,
                        &text(0)?,
                        &mut self.state.read_work,
                        false,
                    )?;
                    self.list(nodes)
                }
                "createElement" => {
                    let tag = text(0)?.to_ascii_lowercase();
                    if tag.is_empty()
                        || tag.len() > 64
                        || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                    {
                        return Err(JsError::runtime("Invalid element tag"));
                    }
                    self.mutate(tag.len())?;
                    Ok(Value::Host(
                        NODE + self.create(NodeKind::Element(Element {
                            tag,
                            attributes: BTreeMap::new(),
                        }))? as u64,
                    ))
                }
                "createTextNode" => {
                    let value = text(0)?;
                    self.mutate(value.len())?;
                    Ok(Value::Host(
                        NODE + self.create(NodeKind::Text(value))? as u64,
                    ))
                }
                _ => Err(JsError::runtime(format!(
                    "Unsupported document method {method}"
                ))),
            };
        }
        if (CLASS..LIST).contains(&object) {
            let id = self.node(NODE + object - CLASS)?;
            let current = self
                .document
                .element(id)
                .and_then(|e| e.attr("class"))
                .unwrap_or("");
            let mut classes = current
                .split_ascii_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            let name = text(0)?;
            if name.is_empty() || name.chars().any(char::is_whitespace) {
                return Err(JsError::runtime("classList requires a single token"));
            }
            let contains = classes.contains(&name);
            match method {
                "contains" => return Ok(Value::Bool(contains)),
                "add" => {
                    if !contains {
                        classes.push(name);
                    }
                }
                "remove" => classes.retain(|class| class != &name),
                "toggle" => {
                    let enable = args.get(1).map_or(!contains, Value::truthy);
                    classes.retain(|class| class != &name);
                    if enable {
                        classes.push(name);
                    }
                    self.set_attribute(id, "class".into(), classes.join(" "))?;
                    return Ok(Value::Bool(enable));
                }
                _ => return Err(JsError::runtime("Unsupported classList method")),
            }
            self.set_attribute(id, "class".into(), classes.join(" "))?;
            return Ok(Value::Undefined);
        }
        let id = self.node(object)?;
        match method {
            "cloneNode" => {
                let cloned = self.clone_node(id, args.first().is_some_and(Value::truthy))?;
                Ok(Self::node_value(cloned))
            }
            "setAttribute" => {
                self.set_attribute(id, text(0)?.to_ascii_lowercase(), text(1)?)?;
                Ok(Value::Undefined)
            }
            "getAttribute" | "hasAttribute" => {
                let name = text(0)?.to_ascii_lowercase();
                let element = self
                    .document
                    .element(id)
                    .ok_or_else(|| JsError::runtime("Attribute methods require an element"))?;
                Ok(if method == "hasAttribute" {
                    Value::Bool(element.attributes.contains_key(&name))
                } else {
                    element.attr(&name).map_or(Value::Null, Value::string)
                })
            }
            "removeAttribute" => {
                let key = text(0)?.to_ascii_lowercase();
                if self.document.element(id).is_none() {
                    return Err(JsError::runtime("Attribute methods require an element"));
                }
                self.mutate(0)?;
                if key.eq_ignore_ascii_case("src") {
                    self.state.media.nodes.remove(&id);
                }
                if let NodeKind::Element(element) = &mut self.document.nodes[id].kind {
                    element.attributes.remove(&key);
                }
                Ok(Value::Undefined)
            }
            "matches" | "closest" => {
                if self.document.element(id).is_none() {
                    return Err(JsError::runtime("Selector methods require an element"));
                }
                let source = text(0)?;
                let matched = selector_ancestor(
                    self.document,
                    id,
                    &source,
                    &mut self.state.read_work,
                    method == "matches",
                )?;
                Ok(if method == "matches" {
                    Value::Bool(matched.is_some())
                } else {
                    matched.map_or(Value::Null, Self::node_value)
                })
            }
            "remove" => {
                self.mutate(0)?;
                self.detach(id);
                Ok(Value::Undefined)
            }
            "querySelector" => Ok(query(
                self.document,
                id,
                &text(0)?,
                &mut self.state.read_work,
                true,
            )?
            .first()
            .map_or(Value::Null, |&id| Value::Host(NODE + id as u64))),
            "querySelectorAll" => {
                let nodes = query(
                    self.document,
                    id,
                    &text(0)?,
                    &mut self.state.read_work,
                    false,
                )?;
                self.list(nodes)
            }
            _ => Err(JsError::runtime(format!("Unsupported DOM method {method}"))),
        }
    }
}
fn number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        Value::Bool(v) => {
            if *v {
                1.
            } else {
                0.
            }
        }
        Value::Null => 0.,
        Value::String(s) => {
            if s.trim().is_empty() {
                0.
            } else {
                s.trim().parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}
fn connected(document: &Document, root: NodeId) -> Vec<NodeId> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        result.push(id);
        if document
            .element(id)
            .is_some_and(|element| matches!(element.tag.as_str(), "template" | "noscript"))
        {
            continue;
        }
        pending.extend(document.nodes[id].children.iter().rev().copied());
    }
    result
}
const SELECTOR_READ_LIMIT: usize = 2_000_000;

fn selector_error(error: web_css::selector_query::SelectorQueryError) -> JsError {
    use web_css::selector_query::SelectorQueryError;
    match error {
        SelectorQueryError::SyntaxOrUnsupported => {
            JsError::runtime("Invalid DOM selector syntax or unsupported selector feature")
        }
        SelectorQueryError::Resource => JsError::resource("DOM selector work budget exceeded"),
    }
}

// CSS counts down while DOM counts up. Transfer consumption even when parsing,
// indexing, traversal or matching fails; never replenish work per candidate.
fn with_selector<T>(
    document: &Document,
    source: &str,
    work: &mut usize,
    operation: impl FnOnce(
        &mut web_css::selector_query::SelectorMatchContext<'_>,
        &web_css::selector_query::SelectorList,
    ) -> Result<T, JsError>,
) -> Result<T, JsError> {
    use web_css::selector_query::{SelectorList, SelectorMatchContext};
    let mut context =
        SelectorMatchContext::new(document, SELECTOR_READ_LIMIT.saturating_sub(*work));
    let result = (|| {
        context
            .charge_work(source.len().saturating_add(1))
            .map_err(selector_error)?;
        let selectors = SelectorList::parse(source).map_err(selector_error)?;
        operation(&mut context, &selectors)
    })();
    *work = work.saturating_add(context.work_used());
    result
}

fn selector_ancestor(
    document: &Document,
    id: NodeId,
    source: &str,
    work: &mut usize,
    only_self: bool,
) -> Result<Option<NodeId>, JsError> {
    with_selector(document, source, work, |context, selectors| {
        let mut node = Some(id);
        for _ in 0..=MAX_DEPTH {
            let Some(current) = node else {
                return Ok(None);
            };
            context.charge_work(1).map_err(selector_error)?;
            if context
                .matches(selectors, current)
                .map_err(selector_error)?
            {
                return Ok(Some(current));
            }
            if only_self {
                return Ok(None);
            }
            node = document.nodes[current].parent;
        }
        Err(JsError::resource("DOM ancestor traversal depth exceeded"))
    })
}

fn query(
    document: &Document,
    root: NodeId,
    source: &str,
    work: &mut usize,
    first_only: bool,
) -> Result<Vec<NodeId>, JsError> {
    with_selector(document, source, work, |context, selectors| {
        if document.nodes.len() > MAX_NODES || root >= document.nodes.len() {
            return Err(JsError::resource("DOM selector node limit exceeded"));
        }
        context.charge_work(1).map_err(selector_error)?;
        let mut result = Vec::new();
        // Frames contain only the current ancestry, not all candidate nodes.
        let mut pending = vec![(root, 0usize)];
        while let Some((parent, next_child)) = pending.last_mut() {
            if document
                .element(*parent)
                .is_some_and(|element| matches!(element.tag.as_str(), "template" | "noscript"))
                || *next_child >= document.nodes[*parent].children.len()
            {
                pending.pop();
                continue;
            }
            context.charge_work(1).map_err(selector_error)?;
            let id = document.nodes[*parent].children[*next_child];
            *next_child += 1;
            if pending.len() > MAX_DEPTH {
                return Err(JsError::resource("DOM selector traversal depth exceeded"));
            }
            if context.matches(selectors, id).map_err(selector_error)? {
                result.push(id);
                if first_only {
                    return Ok(result);
                }
            }
            pending.push((id, 0));
        }
        Ok(result)
    })
}
fn css_property(property: &str) -> Result<String, JsError> {
    if property.is_empty()
        || property.len() > 80
        || !property
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(JsError::runtime("Unsupported style property name"));
    }
    let mut key = String::new();
    for ch in property.chars() {
        if ch.is_ascii_uppercase() {
            key.push('-');
            key.push(ch.to_ascii_lowercase());
        } else {
            key.push(ch);
        }
    }
    Ok(key)
}
fn compact(document: &Document) -> Document {
    let reachable = connected(document, 0);
    let mut mapping = BTreeMap::new();
    let mut output = Document {
        nodes: vec![],
        mode: document.mode,
    };
    for &id in &reachable {
        mapping.insert(id, output.nodes.len());
        output.nodes.push(Node {
            parent: None,
            children: vec![],
            kind: document.nodes[id].kind.clone(),
        });
    }
    for &id in &reachable {
        let next = mapping[&id];
        output.nodes[next].parent = document.nodes[id]
            .parent
            .and_then(|parent| mapping.get(&parent).copied());
        output.nodes[next].children = document.nodes[id]
            .children
            .iter()
            .filter_map(|child| mapping.get(child).copied())
            .collect();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_webassembly_exports_update_dom_from_snapshotted_bytes() {
        let mut session = DocumentSession::new(web_html::parse("<p id=x></p>").unwrap());
        // Core module: (func (export "add") (param i32 i32) (result i32)
        //   local.get 0; local.get 1; i32.add). This is binary source, not WAT.
        session.execute("const bytes=new Uint8Array([0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11]);const valid=WebAssembly.validate(bytes);const module=new WebAssembly.Module(bytes);const instance=new WebAssembly.Instance(module);bytes[0]=255;document.getElementById('x').textContent=valid+':'+instance.exports.add(40,2)+':'+(instance.exports===instance.exports)+':'+typeof instance.exports.add;").unwrap();
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "true:42:true:function");
    }
    #[test]
    fn webassembly_fuel_exhaustion_rolls_back_dom_and_vm_then_disables_scripts() {
        let mut session = DocumentSession::new(web_html::parse("<p id=x>safe</p>").unwrap());
        // Core module: (func (export "spin") (loop (br 0))). No state or imports.
        session.execute("let changed=0;const bytes=new Uint8Array([0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,7,8,1,4,115,112,105,110,0,0,10,9,1,7,0,3,64,12,0,11,11]);const spinning=new WebAssembly.Instance(new WebAssembly.Module(bytes));").unwrap();
        let error = session.execute("changed=1;document.getElementById('x').textContent='bad';try{spinning.exports.spin();}catch(e){document.getElementById('x').textContent='caught';}").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "safe");
        assert_eq!(
            session.runtime.global_value("changed"),
            Some(Value::Number(0.))
        );
        assert!(session.scripts_disabled);
        assert_eq!(session.execute("1;").unwrap_err().kind, ErrorKind::Resource);
    }
    #[test]
    fn suspended_delete_skips_getters_and_preserves_array_holes_for_dom() {
        let mut session = DocumentSession::new(web_html::parse("<p id=x></p>").unwrap());
        session.execute("async function run(){let gets=0;const object={};Object.defineProperty(object,'value',{get:function(){gets++;return 7;},configurable:true});const removed=delete (await Promise.resolve(object))[await Promise.resolve('value')];const values=[1,2];const hole=delete (await Promise.resolve(values))[await Promise.resolve(0)];document.getElementById('x').textContent=removed+':'+gets+':'+hole+':'+(0 in values)+':'+values.length;}run();").unwrap();
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "true:0:true:false:2");
    }
    #[test]
    fn suspended_assignment_and_update_preserve_dom_writes_and_numeric_results() {
        let mut session = DocumentSession::new(web_html::parse("<p id=x></p>").unwrap());
        session.execute("async function run(){const out=document.getElementById('x');(await Promise.resolve(out))[await Promise.resolve('textContent')]=await Promise.resolve('assigned');(await Promise.resolve(out))[await Promise.resolve('textContent')]+=await Promise.resolve(':compound');const value={n:4};const previous=(await Promise.resolve(value))[await Promise.resolve('n')]++;const next=++(await Promise.resolve(value))[await Promise.resolve('n')];out.textContent+=':'+previous+':'+next;}run();").unwrap();
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "assigned:compound:4:6");
    }
    #[test]
    fn suspended_member_reads_and_callees_preserve_dom_dispatch_and_receivers() {
        let mut session = DocumentSession::new(web_html::parse("<p id=x></p>").unwrap());
        session.execute("let order='';const object={n:40};Object.defineProperty(object,'method',{get:function(){order+='g';return function(value){return this.n+value;};}});function key(){order+='k';return 'method';}function argument(){order+='a';return 2;}async function run(){const out=(await Promise.resolve(document))[await Promise.resolve('getElementById')]('x');const number=(await Promise.resolve(object))[await key()](await argument());const suffix=(await Promise.resolve(function(value){return value+1;}))(await Promise.resolve(2));out.textContent=number+':'+suffix+':'+order;const current=(await Promise.resolve(out))[await Promise.resolve('textContent')];out.textContent=current+':read';}run();").unwrap();
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "42:3:kga:read");
    }
    #[test]
    fn suspended_array_and_template_preserve_holes_conversion_order_and_utf16() {
        let mut session = DocumentSession::new(
            web_html::parse("<!doctype html><body><p id=result>old</p></body>").unwrap(),
        );
        session.execute(r"let calls=0;const marker={toString:function(){calls++;return String.fromCharCode(55296);}};async function show(){const values=[await Promise.resolve(1),,...(await Promise.resolve([2]))];const text=`${await marker}:${await Promise.resolve(calls)}`;document.getElementById('result').textContent=values.length+':'+(1 in values)+':'+text.charCodeAt(0)+':'+text.charAt(2);}show();").unwrap();
        let result = session.query_selector("#result").unwrap().unwrap();
        assert_eq!(session.document.text_content(result), "3:false:55296:1");
    }
    #[test]
    fn async_delegation_preserves_direct_values_and_adapts_sync_values_for_dom() {
        let mut session = DocumentSession::new(
            web_html::parse("<!doctype html><body><p id=result>old</p></body>").unwrap(),
        );
        session.execute("let promised=Promise.resolve(5);let calls=0;let direct={next:function(){calls++;return {done:calls>1,value:promised};}};direct[Symbol.asyncIterator]=function(){return this;};async function* source(){yield* direct;yield* [promised];}async function consume(){let out='';for await(const value of source()){out+=(value===promised)+':'+(await value)+';';}document.getElementById('result').textContent=out;}consume();").unwrap();
        let result = session.query_selector("#result").unwrap().unwrap();
        assert_eq!(session.document.text_content(result), "true:5;false:5;");
    }
    #[test]
    fn async_generators_feed_for_await_without_replaying_dom_effects() {
        let mut session = DocumentSession::new(
            web_html::parse("<!doctype html><body><p id=result>old</p></body>").unwrap(),
        );
        session.execute("let steps=0;let closed=0;async function* source(){try{steps++;yield Promise.resolve(2);steps++;yield 3;}finally{await 0;closed++;}}async function consume(){let total=0;for await(const value of source()){total+=value;break;}document.getElementById('result').textContent=total+':'+steps+':'+closed;}consume();").unwrap();
        let result = session.query_selector("#result").unwrap().unwrap();
        assert_eq!(session.document.text_content(result), "2:1:1");
    }
    #[test]
    fn vm_math_namespace_is_shared_with_window_and_bypasses_legacy_host_math() {
        let mut session =
            DocumentSession::new(web_html::parse("<!doctype html><body></body>").unwrap());
        assert_eq!(session.execute("var Math;var Reflect;Math===window.Math && typeof Math==='object' && typeof Reflect==='object' && Reflect===window.Reflect && Math.hypot(3,4)===5 && Math.imul(4294967295,5)===-5 && Object.is(Math.round(-0.5),-0) && typeof Math.random==='undefined';").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("let calls=0;const operand={valueOf(){calls++;return -7;}};window.Math.abs(operand)===7 && calls===1 && Object.keys(Math).length===0;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("const namespace=Math;window.Math={replacement:true};Math===window.Math && Math.replacement && namespace.sqrt(9)===3;").unwrap(), Value::Bool(true));
    }
    #[test]
    fn numeric_globals_precede_host_legacy_values_and_keep_number_aliases() {
        let mut session =
            DocumentSession::new(web_html::parse("<!doctype html><body></body>").unwrap());
        assert_eq!(session.execute("let order='';const text={toString(){order+='s';return 'ff';}};const radix={valueOf(){order+='r';return 16;}};parseInt(text,radix)===255&&order==='sr'&&Number.parseInt===window.parseInt&&Number.parseFloat===window.parseFloat&&window.isFinite('2')&&!Number.isFinite('2')&&parseFloat('Infinitytail')===Infinity;").unwrap(), Value::Bool(true));
    }
    #[test]
    fn document_metadata_is_truthful_and_survives_compaction() {
        for (source, expected) in [
            ("<!doctype html><body></body>", "CSS1Compat"),
            ("<body></body>", "BackCompat"),
        ] {
            let mut document = web_html::parse(source).unwrap();
            let mode = document.mode;
            let mut session = DocumentSession::new(document.clone());
            assert_eq!(
                session.execute("document.compatMode;").unwrap(),
                Value::string(expected)
            );
            assert_eq!(
                session.execute("document.prerendering;").unwrap(),
                Value::Bool(false)
            );
            assert!(session.execute("document.hidden;").is_err());
            assert!(session.execute("document.visibilityState;").is_err());
            execute_scripts(&mut document, &[]);
            assert_eq!(document.mode, mode);
            assert_eq!(document.mode.compat_mode(), Some(expected));
        }
        let document =
            web_html::parse("<!doctype html PUBLIC 'unclassified legacy'><body></body>").unwrap();
        assert!(
            DocumentSession::new(document)
                .execute("document.compatMode;")
                .unwrap_err()
                .message
                .contains("classification")
        );
    }
    #[test]
    fn node_metadata_containment_attributes_and_simple_selectors_are_real() {
        let mut session = DocumentSession::new(web_html::parse("<!doctype html><body id=root><section id=outer class=group><p id=leaf class='active copy'>Hello</p></section></body>").unwrap());
        assert_eq!(session.execute("const root=document.getElementById('root');const outer=document.getElementById('outer');const leaf=document.getElementById('leaf');const text=leaf.childNodes[0];document.nodeType===9 && document.nodeName==='#document' && document.ownerDocument===null && root.parentNode===document.documentElement && root.parentElement===document.documentElement && document.documentElement.parentNode===document && document.documentElement.parentElement===null && leaf.nodeType===1 && leaf.nodeName==='P' && leaf.ownerDocument===document && text.nodeType===3 && text.nodeName==='#text' && text.ownerDocument===document && text.parentElement===leaf;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("document.contains(leaf) && root.contains(leaf) && leaf.contains(text) && leaf.contains(leaf) && !leaf.contains(root) && !document.contains(null) && !root.contains(undefined);").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("leaf.matches('P') && leaf.matches('.active') && leaf.matches('#leaf') && leaf.closest('.active')===leaf && leaf.closest('.group')===outer && leaf.closest('aside')===null;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("leaf.setAttribute('DATA-TEST','yes');leaf.hasAttribute('DATA-TEST') && leaf.getAttribute('DATA-test')==='yes';").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("leaf.removeAttribute('DATA-TEST');!leaf.hasAttribute('data-test') && leaf.getAttribute('data-test')===null;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("outer.remove();!document.contains(leaf) && outer.contains(leaf) && leaf.closest('.group')===outer && outer.parentNode===null && outer.ownerDocument===document;").unwrap(), Value::Bool(true));
        assert_eq!(
            session
                .execute("leaf.matches('p.active') && leaf.closest('body > p')===null;")
                .unwrap(),
            Value::Bool(true)
        );
        for source in [
            "leaf.matches('');",
            "leaf.matches('#123');",
            "text.hasAttribute('x');",
            "text.matches('*');",
            "root.contains(42);",
            "root.contains();",
            "leaf.getAttribute();",
        ] {
            assert!(session.execute(source).is_err(), "{source}");
        }
    }
    #[test]
    fn property_existence_does_not_confuse_null_undefined_or_unsupported_apis() {
        let mut session =
            DocumentSession::new(web_html::parse("<!doctype html><p id=leaf>text</p>").unwrap());
        assert_eq!(session.execute("const leaf=document.getElementById('leaf');'head' in document && document.head.nodeName==='HEAD' && document.head.parentNode===document.documentElement && 'compatMode' in document && 'prerendering' in document && !('hidden' in document) && !('visibilityState' in document) && 'ownerDocument' in leaf && 'hasAttribute' in leaf && !('querySelectorAllUnknown' in leaf) && !('fetch' in window) && 'addEventListener' in window;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("const nodes=document.querySelectorAll('p');'0' in nodes && !('00' in nodes) && !('1' in nodes) && 'length' in nodes;").unwrap(), Value::Bool(true));
        assert!(session.execute("'display' in leaf.style;").is_err());
    }
    #[test]
    fn selector_text_and_ancestor_traversal_consume_shared_budget() {
        let mut document = web_html::parse("<main><p id=x class=small>Text</p></main>").unwrap();
        let node = document
            .elements()
            .find(|(_, element)| element.attr("id") == Some("x"))
            .unwrap()
            .0;
        if let NodeKind::Element(element) = &mut document.nodes[node].kind {
            element
                .attributes
                .insert("class".into(), "a".repeat(1_000_000));
        }
        let mut session = DocumentSession::new(document);
        assert_eq!(session.execute("const x=document.getElementById('x');x.matches('.missing');x.matches('.missing');").unwrap_err().kind, ErrorKind::Resource);
        assert_eq!(session.execute("1;").unwrap_err().kind, ErrorKind::Resource);
    }
    #[test]
    fn exhausted_callbacks_disable_remaining_tasks_without_busy_polling() {
        let mut session = DocumentSession::new(web_html::parse("<p>Preservado</p>").unwrap());
        session
            .execute("setTimeout(() => { while(true) {} }, 1); setTimeout(() => {}, 2);")
            .unwrap();
        assert!(session.next_timer_ms().is_some());
        assert_eq!(
            session.run_due_timers(1).unwrap_err().kind,
            ErrorKind::Resource
        );
        assert_eq!(session.next_timer_ms(), None);
        assert!(session.run_due_timers(100).is_ok());
        assert_eq!(
            session.execute("1+1;").unwrap_err().kind,
            ErrorKind::Resource
        );
    }
    #[test]
    fn scripts_modify_dom_and_dom_content_loaded_runs_after_scripts() {
        let document=web_html::parse("<body><h1 id=title>Before</h1><script>const heading=document.getElementById('title');heading.textContent='Ação';document.addEventListener('DOMContentLoaded',()=>{heading.textContent+=' local';document.title='Ready';});</script></body>").unwrap();
        let mut session = DocumentSession::new(document);
        let report = session.run_initial(&[]);
        assert_eq!(report.executed, 1);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let id = session.query_selector("#title").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "Ação local");
        assert_eq!(session.document.title(), "Ready");
    }
    #[test]
    fn noscript_and_template_scripts_are_inert_with_scripting_enabled() {
        let document=web_html::parse("<noscript><title>Wrong title</title><p id=inactive>Hidden</p><script>document.title='Wrong';</script></noscript><template id=template><script>document.title='Wrong too';</script></template><script>document.title='Active';</script>").unwrap();
        let mut session = DocumentSession::new(document);
        let report = session.run_initial(&[]);
        assert_eq!(report.executed, 1);
        assert_eq!(session.document.title(), "Active");
        assert_eq!(session.query_selector("#inactive").unwrap(), None);
        assert!(session.query_selector("#template").unwrap().is_some());
    }
    #[test]
    fn normalized_document_getters_follow_structure_after_mutation() {
        for source in [
            "",
            "text",
            "<p>text</p>",
            "<html><body><p>text</p></body></html>",
        ] {
            let mut session = DocumentSession::new(web_html::parse(source).unwrap());
            assert_eq!(session.execute("document.documentElement.nodeName==='HTML' && document.documentElement.parentNode===document && document.head.nodeName==='HEAD' && document.body.nodeName==='BODY' && document.head.parentNode===document.documentElement && document.body.parentNode===document.documentElement && document.childElementCount===1;").unwrap(), Value::Bool(true));
        }
        let mut session = DocumentSession::new(web_html::parse("").unwrap());
        assert_eq!(session.execute("const html=document.documentElement;const head=document.head;const body=document.body;body.appendChild(head);document.head===null && head.isConnected && document.body===body;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("html.insertBefore(head,body);head.appendChild(body);document.head===head && document.body===null && body.isConnected;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("html.appendChild(body);body.remove();document.body===null && !body.isConnected && document.head===head;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("html.remove();document.documentElement===null && document.head===null && document.body===null;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("const replacement=document.createElement('main');document.appendChild(replacement);document.documentElement===replacement && document.head===null && document.body===null;").unwrap(), Value::Bool(true));
    }
    #[test]
    fn document_title_has_real_empty_value_and_creates_only_under_head() {
        let mut session = DocumentSession::new(web_html::parse("").unwrap());
        assert_eq!(
            session.execute("document.title;").unwrap(),
            Value::string("")
        );
        assert_eq!(session.execute("document.title='First';document.head.firstChild.nodeName==='TITLE' && document.head.firstChild.parentNode===document.head && document.childElementCount===1 && document.title==='First';").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("document.title='';document.title==='' && document.head.firstChild.childNodes.length===0;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("document.head.remove();document.title='Ignored';document.head===null && document.title==='' && document.documentElement.childElementCount===1;").unwrap(), Value::Bool(true));
    }
    #[test]
    fn document_title_uses_tree_order_and_skips_inert_titles() {
        let mut session =
            DocumentSession::new(web_html::parse("<title>Original</title><body></body>").unwrap());
        assert_eq!(session.execute("const old=document.head.firstChild;const newer=document.createElement('title');newer.textContent='New';document.head.insertBefore(newer,old);document.title==='New';").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("const inert=document.createElement('template');document.head.insertBefore(inert,newer);inert.appendChild(newer);document.title==='Original';").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("document.title='Updated';old.textContent==='Updated' && newer.textContent==='New';").unwrap(), Value::Bool(true));
        assert_eq!(
            session
                .execute("old.textContent='  A\\tB\\nC  ';document.title;")
                .unwrap(),
            Value::string("A B C")
        );
        assert_eq!(
            session
                .execute("old.textContent='\\u00a0A\\u00a0';document.title;")
                .unwrap(),
            Value::string("\u{a0}A\u{a0}")
        );
    }
    #[test]
    fn inner_html_imports_only_fragment_children_without_changing_mode() {
        let mut session = DocumentSession::new(
            web_html::parse("<!doctype html><div id=root><p>Old</p></div>").unwrap(),
        );
        let mode = session.document.mode;
        assert_eq!(session.execute("const root=document.getElementById('root');const old=root.firstChild;root.innerHTML='<!doctype broken><span>New</span><em>Second</em>';root.childElementCount===2 && root.firstChild.nodeName==='SPAN' && root.lastChild.nodeName==='EM' && root.firstChild.parentNode===root && old.parentNode===null && document.querySelectorAll('html').length===1 && document.querySelectorAll('head').length===1 && document.querySelectorAll('body').length===1;").unwrap(), Value::Bool(true));
        assert_eq!(session.document.mode, mode);
        assert_eq!(
            session
                .execute("root.innerHTML='';root.childNodes.length===0;")
                .unwrap(),
            Value::Bool(true)
        );
    }
    #[test]
    fn inner_html_uses_raw_text_and_rcdata_contexts_without_running_scripts() {
        for (tag, expected) in [
            ("script", "<b>&amp;</b>"),
            ("style", "<b>&amp;</b>"),
            ("noscript", "<b>&amp;</b>"),
            ("textarea", "<b>&</b>"),
            ("title", "<b>&</b>"),
        ] {
            let mut session = DocumentSession::new(web_html::parse("").unwrap());
            session.execute(&format!("const target=document.createElement('{tag}');document.body.appendChild(target);target.innerHTML='<b>&amp;</b>'; ")).unwrap();
            assert_eq!(
                session.execute("target.textContent;").unwrap(),
                Value::string(expected)
            );
            assert_eq!(session.execute("target.childNodes.length===1 && target.firstChild.nodeType===3 && target.childElementCount===0;").unwrap(), Value::Bool(true));
        }
        let mut session = DocumentSession::new(web_html::parse("<div id=root></div>").unwrap());
        assert_eq!(session.execute("let ran=false;document.getElementById('root').innerHTML='<script>ran=true;</script><template><script>ran=true;</script></template><noscript><img src=never></noscript>';ran===false && document.querySelectorAll('img').length===0;").unwrap(), Value::Bool(true));
    }
    #[test]
    fn malformed_template_end_tag_does_not_activate_initial_scripts() {
        let document = web_html::parse(
            "<body><template></body><script>document.title='Must not run';</script></template>",
        )
        .unwrap();
        let mut session = DocumentSession::new(document);
        let report = session.run_initial(&[]);
        assert_eq!(report.executed, 0);
        assert_eq!(session.document.document_title(), "");
        assert!(
            session
                .document
                .elements()
                .filter(|(_, element)| element.tag == "script")
                .all(|(id, _)| session.document.is_inactive_subtree(id))
        );
    }
    #[test]
    fn unsupported_fragment_context_is_an_ordinary_atomic_error() {
        for tag in [
            "table",
            "tbody",
            "thead",
            "tfoot",
            "tr",
            "td",
            "th",
            "caption",
            "colgroup",
            "select",
            "optgroup",
            "option",
            "svg",
            "math",
            "foreignobject",
        ] {
            let mut session = DocumentSession::new(web_html::parse("").unwrap());
            session.execute(&format!("const target=document.createElement('{tag}');target.textContent='Preserved';document.body.appendChild(target);const child=target.firstChild;")).unwrap();
            let before = format!("{:?}", session.document.nodes);
            let error = session
                .execute("target.innerHTML='<b>Replacement</b>';")
                .unwrap_err();
            assert_ne!(error.kind, ErrorKind::Resource, "{tag}");
            assert_eq!(format!("{:?}", session.document.nodes), before, "{tag}");
            assert_eq!(session.execute("target.firstChild===child && child.parentNode===target && target.textContent==='Preserved';").unwrap(), Value::Bool(true));
        }
    }
    #[test]
    fn fragment_import_preflights_all_budgets_without_session_rollback() {
        for exhausted in ["mutations", "strings", "reads", "nodes", "depth"] {
            let mut document = web_html::parse("<div id=target><p>Preserved</p></div>").unwrap();
            let mut target = document
                .elements()
                .find(|(_, e)| e.attr("id") == Some("target"))
                .unwrap()
                .0;
            let mut state = State::default();
            match exhausted {
                "mutations" => state.mutations = 2048,
                "strings" => state.string_bytes = 256 * 1024 - 1,
                "reads" => state.read_work = 2_000_000 - 1,
                "nodes" => {
                    while document.nodes.len() < MAX_NODES {
                        document.nodes.push(Node {
                            parent: None,
                            children: Vec::new(),
                            kind: NodeKind::Text(String::new()),
                        });
                    }
                }
                "depth" => {
                    // target starts at depth 3 (document/html/body/div).
                    for _ in 4..MAX_DEPTH - 1 {
                        target = document
                            .append(
                                target,
                                NodeKind::Element(Element {
                                    tag: "div".into(),
                                    attributes: BTreeMap::new(),
                                }),
                            )
                            .unwrap();
                    }
                    document
                        .append(target, NodeKind::Text("Preserved".into()))
                        .unwrap();
                }
                _ => unreachable!(),
            }
            let before = format!("{:?}", document.nodes);
            let fragment = web_html::parse_fragment("<span>Replacement</span>", "div").unwrap();
            let error = DomHost {
                document: &mut document,
                state: &mut state,
            }
            .import_fragment(target, &fragment, true)
            .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource, "{exhausted}");
            assert_eq!(format!("{:?}", document.nodes), before, "{exhausted}");
        }
    }
    #[test]
    fn inner_html_resource_failure_preserves_document_and_disables_session() {
        for html in [
            "x".repeat(65_537),
            "<i>x</i>".repeat(1100),
            "<div>".repeat(MAX_DEPTH + 1),
        ] {
            let mut session =
                DocumentSession::new(web_html::parse("<div id=target>Preserved</div>").unwrap());
            let before = format!("{:?}", session.document.nodes);
            let error = session.execute(&format!("document.title='Rolled back';document.getElementById('target').innerHTML='{html}';")).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Resource);
            assert_eq!(format!("{:?}", session.document.nodes), before);
            assert_eq!(session.execute("1;").unwrap_err().kind, ErrorKind::Resource);
        }
    }
    #[test]
    fn mutation_create_append_style_class_and_inner_html() {
        let document = web_html::parse("<body><div id=root></div></body>").unwrap();
        let mut session = DocumentSession::new(document);
        session.execute("const root=document.querySelector('#root');let p=document.createElement('p');p.textContent='Created';p.classList.add('active');p.style.color='#123456';root.appendChild(p);root.innerHTML='<span id=child>Imported</span>';").unwrap();
        let root = session.query_selector("#root").unwrap().unwrap();
        assert_eq!(session.document.text_content(root), "Imported");
        assert!(session.query_selector(".active").unwrap().is_none());
        assert!(session.query_selector("#child").unwrap().is_some());
    }
    #[test]
    fn ordinary_exception_preserves_prior_effects_and_invalid_mutation_is_rejected() {
        let mut session = DocumentSession::new(web_html::parse("<p id=x>Safe</p>").unwrap());
        assert!(
            session
                .execute(
                    "let changed=1;document.querySelector('#x').textContent='Changed';throw 'stop';"
                )
                .is_err()
        );
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "Changed");
        assert_eq!(session.execute("changed;").unwrap(), Value::Number(1.));
        assert!(
            session
                .execute("document.querySelector('#x').appendChild(document.querySelector('#x'));")
                .is_err()
        );
        assert_eq!(session.document.text_content(id), "Changed");
    }
    #[test]
    fn retained_session_dispatches_real_click_and_bounded_timer_tasks() {
        let mut session = DocumentSession::new(web_html::parse("<button id=b>0</button>").unwrap());
        session.execute("let count=0;const button=document.getElementById('b');button.addEventListener('click',()=>{count++;button.textContent=String(count);});setTimeout(()=>{button.textContent='Later';},50);").unwrap();
        let id = session.query_selector("#b").unwrap().unwrap();
        session.dispatch_event(id, "click").unwrap();
        assert_eq!(session.document.text_content(id), "1");
        session.run_due_timers(49).unwrap();
        assert_eq!(session.document.text_content(id), "1");
        session.run_due_timers(1).unwrap();
        assert_eq!(session.document.text_content(id), "Later");
    }
    #[test]
    fn external_sources_keep_dom_order_and_modules_are_diagnosed() {
        let document=web_html::parse("<p id=x>Old</p><script src='first.js'></script><script>document.getElementById('x').textContent+=suffix;</script><script type=module>import x from 'missing';</script>").unwrap();
        let mut session = DocumentSession::new(document);
        let report = session.run_initial(&[ScriptAsset {
            src: "first.js".into(),
            source: "let suffix=' done';document.getElementById('x').textContent='First';".into(),
        }]);
        assert_eq!(report.executed, 2);
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "First done");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("broker-authorized document URL"))
        );
    }
    #[test]
    fn static_modules_defer_and_share_live_dependency_bindings() {
        let document=web_html::parse("<p id=x>old</p><script type=module src='./entry.mjs'></script><script>var order='classic';document.addEventListener('DOMContentLoaded',()=>{document.getElementById('x').textContent+=' ready';});</script><script nomodule>throw 'must be skipped';</script>").unwrap();
        let mut session = DocumentSession::new(document);
        session
            .set_document_url("https://example.test/page.html")
            .unwrap();
        let report=session.run_initial(&[
            ScriptAsset {src:"noir-module:https://example.test/entry.mjs".into(),source:"import {count,bump} from './dep.mjs'; bump(); document.getElementById('x').textContent=order+' '+count;".into()},
            ScriptAsset {src:"noir-module:https://example.test/dep.mjs".into(),source:"export let count=1;export function bump(){count++;}".into()},
        ]);
        assert_eq!(report.executed, 2);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "classic 2 ready");
        assert_eq!(
            session.execute("typeof count;").unwrap(),
            Value::string("undefined")
        );
    }
    #[test]
    fn import_meta_urls_distinguish_inline_identity_from_document_and_external_source() {
        let document = web_html::parse("<p id=x></p><script type=module>document.getElementById('x').textContent=import.meta.url;</script><script type=module src='./entry.mjs'></script><script type=module>document.getElementById('x').textContent+=' '+import.meta.url;</script>").unwrap();
        let mut session = DocumentSession::new(document);
        session
            .set_document_url("https://example.test/dir/page.html#original")
            .unwrap();
        let report = session.run_initial(&[ScriptAsset {
            src: "noir-module:https://example.test/dir/entry.mjs".into(),
            source: "document.getElementById('x').textContent+=' '+import.meta.url;".into(),
        }]);
        assert_eq!(report.executed, 3);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(
            session.document.text_content(id),
            "https://example.test/dir/page.html#original https://example.test/dir/entry.mjs https://example.test/dir/page.html#original"
        );
    }
    #[test]
    fn dynamic_import_reuses_registered_module_graph_without_loading_classic_assets() {
        let document = web_html::parse(
            "<p id=x>old</p><p id=error></p><script type=module src='./entry.mjs'></script>",
        )
        .unwrap();
        let mut session = DocumentSession::new(document);
        session
            .set_document_url("https://example.test/page.html")
            .unwrap();
        let report=session.run_initial(&[
            ScriptAsset {src:"noir-module:https://example.test/entry.mjs".into(),source:"import {count,bump} from './dep.mjs'; bump(); import('./dep.mjs').then(ns=>{document.getElementById('x').textContent='dynamic '+ns.count;}); import('./classic.mjs').catch(()=>{document.getElementById('error').textContent='unavailable';});".into()},
            ScriptAsset {src:"noir-module:https://example.test/dep.mjs".into(),source:"export let count=1;export function bump(){count++;}".into()},
            ScriptAsset {src:"https://example.test/classic.mjs".into(),source:"document.getElementById('x').textContent='unauthorized';".into()},
        ]);
        assert_eq!(report.executed, 1);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "dynamic 2");
        let error = session.query_selector("#error").unwrap().unwrap();
        assert_eq!(session.document.text_content(error), "unavailable");
    }
    #[test]
    fn modules_never_consume_classic_assets_or_cross_origin_bytes() {
        let document=web_html::parse("<p id=x>safe</p><script type=module src='./entry.mjs'></script><script type=module src='https://other.test/entry.mjs'></script>").unwrap();
        let mut session = DocumentSession::new(document);
        session
            .set_document_url("https://example.test/page.html")
            .unwrap();
        let report = session.run_initial(&[
            ScriptAsset {
                src: "https://example.test/entry.mjs".into(),
                source: "document.getElementById('x').textContent='bad';".into(),
            },
            ScriptAsset {
                src: "noir-module:https://other.test/entry.mjs".into(),
                source: "document.getElementById('x').textContent='bad';".into(),
            },
        ]);
        assert_eq!(report.executed, 0);
        assert_eq!(report.warnings.len(), 2);
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "safe");
    }
    #[test]
    fn marked_module_asset_cannot_be_reused_by_classic_src() {
        let document = web_html::parse(
            "<p id=x>safe</p><script src='noir-module:https://example.test/entry.mjs'></script>",
        )
        .unwrap();
        let mut session = DocumentSession::new(document);
        let report = session.run_initial(&[ScriptAsset {
            src: "noir-module:https://example.test/entry.mjs".into(),
            source: "document.getElementById('x').textContent='bad';".into(),
        }]);
        assert_eq!(report.executed, 0);
        let id = session.query_selector("#x").unwrap().unwrap();
        assert_eq!(session.document.text_content(id), "safe");
    }
    #[test]
    fn native_values_are_data_and_handlers_observe_current_edits() {
        let mut session=DocumentSession::new(web_html::parse("<input id=name value=Before><textarea id=note>Old</textarea><input id=flag type=checkbox value=yes><button id=apply type=button>Apply</button><p id=out>Before</p>").unwrap());
        session.execute("const field=document.getElementById('name');document.getElementById('apply').addEventListener('click',()=>{document.getElementById('out').textContent=field.value;});").unwrap();
        let field = session.query_selector("#name").unwrap().unwrap();
        let button = session.query_selector("#apply").unwrap().unwrap();
        let out = session.query_selector("#out").unwrap().unwrap();
        let literal = "';throw 'must remain data';//";
        session.update_control_value(field, literal).unwrap();
        session.dispatch_event(button, "click").unwrap();
        assert_eq!(session.document.text_content(out), literal);
        assert!(
            session
                .update_control_value(button, "not a text field")
                .is_err()
        );
        let note = session.query_selector("#note").unwrap().unwrap();
        session.update_control_value(note, "Updated").unwrap();
        assert_eq!(session.document.text_content(note), "Updated");
        let flag = session.query_selector("#flag").unwrap().unwrap();
        session.update_control_value(flag, "yes").unwrap();
        assert!(
            session
                .document
                .element(flag)
                .unwrap()
                .attributes
                .contains_key("checked")
        );
        session.update_control_value(flag, "").unwrap();
        assert!(
            !session
                .document
                .element(flag)
                .unwrap()
                .attributes
                .contains_key("checked")
        );
        assert!(
            session
                .update_control_value(field, &"x".repeat(32 * 1024 + 1))
                .is_err()
        );
    }
    #[test]
    fn textarea_initial_script_and_native_values_stay_consistent() {
        let mut session = DocumentSession::new(
            web_html::parse("<textarea id=config>{\"workspace\":\"Pessoal\"}</textarea>").unwrap(),
        );
        assert_eq!(
            session
                .execute("const field=document.getElementById('config');JSON.parse(field.value).workspace;")
                .unwrap(),
            Value::string("Pessoal")
        );
        let id = session.query_selector("#config").unwrap().unwrap();
        session.execute("field.value='Depois';").unwrap();
        assert_eq!(session.document.text_content(id), "Depois");
        assert_eq!(
            session.execute("field.value;").unwrap(),
            Value::string("Depois")
        );
        let literal = "';throw 'still data';//";
        session.update_control_value(id, literal).unwrap();
        assert_eq!(
            session.execute("field.value;").unwrap(),
            Value::string(literal)
        );
        session.execute("field.value='';").unwrap();
        assert_eq!(session.document.text_content(id), "");
        assert_eq!(session.execute("field.value;").unwrap(), Value::string(""));
        assert!(
            session
                .update_control_value(id, &"x".repeat(32 * 1024 + 1))
                .is_err()
        );
    }
    #[test]
    fn callbacks_this_listener_dedup_and_timer_deadlines_are_real() {
        let mut session =
            DocumentSession::new(web_html::parse("<button id=b type=button>0</button>").unwrap());
        session.execute("let n=0;const b=document.getElementById('b');function clicked(){n++;this.textContent=String(n);}b.addEventListener('click',clicked);b.addEventListener('click',clicked);setTimeout(function(value){b.textContent=value;},50,'Timed');").unwrap();
        let id = session.query_selector("#b").unwrap().unwrap();
        session.dispatch_event(id, "click").unwrap();
        assert_eq!(session.document.text_content(id), "1");
        assert_eq!(session.next_timer_ms(), Some(50));
        session.run_due_timers(49).unwrap();
        assert_eq!(session.next_timer_ms(), Some(1));
        session.run_due_timers(1).unwrap();
        assert_eq!(session.document.text_content(id), "Timed");
        assert_eq!(session.next_timer_ms(), None);
        session.execute("b.removeEventListener('click',clicked);b.onclick=()=>{b.textContent='Old';};b.onclick=()=>{b.textContent='Newest';};").unwrap();
        session.dispatch_event(id, "click").unwrap();
        assert_eq!(session.document.text_content(id), "Newest");
        assert_eq!(
            session.execute("typeof setTimeout;").unwrap(),
            Value::string("function")
        );
    }
}
