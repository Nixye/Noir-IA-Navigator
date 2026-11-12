//! A document's VM and DOM stay in the worker until its tab navigates or closes.
use super::{ManagedChild, TIMEOUT, is_remote, validate_page, validate_request};
use browser_ipc::{
    DocumentInput, PROTOCOL_VERSION, RenderRequest, RenderResponse, RenderedPage, SessionRequest,
    Viewport, read_message, write_message,
};
use std::{
    collections::HashSet,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Instant,
};

const MAX_LIVE_SESSIONS: usize = 16;
static LIVE_SESSIONS: AtomicUsize = AtomicUsize::new(0);
struct SessionSlot;
impl SessionSlot {
    fn acquire() -> Result<Self, String> {
        LIVE_SESSIONS.try_update(Ordering::AcqRel,Ordering::Acquire,|n|(n<MAX_LIVE_SESSIONS).then_some(n+1)).map(|_|Self).map_err(|_|format!("Limite de {MAX_LIVE_SESSIONS} documentos ativos atingido; feche uma aba para liberar um renderizador"))
    }
}
impl Drop for SessionSlot {
    fn drop(&mut self) {
        LIVE_SESSIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

type Input = Box<dyn Write + Send>;
type Output = Box<dyn Read + Send>;

pub struct RendererSession {
    child: Box<dyn ManagedChild + Send>,
    stdin: Option<Input>,
    stdout: Option<Output>,
    next_id: u64,
    last_animation_frame_ms: Option<f64>,
    storage_access: browser_ipc::SessionStorageAccess,
    storage_checkpoint: browser_ipc::SessionStorageSnapshot,
    images: HashSet<String>,
    scripts: browser_core::DynamicScriptBroker,
    _slot: SessionSlot,
}

impl RendererSession {
    pub fn spawn(
        executable: &Path,
        document: DocumentInput,
        viewport: Viewport,
    ) -> Result<(Self, RenderedPage), String> {
        Self::spawn_with_storage(
            executable,
            document,
            viewport,
            browser_ipc::SessionStorageSeed::default(),
        )
    }

    pub fn spawn_with_storage(
        executable: &Path,
        document: DocumentInput,
        viewport: Viewport,
        session_storage: browser_ipc::SessionStorageSeed,
    ) -> Result<(Self, RenderedPage), String> {
        let request = RenderRequest {
            session_storage,
            protocol_version: PROTOCOL_VERSION,
            id: 1,
            document,
            viewport,
        };
        validate_request(&request)?;
        let scripts = browser_core::DynamicScriptBroker::new(&request.document.url, executable)?;
        let slot = SessionSlot::acquire()?;
        let (child, stdin, stdout): (Box<dyn ManagedChild + Send>, Input, Output) =
            if is_remote(&request.document.url) {
                security_sandbox::ensure_verified(executable)?;
                #[cfg(windows)]
                {
                    let mut child =
                        security_sandbox::spawn(executable, "--renderer-session", None)?;
                    let stdin = child
                        .stdin
                        .take()
                        .ok_or("Sandbox session stdin unavailable")?;
                    let stdout = child
                        .stdout
                        .take()
                        .ok_or("Sandbox session stdout unavailable")?;
                    (Box::new(child), Box::new(stdin), Box::new(stdout))
                }
                #[cfg(not(windows))]
                return Err("Sandbox de sessão remota indisponível nesta plataforma".into());
            } else {
                let mut command = Command::new(executable);
                command
                    .arg("--renderer-session")
                    .env_clear()
                    .current_dir(std::env::temp_dir())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x0800_0000);
                    if let Some(root) = std::env::var_os("SystemRoot") {
                        command.env("SystemRoot", root);
                    }
                }
                let mut child = security_sandbox::spawn_host_process(&mut command)
                    .map_err(|e| format!("Iniciar sessão de renderização: {e}"))?;
                let stdin = child.stdin.take().ok_or("Session stdin unavailable")?;
                let stdout = child.stdout.take().ok_or("Session stdout unavailable")?;
                (Box::new(child), Box::new(stdin), Box::new(stdout))
            };
        let images = request
            .document
            .images
            .iter()
            .map(|image| image.id.clone())
            .collect();
        let mut session = Self {
            child,
            stdin: Some(stdin),
            stdout: Some(stdout),
            next_id: 2,
            last_animation_frame_ms: None,
            storage_access: request.session_storage.access,
            storage_checkpoint: request.session_storage.entries.clone(),
            images,
            scripts,
            _slot: slot,
        };
        let page = session.exchange(SessionRequest::Begin(request), 1)?;
        Ok((session, page))
    }

