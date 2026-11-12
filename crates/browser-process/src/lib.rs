//! One-shot renderer lifecycle and bounded IPC. Remote documents require the
//! verified Windows AppContainer backend; there is no unsandboxed remote fallback.
use browser_ipc::{
    DisplayItem, DocumentInput, MAX_HTML_BYTES, MAX_IMAGE_BYTES, MAX_IMAGE_TOTAL_BYTES,
    PROTOCOL_VERSION, Rect, RenderRequest, RenderResponse, RenderedPage, Viewport, read_message,
    write_message,
};
use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_COORDINATE: f32 = 1_000_000.0;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

mod platform;
mod session;
mod session_storage;
pub use session::{RendererSession, run_session_worker};
pub use session_storage::{TabSessionStorage, session_storage_origin};

/// Entry point for the executable's `--renderer` mode. Exactly one document is
/// processed, so no DOM, origins or capabilities survive navigation.
pub fn run_worker() -> Result<(), String> {
    harden_worker()?;
    let request: RenderRequest =
        read_message(&mut std::io::stdin().lock()).map_err(|e| format!("IPC request: {e}"))?;
    let mut session_storage = Vec::new();
    let mut pending_scripts = Vec::new();
    let result = validate_request(&request)
        .and_then(|()| {
            if is_remote(&request.document.url) && !security_sandbox::is_appcontainer() {
                Err("Documento remoto exige um renderizador AppContainer verificado".into())
            } else {
                Ok(())
            }
        })
        .and_then(|()| {
            let mut engine = web_renderer::RenderSession::new_with_services(
                &request.document,
                request.viewport,
                request.session_storage.clone(),
                std::sync::Arc::new(platform::SystemServices::new()?),
            )?;
            let page = engine.render(request.viewport)?;
            session_storage = engine.session_storage_snapshot();
            pending_scripts = engine.take_script_requests();
            Ok(page)
        });
    let result = result.map(|mut page| {
        page.metrics.worker_pid = std::process::id();
        page.metrics.resident_memory_bytes =
            memory_stats::memory_stats().map(|m| m.physical_mem as u64);
        page
    });
    let response = RenderResponse {
        pending_scripts,
        session_storage,
        protocol_version: PROTOCOL_VERSION,
        id: request.id,
        result,
    };
    write_message(&mut std::io::stdout().lock(), &response)
        .map_err(|e| format!("IPC response: {e}"))
}

pub fn run_sandbox_probe(private_path: &str) -> Result<(), String> {
    security_sandbox::run_probe(private_path)
}

pub fn verify_remote_sandbox(executable: &Path) -> Result<(), String> {
    security_sandbox::ensure_verified(executable)
}

fn is_remote(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Runs potentially expensive parsing/layout outside the UI. Call from a task
/// thread. Both pipes are serviced concurrently and share a wall-clock deadline.
pub fn render_in_subprocess(
    executable: &Path,
    document: DocumentInput,
    viewport: Viewport,
) -> Result<RenderedPage, String> {
    let request = RenderRequest {
        session_storage: Default::default(),
        protocol_version: PROTOCOL_VERSION,
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        document,
        viewport,
    };
    if is_remote(&request.document.url) {
        security_sandbox::ensure_verified(executable)?;
        #[cfg(windows)]
        {
            validate_request(&request)?;
            let mut child = security_sandbox::spawn(executable, "--renderer", None)?;
            let stdin = child.stdin.take().ok_or("Sandbox stdin unavailable")?;
            let stdout = child.stdout.take().ok_or("Sandbox stdout unavailable")?;
            return exchange_pipes(child, stdin, stdout, request, TIMEOUT);
        }
        #[cfg(not(windows))]
        return Err("Sandbox de navegação remota indisponível nesta plataforma".into());
    }
    let mut command = Command::new(executable);
    command
        .arg("--renderer")
        .env_clear()
        .current_dir(std::env::temp_dir());
    // Preserve only Windows' installation path, needed by OS components. Never
    // inherit API keys, proxy settings, tool configuration or profile secrets.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        if let Some(value) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", value);
        }
    }
    exchange(command, request, TIMEOUT)
}

