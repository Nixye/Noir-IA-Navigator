//! Engine entry point: parse → cascade → layout → data-only display list.
use browser_ipc::{DocumentInput, ImageAsset, RenderMetrics, RenderedPage, Viewport};
use std::sync::Arc;
use std::time::Instant;
mod geometry;
pub use web_bindings::{PlatformError, PlatformServices};

pub fn render(document: &DocumentInput, viewport: Viewport) -> Result<RenderedPage, String> {
    RenderSession::new_with_viewport(document, viewport)?.render(viewport)
}

/// A retained DOM and original JS VM. The protocol supervisor owns the lifetime
/// and dispatches user input to this exact session; page scripts are not replayed.
pub struct RenderSession {
    session: web_bindings::DocumentSession,
    images: Arc<Vec<ImageAsset>>,
    stylesheets: Arc<Vec<browser_ipc::StylesheetAsset>>,
    geometry_viewport: Viewport,
    parse_us: u64,
    warnings: Vec<String>,
    // Kept outside the restorable DOM/VM checkpoint, including failed callbacks.
    last_animation_frame_ms: Option<f64>,
}

impl RenderSession {
    pub fn new(document: &DocumentInput) -> Result<Self, String> {
        Self::new_with_viewport(document, Viewport::default())
    }

    pub fn new_with_viewport(document: &DocumentInput, viewport: Viewport) -> Result<Self, String> {
        Self::new_with_storage(
            document,
            viewport,
            browser_ipc::SessionStorageSeed::default(),
        )
    }

    pub fn new_with_storage(
        document: &DocumentInput,
        viewport: Viewport,
        seed: browser_ipc::SessionStorageSeed,
    ) -> Result<Self, String> {
        Self::new_with_optional_services(document, viewport, seed, None)
    }

    pub fn new_with_services(
        document: &DocumentInput,
        viewport: Viewport,
        seed: browser_ipc::SessionStorageSeed,
        services: Arc<dyn PlatformServices>,
    ) -> Result<Self, String> {
        Self::new_with_optional_services(document, viewport, seed, Some(services))
    }

    fn new_with_optional_services(
        document: &DocumentInput,
        viewport: Viewport,
        seed: browser_ipc::SessionStorageSeed,
        services: Option<Arc<dyn PlatformServices>>,
    ) -> Result<Self, String> {
        browser_ipc::validate_session_storage_seed(&seed)?;
        browser_ipc::validate_audio_metadata(&document.audio_metadata)?;
        validate_images(document)?;
        validate_stylesheets(document)?;
        let start = Instant::now();
        let dom = web_html::parse(&document.html)?;
        let parse_us = micros(start.elapsed());
        let mut session = web_bindings::DocumentSession::new(dom);
        if let Some(services) = services {
            session
                .set_platform_services(services)
                .map_err(|error| error.to_string())?;
        }
        session
            .set_document_url(&document.url)
            .map_err(|error| error.to_string())?;
        session
            .set_audio_metadata(document.audio_metadata.clone())
            .map_err(|error| error.to_string())?;
        session
            .set_session_storage(seed)
            .map_err(|error| error.to_string())?;
        let images = Arc::new(document.images.clone());
        let stylesheets = Arc::new(document.stylesheets.clone());
        session.set_geometry_resolver(Arc::new(geometry::PageGeometry {
            viewport,
            images: images.clone(),
            stylesheets: stylesheets.clone(),
        }));
        let report = session.run_initial(&document.scripts);
        let warnings = report.warnings;
        Ok(Self {
            session,
            images,
            stylesheets,
            geometry_viewport: viewport,
            parse_us,
            warnings,
            last_animation_frame_ms: None,
        })
    }

    pub fn session_storage_snapshot(&self) -> browser_ipc::SessionStorageSnapshot {
        self.session.session_storage_snapshot()
    }

    pub fn take_script_requests(&mut self) -> Vec<browser_ipc::DynamicScriptRequest> {
        self.session.take_script_requests()
    }

    pub fn complete_scripts(
        &mut self,
        responses: &[browser_ipc::DynamicScriptResponse],
    ) -> Result<(), String> {
        let report = self
            .session
            .complete_scripts(responses)
            .map_err(|error| error.to_string())?;
        self.warnings.extend(
            report
                .warnings
                .into_iter()
                .take(32usize.saturating_sub(self.warnings.len())),
        );
        Ok(())
    }