    pub fn storage_checkpoint(&self) -> &browser_ipc::SessionStorageSnapshot {
        &self.storage_checkpoint
    }

    pub fn render(
        &mut self,
        viewport: Viewport,
        event: Option<(u64, String)>,
    ) -> Result<RenderedPage, String> {
        if let Some((node, event)) = event {
            return self.event_with_inputs(viewport, node, event, vec![]);
        }
        validate_viewport(viewport)?;
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Sessão excedeu o limite de operações")?;
        let request = SessionRequest::Reflow { id, viewport };
        self.exchange(request, id)
    }

    pub fn event_with_inputs(
        &mut self,
        viewport: Viewport,
        node: u64,
        event: String,
        inputs: Vec<(u64, String)>,
    ) -> Result<RenderedPage, String> {
        validate_viewport(viewport)?;
        validate_event(node, &event)?;
        validate_inputs(&inputs)?;
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Sessão excedeu o limite de operações")?;
        self.exchange(
            SessionRequest::Event {
                id,
                node,
                event,
                viewport,
                inputs,
            },
            id,
        )
    }

    pub fn advance_timers(
        &mut self,
        viewport: Viewport,
        elapsed_ms: u64,
    ) -> Result<RenderedPage, String> {
        validate_viewport(viewport)?;
        if elapsed_ms > 60_000 {
            return Err("Avanço de relógio excede 60 segundos por operação".into());
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Sessão excedeu o limite de operações")?;
        self.exchange(
            SessionRequest::Tick {
                id,
                elapsed_ms,
                viewport,
            },
            id,
        )
    }

    /// A serialized pulse for this retained document; no timer advancement.
    pub fn animation_frame(
        &mut self,
        timestamp_ms: f64,
        viewport: Viewport,
    ) -> Result<RenderedPage, String> {
        let id = accept_animation_frame_pulse(
            timestamp_ms,
            viewport,
            &mut self.last_animation_frame_ms,
            &mut self.next_id,
        )?;
        self.exchange(
            SessionRequest::AnimationFrame {
                id,
                timestamp_ms,
                viewport,
            },
            id,
        )
    }

    fn exchange(
        &mut self,
        mut request: SessionRequest,
        mut expected: u64,
    ) -> Result<RenderedPage, String> {
        let viewport = request_viewport(&request);
        let mut remaining = TIMEOUT;
        let result = (|| {
            loop {
                if remaining.is_zero() {
                    return Err("Session operation exhausted its renderer deadline".into());
                }
                let started = Instant::now();
                let (page, pending) = self.exchange_once(request, expected, remaining)?;
                remaining = remaining.saturating_sub(started.elapsed());
                if pending.is_empty() {
                    return Ok(page);
                }
                // Identity, page, storage and all source requests were validated
                // together before crossing back into the privileged broker.
                let scripts = self.scripts.resolve(&pending)?;
                expected = self.next_id;
                self.next_id = self
                    .next_id
                    .checked_add(1)
                    .ok_or("Session operation limit exceeded")?;
                request = SessionRequest::Scripts {
                    id: expected,
                    scripts,
                    viewport,
                };
            }
        })();
        if result.is_err() {
            self.child.terminate();
            self.stdin = None;
            self.stdout = None;
        }
        result
    }

    fn exchange_once(
        &mut self,
        request: SessionRequest,
        expected: u64,
        timeout: std::time::Duration,
    ) -> Result<(RenderedPage, Vec<browser_ipc::DynamicScriptRequest>), String> {
        let mut stdin = self
            .stdin
            .take()
            .ok_or("Sessão de renderização encerrada")?;
        let mut stdout = self
            .stdout
            .take()
            .ok_or("Sessão de renderização encerrada")?;
        let (sender, receiver) = mpsc::sync_channel(2);
        let write_sender = sender.clone();
        std::thread::spawn(move || {
            let result =
                write_message(&mut stdin, &request).map_err(|e| format!("Session IPC write: {e}"));
            let _ = write_sender.send(Transaction::Written(stdin, result));
        });
        std::thread::spawn(move || {
            let result = read_message::<_, RenderResponse>(&mut stdout)
                .map(Box::new)
                .map_err(|e| format!("Session IPC read: {e}"));
            let _ = sender.send(Transaction::Read(stdout, result));
        });
        let started = Instant::now();
        let mut response = None;
        let result = (|| {
            for _ in 0..2 {
                match receiver
                    .recv_timeout(timeout.saturating_sub(started.elapsed()))
                    .map_err(|_| "Sessão de renderização excedeu o prazo de 10 segundos")?
                {
                    Transaction::Written(stdin, result) => {
                        self.stdin = Some(stdin);
                        result?;
                    }
                    Transaction::Read(stdout, result) => {
                        self.stdout = Some(stdout);
                        response = Some(result?);
                    }
                }
            }
            let response = response.ok_or("Sessão sem resposta")?;
            self.scripts.validate_requests(&response.pending_scripts)?;
            let pending = response.pending_scripts.clone();
            let page = accept_response(
                *response,
                expected,
                &self.images,
                self.storage_access,
                &mut self.storage_checkpoint,
            )?;
            Ok((page, pending))
        })();
        if result.is_err() {
            self.child.terminate();
            self.stdin = None;
            self.stdout = None;
        }
        result
    }
}
impl Drop for RendererSession {
    fn drop(&mut self) {
        self.child.terminate();
    }
}

/// Commit only after the complete response, identity and granted context pass.
/// Transport failures never call this function, leaving the checkpoint intact.
fn accept_response(
    response: RenderResponse,
    expected: u64,
    images: &HashSet<String>,
    access: browser_ipc::SessionStorageAccess,
    checkpoint: &mut browser_ipc::SessionStorageSnapshot,
) -> Result<RenderedPage, String> {
    if response.protocol_version != PROTOCOL_VERSION || response.id != expected {
        return Err("Invalid session response version or identity".into());
    }
    let page = response.result?;
    validate_page(&page, images)?;
    browser_ipc::validate_dynamic_script_requests(&response.pending_scripts)?;
    browser_ipc::validate_session_storage_entries(&response.session_storage)?;
    if access != browser_ipc::SessionStorageAccess::ReadWrite
        && !response.session_storage.is_empty()
    {
        return Err("Renderer wrote unavailable session storage".into());
    }
    *checkpoint = response.session_storage;
    Ok(page)
}
enum Transaction {
    Written(Input, Result<(), String>),
    Read(Output, Result<Box<RenderResponse>, String>),
}

fn request_viewport(request: &SessionRequest) -> Viewport {
    match request {
        SessionRequest::Begin(request) => request.viewport,
        SessionRequest::Scripts { viewport, .. }
        | SessionRequest::AnimationFrame { viewport, .. }
        | SessionRequest::Reflow { viewport, .. }
        | SessionRequest::Event { viewport, .. }
        | SessionRequest::Tick { viewport, .. } => *viewport,
    }
}

fn request_id(request: &SessionRequest) -> u64 {
    match request {
        SessionRequest::Begin(request) => request.id,
        SessionRequest::Scripts { id, .. }
        | SessionRequest::AnimationFrame { id, .. }
        | SessionRequest::Reflow { id, .. }
        | SessionRequest::Event { id, .. }
        | SessionRequest::Tick { id, .. } => *id,
    }
}

fn accept_animation_frame_pulse(
    timestamp_ms: f64,
    viewport: Viewport,
    previous: &mut Option<f64>,
    next_id: &mut u64,
) -> Result<u64, String> {
    validate_viewport(viewport)?;
    browser_ipc::validate_animation_frame_timestamp(timestamp_ms, *previous)?;
    let id = *next_id;
    let successor = id
        .checked_add(1)
        .ok_or("Session operation limit exceeded")?;
    // Commit outside any DOM/VM checkpoint, before IPC or callbacks. Transport
    // failure kills the session, and must never renew the accepted clock.
    *previous = Some(timestamp_ms);
    *next_id = successor;
    Ok(id)
}

fn validate_viewport(viewport: Viewport) -> Result<(), String> {
    browser_ipc::validate_view_context(viewport.view_context)?;
    if !viewport.width.is_finite()
        || !viewport.height.is_finite()
        || !(1.0..=16384.).contains(&viewport.width)
        || !(1.0..=16384.).contains(&viewport.height)
    {
        return Err("Dimensões da sessão inválidas".into());
    }
    Ok(())
}
fn validate_event(node: u64, event: &str) -> Result<(), String> {
    if node > 100_000 || !matches!(event, "click" | "input" | "change") {
        return Err("Evento de documento não permitido".into());
    }
    Ok(())
}

fn validate_inputs(inputs: &[(u64, String)]) -> Result<(), String> {
    if inputs.len() > 256 {
        return Err("Controles de entrada em excesso".into());
    }
    let mut ids = HashSet::new();
    let mut bytes = 0;
    for (id, value) in inputs {
        if *id > 100_000 || !ids.insert(id) || value.len() > 32768 || value.chars().count() > 8192 {
            return Err("Entrada de controle inválida".into());
        }
        bytes += value.len();
    }
    if bytes > 256 * 1024 {
        return Err("Entradas de controles excedem o orçamento".into());
    }
    Ok(())
}

pub fn run_session_worker() -> Result<(), String> {
    // A live document can outlive an eight-second cumulative CPU budget. Each
    // operation instead has the supervisor's wall-clock deadline and VM budget.
    #[cfg(target_os = "linux")]
    {
        rlimit::Resource::CORE
            .set(0, 0)
            .map_err(|e| e.to_string())?;
        rlimit::Resource::AS
            .set(1024 * 1024 * 1024, 1024 * 1024 * 1024)
            .map_err(|e| e.to_string())?;
    }
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut engine: Option<web_renderer::RenderSession> = None;
    let mut last_id = 0;
    loop {
        let request: SessionRequest = match read_message(&mut input) {
            Ok(request) => request,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(format!("Session IPC request: {e}")),
        };
        let incoming_id = request_id(&request);
        if incoming_id <= last_id {
            return Err("Session request identity must be fresh and nonzero".into());
        }
        last_id = incoming_id;
        let (id, result) = match request {
            SessionRequest::Begin(request) => {
                let id = request.id;
                let result = if engine.is_some() {
                    Err("Uma sessão não pode trocar de documento ou origem".into())
                } else {
                    validate_request(&request).and_then(|()| {
                        if is_remote(&request.document.url) && !security_sandbox::is_appcontainer()
                        {
                            return Err("Documento remoto exige AppContainer".into());
                        }
                        let mut session = web_renderer::RenderSession::new_with_services(
                            &request.document,
                            request.viewport,
                            request.session_storage,
                            std::sync::Arc::new(crate::platform::SystemServices::new()?),
                        )?;
                        let page = session.render(request.viewport)?;
                        engine = Some(session);
                        Ok(page)
                    })
                };
                (id, result)
            }
            SessionRequest::Scripts {
                id,
                scripts,
                viewport,
            } => {
                let result = validate_viewport(viewport)
                    .and_then(|()| browser_ipc::validate_dynamic_script_responses(&scripts))
                    .and_then(|()| {
                        let session = engine
                            .as_mut()
                            .ok_or_else(|| "Begin required before Scripts".to_string())?;
                        session.update_geometry(viewport)?;
                        session.complete_scripts(&scripts)?;
                        session.render(viewport)
                    });
                (id, result)
            }
            SessionRequest::AnimationFrame {
                id,
                timestamp_ms,
                viewport,
            } => {
                let result = validate_viewport(viewport).and_then(|()| {
                    engine
                        .as_mut()
                        .ok_or_else(|| "Begin required before AnimationFrame".to_string())?
                        .animation_frame(timestamp_ms, viewport)
                });
                (id, result)
            }
            SessionRequest::Reflow { id, viewport } => {
                let result = validate_viewport(viewport).and_then(|()| {
                    engine
                        .as_mut()
                        .ok_or_else(|| "Begin obrigatório antes de reflow".to_string())?
                        .render(viewport)
                });
                (id, result)
            }
            SessionRequest::Event {
                id,
                node,
                event,
                viewport,
                inputs,
            } => {
                let result = validate_viewport(viewport)
                    .and_then(|()| validate_event(node, &event))
                    .and_then(|()| validate_inputs(&inputs))
                    .and_then(|()| {
                        let engine = engine
                            .as_mut()
                            .ok_or_else(|| "Begin obrigatório antes de evento".to_string())?;
                        for (input, value) in inputs {
                            engine.update_control_value(input, &value)?;
                        }
                        engine.dispatch_event(node, &event, viewport)
                    });
                (id, result)
            }
            SessionRequest::Tick {
                id,
                elapsed_ms,
                viewport,
            } => {
                let result = validate_viewport(viewport).and_then(|()| {
                    if elapsed_ms > 60_000 {
                        return Err("Avanço de relógio inválido".into());
                    }
                    engine
                        .as_mut()
                        .ok_or_else(|| "Begin obrigatório antes de Tick".to_string())?
                        .advance_timers(elapsed_ms, viewport)
                });
                (id, result)
            }
        };
        let result = result.map(|mut page| {
            page.metrics.worker_pid = std::process::id();
            page.metrics.resident_memory_bytes =
                memory_stats::memory_stats().map(|m| m.physical_mem as u64);
            page
        });
        let failed = result.is_err();
        write_message(
            &mut output,
            &RenderResponse {
                pending_scripts: if failed {
                    vec![]
                } else {
                    engine
                        .as_mut()
                        .map_or_else(Vec::new, |session| session.take_script_requests())
                },
                session_storage: engine
                    .as_ref()
                    .map_or_else(Vec::new, |session| session.session_storage_snapshot()),
                protocol_version: PROTOCOL_VERSION,
                id,
                result,
            },
        )
        .map_err(|e| e.to_string())?;
        if failed {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_pulses_validate_before_consuming_identity_or_time() {
        let viewport = Viewport::default();
        let mut previous = None;
        let mut next = 2;
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.,
            browser_ipc::MAX_ANIMATION_FRAME_TIMESTAMP_MS + 1.,
        ] {
            assert!(
                accept_animation_frame_pulse(value, viewport, &mut previous, &mut next).is_err()
            );
            assert_eq!(previous, None);
            assert_eq!(next, 2);
        }
        assert_eq!(
            accept_animation_frame_pulse(20., viewport, &mut previous, &mut next).unwrap(),
            2
        );
        assert_eq!(previous, Some(20.));
        assert_eq!(next, 3);
        assert!(accept_animation_frame_pulse(19., viewport, &mut previous, &mut next).is_err());
        assert_eq!(next, 3);
        assert_eq!(
            accept_animation_frame_pulse(20., viewport, &mut previous, &mut next).unwrap(),
            3
        );
        next = u64::MAX;
        assert!(accept_animation_frame_pulse(21., viewport, &mut previous, &mut next).is_err());
        assert_eq!(previous, Some(20.));
        assert_eq!(next, u64::MAX);
    }

    #[test]
    fn frame_response_preserves_identity_checks_and_transactional_storage() {
        use browser_ipc::{Color, RenderMetrics, SessionStorageAccess};
        let page = RenderedPage {
            title: String::new(),
            width: 800.,
            height: 600.,
            background: Color::default(),
            items: vec![],
            links: vec![],
            metrics: RenderMetrics::default(),
            warnings: vec![],
            next_timer_ms: None,
            animation_frame_pending: true,
        };
        let old = vec![("key".into(), "confirmed".into())];
        let new = vec![("key".into(), "next".into())];
        let response = RenderResponse {
            pending_scripts: vec![],
            protocol_version: PROTOCOL_VERSION,
            id: 2,
            result: Ok(page),
            session_storage: new.clone(),
        };
        let mut invalid = Vec::new();
        let mut r = response.clone();
        r.pending_scripts.push(browser_ipc::DynamicScriptRequest {
            id: 0,
            src: "app.js".into(),
        });
        invalid.push(r);
        let mut r = response.clone();
        r.pending_scripts.push(browser_ipc::DynamicScriptRequest {
            id: 1,
            src: "x".repeat(browser_ipc::MAX_DYNAMIC_SCRIPT_SRC_BYTES + 1),
        });
        invalid.push(r);
        let mut r = response.clone();
        r.protocol_version -= 1;
        invalid.push(r);
        let mut r = response.clone();
        r.id += 1;
        invalid.push(r);
        let mut r = response.clone();
        r.result.as_mut().unwrap().width = f32::NAN;
        invalid.push(r);
        let mut r = response.clone();
        r.result = Err("worker failure".into());
        invalid.push(r);
        let mut r = response.clone();
        r.session_storage.push(r.session_storage[0].clone());
        invalid.push(r);
        let mut r = response.clone();
        r.session_storage[0].1 = "x".repeat(browser_ipc::SESSION_STORAGE_MAX_VALUE_BYTES + 1);
        invalid.push(r);
        for r in invalid {
            let mut checkpoint = old.clone();
            assert!(
                accept_response(
                    r,
                    2,
                    &HashSet::new(),
                    SessionStorageAccess::ReadWrite,
                    &mut checkpoint
                )
                .is_err()
            );
            assert_eq!(checkpoint, old);
        }
        for access in [
            SessionStorageAccess::Unavailable,
            SessionStorageAccess::QuotaExceeded,
        ] {
            let mut checkpoint = vec![];
            assert!(
                accept_response(
                    response.clone(),
                    2,
                    &HashSet::new(),
                    access,
                    &mut checkpoint
                )
                .is_err()
            );
            assert!(checkpoint.is_empty());
        }
        let mut checkpoint = old;
        let page = accept_response(
            response,
            2,
            &HashSet::new(),
            SessionStorageAccess::ReadWrite,
            &mut checkpoint,
        )
        .unwrap();
        assert!(page.animation_frame_pending);
        assert_eq!(checkpoint, new);
    }
    #[test]
    fn collection_read_failure_never_reaches_checkpoint_commit() {
        // An in-memory transport regression, not a subprocess termination test.
        let old = vec![("key".into(), "confirmed".into())];
        let response = RenderResponse {
            pending_scripts: vec![],
            protocol_version: PROTOCOL_VERSION,
            id: 2,
            session_storage: (0..=browser_ipc::SESSION_STORAGE_MAX_ENTRIES)
                .map(|n| (n.to_string(), String::new()))
                .collect(),
            result: Err("worker failure".into()),
        };
        let mut frame = Vec::new();
        write_message(&mut frame, &response).unwrap();
        let mut checkpoint = old.clone();
        let result = read_message::<_, RenderResponse>(&mut frame.as_slice())
            .map_err(|e| e.to_string())
            .and_then(|response| {
                accept_response(
                    response,
                    2,
                    &HashSet::new(),
                    browser_ipc::SessionStorageAccess::ReadWrite,
                    &mut checkpoint,
                )
            });
        assert!(result.unwrap_err().contains("collection limit"));
        assert_eq!(checkpoint, old);
    }

    #[test]
    fn decoded_collection_bounds_do_not_replace_semantic_validation() {
        use browser_ipc::{Color, DisplayItem, Rect, RenderMetrics, SessionStorageAccess};
        let page = RenderedPage {
            title: String::new(),
            width: 800.,
            height: 600.,
            background: Color::default(),
            items: vec![],
            links: vec![],
            metrics: RenderMetrics::default(),
            warnings: vec![],
            next_timer_ms: None,
            animation_frame_pending: false,
        };
        for item in [
            DisplayItem::PopClip,
            DisplayItem::Image {
                rect: Rect::default(),
                asset_id: "unauthorized".into(),
            },
            DisplayItem::Rect {
                rect: Rect {
                    width: -1.,
                    ..Rect::default()
                },
                color: Color::default(),
            },
        ] {
            let mut p = page.clone();
            p.items.push(item);
            let response = RenderResponse {
                pending_scripts: vec![],
                protocol_version: PROTOCOL_VERSION,
                id: 2,
                session_storage: vec![],
                result: Ok(p),
            };
            let mut frame = Vec::new();
            write_message(&mut frame, &response).unwrap();
            let response = read_message::<_, RenderResponse>(&mut frame.as_slice()).unwrap();
            let old = vec![("key".into(), "confirmed".into())];
            let mut checkpoint = old.clone();
            assert!(
                accept_response(
                    response,
                    2,
                    &HashSet::new(),
                    SessionStorageAccess::ReadWrite,
                    &mut checkpoint
                )
                .is_err()
            );
            assert_eq!(checkpoint, old);
        }
    }
    #[test]
    fn live_process_capacity_is_released_by_raii() {
        let mut slots = Vec::new();
        for _ in 0..MAX_LIVE_SESSIONS {
            slots.push(SessionSlot::acquire().unwrap());
        }
        assert!(SessionSlot::acquire().is_err());
        slots.pop();
        slots.push(SessionSlot::acquire().unwrap());
        drop(slots);
        assert_eq!(LIVE_SESSIONS.load(Ordering::Acquire), 0);
    }
    #[test]
    fn ui_control_updates_are_bounded_and_never_source_code() {
        assert!(validate_inputs(&[(1, "'); delete user.files; ('".into())]).is_ok());
        assert!(validate_inputs(&[(1, "first".into()), (1, "duplicate".into())]).is_err());
        assert!(validate_inputs(&[(1, "x".repeat(32769))]).is_err());
        assert!(validate_inputs(&(0..257).map(|id| (id, "".into())).collect::<Vec<_>>()).is_err());
        assert!(validate_event(1, "click").is_ok());
        assert!(validate_event(1, "__proto__").is_err());
    }
}