fn exchange(
    mut command: Command,
    request: RenderRequest,
    timeout: Duration,
) -> Result<RenderedPage, String> {
    validate_request(&request)?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = security_sandbox::spawn_host_process(&mut command)
        .map_err(|e| format!("Não foi possível iniciar o renderizador: {e}"))?;
    let stdin = child.stdin.take().ok_or("Pipe stdin indisponível.")?;
    let stdout = child.stdout.take().ok_or("Pipe stdout indisponível.")?;
    exchange_pipes(child, stdin, stdout, request, timeout)
}

trait ManagedChild {
    fn terminate(&mut self);
}
impl ManagedChild for std::process::Child {
    fn terminate(&mut self) {
        let _ = self.kill();
        let _ = self.wait();
    }
}
#[cfg(windows)]
impl ManagedChild for security_sandbox::SandboxedChild {
    fn terminate(&mut self) {
        security_sandbox::SandboxedChild::terminate(self);
    }
}

fn exchange_pipes<
    C: ManagedChild,
    W: std::io::Write + Send + 'static,
    R: std::io::Read + Send + 'static,
>(
    mut child: C,
    stdin: W,
    stdout: R,
    request: RenderRequest,
    timeout: Duration,
) -> Result<RenderedPage, String> {
    let image_ids: HashSet<String> = request
        .document
        .images
        .iter()
        .map(|a| a.id.clone())
        .collect();
    let expected_id = request.id;
    let storage_access = request.session_storage.access;
    let (sender, receiver) = mpsc::sync_channel(2);
    let write_sender = sender.clone();
    let writer = std::thread::spawn(move || {
        let result = write_message(&mut { stdin }, &request).map_err(|e| format!("IPC write: {e}"));
        let _ = write_sender.send(PipeResult::Write(result));
    });
    let reader = std::thread::spawn(move || {
        let result = read_message::<_, RenderResponse>(&mut { stdout })
            .map(Box::new)
            .map_err(|e| format!("IPC read: {e}"));
        let _ = sender.send(PipeResult::Read(result));
    });
    let start = std::time::Instant::now();
    let mut response = None;
    let mut error = None;
    for _ in 0..2 {
        let remaining = timeout.saturating_sub(start.elapsed());
        match receiver.recv_timeout(remaining) {
            Ok(PipeResult::Write(Ok(()))) => {}
            Ok(PipeResult::Write(Err(e))) => {
                error = Some(e);
                break;
            }
            Ok(PipeResult::Read(Ok(value))) => response = Some(value),
            Ok(PipeResult::Read(Err(e))) => {
                error = Some(e);
                break;
            }
            Err(_) => {
                error = Some(
                    "O renderizador excedeu o tempo limite ou encerrou inesperadamente.".to_owned(),
                );
                break;
            }
        }
    }
    // Even a renderer that writes a response and then stalls must not escape the
    // deadline. It is a one-shot worker; terminating after response is expected.
    child.terminate();
    // Do not join a stuck pipe thread: a compromised renderer could pass an
    // inherited handle to another process. OS sandboxing must prohibit that
    // before untrusted general-purpose content is supported.
    if writer.is_finished() {
        let _ = writer.join();
    }
    if reader.is_finished() {
        let _ = reader.join();
    }
    if let Some(error) = error {
        return Err(error);
    }
    let response = response.ok_or("O renderizador não retornou uma resposta.")?;
    if response.protocol_version != PROTOCOL_VERSION || response.id != expected_id {
        return Err("Versão ou identificação inválida na resposta do renderizador.".to_owned());
    }
    let mut page = response.result?;
    validate_page(&page, &image_ids)?;
    browser_ipc::validate_dynamic_script_requests(&response.pending_scripts)?;
    if !response.pending_scripts.is_empty() {
        if page.warnings.len() == browser_ipc::MAX_PAGE_WARNINGS {
            page.warnings.pop();
        }
        page.warnings.push("Dynamic external scripts require a retained renderer session; one-shot rendering performs no additional I/O".into());
    }
    browser_ipc::validate_session_storage_entries(&response.session_storage)?;
    if storage_access != browser_ipc::SessionStorageAccess::ReadWrite
        && !response.session_storage.is_empty()
    {
        return Err("Renderer wrote unavailable session storage".into());
    }
    Ok(page)
}