    pub fn render(&mut self, viewport: Viewport) -> Result<RenderedPage, String> {
        self.update_geometry(viewport);
        // Remove detached content from this paint-only snapshot without renumbering
        // node IDs: native controls must continue targeting their retained DOM node.
        let dom = geometry::paint_document(&self.session.document)?;
        let start = Instant::now();
        let computed = web_css::compute_with_stylesheets(&dom, &self.stylesheets, viewport)?;
        let style_us = micros(start.elapsed());
        let start = Instant::now();
        let layout = web_layout::layout(&dom, &computed.styles, &self.images, viewport)?;
        let layout_us = micros(start.elapsed());
        let mut warnings = self.warnings.clone();
        warnings.extend(computed.warnings);
        warnings.extend(layout.warnings);
        Ok(RenderedPage {
            title: dom.title().chars().take(1024).collect(),
            width: layout.width,
            height: layout.height,
            background: layout.background,
            items: layout.items,
            links: layout.links,
            metrics: RenderMetrics {
                parse_us: self.parse_us,
                style_us,
                layout_us,
                node_count: dom.nodes.len(),
                worker_pid: std::process::id(),
                resident_memory_bytes: None,
            },
            warnings,
            next_timer_ms: self.session.next_timer_ms(),
            animation_frame_pending: self.session.animation_frame_pending(),
        })
    }

    pub fn dispatch_event(
        &mut self,
        node: u64,
        event: &str,
        viewport: Viewport,
    ) -> Result<RenderedPage, String> {
        self.update_geometry(viewport);
        let node = usize::try_from(node).map_err(|_| "Invalid DOM event node")?;
        if event.len() > 64 {
            return Err("DOM event type exceeds limit".into());
        }
        if let Err(error) = self.session.dispatch_event(node, event)
            && self.warnings.len() < 32
        {
            self.warnings
                .push(format!("JavaScript event error: {error}"));
        }
        self.render(viewport)
    }

    pub fn update_control_value(&mut self, node: u64, value: &str) -> Result<(), String> {
        let node = usize::try_from(node).map_err(|_| "Invalid native control node")?;
        self.session
            .update_control_value(node, value)
            .map_err(|error| error.to_string())
    }

    pub fn advance_timers(
        &mut self,
        elapsed_ms: u64,
        viewport: Viewport,
    ) -> Result<RenderedPage, String> {
        self.update_geometry(viewport);
        if let Err(error) = self.session.run_due_timers(elapsed_ms)
            && self.warnings.len() < 32
        {
            self.warnings
                .push(format!("JavaScript timer error: {error}"));
        }
        self.render(viewport)
    }

    /// Execute one explicit callback batch, then one display-list render. This
    /// does not advance timers and promises neither vsync nor GPU presentation.
    pub fn animation_frame(
        &mut self,
        timestamp_ms: f64,
        viewport: Viewport,
    ) -> Result<RenderedPage, String> {
        browser_ipc::validate_animation_frame_timestamp(
            timestamp_ms,
            self.last_animation_frame_ms,
        )?;
        if !viewport.width.is_finite()
            || !viewport.height.is_finite()
            || !(1.0..=16384.).contains(&viewport.width)
            || !(1.0..=16384.).contains(&viewport.height)
        {
            return Err("Invalid animation frame viewport".into());
        }
        self.last_animation_frame_ms = Some(timestamp_ms);
        self.update_geometry(viewport);
        if let Err(error) = self.session.run_animation_frame(timestamp_ms)
            && self.warnings.len() < 32
        {
            self.warnings
                .push(format!("JavaScript animation frame error: {error}"));
        }
        self.render(viewport)
    }

    fn update_geometry(&mut self, viewport: Viewport) {
        if self.geometry_viewport.width != viewport.width
            || self.geometry_viewport.height != viewport.height
        {
            self.session
                .set_geometry_resolver(Arc::new(geometry::PageGeometry {
                    viewport,
                    images: self.images.clone(),
                    stylesheets: self.stylesheets.clone(),
                }));
            self.geometry_viewport = viewport;
        }
    }
}

fn micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}