enum PipeResult {
    Write(Result<(), String>),
    Read(Result<Box<RenderResponse>, String>),
}

fn harden_worker() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use rlimit::Resource;
        Resource::CORE
            .set(0, 0)
            .map_err(|e| format!("RLIMIT_CORE: {e}"))?;
        Resource::CPU
            .set(8, 8)
            .map_err(|e| format!("RLIMIT_CPU: {e}"))?;
        Resource::AS
            .set(1024 * 1024 * 1024, 1024 * 1024 * 1024)
            .map_err(|e| format!("RLIMIT_AS: {e}"))?;
    }
    Ok(())
}

fn validate_request(request: &RenderRequest) -> Result<(), String> {
    browser_ipc::validate_view_context(request.viewport.view_context)?;
    browser_ipc::validate_session_storage_seed(&request.session_storage)?;
    browser_ipc::validate_audio_metadata(&request.document.audio_metadata)?;
    if !request.document.audio_metadata.is_empty()
        && !url::Url::parse(&request.document.url).is_ok_and(|url| url.scheme() == "file")
    {
        return Err("Audio metadata requires a file document".into());
    }
    if !is_remote(&request.document.url)
        && request.session_storage.access != browser_ipc::SessionStorageAccess::Unavailable
    {
        return Err("Session storage requires an HTTP(S) document".into());
    }
    if request.protocol_version != PROTOCOL_VERSION {
        return Err("Versão de protocolo não suportada.".to_owned());
    }
    if request.document.html.len() > MAX_HTML_BYTES || request.document.url.len() > 8192 {
        return Err("Documento excede os limites de entrada.".to_owned());
    }
    let viewport = request.viewport;
    if !viewport.width.is_finite()
        || !viewport.height.is_finite()
        || !(1.0..=16384.0).contains(&viewport.width)
        || !(1.0..=16384.0).contains(&viewport.height)
    {
        return Err("Dimensões da janela inválidas.".to_owned());
    }
    if request.document.images.len() > 32 {
        return Err("Imagens em excesso.".to_owned());
    }
    if request.document.scripts.len() > 4 {
        return Err("Scripts externos em excesso".into());
    }
    let mut script_bytes = 0;
    let mut script_ids = HashSet::new();
    for script in &request.document.scripts {
        if script.src.len() > 8192
            || !script_ids.insert(&script.src)
            || script.source.len() > 128 * 1024
        {
            return Err("Script inválido ou acima dos limites".into());
        }
        script_bytes += script.source.len();
    }
    if script_bytes > 256 * 1024 {
        return Err("Limite total de scripts excedido".into());
    }
    if request.document.stylesheets.len() > browser_ipc::MAX_STYLESHEETS {
        return Err("Folhas de estilo externas em excesso".into());
    }
    let mut style_bytes = 0usize;
    let mut style_ids = HashSet::new();
    for style in &request.document.stylesheets {
        if style.href.is_empty()
            || style.href.len() > 8192
            || style.href.chars().any(char::is_control)
            || !style_ids.insert(&style.href)
            || style.source.len() > browser_ipc::MAX_STYLESHEET_BYTES
        {
            return Err("Folha de estilo inválida ou acima dos limites".into());
        }
        style_bytes = style_bytes
            .checked_add(style.source.len())
            .ok_or("Tamanho de estilos excedido")?;
        if style_bytes > browser_ipc::MAX_STYLESHEET_TOTAL_BYTES {
            return Err("Limite total de estilos excedido".into());
        }
    }
    let mut total = 0;
    let mut seen = HashSet::new();
    let mut image_id_bytes = 0usize;
    for image in &request.document.images {
        image_id_bytes = image_id_bytes
            .checked_add(image.id.len())
            .ok_or("Image identifier size overflow")?;
        let expected = u64::from(image.width)
            .checked_mul(u64::from(image.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("Image dimensions overflow")?;
        if !browser_ipc::valid_image_id(&image.id)
            || image_id_bytes > browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES
            || !seen.insert(&image.id)
            || image.width == 0
            || image.height == 0
            || !browser_ipc::valid_natural_image_dimensions(
                image.natural_width,
                image.natural_height,
            )
            || image.width > 1024
            || image.height > 1024
            || expected != image.rgba.len() as u64
            || image.rgba.len() > MAX_IMAGE_BYTES
        {
            return Err("Imagem inválida ou acima dos limites.".to_owned());
        }
        total += image.rgba.len();
    }
    if total > MAX_IMAGE_TOTAL_BYTES {
        return Err("Limite total de imagens excedido.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod audio_boundary_tests {
    use super::*;
    #[test]
    fn broker_rejects_remote_audio_metadata_before_renderer() {
        let mut request = super::tests::request();
        request.document.audio_metadata = vec![browser_ipc::AudioMetadata {
            src: "a.wav".into(),
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
            block_align: 2,
            frames: 1,
        }];
        request.document.url = "https://example.test/".into();
        assert!(validate_request(&request).is_err());
        request.document.url = "file:///a.html".into();
        assert!(validate_request(&request).is_ok());
        request.document.audio_metadata[0].frames = u64::MAX;
        assert!(validate_request(&request).is_err());
    }
}

fn valid_rect(rect: &Rect) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE)
        && rect.width >= 0.0
        && rect.height >= 0.0
}

/// Treat returned display lists as untrusted input, even though the worker is
/// our own binary: malformed geometry must never reach the GPU/UI.
fn validate_page(page: &RenderedPage, image_ids: &HashSet<String>) -> Result<(), String> {
    if !page.width.is_finite()
        || !page.height.is_finite()
        || page.width < 0.0
        || page.height < 0.0
        || page.width > MAX_COORDINATE
        || page.height > MAX_COORDINATE
        || page.items.len() > browser_ipc::MAX_DISPLAY_ITEMS
        || page.links.len() > browser_ipc::MAX_PAGE_LINKS
        || page.title.len() > 4096
        || page.warnings.len() > browser_ipc::MAX_PAGE_WARNINGS
        || page.next_timer_ms.is_some_and(|delay| delay > 86_400_000)
    {
        return Err("Metadados inválidos na resposta do renderizador.".to_owned());
    }
    let mut text_bytes = 0;
    let mut image_id_bytes = 0usize;
    let mut controls = HashSet::new();
    let mut clip_depth = 0usize;
    for item in &page.items {
        match item {
            DisplayItem::PushClip { rect } => {
                if !valid_rect(rect) || clip_depth >= browser_ipc::MAX_CLIP_DEPTH {
                    return Err("Recorte inválido ou acima do limite.".into());
                }
                clip_depth += 1;
            }
            DisplayItem::PopClip => {
                clip_depth = clip_depth.checked_sub(1).ok_or("Recorte desbalanceado")?;
            }
            DisplayItem::Rect { rect, .. } => {
                if !valid_rect(rect) {
                    return Err("Geometria inválida.".to_owned());
                }
            }
            DisplayItem::Text {
                rect, text, size, ..
            } => {
                if !valid_rect(rect) || !size.is_finite() || !(1.0..=512.0).contains(size) {
                    return Err("Texto com geometria inválida.".to_owned());
                }
                text_bytes += text.len();
            }
            DisplayItem::Image { rect, asset_id } => {
                image_id_bytes = image_id_bytes
                    .checked_add(asset_id.len())
                    .ok_or("Image reference size overflow")?;
                if image_id_bytes > 2 * browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES {
                    return Err("Image reference byte budget exceeded".into());
                }
                if !valid_rect(rect) || !image_ids.contains(asset_id) {
                    return Err("Imagem não autorizada na resposta.".to_owned());
                }
            }
            DisplayItem::Control {
                rect,
                id,
                name,
                value,
                placeholder,
                form,
                appearance,
                options,
                kind,
                ..
            } => {
                if !valid_rect(rect)
                    || !controls.insert(*id)
                    || controls.len() > browser_ipc::MAX_PAGE_CONTROLS
                    || value.len() > 32768
                    || value.chars().count() > 8192
                    || name.chars().count() > 512
                    || placeholder.chars().count() > 512
                    || !appearance.font_size.is_finite()
                    || !(1.0..=512.0).contains(&appearance.font_size)
                    || !appearance.border_width.is_finite()
                    || !(0.0..=128.0).contains(&appearance.border_width)
                    || options.len() > browser_ipc::MAX_CONTROL_OPTIONS
                    || (*kind != browser_ipc::ControlKind::Select && !options.is_empty())
                {
                    return Err("Controle de formulário inválido ou acima dos limites".into());
                }
                text_bytes += name.len() + value.len() + placeholder.len();
                for option in options {
                    if option.label.chars().count() > 256 || option.value.len() > 1024 {
                        return Err("Select option exceeds bounds".into());
                    }
                    text_bytes = text_bytes
                        .checked_add(option.label.len() + option.value.len())
                        .ok_or("Control text size overflow")?;
                }
                if let Some(form) = form {
                    if form.action.len() > 8192 || form.method.chars().count() > 16 {
                        return Err("Metadados de formulário inválidos".into());
                    }
                    text_bytes += form.action.len() + form.method.len();
                }
            }
        }
    }
    if clip_depth != 0 {
        return Err("Recorte desbalanceado".into());
    }
    if text_bytes > 2 * MAX_HTML_BYTES {
        return Err("Display list de texto excede o limite.".to_owned());
    }
    if page
        .links
        .iter()
        .any(|l| !valid_rect(&l.rect) || l.href.len() > 8192)
        || page.warnings.iter().any(|w| w.len() > 8192)
    {
        return Err("Links ou diagnósticos inválidos.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use browser_ipc::{Color, ImageAsset, RenderMetrics};

    pub(super) fn request() -> RenderRequest {
        RenderRequest {
            session_storage: Default::default(),
            protocol_version: PROTOCOL_VERSION,
            id: 1,
            document: DocumentInput {
                url: "noir:welcome".to_owned(),
                html: "<p>Olá</p>".to_owned(),
                images: vec![],
                audio_metadata: vec![],
                scripts: vec![],
                stylesheets: vec![],
            },
            viewport: Viewport::default(),
        }
    }

    #[test]
    fn request_storage_requires_http_origin_and_valid_seed() {
        let mut input = request();
        input.session_storage.access = browser_ipc::SessionStorageAccess::ReadWrite;
        assert!(validate_request(&input).is_err());
        input.document.url = "https://example.test/path".into();
        assert!(validate_request(&input).is_ok());
        input.session_storage.entries = vec![("k".into(), "a".into()), ("k".into(), "b".into())];
        assert!(validate_request(&input).is_err());
        input.session_storage.entries = vec![("k".into(), "a".into())];
        input.session_storage.access = browser_ipc::SessionStorageAccess::QuotaExceeded;
        assert!(validate_request(&input).is_err());
    }
    #[test]
    fn natural_metadata_is_validated_without_changing_texture_budget() {
        let mut image = ImageAsset {
            id: "a".into(),
            width: 1,
            height: 1,
            natural_width: 2048.,
            natural_height: 1024.,
            rgba: vec![1, 2, 3, 255],
        };
        let mut input = request();
        input.document.images = vec![image.clone()];
        assert!(validate_request(&input).is_ok());
        for (width, height) in [
            (0., 1.),
            (-1., 1.),
            (f32::NAN, 1.),
            (1., f32::INFINITY),
            (4097., 1.),
            (4096., 1024.25),
            (1., 0.),
            (1., -1.),
        ] {
            image.natural_width = width;
            image.natural_height = height;
            input.document.images = vec![image.clone()];
            assert!(validate_request(&input).is_err());
        }
    }
    #[test]
    fn request_rejects_invalid_version_geometry_and_images() {
        let mut input = request();
        assert!(validate_request(&input).is_ok());
        input.protocol_version += 1;
        assert!(validate_request(&input).is_err());
        input.protocol_version = PROTOCOL_VERSION;
        input.viewport.width = f32::INFINITY;
        assert!(validate_request(&input).is_err());
        input.viewport = Viewport::default();
        input.document.images.push(ImageAsset {
            id: "test".to_owned(),
            width: 1024,
            height: 1024,
            natural_width: 1024.,
            natural_height: 1024.,
            rgba: vec![0; 4],
        });
        assert!(validate_request(&input).is_err());
    }

    #[test]
    fn script_sources_are_bounded_data_and_duplicate_capabilities_are_refused() {
        let mut input = request();
        input.document.scripts.push(browser_ipc::ScriptAsset {
            src: "app.js".into(),
            source: "document.title = 'Local';".into(),
        });
        assert!(validate_request(&input).is_ok());
        input
            .document
            .scripts
            .push(input.document.scripts[0].clone());
        assert!(validate_request(&input).is_err());
        input.document.scripts.pop();
        input.document.scripts[0].source = "x".repeat(128 * 1024 + 1);
        assert!(validate_request(&input).is_err());
    }
    #[test]
    fn stylesheet_assets_are_bounded_data_and_unique_capabilities() {
        let mut input = request();
        input
            .document
            .stylesheets
            .push(browser_ipc::StylesheetAsset {
                href: "https://cdn.example.org/style.css".into(),
                source: "body{color:red}".into(),
            });
        assert!(validate_request(&input).is_ok());
        input
            .document
            .stylesheets
            .push(input.document.stylesheets[0].clone());
        assert!(validate_request(&input).is_err());
        input.document.stylesheets.pop();
        input.document.stylesheets[0].source = "x".repeat(browser_ipc::MAX_STYLESHEET_BYTES + 1);
        assert!(validate_request(&input).is_err());
        input.document.stylesheets[0].source.clear();
        input.document.stylesheets[0].href = "x\0y".into();
        assert!(validate_request(&input).is_err());
    }

    #[test]
    fn response_rejects_nonfinite_geometry_and_ungranted_images() {
        let mut page = RenderedPage {
            title: "Test".to_owned(),
            width: 800.0,
            height: 600.0,
            background: Color(255, 255, 255, 255),
            items: vec![],
            links: vec![],
            metrics: RenderMetrics::default(),
            warnings: vec![],
            next_timer_ms: None,
            animation_frame_pending: false,
        };
        assert!(validate_page(&page, &HashSet::new()).is_ok());
        page.items.push(DisplayItem::Rect {
            rect: Rect {
                x: f32::NAN,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            color: Color::default(),
        });
        assert!(validate_page(&page, &HashSet::new()).is_err());
        page.items.clear();
        page.items.push(DisplayItem::Image {
            rect: Rect::default(),
            asset_id: "secret.png".to_owned(),
        });
        assert!(validate_page(&page, &HashSet::new()).is_err());
    }

    #[test]
    fn untrusted_form_controls_have_unique_ids_and_bounded_fields() {
        let control = DisplayItem::Control {
            rect: Rect {
                x: 0.,
                y: 0.,
                width: 200.,
                height: 32.,
            },
            id: 7,
            kind: browser_ipc::ControlKind::Text,
            name: "q".into(),
            value: "Rust".into(),
            placeholder: "Search".into(),
            form: Some(browser_ipc::FormInfo {
                id: 4,
                action: "/search".into(),
                method: "get".into(),
            }),
            disabled: false,
            appearance: Default::default(),
            options: vec![],
        };
        let mut page = RenderedPage {
            title: "Forms".into(),
            width: 800.,
            height: 600.,
            background: Color::default(),
            items: vec![control.clone()],
            links: vec![],
            metrics: Default::default(),
            warnings: vec![],
            next_timer_ms: None,
            animation_frame_pending: false,
        };
        assert!(validate_page(&page, &HashSet::new()).is_ok());
        page.items.push(control);
        assert!(validate_page(&page, &HashSet::new()).is_err());
        page.items.pop();
        if let DisplayItem::Control { value, .. } = &mut page.items[0] {
            *value = "x".repeat(8193);
        }
        assert!(validate_page(&page, &HashSet::new()).is_err());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn watchdog_terminates_an_unresponsive_child_with_both_pipes_open() {
        #[cfg(unix)]
        let command = {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "while :; do :; done"]);
            command
        };
        #[cfg(windows)]
        let command = {
            use std::os::windows::process::CommandExt;
            let executable = std::env::var_os("SystemRoot")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("C:\\Windows"))
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let mut command = Command::new(executable);
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "while ($true) {}",
            ]);
            command.creation_flags(0x0800_0000);
            command
        };
        let start = std::time::Instant::now();
        let error = exchange(command, request(), Duration::from_millis(150)).unwrap_err();
        assert!(error.contains("tempo limite"), "{error}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