fn validate_images(document: &DocumentInput) -> Result<(), String> {
    let mut total = 0usize;
    let mut id_bytes = 0usize;
    let mut ids = std::collections::HashSet::new();
    if document.images.len() > 32 {
        return Err("Image asset count limit exceeded".into());
    }
    for image in &document.images {
        id_bytes = id_bytes
            .checked_add(image.id.len())
            .ok_or("Image identifier size overflow")?;
        if !browser_ipc::valid_image_id(&image.id)
            || !ids.insert(&image.id)
            || id_bytes > browser_ipc::MAX_IMAGE_ID_TOTAL_BYTES
        {
            return Err("Invalid image identifier".into());
        }
        let expected = (image.width as usize)
            .checked_mul(image.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("Image dimensions overflow")?;
        if image.width == 0
            || image.height == 0
            || image.width > 1024
            || image.height > 1024
            || !browser_ipc::valid_natural_image_dimensions(
                image.natural_width,
                image.natural_height,
            )
            || expected != image.rgba.len()
            || expected > browser_ipc::MAX_IMAGE_BYTES
        {
            return Err("Invalid or oversized RGBA image asset".into());
        }
        total = total.checked_add(expected).ok_or("Image total overflow")?;
        if total > browser_ipc::MAX_IMAGE_TOTAL_BYTES {
            return Err("Image asset budget exceeded".into());
        }
    }
    Ok(())
}

fn validate_stylesheets(document: &DocumentInput) -> Result<(), String> {
    use browser_ipc::{MAX_STYLESHEET_BYTES, MAX_STYLESHEET_TOTAL_BYTES, MAX_STYLESHEETS};
    if document.stylesheets.len() > MAX_STYLESHEETS {
        return Err("Stylesheet count exceeded".into());
    }
    let mut total = 0usize;
    let mut seen = std::collections::HashSet::new();
    for sheet in &document.stylesheets {
        if sheet.href.is_empty()
            || sheet.href.len() > 8192
            || sheet.href.chars().any(char::is_control)
            || !seen.insert(&sheet.href)
            || sheet.source.len() > MAX_STYLESHEET_BYTES
        {
            return Err("Invalid stylesheet asset".into());
        }
        total = total
            .checked_add(sheet.source.len())
            .ok_or("Stylesheet size overflow")?;
        if total > MAX_STYLESHEET_TOTAL_BYTES {
            return Err("Stylesheet byte budget exceeded".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn frame_callbacks_precede_paint_and_never_advance_timers() {
        let document = input(
            "<title>initial</title><p id=out>before</p><script>setTimeout(()=>{document.title='timer';},1);requestAnimationFrame(t=>{document.getElementById('out').textContent=String(t);requestAnimationFrame(()=>{document.title='next frame';});});</script>",
        );
        let viewport = Viewport::default();
        let mut session = RenderSession::new(&document).unwrap();
        let initial = session.render(viewport).unwrap();
        assert!(initial.warnings.is_empty(), "{:?}", initial.warnings);
        assert!(initial.animation_frame_pending);
        assert!(session.render(viewport).unwrap().animation_frame_pending);
        assert!(
            session
                .advance_timers(0, viewport)
                .unwrap()
                .animation_frame_pending
        );
        let page = session.animation_frame(12.5, viewport).unwrap();
        assert!(page.warnings.is_empty(), "{:?}", page.warnings);
        assert_eq!(page.title, "initial");
        assert_eq!(page.next_timer_ms, Some(1));
        assert!(page.animation_frame_pending);
        assert!(
            page.items
                .iter()
                .any(|item| matches!(item, DisplayItem::Text { text, .. } if text == "12.5"))
        );
        let next = session.animation_frame(12.5, viewport).unwrap();
        assert_eq!(next.title, "next frame");
        assert!(!next.animation_frame_pending);
        assert_eq!(session.advance_timers(1, viewport).unwrap().title, "timer");
    }

    #[test]
    fn invalid_frame_pulses_leave_callbacks_pending_and_time_does_not_rewind() {
        let document = input(
            "<title>before</title><script>requestAnimationFrame(()=>{document.title='after';});</script>",
        );
        let viewport = Viewport::default();
        let mut session = RenderSession::new(&document).unwrap();
        for timestamp in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.,
            browser_ipc::MAX_ANIMATION_FRAME_TIMESTAMP_MS + 1.,
        ] {
            assert!(session.animation_frame(timestamp, viewport).is_err());
            let page = session.render(viewport).unwrap();
            assert!(page.animation_frame_pending);
            assert_eq!(page.title, "before");
        }
        assert!(
            session
                .animation_frame(
                    100.,
                    Viewport {
                        width: f32::NAN,
                        height: 600.
                    }
                )
                .is_err()
        );
        assert_eq!(
            session.animation_frame(2., viewport).unwrap().title,
            "after"
        );
        assert!(session.animation_frame(1., viewport).is_err());
        assert!(session.animation_frame(2., viewport).is_ok());
    }

    #[test]
    fn resource_frame_rolls_back_storage_and_dom_without_renewing_time() {
        let mut document = input(
            "<title>before</title><script>requestAnimationFrame(()=>{document.title='lost';sessionStorage.setItem('key','lost');new Event('x'.repeat(65));});</script>",
        );
        document.url = "https://example.test/".into();
        let viewport = Viewport::default();
        let mut session = RenderSession::new_with_storage(
            &document,
            viewport,
            browser_ipc::SessionStorageSeed {
                access: browser_ipc::SessionStorageAccess::ReadWrite,
                entries: vec![("key".into(), "saved".into())],
            },
        )
        .unwrap();
        let page = session.animation_frame(20., viewport).unwrap();
        assert_eq!(page.title, "before");
        assert!(
            page.warnings
                .iter()
                .any(|warning| warning.contains("animation frame error"))
        );
        assert!(!page.animation_frame_pending);
        assert_eq!(
            session.session_storage_snapshot(),
            vec![("key".into(), "saved".into())]
        );
        assert!(session.animation_frame(19., viewport).is_err());
        assert_eq!(
            session.animation_frame(20., viewport).unwrap().title,
            "before"
        );
    }

    #[test]
    fn audio_metadata_rejects_remote_origin_and_invalid_facts() {
        let mut document = input("<audio src=a.wav preload=metadata></audio>");
        document.audio_metadata = vec![browser_ipc::AudioMetadata {
            src: "a.wav".into(),
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
            block_align: 2,
            frames: 8000,
        }];
        document.url = "https://example.test/".into();
        assert!(RenderSession::new(&document).is_err());
        document.url = "file:///a.html".into();
        assert!(RenderSession::new(&document).is_ok());
        document.audio_metadata[0].frames = u64::MAX;
        assert!(RenderSession::new(&document).is_err());
    }
    use super::*;
    use browser_ipc::{Color, DisplayItem, ImageAsset};
    fn input(html: &str) -> DocumentInput {
        DocumentInput {
            url: "noir:test".into(),
            html: html.into(),
            images: vec![],
            audio_metadata: vec![],
            scripts: vec![],
            stylesheets: vec![],
        }
    }
    // These fixtures have one painted paragraph plus a native button control.
    fn assert_output_text(session: &RenderSession, page: &RenderedPage, expected: &str) {
        assert!(page.warnings.is_empty(), "{:?}", page.warnings);
        let dom = &session.session.document;
        let output = dom
            .elements()
            .find_map(|(id, element)| (element.attr("id") == Some("out")).then_some(id))
            .unwrap();
        assert_eq!(dom.text_content(output), expected);
        let fragments = page
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text {
                    text, rect, size, ..
                } => Some((text.as_str(), rect, *size)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            fragments
                .iter()
                .map(|(text, _, _)| *text)
                .collect::<Vec<_>>(),
            expected.split(' ').collect::<Vec<_>>()
        );
        for (text, rect, size) in &fragments {
            assert!((rect.width - web_layout::measure_text(text, *size)).abs() < 0.001);
        }
        // Collapsed spaces advance the pen, rather than creating Text items.
        // Checking geometry prevents a join(" ") from hiding concatenated words.
        for pair in fragments.windows(2) {
            let (_, left, _) = pair[0];
            let (_, right, size) = pair[1];
            assert_eq!(left.y, right.y);
            let gap = right.x - (left.x + left.width);
            assert!(gap > 0.);
            assert!((gap - web_layout::measure_text(" ", size)).abs() < 0.001);
        }
    }

    #[test]
    fn storage_seed_precedes_initial_scripts_and_survives_event_tick_reflow() {
        let mut document = input(
            "<button id=b type=button>Write</button><p id=out></p><script>document.getElementById('out').textContent=sessionStorage.getItem('seed');sessionStorage.setItem('initial','yes');document.getElementById('b').addEventListener('click',()=>{sessionStorage.setItem('event','yes');});setTimeout(()=>{sessionStorage.setItem('timer','yes');},5);</script>",
        );
        document.url = "https://example.test/path".into();
        let seed = browser_ipc::SessionStorageSeed {
            access: browser_ipc::SessionStorageAccess::ReadWrite,
            entries: vec![("seed".into(), "before scripts".into())],
        };
        let viewport = Viewport::default();
        let mut session = RenderSession::new_with_storage(&document, viewport, seed).unwrap();
        let page = session.render(viewport).unwrap();
        assert_output_text(&session, &page, "before scripts");
        assert!(
            session
                .session_storage_snapshot()
                .contains(&("initial".into(), "yes".into()))
        );
        let button = page
            .items
            .iter()
            .find_map(|item| match item {
                DisplayItem::Control {
                    id,
                    kind: browser_ipc::ControlKind::Button,
                    ..
                } => Some(*id),
                _ => None,
            })
            .unwrap();
        let page = session.dispatch_event(button, "click", viewport).unwrap();
        assert_output_text(&session, &page, "before scripts");
        assert!(
            session
                .session_storage_snapshot()
                .contains(&("event".into(), "yes".into()))
        );
        let page = session.advance_timers(10, viewport).unwrap();
        assert_output_text(&session, &page, "before scripts");
        assert!(
            session
                .session_storage_snapshot()
                .contains(&("timer".into(), "yes".into()))
        );
        let saved = session.session_storage_snapshot();
        let page = session
            .render(Viewport {
                width: 900.,
                height: 600.,
            })
            .unwrap();
        assert_output_text(&session, &page, "before scripts");
        assert_eq!(session.session_storage_snapshot(), saved);
    }
    #[test]
    fn flex_stretch_client_height_recomputes_after_mutation_and_viewport_change() {
        let document = input(
            "<!doctype html><style>body{margin:0}#row{display:flex;height:100px}#row.tall{height:150px}#item{border:2px solid black;padding:5px}@media(min-width:500px){#row.tall{height:200px}}</style><div id=row><div id=item></div></div><button id=apply type=button>Measure</button><p id=out></p><script>const row=document.getElementById('row');const item=document.getElementById('item');const out=document.getElementById('out');out.textContent=String(item.clientHeight);let changed=false;document.getElementById('apply').addEventListener('click',()=>{if(!changed){row.setAttribute('class','tall');changed=true;}out.textContent=String(item.clientHeight);});</script>",
        );
        let narrow = Viewport {
            width: 320.,
            height: 300.,
        };
        let wide = Viewport {
            width: 600.,
            height: 300.,
        };
        let mut session = RenderSession::new_with_viewport(&document, narrow).unwrap();
        let page = session.render(narrow).unwrap();
        let contains = |page: &RenderedPage, expected: &str| {
            page.items
                .iter()
                .any(|item| matches!(item, DisplayItem::Text { text, .. } if text == expected))
        };
        assert!(page.warnings.is_empty(), "{:?}", page.warnings);
        assert!(contains(&page, "96"));
        let button = page
            .items
            .iter()
            .find_map(|item| match item {
                DisplayItem::Control {
                    id,
                    kind: browser_ipc::ControlKind::Button,
                    ..
                } => Some(*id),
                _ => None,
            })
            .unwrap();
        let mutated = session.dispatch_event(button, "click", narrow).unwrap();
        assert!(mutated.warnings.is_empty(), "{:?}", mutated.warnings);
        assert!(contains(&mutated, "146"));
        // The second callback only measures: viewport invalidation must suffice.
        let resized = session.dispatch_event(button, "click", wide).unwrap();
        assert!(resized.warnings.is_empty(), "{:?}", resized.warnings);
        assert!(contains(&resized, "196"));
    }

    #[test]
    fn compatibility_fixture_executes_language_dom_and_css_together() {
        let document = input(include_str!("../../../fixtures/compatibility-v06.html"));
        let page = render(&document, Viewport::default()).unwrap();
        assert!(page.warnings.is_empty(), "{:?}", page.warnings);
        let text = page
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("JS ok:"), "{text}");
        assert!(text.contains("DOM ok:"), "{text}");
        assert!(!text.contains("Aguardando"), "{text}");
        assert!(page.items.iter().any(|item| matches!(
            item,
            DisplayItem::Rect {
                color: Color(217, 237, 204, 255),
                ..
            }
        )));
        assert!(page.items.iter().any(|item| matches!(item,
            DisplayItem::Text {text,color: Color(23,98,56,255),bold:true,..} if text == "JS"
        )));
    }
    #[test]
    fn inert_ancestor_end_tags_cannot_execute_inline_scripts() {
        for tag in ["template", "noscript"] {
            let html = format!(
                "<title>Safe</title><body><{tag}></body></html><script>document.title='Escaped';</script></{tag}><p>Live</p>"
            );
            let page = render(&input(&html), Viewport::default()).unwrap();
            assert_eq!(page.title, "Safe", "{tag}");
            assert!(page.warnings.is_empty(), "{tag}: {:?}", page.warnings);
            assert!(
                page.items.iter().any(|item| matches!(
                    item, DisplayItem::Text { text, .. } if text == "Live"
                )),
                "{tag}"
            );
        }
    }
    #[test]
    fn external_css_keeps_dom_order_and_cannot_inject_html() {
        let mut document = input(
            "<title>Safe</title><link rel=stylesheet href=sheet.css><style>p{color:#0000ff}</style><p>Safe text</p>",
        );
        document.stylesheets.push(browser_ipc::StylesheetAsset {
            href: "sheet.css".into(),
            source: "p{color:#ff0000}/* </style><script>document.title='Injected'</script> */"
                .into(),
        });
        let page = render(&document, Viewport::default()).unwrap();
        assert_eq!(page.title, "Safe");
        assert!(page.items.iter().any(|item| matches!(item, DisplayItem::Text { color: Color(0,0,255,255), text, .. } if text.contains("Safe"))));
        document.html =
            "<style>p{color:#0000ff}</style><link rel=stylesheet href=sheet.css><p>Safe text</p>"
                .into();
        let page = render(&document, Viewport::default()).unwrap();
        assert!(page.items.iter().any(|item| matches!(item, DisplayItem::Text { color: Color(255,0,0,255), text, .. } if text.contains("Safe"))));
    }
    #[test]
    fn real_document_wraps_links_and_respects_cascade() {
        let page=render(&input("<title>Teste</title><style>body{margin:0;padding:8px} .card{width:140px;padding:12px;background:#123456} .hide{display:none}</style><body><div class=card><h2>Ação local</h2><a href='next.html'>Uma ligação com palavras para quebrar</a><span class=hide>secret</span></div></body>"),Viewport{width:320.0,height:200.0}).unwrap();
        assert_eq!(page.title, "Teste");
        assert!(!page.links.is_empty());
        assert!(page.items.iter().any(|item| matches!(
            item,
            DisplayItem::Rect {
                color: Color(18, 52, 86, 255),
                ..
            }
        )));
        assert!(
            !page
                .items
                .iter()
                .any(|item| matches!(item,DisplayItem::Text{text,..} if text.contains("secret")))
        );
        assert!(page.links.iter().all(|link| link.href == "next.html"));
        let ys = page
            .links
            .iter()
            .map(|l| l.rect.y as u32)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(ys.len() > 1);
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
        let mut document = input("<img src=a>");
        document.images = vec![image.clone()];
        assert!(validate_images(&document).is_ok());
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
            document.images = vec![image.clone()];
            assert!(validate_images(&document).is_err());
            assert!(render(&document, Viewport::default()).is_err());
        }
    }
    #[test]
    fn images_use_supplied_assets_and_script_failures_are_diagnosed() {
        let mut d = input(
            "<script>throw 'must not run'</script><img src=a width=20 height=10><img src=missing alt='Sem imagem'>",
        );
        d.images.push(ImageAsset {
            id: "a".into(),
            width: 1,
            height: 1,
            natural_width: 1.,
            natural_height: 1.,
            rgba: vec![255, 0, 0, 255],
        });
        let page = render(&d, Viewport::default()).unwrap();
        assert!(page.items.iter().any(|i|matches!(i,DisplayItem::Image{rect,asset_id} if asset_id=="a"&&rect.width==20.0&&rect.height==10.0)));
        assert!(page.warnings.iter().any(|w| w.contains("JavaScript")));
        assert!(page.warnings.iter().any(|w| w.contains("missing")));
    }
    #[test]
    fn adversarial_content_is_bounded() {
        assert!(render(&input(&"<div>".repeat(150)), Viewport::default()).is_err());
        let d = input(&format!(
            "<style>p{{width:NaNpx}}</style><p>{}</p>",
            "é".repeat(10_000)
        ));
        let page = render(
            &d,
            Viewport {
                width: 200.0,
                height: 100.0,
            },
        )
        .unwrap();
        assert!(page.height.is_finite() && page.items.len() < 40_000);
        assert!(page.items.iter().all(|i| match i {
            DisplayItem::PopClip => true,
            DisplayItem::PushClip { rect }
            | DisplayItem::Rect { rect, .. }
            | DisplayItem::Text { rect, .. }
            | DisplayItem::Image { rect, .. }
            | DisplayItem::Control { rect, .. } =>
                rect.x.is_finite()
                    && rect.y.is_finite()
                    && rect.width.is_finite()
                    && rect.height.is_finite(),
        }));
    }
    #[test]
    fn retained_js_session_updates_controls_and_survives_reflow() {
        let document = input(
            "<input id=name value=Before><button id=b type=button>Apply</button><p id=out>Before</p><script>let count=0;const button=document.getElementById('b');button.addEventListener('click',()=>{count++;button.textContent=String(count);document.getElementById('out').textContent=document.getElementById('name').value;});</script>",
        );
        let mut session = RenderSession::new(&document).unwrap();
        let page = session.render(Viewport::default()).unwrap();
        let field = page
            .items
            .iter()
            .find_map(|item| {
                if let DisplayItem::Control {
                    id,
                    kind: browser_ipc::ControlKind::Text,
                    ..
                } = item
                {
                    Some(*id)
                } else {
                    None
                }
            })
            .unwrap();
        let button = page
            .items
            .iter()
            .find_map(|item| {
                if let DisplayItem::Control {
                    id,
                    kind: browser_ipc::ControlKind::Button,
                    ..
                } = item
                {
                    Some(*id)
                } else {
                    None
                }
            })
            .unwrap();
        session.update_control_value(field, "Atualizado").unwrap();
        let first = session
            .dispatch_event(button, "click", Viewport::default())
            .unwrap();
        assert!(
            first
                .items
                .iter()
                .any(|item| matches!(item,DisplayItem::Text{text,..}if text=="Atualizado"))
        );
        session
            .render(Viewport {
                width: 600.,
                height: 400.,
            })
            .unwrap();
        let second = session
            .dispatch_event(button, "click", Viewport::default())
            .unwrap();
        assert!(second.items.iter().any(
            |item| matches!(item,DisplayItem::Control{id,value,..}if *id==button&&value=="2")
        ));
    }

    #[test]
    fn native_click_dispatches_custom_event_and_reflows_updated_dom() {
        let document = input(
            "<button id=b type=button>Apply</button><p id=out>Before</p><script>const button=document.getElementById('b');document.addEventListener('update',event=>{document.getElementById('out').textContent=event.detail.label;});button.addEventListener('click',()=>{button.dispatchEvent(new CustomEvent('update',{bubbles:true,detail:{label:'Custom updated'}}));});</script>",
        );
        let mut session = RenderSession::new(&document).unwrap();
        let page = session.render(Viewport::default()).unwrap();
        assert_output_text(&session, &page, "Before");
        let button = page
            .items
            .iter()
            .find_map(|item| {
                if let DisplayItem::Control {
                    id,
                    kind: browser_ipc::ControlKind::Button,
                    ..
                } = item
                {
                    Some(*id)
                } else {
                    None
                }
            })
            .unwrap();
        let page = session
            .dispatch_event(button, "click", Viewport::default())
            .unwrap();
        assert_output_text(&session, &page, "Custom updated");
    }
}
