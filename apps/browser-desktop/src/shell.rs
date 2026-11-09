use crate::{
    ai_panel::AiPanel,
    find::Matches,
    home::{self, Home},
    icons::{self, Icon},
    theme::{self, ACCENT, AMBER, BG, MUTED, PANEL, SURFACE, TEXT},
};
use browser_config::{MAX_TABS, MAX_TOTAL_TABS, Profile};
use browser_ipc::{ControlKind, DisplayItem, DocumentInput, FormInfo, RenderedPage, Viewport};
use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Vec2};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
const HOME: &str = "noir:welcome";
const SETTINGS: &str = "noir:settings";
#[derive(Clone, Copy)]
enum Navigation {
    Push,
    Traverse(usize),
    Refresh,
    Reflow,
    Event(u64),
    Input(u64),
    Change(u64),
    Tick(u64),
    Frame(f64),
}
// Session-only CSS coordinates, never serialized into the profile.
const MAX_HISTORY_SCROLL: f32 = 1_000_000.;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct HistoryScroll {
    x: f32,
    y: f32,
}
impl HistoryScroll {
    fn from_display(offset: Vec2, content: Vec2, viewport: Vec2, zoom: f32) -> Self {
        if !zoom.is_finite() || zoom <= 0. {
            return Self::default();
        }
        fn axis(offset: f32, content: f32, viewport: f32, zoom: f32) -> f32 {
            if !offset.is_finite()
                || !content.is_finite()
                || !viewport.is_finite()
                || content < 0.
                || viewport < 0.
            {
                return 0.;
            }
            let maximum = ((content / zoom).min(MAX_HISTORY_SCROLL) - viewport / zoom).max(0.);
            (offset / zoom).clamp(0., maximum)
        }
        Self {
            x: axis(offset.x, content.x, viewport.x, zoom),
            y: axis(offset.y, content.y, viewport.y, zoom),
        }
    }
    fn display_offset(self, zoom: f32) -> Vec2 {
        Vec2::new(self.x * zoom, self.y * zoom)
    }
}
#[derive(Debug, PartialEq)]
struct HistoryEntry {
    url: String,
    scroll: HistoryScroll,
}
impl HistoryEntry {
    fn new(url: String) -> Self {
        Self {
            url,
            scroll: HistoryScroll::default(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingScroll {
    document: u64,
    position: HistoryScroll,
}
fn reset_document_scroll(ui: &egui::Ui, tab: u64) -> egui::Id {
    // ScrollArea hashes the supplied salt once before make_persistent_id.
    let id = ui.make_persistent_id(egui::IdSalt::new(("page", tab)));
    egui::scroll_area::State::default().store(ui.ctx(), id);
    id
}
type SharedSession = Arc<Mutex<browser_process::RendererSession>>;
struct LoadedDocument {
    document: Arc<DocumentInput>,
    page: RenderedPage,
    session: SharedSession,
    // Trusted loader metadata from the final broker URL, never from the frame.
    storage_origin: Option<String>,
    animation_origin: Instant,
}
type LoadResult = Result<LoadedDocument, String>;
struct CommittedStorageOperation {
    document: u64,
    origin: Option<String>,
    session: SharedSession,
}
fn retains_session(navigation: Navigation) -> bool {
    matches!(
        navigation,
        Navigation::Reflow
            | Navigation::Event(_)
            | Navigation::Input(_)
            | Navigation::Change(_)
            | Navigation::Tick(_)
            | Navigation::Frame(_)
    )
}
fn accepts_committed_checkpoint(
    navigation: Navigation,
    expected: u64,
    current: u64,
    same_session: bool,
) -> bool {
    retains_session(navigation) && expected == current && same_session
}
fn animation_opportunity_available(
    opportunity: (egui::ViewportId, u64),
    previous: Option<(egui::ViewportId, u64)>,
    visible: Option<bool>,
    closing: bool,
    discarded: bool,
    priority_work: bool,
) -> bool {
    opportunity.0 == egui::ViewportId::ROOT
        && previous != Some(opportunity)
        && visible != Some(false)
        && !closing
        && !discarded
        && !priority_work
}
struct Pending {
    receiver: mpsc::Receiver<LoadResult>,
    committed_storage: Option<CommittedStorageOperation>,
    navigation: Navigation,
    started: Instant,
    viewport: Viewport,
}
struct Tab {
    id: u64,
    generation: u64,
    selected_element: Option<String>,
    focus_element: bool,
    dismissed_compatibility: bool,
    pinned: bool,
    title: String,
    url: String,
    address: String,
    history: Vec<HistoryEntry>,
    history_index: usize,
    document: Option<Arc<DocumentInput>>,
    page: Option<RenderedPage>,
    pending: Option<Pending>,
    queued: Option<(String, Navigation)>,
    error: Option<String>,
    textures: HashMap<String, egui::TextureHandle>,
    viewport: Viewport,
    rendered_width: f32,
    needs_initial_load: bool,
    // Unlike generation, this changes only on accepted document navigation.
    committed_document: u64,
    displayed_document: Option<u64>,
    pending_scroll: Option<PendingScroll>,
    matches: Matches,
    controls: HashMap<u64, String>,
    session: Option<SharedSession>,
    session_origin: Option<String>,
    session_storage: browser_process::TabSessionStorage,
    home: Home,
    timer_mark: Instant,
    animation_origin: Option<Instant>,
}
impl Tab {
    fn animation_frame_ready(&self) -> bool {
        self.url != HOME
            && self.url != SETTINGS
            && !self.needs_initial_load
            && self.error.is_none()
            && self.pending.is_none()
            && self.queued.is_none()
            && self.document.is_some()
            && self.session.is_some()
            && self.animation_origin.is_some()
            && self.displayed_document == Some(self.committed_document)
            && self
                .page
                .as_ref()
                .is_some_and(|page| page.animation_frame_pending)
    }
    fn new(id: u64, target: String) -> Self {
        let home = target == HOME || target == SETTINGS;
        Self {
            id,
            generation: 0,
            selected_element: None,
            focus_element: false,
            dismissed_compatibility: false,
            pinned: false,
            title: if home {
                if target == SETTINGS {
                    "Configurações".into()
                } else {
                    "Nova aba".into()
                }
            } else {
                "Carregando…".into()
            },
            url: target.clone(),
            address: if target == HOME {
                String::new()
            } else {
                target.clone()
            },
            history: if home {
                vec![HistoryEntry::new(target)]
            } else {
                vec![]
            },
            history_index: 0,
            document: None,
            page: None,
            pending: None,
            queued: None,
            error: None,
            textures: HashMap::new(),
            viewport: Viewport::default(),
            rendered_width: 0.,
            needs_initial_load: !home,
            committed_document: 0,
            displayed_document: None,
            pending_scroll: None,
            matches: Default::default(),
            controls: HashMap::new(),
            session: None,
            session_origin: None,
            session_storage: Default::default(),
            home: Default::default(),
            timer_mark: Instant::now(),
            animation_origin: None,
        }
    }
    fn checkpoint_session(&mut self) {
        if let (Some(origin), Some(session)) = (&self.session_origin, &self.session)
            && let Ok(worker) = session.try_lock()
        {
            // Only fully validated responses are exposed. Revalidate atomically
            // in the tab helper and never surface storage in diagnostics.
            let _ = self
                .session_storage
                .commit(origin, worker.storage_checkpoint());
        }
    }
    fn checkpoint_operation(
        &mut self,
        navigation: Navigation,
        operation: &CommittedStorageOperation,
    ) {
        let same_session = self
            .session
            .as_ref()
            .is_some_and(|session| Arc::ptr_eq(session, &operation.session));
        if accepts_committed_checkpoint(
            navigation,
            operation.document,
            self.committed_document,
            same_session,
        ) && self.session_origin == operation.origin
        {
            self.checkpoint_session();
        }
    }
    fn document_scroll_is_current(&self) -> bool {
        self.displayed_document == Some(self.committed_document)
            && self.error.is_none()
            && self.queued.is_none()
            && self.pending.as_ref().is_none_or(|pending| {
                matches!(
                    pending.navigation,
                    Navigation::Reflow
                        | Navigation::Event(_)
                        | Navigation::Input(_)
                        | Navigation::Change(_)
                        | Navigation::Tick(_)
                        | Navigation::Frame(_)
                )
            })
            && self.url != HOME
            && self.url != SETTINGS
    }
    fn scroll_to_restore(&self) -> Option<HistoryScroll> {
        self.pending_scroll
            .filter(|pending| {
                self.document_scroll_is_current() && pending.document == self.committed_document
            })
            .map(|pending| pending.position)
    }
    fn capture_scroll(&mut self, position: HistoryScroll) {
        if self.document_scroll_is_current() {
            if let Some(entry) = self.history.get_mut(self.history_index) {
                entry.scroll = position;
            }
            self.pending_scroll = None;
        }
    }
    fn commit(&mut self, url: String, title: String, nav: Navigation) {
        if !matches!(nav, Navigation::Tick(_) | Navigation::Frame(_)) {
            self.generation = self.generation.saturating_add(1);
        }
        if !matches!(
            nav,
            Navigation::Reflow
                | Navigation::Event(_)
                | Navigation::Input(_)
                | Navigation::Change(_)
                | Navigation::Tick(_)
                | Navigation::Frame(_)
        ) {
            match nav {
                Navigation::Push => {
                    self.history.truncate(if self.history.is_empty() {
                        0
                    } else {
                        self.history_index + 1
                    });
                    if self.history.last().is_none_or(|entry| entry.url != url) {
                        self.history.push(HistoryEntry::new(url.clone()));
                    }
                    if self.history.len() > 200 {
                        self.history.remove(0);
                    }
                    self.history_index = self.history.len().saturating_sub(1);
                }
                Navigation::Traverse(i) => self.history_index = i,
                _ => {}
            }
            if !matches!(nav, Navigation::Traverse(_))
                && let Some(entry) = self.history.get_mut(self.history_index)
            {
                entry.scroll = HistoryScroll::default();
            }
            self.committed_document = self.committed_document.wrapping_add(1);
            self.displayed_document = None;
            self.pending_scroll = if url == HOME || url == SETTINGS {
                None
            } else {
                Some(PendingScroll {
                    document: self.committed_document,
                    position: self
                        .history
                        .get(self.history_index)
                        .map_or_else(HistoryScroll::default, |entry| entry.scroll),
                })
            };
            self.dismissed_compatibility = false;
            self.textures.clear();
            self.controls.clear();
            self.selected_element = None;
            self.focus_element = false;
            self.timer_mark = Instant::now();
            self.address = if url == HOME {
                String::new()
            } else {
                url.clone()
            };
        }
        self.url = url;
        self.title = title;
        self.needs_initial_load = false;
        self.error = None;
    }
}
#[derive(Default)]
struct Editor {
    open: bool,
    id: Option<u64>,
    name: String,
    url: String,
    error: Option<String>,
}
struct AiOperation {
    tab: u64,
    expected_generation: u64,
    started: Instant,
}
type SnapshotKey = (u64, u64, Option<String>, usize);
pub struct Noir {
    tabs: Vec<Tab>,
    active: usize,
    parked: HashMap<u64, (Vec<Tab>, usize)>,
    next_id: u64,
    profile: Profile,
    profile_path: Option<PathBuf>,
    notice: Option<String>,
    show_ai: bool,
    show_inspector: bool,
    show_find: bool,
    find: String,
    focus_address: bool,
    focus_find: bool,
    zoom: f32,
    permissions: ai_context::PermissionGateway,
    capture: Option<PathBuf>,
    capture_ready_frames: u8,
    started: Instant,
    transition: Instant,
    ai: AiPanel,
    favorite_editor: Editor,
    workspace_editor: Editor,
    backdrop: crate::backdrop::Backdrop,
    backdrop_notice: Option<String>,
    settings_section: u8,
    ai_operation: Option<AiOperation>,
    snapshot_key: Option<SnapshotKey>,
    page_gpu: Option<crate::page_gpu::Gpu>,
    page_scene: crate::page_scene::Cache,
    graphics_error: Option<String>,
    frame_opportunity: Option<(egui::ViewportId, u64)>,
    displayed_frame_target: Option<(u64, u64, SharedSession)>,
    priority_work_this_pass: bool,
    closing: bool,
}
impl Noir {
    fn offer_animation_frame(&mut self, ctx: &egui::Context) {
        let opportunity = (ctx.viewport_id(), ctx.cumulative_frame_nr());
        // `visible` is advisory: None means not known to be hidden. Focus is
        // deliberately irrelevant. This is an egui UI opportunity, not a
        // guarantee of vsync, same-frame painting, or GPU presentation.
        let (visible, close_requested) = ctx.input(|input| {
            (
                input.viewport().visible(),
                input.viewport().close_requested(),
            )
        });
        if !animation_opportunity_available(
            opportunity,
            self.frame_opportunity,
            visible,
            self.closing || close_requested,
            ctx.will_discard(),
            self.priority_work_this_pass,
        ) || self.ai_operation.is_some()
            || self.ai.browser_busy()
        {
            return;
        }
        let tab = &self.tabs[self.active];
        let displayed =
            self.displayed_frame_target
                .as_ref()
                .is_some_and(|(id, document, session)| {
                    *id == tab.id
                        && *document == tab.committed_document
                        && tab
                            .session
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, session))
                });
        if !displayed || !tab.animation_frame_ready() {
            return;
        }
        // Real elapsed time keeps hidden/inactive intervals without replay,
        // clamping or synthetic increments; equal timestamps are permitted.
        let timestamp_ms = tab
            .animation_origin
            .expect("ready origin")
            .elapsed()
            .as_secs_f64()
            * 1000.;
        let target = tab.url.clone();
        self.navigate(target, Navigation::Frame(timestamp_ms), ctx);
        if self.tabs[self.active].pending.is_some() {
            self.frame_opportunity = Some(opportunity);
            // A repaint requests a later UI opportunity, never another clock.
            ctx.request_repaint();
        }
        // Existing due timers have priority and may delay rAF, including
        // zero-delay timers. This is not a replacement event-loop scheduler.
    }
    pub fn open_assistant(&mut self) {
        self.show_ai = true;
    }
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial: Option<String>,
        capture: Option<PathBuf>,
        temporary_profile: bool,
    ) -> Self {
        theme::install(&cc.egui_ctx);
        let profile_path = if capture.is_some() || temporary_profile {
            None
        } else {
            browser_config::default_path()
        };
        let (profile, notice) = match profile_path
            .as_ref()
            .map(|p| browser_config::load_with_report(p))
        {
            Some(Ok(report)) => (
                report.profile,
                report
                    .recovered_from_backup
                    .then(|| "Seu perfil foi recuperado da cópia de segurança.".into()),
            ),
            Some(Err(e)) => (
                Profile::default(),
                Some(format!("Não foi possível ler o perfil: {e}")),
            ),
            None => (
                Profile::default(),
                (!temporary_profile && capture.is_none())
                    .then(|| "Preferências indisponíveis neste ambiente".into()),
            ),
        };
        let mut app = Self::from_profile(
            profile,
            profile_path,
            notice,
            initial,
            capture,
            AiPanel::new(&cc.egui_ctx),
        );
        app.page_gpu = crate::page_gpu::Gpu::install(cc);
        app
    }
    fn from_profile(
        profile: Profile,
        profile_path: Option<PathBuf>,
        notice: Option<String>,
        initial: Option<String>,
        capture: Option<PathBuf>,
        ai: AiPanel,
    ) -> Self {
        let mut next_id = 1;
        let mut parked = HashMap::new();
        for workspace in &profile.workspaces {
            let targets = workspace
                .tabs
                .iter()
                .enumerate()
                .filter(|(i, _)| profile.restore_session || workspace.pinned_tabs.contains(i))
                .map(|(i, target)| (target.clone(), workspace.pinned_tabs.contains(&i)))
                .collect::<Vec<_>>();
            let targets = if targets.is_empty() {
                vec![(HOME.into(), false)]
            } else {
                targets
            };
            let tabs = targets
                .into_iter()
                .map(|(target, pinned)| {
                    let mut tab = Tab::new(next_id, target);
                    tab.pinned = pinned;
                    next_id += 1;
                    tab
                })
                .collect::<Vec<_>>();
            parked.insert(workspace.id, (tabs, workspace.active_tab));
        }
        let (mut tabs, mut active) = parked
            .remove(&profile.active_workspace)
            .unwrap_or_else(|| (vec![Tab::new(next_id, HOME.into())], 0));
        active = active.min(tabs.len() - 1);
        if let Some(target) = initial {
            tabs = vec![Tab::new(next_id, target)];
            next_id += 1;
            active = 0;
        }
        Self {
            tabs,
            active,
            parked,
            next_id,
            profile,
            profile_path,
            notice,
            show_ai: false,
            show_inspector: false,
            show_find: false,
            find: String::new(),
            focus_address: false,
            focus_find: false,
            zoom: 1.,
            permissions: Default::default(),
            capture,
            capture_ready_frames: 0,
            started: Instant::now(),
            transition: Instant::now(),
            ai,
            favorite_editor: Default::default(),
            workspace_editor: Default::default(),
            backdrop: Default::default(),
            backdrop_notice: None,
            settings_section: 0,
            ai_operation: None,
            snapshot_key: None,
            page_gpu: None,
            page_scene: Default::default(),
            graphics_error: None,
            frame_opportunity: None,
            displayed_frame_target: None,
            priority_work_this_pass: false,
            closing: false,
        }
    }
    fn save_profile(&mut self) {
        for workspace in &mut self.profile.workspaces {
            let state = if workspace.id == self.profile.active_workspace {
                Some((&self.tabs, self.active))
            } else {
                self.parked
                    .get(&workspace.id)
                    .map(|(tabs, active)| (tabs, *active))
            };
            if let Some((tabs, active)) = state {
                workspace.tabs = tabs.iter().map(|t| t.url.clone()).collect();
                workspace.active_tab = active;
                workspace.pinned_tabs = tabs
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| t.pinned.then_some(i))
                    .collect();
            }
        }
        if let Some(path) = &self.profile_path
            && let Err(e) = browser_config::save(path, &self.profile)
        {
            self.notice = Some(format!("Não foi possível salvar suas preferências: {e}"));
        }
    }
    fn switch_workspace(&mut self, id: u64) {
        if id == self.profile.active_workspace {
            return;
        }
        let old = self.profile.active_workspace;
        if let Err(e) = self.profile.select_workspace(id) {
            self.notice = Some(e);
            return;
        }
        let (tabs, active) = self.parked.remove(&id).unwrap_or_else(|| {
            let t = Tab::new(self.next_id, HOME.into());
            self.next_id += 1;
            (vec![t], 0)
        });
        let old_tabs = std::mem::replace(&mut self.tabs, tabs);
        let old_active = std::mem::replace(&mut self.active, active);
        self.parked.insert(old, (old_tabs, old_active));
        self.active = self.active.min(self.tabs.len() - 1);
        self.ai.clear_page_context();
        self.transition = Instant::now();
        self.save_profile();
    }
    fn total_tabs(&self) -> usize {
        self.tabs.len()
            + self
                .parked
                .values()
                .map(|(tabs, _)| tabs.len())
                .sum::<usize>()
    }
    fn new_tab(&mut self) {
        if self.tabs.len() >= MAX_TABS || self.total_tabs() >= MAX_TOTAL_TABS {
            self.notice =
                Some("Você atingiu o limite de abas. Feche uma aba para continuar.".into());
            return;
        }
        self.tabs.push(Tab::new(self.next_id, HOME.into()));
        self.next_id += 1;
        self.active = self.tabs.len() - 1;
        self.ai.clear_page_context();
        self.transition = Instant::now();
        self.save_profile();
    }
    fn close_tab(&mut self, index: usize) {
        self.permissions.revoke_tab(self.tabs[index].id);
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::new(self.next_id, HOME.into()));
            self.next_id += 1;
        }
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.ai.clear_page_context();
        self.save_profile();
    }
    fn move_tab(&mut self, index: usize, destination: u64) {
        if destination == self.profile.active_workspace {
            return;
        }
        if !self.profile.workspaces.iter().any(|w| w.id == destination) {
            return;
        }
        if self.tabs.len() == 1 && self.total_tabs() >= MAX_TOTAL_TABS {
            self.notice = Some("Feche uma aba antes de mover a última aba deste espaço.".into());
            return;
        }
        if self
            .parked
            .get(&destination)
            .is_some_and(|(t, _)| t.len() >= MAX_TABS)
        {
            self.notice = Some("O espaço de destino atingiu o limite de abas.".into());
            return;
        }
        let selected_id = self.tabs[self.active].id;
        let tab = self.tabs.remove(index);
        self.permissions.revoke_tab(tab.id);
        let entry = self
            .parked
            .entry(destination)
            .or_insert_with(|| (Vec::new(), 0));
        entry.0.push(tab);
        entry.1 = entry.0.len() - 1;
        if self.tabs.is_empty() {
            self.tabs.push(Tab::new(self.next_id, HOME.into()));
            self.next_id += 1;
        }
        self.active = self
            .tabs
            .iter()
            .position(|tab| tab.id == selected_id)
            .unwrap_or(self.active.min(self.tabs.len() - 1));
        self.ai.clear_page_context();
        self.save_profile();
    }
    fn navigate_user(&mut self, input: String, ctx: &egui::Context) {
        if input.trim() == SETTINGS {
            self.navigate(SETTINGS.into(), Navigation::Push, ctx);
            return;
        }
        match browser_core::normalize_address(&input) {
            Ok(target) => self.navigate(target, Navigation::Push, ctx),
            Err(e) => self.notice = Some(e),
        }
    }
    fn navigate(&mut self, target: String, navigation: Navigation, ctx: &egui::Context) {
        if matches!(navigation, Navigation::Frame(_)) {
            // UI opportunities are disposable, never queued behind other work.
            if !self.tabs[self.active].animation_frame_ready() {
                return;
            }
        } else {
            self.priority_work_this_pass = true;
        }
        if !matches!(
            navigation,
            Navigation::Tick(_) | Navigation::Frame(_) | Navigation::Reflow
        ) {
            self.ai.clear_page_context();
        }
        let tab = &mut self.tabs[self.active];
        if !matches!(
            navigation,
            Navigation::Tick(_) | Navigation::Frame(_) | Navigation::Reflow
        ) {
            tab.generation = tab.generation.saturating_add(1);
        }
        if !matches!(navigation, Navigation::Frame(_)) {
            self.permissions.revoke_tab(tab.id);
        }
        if tab.pending.is_some() {
            tab.queued = Some((target, navigation));
            return;
        }
        if !retains_session(navigation) {
            tab.checkpoint_session();
        }
        if target == HOME || target == SETTINGS {
            let title = if target == SETTINGS {
                "Configurações"
            } else {
                "Nova aba"
            };
            tab.commit(target, title.into(), navigation);
            tab.document = None;
            tab.page = None;
            tab.queued = None;
            tab.session = None;
            tab.session_origin = None;
            tab.animation_origin = None;
            self.save_profile();
            return;
        }
        let executable = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => {
                tab.error = Some(e.to_string());
                return;
            }
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let viewport = tab.viewport;
        let retained = matches!(
            navigation,
            Navigation::Reflow
                | Navigation::Event(_)
                | Navigation::Input(_)
                | Navigation::Change(_)
                | Navigation::Tick(_)
                | Navigation::Frame(_)
        );
        let cached = if retained { tab.document.clone() } else { None };
        let session = if retained {
            tab.session.clone()
        } else {
            // The last displayed offset was already saved in the origin entry by
            // capture_scroll. Never sample the spinner or the destination here.
            tab.session = None;
            tab.session_origin = None;
            tab.animation_origin = None;
            tab.page = None;
            tab.displayed_document = None;
            tab.document = None;
            None
        };
        let inputs = tab.page.as_ref().map_or_else(Vec::new, |page| {
            page.items
                .iter()
                .filter_map(|item| {
                    if let DisplayItem::Control {
                        id,
                        kind,
                        disabled: false,
                        ..
                    } = item
                    {
                        if matches!(
                            kind,
                            ControlKind::Text
                                | ControlKind::Password
                                | ControlKind::TextArea
                                | ControlKind::Checkbox
                                | ControlKind::Select
                        ) {
                            tab.controls.get(id).map(|value| (*id, value.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        });
        let event = match navigation {
            Navigation::Event(node) => Some((node, "click")),
            Navigation::Input(node) => Some((node, "input")),
            Navigation::Change(node) => Some((node, "change")),
            _ => None,
        };
        let elapsed = tab.timer_mark.elapsed().as_millis().min(60000) as u64;
        let tick = if let Navigation::Tick(elapsed) = navigation {
            Some(elapsed)
        } else {
            None
        };
        if event.is_some() || tick.is_some() {
            tab.timer_mark = Instant::now();
        }
        let context = ctx.clone();
        let storage_origin = tab.session_origin.clone();
        let animation_origin = tab.animation_origin;
        let committed_storage = if retained {
            session.as_ref().map(|session| CommittedStorageOperation {
                document: tab.committed_document,
                origin: storage_origin.clone(),
                session: session.clone(),
            })
        } else {
            None
        };
        // Bounded native-only copy. The renderer receives one final-origin seed.
        let storage = if retained {
            None
        } else {
            Some(tab.session_storage.clone())
        };
        tab.pending = Some(Pending {
            receiver,
            committed_storage,
            navigation,
            started: Instant::now(),
            viewport,
        });
        tab.needs_initial_load = false;
        tab.error = None;
        std::thread::spawn(move || {
            let result = (|| {
                if retained {
                    let document =
                        cached.ok_or("Recarregue a página para restaurar sua sessão.")?;
                    let session = session
                        .ok_or("A sessão desta página foi encerrada. Recarregue para continuar.")?;
                    let page = {
                        let mut worker = session
                            .lock()
                            .map_err(|_| "Sessão de renderização interrompida")?;
                        if let Some((node, event)) = event {
                            worker.advance_timers(viewport, elapsed)?;
                            worker.event_with_inputs(viewport, node, event.into(), inputs)?
                        } else if let Navigation::Frame(timestamp_ms) = navigation {
                            // Frame delivery never advances timers implicitly.
                            worker.animation_frame(timestamp_ms, viewport)?
                        } else if let Some(elapsed) = tick {
                            worker.advance_timers(viewport, elapsed)?
                        } else {
                            worker.render(viewport, None)?
                        }
                    };
                    Ok(LoadedDocument {
                        document,
                        page,
                        session,
                        storage_origin,
                        animation_origin: animation_origin.ok_or("Missing document time origin")?,
                    })
                } else {
                    let document = Arc::new(browser_core::load_document(target.trim())?);
                    let (storage_origin, seed) = storage
                        .expect("initial storage snapshot")
                        .seed_for_url(&document.url);
                    let renderer_document = (*document).clone();
                    let animation_origin = Instant::now();
                    let (session, page) = browser_process::RendererSession::spawn_with_storage(
                        &executable,
                        renderer_document,
                        viewport,
                        seed,
                    )?;
                    Ok(LoadedDocument {
                        document,
                        page,
                        session: Arc::new(Mutex::new(session)),
                        storage_origin,
                        animation_origin,
                    })
                }
            })();
            let _ = sender.send(result);
            context.request_repaint();
        });
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let previous_generation = self.tabs[self.active].generation;
        let mut changed = false;
        for tab in &mut self.tabs {
            changed |= poll_tab(tab);
        }
        for (tabs, _) in self.parked.values_mut() {
            for tab in tabs {
                changed |= poll_tab(tab);
            }
        }
        if changed {
            if previous_generation != self.tabs[self.active].generation {
                self.ai.clear_page_context();
            }
            self.save_profile();
        }
        let queued = if self.tabs[self.active].pending.is_none() {
            self.tabs[self.active].queued.take()
        } else {
            None
        };
        if let Some((target, nav)) = queued {
            self.navigate(target, nav, ctx);
        }
        let tab = &self.tabs[self.active];
        if self.ai_operation.is_none()
            && !self.ai.browser_busy()
            && tab.pending.is_none()
            && tab.error.is_none()
            && let Some(delay) = tab.page.as_ref().and_then(|page| page.next_timer_ms)
        {
            let elapsed = tab.timer_mark.elapsed().as_millis().min(60000) as u64;
            if elapsed >= delay {
                self.navigate(tab.url.clone(), Navigation::Tick(elapsed), ctx);
            } else {
                ctx.request_repaint_after(Duration::from_millis((delay - elapsed).clamp(16, 1000)));
            }
        }
        if self.tabs.iter().any(|t| t.pending.is_some())
            || self
                .parked
                .values()
                .any(|(tabs, _)| tabs.iter().any(|t| t.pending.is_some()))
        {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }
    fn history(&mut self, forward: bool, ctx: &egui::Context) {
        let t = &self.tabs[self.active];
        let index = if forward {
            t.history_index + 1
        } else {
            t.history_index.saturating_sub(1)
        };
        if index != t.history_index && index < t.history.len() {
            self.navigate(
                t.history[index].url.clone(),
                Navigation::Traverse(index),
                ctx,
            );
        }
    }
    fn open_file(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Documentos HTML", &["html", "htm"])
            .pick_file()
        {
            self.navigate(path.to_string_lossy().into_owned(), Navigation::Push, ctx);
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, key));
        if pressed(egui::Key::L) {
            self.focus_address = true;
        }
        if pressed(egui::Key::T) {
            self.new_tab();
        }
        if pressed(egui::Key::W) {
            self.close_tab(self.active);
        }
        if pressed(egui::Key::O) {
            self.open_file(ctx);
        }
        if pressed(egui::Key::R) {
            self.navigate(self.tabs[self.active].url.clone(), Navigation::Refresh, ctx);
        }
        if pressed(egui::Key::D) {
            self.edit_favorite(None, true);
        }
        if pressed(egui::Key::F) {
            self.show_find = true;
            self.focus_find = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft)) {
            self.history(false, ctx);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight)) {
            self.history(true, ctx);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.show_find = false;
        }
    }
    fn open_settings(&mut self, ctx: &egui::Context) {
        if let Some(index) = self.tabs.iter().position(|t| t.url == SETTINGS) {
            self.active = index;
            self.ai.clear_page_context();
        } else {
            let count = self.tabs.len();
            self.new_tab();
            if self.tabs.len() > count {
                self.navigate(SETTINGS.into(), Navigation::Push, ctx);
            }
        }
    }
    fn glass(&self, color: Color32) -> Color32 {
        theme::glass(
            color,
            if self.profile.acrylic_enabled {
                self.profile.acrylic_opacity
            } else {
                255
            },
        )
    }
    fn tabstrip(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut switch = None;
        let mut rename = None;
        let mut remove = None;
        let mut pin_group = None;
        let mut select = None;
        let mut close = None;
        let mut pin = None;
        let mut move_to = None;
        egui::Panel::top("tabs-top")
            .exact_size(49.)
            .frame(
                egui::Frame::new()
                    .fill(self.glass(PANEL))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ui, |ui| {
                let drag = ui.interact(
                    ui.max_rect(),
                    ui.scope_id().with("window-drag"),
                    Sense::click_and_drag(),
                );
                if drag.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                        !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
                    ));
                }
                ui.horizontal(|ui| {
                    icons::mark(ui, Icon::Diamond, 22., ACCENT);
                    ui.label(RichText::new("noir").size(18.).strong().color(TEXT));
                    ui.add_space(6.);
                    let active = self.profile.active_workspace();
                    let color =
                        Color32::from_rgb(active.color[0], active.color[1], active.color[2]);
                    let group_menu = ui.menu_button(
                        RichText::new(format!("{}     ", short_title(&active.name, 12)))
                            .color(color)
                            .size(12.),
                        |ui| {
                            ui.set_min_width(230.);
                            ui.horizontal(|ui| {
                                icons::mark(ui, Icon::Workspace, 15., MUTED);
                                ui.label(RichText::new("Seus grupos").color(MUTED).size(11.));
                            });
                            let mut groups = self.profile.workspaces.iter().collect::<Vec<_>>();
                            groups.sort_by_key(|g| !g.pinned);
                            for group in groups {
                                let response = ui
                                    .horizontal(|ui| {
                                        icons::mark(
                                            ui,
                                            if group.pinned {
                                                Icon::Pin
                                            } else {
                                                Icon::Workspace
                                            },
                                            14.,
                                            Color32::from_rgb(
                                                group.color[0],
                                                group.color[1],
                                                group.color[2],
                                            ),
                                        );
                                        ui.selectable_label(
                                            group.id == self.profile.active_workspace,
                                            &group.name,
                                        )
                                    })
                                    .inner;
                                if response.clicked() {
                                    switch = Some(group.id);
                                    ui.close();
                                }
                                response.context_menu(|ui| {
                                    if ui
                                        .button(if group.pinned {
                                            "Desafixar grupo"
                                        } else {
                                            "Fixar grupo"
                                        })
                                        .clicked()
                                    {
                                        pin_group = Some(group.id);
                                        ui.close();
                                    }
                                    if ui.button("Renomear grupo").clicked() {
                                        rename = Some((group.id, group.name.clone()));
                                        ui.close();
                                    }
                                    if ui
                                        .add_enabled(
                                            self.profile.workspaces.len() > 1,
                                            egui::Button::new("Excluir grupo e suas abas"),
                                        )
                                        .clicked()
                                    {
                                        remove = Some(group.id);
                                        ui.close();
                                    }
                                });
                            }
                            ui.separator();
                            if ui.button("+  Novo grupo").clicked() {
                                self.workspace_editor = Editor {
                                    open: true,
                                    ..Default::default()
                                };
                                ui.close();
                            }
                            if ui
                                .button(if active.pinned {
                                    "Desafixar este grupo"
                                } else {
                                    "Fixar este grupo"
                                })
                                .clicked()
                            {
                                pin_group = Some(active.id);
                                ui.close();
                            }
                        },
                    );
                    icons::paint(
                        ui,
                        Icon::ChevronDown,
                        Rect::from_center_size(
                            group_menu.response.rect.right_center() - Vec2::new(11., 0.),
                            Vec2::splat(11.),
                        ),
                        color,
                    );
                    for group in self
                        .profile
                        .workspaces
                        .iter()
                        .filter(|g| g.pinned && g.id != self.profile.active_workspace)
                        .take(2)
                    {
                        let color =
                            Color32::from_rgb(group.color[0], group.color[1], group.color[2]);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(short_title(&group.name, 8))
                                        .color(color)
                                        .size(11.),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Alternar para grupo fixado")
                            .clicked()
                        {
                            switch = Some(group.id);
                        }
                    }
                    ui.separator();
                    let tabs_width = (ui.available_width() - 138.).max(70.);
                    ui.allocate_ui_with_layout(
                        Vec2::new(tabs_width, 36.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("top-tabs-scroll")
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let mut order = (0..self.tabs.len()).collect::<Vec<_>>();
                                        order.sort_by_key(|i| !self.tabs[*i].pinned);
                                        for index in order {
                                            let tab = &self.tabs[index];
                                            let selected = index == self.active;
                                            let width = if tab.pinned {
                                                42.
                                            } else {
                                                ((tabs_width - 40.) / self.tabs.len().min(5) as f32)
                                                    .clamp(98., 195.)
                                            };
                                            ui.push_id(tab.id, |ui| {
                                                let (rect, response) = ui.allocate_exact_size(
                                                    Vec2::new(width, 35.),
                                                    Sense::click(),
                                                );
                                                response.widget_info(|| {
                                                    egui::WidgetInfo::labeled(
                                                        egui::Role::Button,
                                                        true,
                                                        &tab.title,
                                                    )
                                                });
                                                let t = ctx.animate_bool_with_time(
                                                    response.id.with("active"),
                                                    selected || response.hovered(),
                                                    if self.profile.reduce_motion {
                                                        0.
                                                    } else {
                                                        0.24
                                                    },
                                                );
                                                ui.painter().rect_filled(
                                                    rect,
                                                    10.,
                                                    theme::blend(Color32::TRANSPARENT, SURFACE, t),
                                                );
                                                if selected {
                                                    let x = ctx.animate_value_with_time(
                                                        egui::Id::unique("active-tab-position"),
                                                        rect.center().x,
                                                        if self.profile.reduce_motion {
                                                            0.
                                                        } else {
                                                            0.25
                                                        },
                                                    );
                                                    ui.painter().line_segment(
                                                        [
                                                            egui::pos2(
                                                                x - width * 0.27,
                                                                rect.bottom() - 1.,
                                                            ),
                                                            egui::pos2(
                                                                x + width * 0.27,
                                                                rect.bottom() - 1.,
                                                            ),
                                                        ],
                                                        egui::Stroke::new(1.5, ACCENT),
                                                    );
                                                }
                                                let icon = if tab.pinned {
                                                    Icon::Pin
                                                } else if tab.url == HOME {
                                                    Icon::Home
                                                } else if tab.url == SETTINGS {
                                                    Icon::Settings
                                                } else {
                                                    Icon::Globe
                                                };
                                                let icon_rect = Rect::from_center_size(
                                                    rect.min
                                                        + Vec2::new(
                                                            if tab.pinned { 21. } else { 18. },
                                                            17.,
                                                        ),
                                                    Vec2::splat(15.),
                                                );
                                                icons::paint(
                                                    ui,
                                                    icon,
                                                    icon_rect,
                                                    if selected { ACCENT } else { MUTED },
                                                );
                                                if tab.pending.is_some() {
                                                    let phase = ui.input(|i| i.time) as f32 * 3.;
                                                    let center = icon_rect.center();
                                                    let points = (0..=16)
                                                        .map(|i| {
                                                            let a = phase + i as f32 * 0.15;
                                                            center + Vec2::angled(a) * 10.
                                                        })
                                                        .collect();
                                                    ui.painter().add(egui::Shape::line(
                                                        points,
                                                        egui::Stroke::new(1., ACCENT),
                                                    ));
                                                }
                                                if !tab.pinned {
                                                    ui.painter().text(
                                                        rect.min + Vec2::new(33., 17.),
                                                        Align2::LEFT_CENTER,
                                                        short_title(
                                                            &tab.title,
                                                            ((width - 56.) / 6.5) as usize,
                                                        ),
                                                        FontId::proportional(12.),
                                                        if selected { TEXT } else { MUTED },
                                                    );
                                                    let cr = Rect::from_center_size(
                                                        rect.right_center() - Vec2::new(14., 0.),
                                                        Vec2::splat(20.),
                                                    );
                                                    let close_response = ui.interact(
                                                        cr,
                                                        response.id.with("close"),
                                                        Sense::click(),
                                                    );
                                                    if selected
                                                        || response.hovered()
                                                        || close_response.hovered()
                                                    {
                                                        icons::paint(
                                                            ui,
                                                            Icon::Close,
                                                            cr.shrink(5.),
                                                            MUTED,
                                                        );
                                                    }
                                                    if close_response.clicked() {
                                                        close = Some(index);
                                                    }
                                                }
                                                if response.clicked() {
                                                    select = Some(index);
                                                }
                                                if response.clicked_by(egui::PointerButton::Middle)
                                                    && !tab.pinned
                                                {
                                                    close = Some(index);
                                                }
                                                response.context_menu(|ui| {
                                                    if ui
                                                        .button(if tab.pinned {
                                                            "Desafixar aba"
                                                        } else {
                                                            "Fixar aba"
                                                        })
                                                        .clicked()
                                                    {
                                                        pin = Some(index);
                                                        ui.close();
                                                    }
                                                    ui.menu_button("Mover para grupo", |ui| {
                                                        for group in &self.profile.workspaces {
                                                            if group.id
                                                                != self.profile.active_workspace
                                                                && ui.button(&group.name).clicked()
                                                            {
                                                                move_to = Some((index, group.id));
                                                                ui.close();
                                                            }
                                                        }
                                                    });
                                                    if ui
                                                        .button("Novo grupo com esta aba")
                                                        .clicked()
                                                    {
                                                        move_to = Some((index, 0));
                                                        ui.close();
                                                    }
                                                    if ui.button("Fechar aba").clicked() {
                                                        close = Some(index);
                                                        ui.close();
                                                    }
                                                });
                                                response.on_hover_text(&tab.title);
                                            });
                                        }
                                        if icons::button(ui, Icon::Plus, "Nova aba · Ctrl+T", true)
                                            .clicked()
                                        {
                                            self.new_tab();
                                        }
                                    });
                                });
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Close, "Fechar janela", true).clicked() {
                            self.closing = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if icons::button(ui, Icon::Maximize, "Maximizar ou restaurar", true)
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                                !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
                            ));
                        }
                        if icons::button(ui, Icon::Minimize, "Minimizar", true).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });
        if let Some(i) = select {
            self.active = i;
            self.transition = Instant::now();
            self.ai.clear_page_context();
            self.save_profile();
        }
        if let Some(i) = pin {
            self.tabs[i].pinned = !self.tabs[i].pinned;
            self.save_profile();
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
        if let Some((i, mut id)) = move_to {
            if id == 0 {
                match self.profile.create_workspace("Novo grupo") {
                    Ok(created) => id = created,
                    Err(error) => {
                        self.notice = Some(error);
                        return;
                    }
                }
            }
            self.move_tab(i, id);
        }
        if let Some(id) = switch {
            self.switch_workspace(id);
        }
        if let Some(id) = pin_group {
            if let Some(group) = self.profile.workspaces.iter_mut().find(|g| g.id == id) {
                group.pinned = !group.pinned;
            }
            self.save_profile();
        }
        if let Some((id, name)) = rename {
            self.workspace_editor = Editor {
                open: true,
                id: Some(id),
                name,
                ..Default::default()
            };
        }
        if let Some(id) = remove {
            if self.profile.active_workspace == id
                && let Some(other) = self
                    .profile
                    .workspaces
                    .iter()
                    .find(|w| w.id != id)
                    .map(|w| w.id)
            {
                self.switch_workspace(other);
            }
            match self.profile.remove_workspace(id) {
                Ok(()) => {
                    if let Some((tabs, _)) = self.parked.remove(&id) {
                        for tab in tabs {
                            self.permissions.revoke_tab(tab.id);
                        }
                    }
                    self.save_profile();
                }
                Err(error) => self.notice = Some(error),
            }
        }
    }
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar")
            .exact_size(56.)
            .frame(
                egui::Frame::new()
                    .fill(self.glass(PANEL))
                    .inner_margin(egui::Margin::symmetric(16, 9)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let tab = &self.tabs[self.active];
                    if icons::button(
                        ui,
                        Icon::Back,
                        "Voltar · Alt+Esquerda",
                        tab.history_index > 0,
                    )
                    .clicked()
                    {
                        self.history(false, &ui.ctx().clone());
                    }
                    let tab = &self.tabs[self.active];
                    if icons::button(
                        ui,
                        Icon::Forward,
                        "Avançar · Alt+Direita",
                        tab.history_index + 1 < tab.history.len(),
                    )
                    .clicked()
                    {
                        self.history(true, &ui.ctx().clone());
                    }
                    if icons::button(ui, Icon::Reload, "Recarregar · Ctrl+R", true).clicked() {
                        self.navigate(
                            self.tabs[self.active].url.clone(),
                            Navigation::Refresh,
                            &ui.ctx().clone(),
                        );
                    }
                    let available = (ui.available_width() - 182.).max(100.);
                    let tab = &mut self.tabs[self.active];
                    let address = egui::Frame::new()
                        .fill(Color32::from_black_alpha(42))
                        .stroke(egui::Stroke::new(1., Color32::from_white_alpha(16)))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(12, 3))
                        .show(ui, |ui| {
                            ui.set_width((available - 24.).max(80.));
                            ui.horizontal(|ui| {
                                icons::mark(
                                    ui,
                                    if tab.url == HOME {
                                        Icon::Search
                                    } else if tab.url == SETTINGS {
                                        Icon::Settings
                                    } else {
                                        Icon::Globe
                                    },
                                    16.,
                                    MUTED,
                                );
                                ui.add_sized(
                                    [(available - 52.).max(60.), 28.],
                                    egui::TextEdit::singleline(&mut tab.address)
                                        .id(egui::Id::unique("address"))
                                        .hint_text("Pesquisar no Google ou digitar um endereço")
                                        .font(FontId::proportional(13.))
                                        .frame(egui::Frame::NONE)
                                        .margin(Vec2::new(0., 6.)),
                                )
                            })
                            .inner
                        })
                        .inner;
                    if self.focus_address {
                        address.request_focus();
                        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), address.id) {
                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(tab.address.chars().count()),
                                )));
                            state.store(ui.ctx(), address.id);
                        }
                        self.focus_address = false;
                    }
                    if address.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.navigate_user(
                            self.tabs[self.active].address.clone(),
                            &ui.ctx().clone(),
                        );
                    }
                    if icons::button(ui, Icon::Settings, "Configurações", true).clicked() {
                        self.open_settings(&ui.ctx().clone());
                    }
                    if icons::button(ui, Icon::Star, "Salvar favorito · Ctrl+D", true).clicked() {
                        self.edit_favorite(None, true);
                    }
                    if icons::button(ui, Icon::Home, "Página inicial", true).clicked() {
                        self.navigate(HOME.into(), Navigation::Push, &ui.ctx().clone());
                    }
                    if icons::button(ui, Icon::Spark, "Abrir assistente", true).clicked() {
                        self.show_ai = !self.show_ai;
                    }
                });
            });
        if self.show_find {
            egui::Panel::top("find")
                .frame(egui::Frame::new().fill(PANEL).inner_margin(10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Buscar na página");
                        let response =
                            ui.add(egui::TextEdit::singleline(&mut self.find).desired_width(220.));
                        if self.focus_find {
                            response.request_focus();
                            self.focus_find = false;
                        }
                        let tab = &mut self.tabs[self.active];
                        if let Some(page) = &tab.page {
                            tab.matches.update(page, &self.find);
                        }
                        ui.label(format!("{} resultados", tab.matches.count));
                        if ui.button("Fechar").clicked() {
                            self.show_find = false;
                        }
                    });
                });
        }
    }
    fn status(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status")
            .exact_size(27.)
            .frame(egui::Frame::new().fill(BG).inner_margin(5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let tab = &self.tabs[self.active];
                    if let Some(pending) = &tab.pending {
                        ui.spinner();
                        ui.label(
                            RichText::new(format!(
                                "Abrindo página · {:.1}s",
                                pending.started.elapsed().as_secs_f32()
                            ))
                            .size(10.)
                            .color(MUTED),
                        );
                    } else {
                        ui.label(
                            RichText::new(if tab.url == HOME {
                                "Um espaço só seu"
                            } else {
                                "Página aberta"
                            })
                            .size(10.)
                            .color(MUTED),
                        );
                    }
                    if self.profile.show_metrics
                        && let Some(page) = &tab.page
                    {
                        let m = &page.metrics;
                        ui.label(
                            RichText::new(format!(
                                "{} nós · {:.1} ms",
                                m.node_count,
                                (m.parse_us + m.style_us + m.layout_us) as f64 / 1000.
                            ))
                            .size(10.)
                            .color(MUTED),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(self.ai.status_label())
                                .size(10.)
                                .color(ACCENT),
                        );
                    });
                });
            });
    }
    fn home_content(&mut self, ui: &mut egui::Ui) {
        let mut action = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.glass(BG)))
            .show(ui, |ui| {
                if let Some(notice) = self.notice.clone() {
                    egui::Frame::new()
                        .fill(SURFACE)
                        .inner_margin(14)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(notice);
                                if ui.small_button("Fechar").clicked() {
                                    self.notice = None;
                                }
                            });
                        });
                }
                let previous_query = self.tabs[self.active].home.query.clone();
                let workspace = self.profile.active_workspace().name.clone();
                action = self.tabs[self.active].home.show(
                    ui,
                    &self.profile.favorites,
                    &workspace,
                    self.profile.reduce_motion,
                    if self.profile.acrylic_enabled {
                        self.profile.acrylic_opacity
                    } else {
                        255
                    },
                );
                if self.tabs[self.active].home.query != previous_query {
                    self.tabs[self.active].generation =
                        self.tabs[self.active].generation.saturating_add(1);
                }
            });
        if let Some(action) = action {
            let ctx = ui.ctx().clone();
            match action {
                home::Action::Navigate(target) => self.navigate_user(target, &ctx),
                home::Action::AddFavorite => self.edit_favorite(None, false),
                home::Action::EditFavorite(id) => self.edit_favorite(Some(id), false),
                home::Action::RemoveFavorite(id) => {
                    self.profile.remove_favorite(id);
                    self.save_profile();
                }
                home::Action::OpenFile => self.open_file(&ctx),
                home::Action::OpenAi => self.show_ai = true,
                home::Action::OpenSettings => self.open_settings(&ctx),
                home::Action::NewWorkspace => {
                    self.workspace_editor = Editor {
                        open: true,
                        ..Default::default()
                    }
                }
            }
        }
    }
    fn edit_favorite(&mut self, id: Option<u64>, current: bool) {
        let mut editor = Editor {
            open: true,
            id,
            ..Default::default()
        };
        if let Some(favorite) = id.and_then(|id| self.profile.favorites.iter().find(|f| f.id == id))
        {
            editor.name = favorite.title.clone();
            editor.url = favorite.url.clone();
        } else if current {
            editor.name = self.tabs[self.active].title.clone();
            editor.url = self.tabs[self.active].url.clone();
        }
        self.favorite_editor = editor;
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.favorite_editor.open {
            let mut save = false;
            let mut cancel = false;
            egui::Window::new(if self.favorite_editor.id.is_some() {
                "Editar favorito"
            } else {
                "Adicionar favorito"
            })
            .id(egui::Id::unique("favorite-editor"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(390.)
            .show(ctx, |ui| {
                ui.label(RichText::new("Um atalho para o que importa.").color(MUTED));
                ui.add_space(8.);
                ui.label("Nome");
                ui.text_edit_singleline(&mut self.favorite_editor.name);
                ui.label("Endereço");
                ui.text_edit_singleline(&mut self.favorite_editor.url);
                if let Some(error) = &self.favorite_editor.error {
                    ui.colored_label(AMBER, error);
                }
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                    if ui
                        .button(RichText::new("Salvar favorito").color(ACCENT))
                        .clicked()
                    {
                        save = true;
                    }
                });
            });
            if cancel {
                self.favorite_editor.open = false;
            }
            if save {
                match browser_core::normalize_address(&self.favorite_editor.url) {
                    Ok(url) => {
                        let result = if let Some(id) = self.favorite_editor.id {
                            self.profile
                                .edit_favorite(id, &self.favorite_editor.name, &url)
                                .map(|_| ())
                        } else {
                            self.profile
                                .add_favorite(&self.favorite_editor.name, &url)
                                .map(|_| ())
                        };
                        match result {
                            Ok(()) => {
                                self.favorite_editor.open = false;
                                self.save_profile();
                            }
                            Err(e) => self.favorite_editor.error = Some(e),
                        }
                    }
                    Err(e) => self.favorite_editor.error = Some(e),
                }
            }
        }
        if self.workspace_editor.open {
            let mut save = false;
            let mut cancel = false;
            egui::Window::new(if self.workspace_editor.id.is_some() {
                "Renomear espaço"
            } else {
                "Novo espaço de trabalho"
            })
            .id(egui::Id::unique("workspace-editor"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(360.)
            .show(ctx, |ui| {
                ui.label("Cada espaço mantém suas próprias abas.");
                ui.add_space(8.);
                ui.label("Nome do espaço");
                ui.text_edit_singleline(&mut self.workspace_editor.name);
                if let Some(error) = &self.workspace_editor.error {
                    ui.colored_label(AMBER, error);
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                    if ui
                        .button(RichText::new("Salvar espaço").color(ACCENT))
                        .clicked()
                    {
                        save = true;
                    }
                });
            });
            if cancel {
                self.workspace_editor.open = false;
            }
            if save {
                let result = if let Some(id) = self.workspace_editor.id {
                    self.profile
                        .rename_workspace(id, &self.workspace_editor.name)
                        .map(|_| id)
                } else {
                    self.profile.create_workspace(&self.workspace_editor.name)
                };
                match result {
                    Ok(id) => {
                        self.workspace_editor.open = false;
                        self.switch_workspace(id);
                        self.save_profile();
                    }
                    Err(e) => self.workspace_editor.error = Some(e),
                }
            }
        }
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        if !self.show_inspector {
            return;
        }
        egui::Panel::bottom("inspector")
            .default_size(190.)
            .size_range(120.0..=420.)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    icons::mark(ui, Icon::Inspect, 17., ACCENT);
                    ui.strong("Documento");
                    if ui.small_button("Fechar").clicked() {
                        self.show_inspector = false;
                    }
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(error) = &self.graphics_error {
                        ui.colored_label(AMBER, error);
                    }
                    if let Some(page) = &self.tabs[self.active].page {
                        for warning in &page.warnings {
                            ui.colored_label(AMBER, warning);
                        }
                        for item in page.items.iter().take(150) {
                            ui.monospace(format!("{item:?}"));
                        }
                    } else {
                        ui.label("A página inicial faz parte da interface nativa.");
                    }
                });
            });
    }
    fn browser_snapshot(&self) -> ai_context::BrowserSnapshot {
        use ai_context::{
            BoundedText, BrowserElement, BrowserSnapshot, BrowserTab, DocumentScope, bounded_text,
            snapshot_limits as limits,
        };
        let tab = &self.tabs[self.active];
        let mut text = String::new();
        let mut elements = Vec::new();
        if let Some(page) = &tab.page {
            text = page
                .items
                .iter()
                .filter_map(|item| match item {
                    DisplayItem::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .flat_map(|t| t.chars().chain(std::iter::once(' ')))
                .take(limits::TEXT_CHARS)
                .collect();
            for item in &page.items {
                if elements.len() >= limits::CONTROLS {
                    break;
                }
                if let DisplayItem::Control {
                    id,
                    kind,
                    name,
                    value,
                    placeholder,
                    options,
                    disabled: false,
                    ..
                } = item
                {
                    if matches!(kind, ControlKind::Password | ControlKind::Hidden) {
                        continue;
                    }
                    let kind_name = match kind {
                        ControlKind::Text => "text",
                        ControlKind::TextArea => "textarea",
                        ControlKind::Button => "button",
                        ControlKind::Submit => "submit",
                        ControlKind::Checkbox => "checkbox",
                        ControlKind::Select => "select",
                        _ => continue,
                    };
                    elements.push(BrowserElement {
                        id: format!("control:{id}"),
                        kind: kind_name.into(),
                        label: {
                            let label = if placeholder.is_empty() {
                                if name.is_empty() {
                                    value.as_str()
                                } else {
                                    name.as_str()
                                }
                            } else {
                                placeholder.as_str()
                            };
                            let mut bounded = BoundedText::new(limits::ELEMENT_LABEL_CHARS);
                            bounded.push_str(label);
                            if *kind == ControlKind::Select && !bounded.is_full() {
                                bounded.push_str(" [");
                                let mut options = options.iter().filter(|option| !option.disabled);
                                for index in 0..8 {
                                    if bounded.is_full() {
                                        break;
                                    }
                                    let Some(option) = options.next() else {
                                        break;
                                    };
                                    if index != 0 {
                                        bounded.push_str(", ");
                                    }
                                    bounded.push_str(&option.label);
                                    bounded.push_str("=");
                                    bounded.push_str(&option.value);
                                }
                                bounded.push_str("]");
                            }
                            bounded.finish()
                        },
                        value: bounded_text(
                            tab.controls.get(id).unwrap_or(value),
                            limits::ELEMENT_VALUE_CHARS,
                        ),
                    });
                }
            }
            for (i, link) in page
                .links
                .iter()
                .enumerate()
                .take(limits::ELEMENTS - elements.len())
            {
                let label = page
                    .items
                    .iter()
                    .filter_map(|item| {
                        if let DisplayItem::Text { rect, text, .. } = item
                            && rect.x < link.rect.x + link.rect.width
                            && rect.x + rect.width > link.rect.x
                            && rect.y < link.rect.y + link.rect.height
                            && rect.y + rect.height > link.rect.y
                        {
                            return Some(text.as_str());
                        }
                        None
                    })
                    .take(4)
                    .flat_map(|text| text.chars().chain(std::iter::once(' ')))
                    .take(limits::ELEMENT_LABEL_CHARS)
                    .collect::<String>();
                elements.push(BrowserElement {
                    id: format!("link:{i}"),
                    kind: "link".into(),
                    label: if label.trim().is_empty() {
                        bounded_text(&link.href, limits::ELEMENT_LABEL_CHARS)
                    } else {
                        label
                    },
                    value: bounded_text(&link.href, limits::ELEMENT_VALUE_CHARS),
                });
            }
        } else if tab.url == HOME {
            text = "Página inicial do Noir, pesquisa Google e favoritos.".into();
            elements.push(BrowserElement {
                id: "home:search".into(),
                kind: "text".into(),
                label: "Pesquisar no Google".into(),
                value: bounded_text(&tab.home.query, limits::ELEMENT_VALUE_CHARS),
            });
            elements.push(BrowserElement {
                id: "home:submit".into(),
                kind: "button".into(),
                label: "Pesquisar".into(),
                value: String::new(),
            });
        } else if tab.url == SETTINGS {
            text = "Configurações do Noir. Preferências e permissões são alteradas pela pessoa que usa o navegador.".into();
        }
        let selection = tab
            .selected_element
            .as_ref()
            .and_then(|id| elements.iter().find(|e| &e.id == id))
            .map_or_else(String::new, |e| {
                let mut selection = BoundedText::new(limits::SELECTION_CHARS);
                selection.push_str(&e.label);
                selection.push_str(": ");
                selection.push_str(&e.value);
                selection.finish()
            });
        let mut tabs = self
            .tabs
            .iter()
            .take(limits::TABS)
            .map(|t| BrowserTab {
                id: t.id,
                title: bounded_text(&t.title, limits::TAB_TITLE_CHARS),
                url: bounded_text(&t.url, limits::URL_CHARS),
            })
            .collect::<Vec<_>>();
        for (id, (parked, _)) in &self.parked {
            if tabs.len() == limits::TABS {
                break;
            }
            let group = self
                .profile
                .workspaces
                .iter()
                .find(|g| g.id == *id)
                .map_or("Grupo", |g| g.name.as_str());
            let remaining = limits::TABS - tabs.len();
            tabs.extend(parked.iter().take(remaining).map(|t| {
                let mut title = BoundedText::new(limits::TAB_TITLE_CHARS);
                title.push_str("[");
                title.push_str(group);
                title.push_str("] ");
                title.push_str(&t.title);
                BrowserTab {
                    id: t.id,
                    title: title.finish(),
                    url: bounded_text(&t.url, limits::URL_CHARS),
                }
            }));
        }
        BrowserSnapshot {
            scope: DocumentScope {
                tab: tab.id,
                generation: tab.generation,
            },
            title: bounded_text(&tab.title, limits::TITLE_CHARS),
            url: bounded_text(&tab.url, limits::URL_CHARS),
            text,
            selection,
            tabs,
            elements,
        }
        .bounded()
    }
    fn sync_browser_snapshot(&mut self) {
        if !self.show_ai && !self.ai.browser_busy() {
            return;
        }
        let tab = &self.tabs[self.active];
        if tab.pending.is_none() && tab.queued.is_none() && !tab.needs_initial_load {
            let key = (
                tab.id,
                tab.generation,
                tab.selected_element.clone(),
                self.total_tabs(),
            );
            let scope = ai_context::DocumentScope {
                tab: tab.id,
                generation: tab.generation,
            };
            if self.snapshot_key.as_ref() == Some(&key)
                && self.ai.browser_snapshot_scope() == Some(scope)
            {
                return;
            }
            self.ai.set_browser_snapshot(self.browser_snapshot());
            self.snapshot_key = Some(key);
        }
    }
    fn ai_target(input: &str) -> Result<String, String> {
        if input == HOME {
            return Ok(input.into());
        }
        let target = browser_core::normalize_address(input)?;
        let parsed = url::Url::parse(&target).map_err(|_| "Endereço de navegação inválido")?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("A IA só pode abrir páginas HTTP ou HTTPS, sem credenciais na URL.".into());
        }
        Ok(target)
    }
    fn dispatch_browser_action(
        &mut self,
        action: ai_context::ScopedAction,
        ctx: &egui::Context,
    ) -> Result<bool, String> {
        use ai_context::BrowserAction;
        action.action.validate().map_err(str::to_owned)?;
        let snapshot = self.browser_snapshot();
        if action.scope != snapshot.scope {
            return Err("A página mudou antes da execução da ação.".into());
        }
        if self.tabs[self.active].pending.is_some() || self.tabs[self.active].queued.is_some() {
            return Err("Aguarde a página terminar de carregar.".into());
        }
        match action.action {
            BrowserAction::ReadPage => return Ok(false),
            BrowserAction::Navigate { url } => {
                let target = Self::ai_target(&url)?;
                self.navigate(target, Navigation::Push, ctx);
            }
            BrowserAction::OpenTab { url } => {
                let target = Self::ai_target(&url)?;
                let count = self.tabs.len();
                self.new_tab();
                if self.tabs.len() == count {
                    return Err("O limite de abas foi atingido.".into());
                }
                self.navigate(target, Navigation::Push, ctx);
            }
            BrowserAction::SwitchTab { tab } => {
                if let Some(index) = self.tabs.iter().position(|t| t.id == tab) {
                    self.active = index;
                } else {
                    let (group, index) = self
                        .parked
                        .iter()
                        .find_map(|(group, (tabs, _))| {
                            tabs.iter()
                                .position(|t| t.id == tab)
                                .map(|index| (*group, index))
                        })
                        .ok_or("A aba solicitada não está aberta")?;
                    self.switch_workspace(group);
                    self.active = index;
                }
                self.ai.clear_page_context();
                self.transition = Instant::now();
                self.save_profile();
                if self.tabs[self.active].needs_initial_load {
                    self.navigate(self.tabs[self.active].url.clone(), Navigation::Push, ctx);
                }
            }
            BrowserAction::Select { element } => {
                if !snapshot.elements.iter().any(|e| e.id == element) {
                    return Err("Elemento indisponível na página atual.".into());
                }
                let tab = &mut self.tabs[self.active];
                tab.home.focus_query = element == "home:search";
                tab.selected_element = Some(element);
                tab.focus_element = true;
            }
            BrowserAction::Type { element, text } => {
                if !snapshot.elements.iter().any(|e| {
                    e.id == element && matches!(e.kind.as_str(), "text" | "textarea" | "select")
                }) {
                    return Err("O elemento não é um campo de texto permitido.".into());
                }
                if element == "home:search" {
                    let tab = &mut self.tabs[self.active];
                    tab.home.query = text;
                    tab.generation = tab.generation.saturating_add(1);
                } else {
                    let id = element
                        .strip_prefix("control:")
                        .and_then(|id| id.parse::<u64>().ok())
                        .ok_or("Campo inválido")?;
                    let (text, event) = if let Some(DisplayItem::Control { kind: ControlKind::Select, options, .. }) = self.tabs[self.active].page.as_ref().and_then(|page| page.items.iter().find(|item| matches!(item,DisplayItem::Control{id: candidate,..} if *candidate==id))) {
                        let option=options.iter().find(|option| !option.disabled && (option.value==text || option.label.eq_ignore_ascii_case(&text))).ok_or("A opção solicitada não está disponível neste campo")?;
                        (option.value.clone(), Navigation::Change(id))
                    } else { (text, Navigation::Input(id)) };
                    self.tabs[self.active].controls.insert(id, text);
                    self.navigate(self.tabs[self.active].url.clone(), event, ctx);
                }
                self.tabs[self.active].home.focus_query = element == "home:search";
                self.tabs[self.active].selected_element = Some(element);
                self.tabs[self.active].focus_element = true;
            }
            BrowserAction::Click { element } => {
                if !snapshot.elements.iter().any(|e| e.id == element) {
                    return Err("O elemento solicitado não está disponível para interação.".into());
                }
                if element == "home:submit" {
                    let target = Self::ai_target(&self.tabs[self.active].home.query)?;
                    self.navigate(target, Navigation::Push, ctx);
                } else if element == "home:search" {
                    self.tabs[self.active].home.focus_query = true;
                    self.tabs[self.active].selected_element = Some(element);
                    self.tabs[self.active].focus_element = true;
                } else if let Some(index) = element
                    .strip_prefix("link:")
                    .and_then(|id| id.parse::<usize>().ok())
                {
                    let tab = &self.tabs[self.active];
                    let link = tab
                        .page
                        .as_ref()
                        .and_then(|p| p.links.get(index))
                        .ok_or("Link indisponível")?;
                    let resolved = browser_core::resolve_navigation(&tab.url, &link.href)?;
                    self.navigate(Self::ai_target(&resolved)?, Navigation::Push, ctx);
                } else {
                    let id = element
                        .strip_prefix("control:")
                        .and_then(|id| id.parse::<u64>().ok())
                        .ok_or("Controle inválido")?;
                    let tab = &self.tabs[self.active];
                    let page = tab.page.as_ref().ok_or("Página indisponível")?;
                    let item = page.items.iter().find(|item| matches!(item, DisplayItem::Control { id: candidate, .. } if *candidate == id)).cloned().ok_or("Controle indisponível")?;
                    if let DisplayItem::Control {
                        kind,
                        disabled,
                        form,
                        value,
                        ..
                    } = item
                    {
                        if disabled || matches!(kind, ControlKind::Password | ControlKind::Hidden) {
                            return Err("Este campo não pode ser acessado pela IA.".into());
                        }
                        if kind == ControlKind::Submit
                            && let Some(form) = form
                        {
                            // Granular mode requires manual handling; full access explicitly authorizes browser actions.
                            if !self.ai.has_full_access() && page.items.iter().any(|item| matches!(item, DisplayItem::Control { kind: ControlKind::Password, form: Some(owner), .. } if owner.id == form.id)) { return Err("Formulários com senha devem ser enviados manualmente.".into()); }
                            let url =
                                form_navigation(&tab.url, &form, id, &page.items, &tab.controls)?;
                            self.navigate(Self::ai_target(&url)?, Navigation::Push, ctx);
                        } else if kind == ControlKind::Checkbox {
                            let current =
                                self.tabs[self.active].controls.get(&id).unwrap_or(&value);
                            let next = if current.is_empty() {
                                "on".into()
                            } else {
                                String::new()
                            };
                            self.tabs[self.active].controls.insert(id, next);
                            self.navigate(
                                self.tabs[self.active].url.clone(),
                                Navigation::Change(id),
                                ctx,
                            );
                        } else if matches!(
                            kind,
                            ControlKind::Text | ControlKind::TextArea | ControlKind::Select
                        ) {
                            self.tabs[self.active].selected_element = Some(element);
                            self.tabs[self.active].focus_element = true;
                        } else {
                            self.navigate(
                                self.tabs[self.active].url.clone(),
                                Navigation::Event(id),
                                ctx,
                            );
                        }
                    }
                }
            }
        }
        Ok(self.tabs[self.active].pending.is_some())
    }
    fn process_browser_actions(&mut self, ctx: &egui::Context) {
        if let Some(operation) = &self.ai_operation {
            let tab = &self.tabs[self.active];
            let result = if tab.id != operation.tab {
                Some(Err("A aba mudou durante a ação da IA.".into()))
            } else if operation.started.elapsed() > Duration::from_secs(90) {
                Some(Err("A página excedeu o tempo de espera da ação.".into()))
            } else if tab.pending.is_none() && tab.queued.is_none() {
                Some(if let Some(error) = &tab.error {
                    Err(error.clone())
                } else if tab.generation != operation.expected_generation {
                    Err(
                        "A página mudou por outra operação. A sequência da IA foi interrompida."
                            .into(),
                    )
                } else {
                    Ok("Ação concluída. A observação atualizada está disponível.".into())
                })
            } else {
                None
            };
            if let Some(result) = result {
                self.ai_operation = None;
                self.sync_browser_snapshot();
                self.ai.complete_browser_action(result, ctx);
            }
            return;
        }
        if self.tabs[self.active].pending.is_some() {
            return;
        }
        if let Some(action) = self.ai.take_browser_action() {
            match self.dispatch_browser_action(action, ctx) {
                Ok(true) => {
                    let tab = &self.tabs[self.active];
                    self.ai_operation = Some(AiOperation {
                        tab: tab.id,
                        expected_generation: tab.generation.saturating_add(1),
                        started: Instant::now(),
                    });
                }
                Ok(false) => {
                    self.sync_browser_snapshot();
                    self.ai.complete_browser_action(
                        Ok("Ação concluída. Consulte a observação atualizada.".into()),
                        ctx,
                    );
                }
                Err(error) => {
                    self.sync_browser_snapshot();
                    self.ai.complete_browser_action(Err(error), ctx);
                }
            }
        }
    }
    fn settings_content(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.glass(BG)).inner_margin(30))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let width = ui.available_width().min(850.);
                    ui.horizontal(|ui| {
                        ui.add_space(((ui.available_width() - width) / 2.).max(0.));
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.add_space(22.);
                            ui.horizontal(|ui| { icons::mark(ui, Icon::Settings, 24., ACCENT); ui.label(RichText::new("Configurações").size(30.)); });
                            ui.label(RichText::new("Um navegador com o seu jeito.").color(MUTED).size(15.));
                            ui.add_space(26.);
                            ui.horizontal_wrapped(|ui| {
                                for (index, label) in ["Aparência", "Navegação", "Inteligência", "Avançado"].iter().enumerate() {
                                    if ui.add(egui::Button::new(*label).selected(self.settings_section == index as u8).corner_radius(18)).clicked() { self.settings_section = index as u8; }
                                }
                            });
                            ui.add_space(24.);
                            egui::Frame::new().fill(self.glass(PANEL)).stroke(egui::Stroke::new(1., theme::LINE.gamma_multiply(0.6))).corner_radius(20).inner_margin(26).show(ui, |ui| {
                                ui.set_min_width((width - 52.).max(240.));
                                match self.settings_section {
                                    0 => {
                                        ui.label(RichText::new("Luz, vidro e movimento").size(22.));
                                        ui.add_space(8.);
                                        ui.label(RichText::new("Uma superfície leve para manter o foco no que importa.").color(MUTED));
                                        ui.add_space(24.);
                                        let (preview, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 132.), Sense::hover());
                                        let painter = ui.painter_at(preview);
                                        painter.rect_filled(preview, 14., Color32::from_rgb(34, 44, 42));
                                        for i in (0..22).rev() {
                                            painter.circle_filled(preview.center() + Vec2::new(-preview.width() * 0.22, 16.), 20. + i as f32 * 5., Color32::from_rgba_unmultiplied(191, 218, 145, 3));
                                            painter.circle_filled(preview.center() + Vec2::new(preview.width() * 0.3, -22.), 10. + i as f32 * 4., Color32::from_rgba_unmultiplied(111, 167, 170, 3));
                                        }
                                        let card = Rect::from_center_size(preview.center(), Vec2::new(preview.width().min(320.) - 30., 86.));
                                        painter.rect_filled(card, 12., self.glass(PANEL));
                                        painter.rect_stroke(card, 12., egui::Stroke::new(1., Color32::from_white_alpha(24)), egui::StrokeKind::Inside);
                                        icons::paint(ui, Icon::Diamond, Rect::from_center_size(card.left_center() + Vec2::new(34., 0.), Vec2::splat(26.)), ACCENT);
                                        painter.text(card.center() + Vec2::new(10., -9.), Align2::CENTER_CENTER, "Clareza em cada detalhe.", FontId::proportional(14.), TEXT);
                                        painter.text(card.center() + Vec2::new(10., 14.), Align2::CENTER_CENTER, "NOIR", FontId::proportional(10.), MUTED);
                                        ui.add_space(22.);
                                        changed |= ui.checkbox(&mut self.profile.acrylic_enabled, "Fundo acrílico").changed();
                                        ui.label(RichText::new("O desfoque usa o compositor do Windows.").size(12.).color(MUTED));
                                        ui.add_space(12.);
                                        let mut transparency = (255. - self.profile.acrylic_opacity as f32) / 255. * 100.;
                                        ui.horizontal(|ui| {
                                            ui.label("Transparência");
                                            let response = ui.add_enabled(self.profile.acrylic_enabled, egui::Slider::new(&mut transparency, 0.0..=88.0).integer().max_decimals(0).suffix("%"));
                                            if response.changed() { self.profile.acrylic_opacity = (255. * (1. - transparency / 100.)).round() as u8; changed = true; }
                                        });
                                        ui.add_space(20.); ui.separator(); ui.add_space(10.);
                                        changed |= ui.checkbox(&mut self.profile.reduce_motion, "Reduzir animações").changed();
                                        ui.label(RichText::new("Desativa movimentos de fundo e suavização das transições.").size(12.).color(MUTED));
                                        if let Some(message) = &self.backdrop_notice { ui.add_space(16.); ui.colored_label(AMBER, message); }
                                    }
                                    1 => {
                                        ui.label(RichText::new("Seu próximo ponto de partida").size(22.)); ui.add_space(18.);
                                        changed |= ui.checkbox(&mut self.profile.restore_session, "Continuar com as abas da última sessão").changed();
                                        ui.label(RichText::new("Abas fixadas são sempre lembradas. Cada grupo mantém suas próprias abas.").color(MUTED));
                                        ui.add_space(22.); ui.separator(); ui.add_space(12.);
                                        ui.horizontal(|ui| { icons::mark(ui, Icon::Search, 18., ACCENT); ui.label("Pesquisa padrão"); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.strong("Google"); }); });
                                        ui.add_space(18.);
                                        ui.horizontal(|ui| {
                                            ui.label("Zoom das páginas");
                                            egui::ComboBox::from_id_salt("settings-zoom").selected_text(format!("{}%", (self.zoom * 100.) as u32)).show_ui(ui, |ui| {
                                                for zoom in [0.75, 1., 1.25, 1.5, 2.] { ui.selectable_value(&mut self.zoom, zoom, format!("{}%", (zoom * 100.) as u32)); }
                                            });
                                        });
                                        ui.add_space(24.);
                                        ui.label(RichText::new("Dica: clique com o botão direito em uma aba para fixar ou mover para outro grupo.").color(MUTED));
                                    }
                                    2 => { self.ai.settings_page(ui); }
                                    _ => {
                                        ui.label(RichText::new("Ferramentas e informações").size(22.)); ui.add_space(16.);
                                        changed |= ui.checkbox(&mut self.profile.show_metrics, "Exibir diagnósticos técnicos").changed();
                                        ui.label(RichText::new("Métricas e avisos do documento ficam ocultos na navegação normal.").color(MUTED));
                                        if ui.button("Ferramentas do documento").clicked() { self.show_inspector = !self.show_inspector; }
                                        ui.add_space(22.); ui.separator(); ui.add_space(12.);
                                        ui.strong(concat!("Noir ", env!("CARGO_PKG_VERSION")));
                                        ui.label(format!("Compilação: {}", option_env!("NOIR_BUILD_ID").unwrap_or("development")));
                                        if let Ok(executable)=std::env::current_exe(){ui.label(RichText::new(executable.display().to_string()).size(11.).color(MUTED));}
                                        ui.label("Motor Web e JavaScript próprios.");
                                        ui.label(RichText::new("A compatibilidade com a Web continua em evolução. Sites que exigem APIs ainda não suportadas podem apresentar limitações.").color(MUTED));
                                        ui.add_space(12.);
                                        icons::mark(ui, Icon::File, 18., MUTED);
                                        if ui.button("Abrir documento local").clicked() { self.open_file(&ui.ctx().clone()); }
                                    }
                                }
                            });
                            ui.add_space(26.);
                            ui.label(RichText::new("Suas preferências são salvas automaticamente neste computador.").size(12.).color(MUTED));
                        });
                    });
                });
            });
        if changed {
            self.save_profile();
        }
    }
    fn content(&mut self, ui: &mut egui::Ui) {
        if self.tabs[self.active].url == SETTINGS {
            self.page_scene.clear();
            self.graphics_error = None;
            self.settings_content(ui);
            return;
        }
        if self.tabs[self.active].url == HOME && self.tabs[self.active].error.is_none() {
            self.page_scene.clear();
            self.graphics_error = None;
            self.home_content(ui);
            return;
        }
        let ctx = ui.ctx().clone();
        let gpu = self.page_gpu.clone();
        let mut graphics_denied = false;
        let graphics_scene = if let Some(driver) = &gpu {
            let tab = &self.tabs[self.active];
            match (&tab.page, &tab.document) {
                (Some(page), Some(document)) => {
                    match self.page_scene.prepare(
                        tab.id,
                        tab.generation,
                        ctx.pixels_per_point() * self.zoom,
                        page,
                        &document.images,
                    ) {
                        Ok(scene) if driver.supports(&scene) => {
                            self.graphics_error = None;
                            Some(scene)
                        }
                        result => {
                            let error = result
                                .err()
                                .unwrap_or_else(|| "Scene exceeds graphics device limits".into());
                            if self.graphics_error.as_ref() != Some(&error) {
                                self.notice=Some("Não foi possível pintar esta página dentro dos recursos gráficos disponíveis. Consulte os detalhes do documento.".into());
                            }
                            self.graphics_error = Some(error);
                            graphics_denied = true;
                            None
                        }
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        let mut clicked = None;
        let mut submitted: Option<(FormInfo, u64)> = None;
        let mut js_clicked = None;
        let mut input_event = None;
        let mut reflow = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.glass(BG)))
            .show(ui, |ui| {
                if let Some(notice) = self.notice.clone() {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(65, 46, 29))
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(notice);
                                if ui.small_button("×").clicked() {
                                    self.notice = None;
                                }
                            });
                        });
                }
                let tab = &mut self.tabs[self.active];
                if let Some(page)=&tab.page {
                    let limited=page.warnings.iter().any(|warning| warning.starts_with("JavaScript") && (warning.contains("error") || warning.contains("not implemented") || warning.contains("budget"))) || page.links.iter().any(|link|link.href.contains("/enablejs"));
                    if limited && !tab.dismissed_compatibility {
                        egui::Frame::new().fill(Color32::from_rgb(40,41,31)).inner_margin(10).show(ui,|ui|{
                            ui.horizontal_wrapped(|ui|{
                                icons::mark(ui,Icon::Globe,15.,AMBER);
                                ui.label(RichText::new("Parte desta página ainda não pode ser exibida corretamente.").size(12.).color(TEXT));
                                if ui.small_button("Detalhes").clicked(){self.show_inspector=true;}
                                if icons::button(ui,Icon::Close,"Ocultar aviso desta página",true).clicked(){tab.dismissed_compatibility=true;}
                            });
                        });
                    }
                }
                if let Some(error) = &tab.error {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(64, 32, 35))
                        .inner_margin(24)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Não foi possível abrir o documento").size(22.));
                            ui.label(error);
                            ui.label("Verifique o endereço ou abra uma nova aba para continuar.");
                        });
                }
                let width = (ui.available_width() / self.zoom - 12.).max(64.);
                tab.viewport.width = width;
                if (tab.rendered_width - width).abs() > 2. {
                    reflow = tab.document.is_some() && tab.pending.is_none();
                }
                tab.viewport.height = (ui.available_height() / self.zoom).clamp(64., 8192.);
                let restore = tab.scroll_to_restore();
                if let Some(page) = &tab.page {
                    let mut scroll = egui::ScrollArea::both()
                        .id_salt(("page", tab.id))
                        .auto_shrink([false, false]);
                    // Preclamp before egui paints: its final clamp occurs after the closure.
                    let restore = restore.map(|position| HistoryScroll::from_display(
                        position.display_offset(self.zoom),
                        Vec2::new(page.width, page.height) * self.zoom,
                        ui.available_size(), self.zoom,
                    ));
                    if let Some(position) = restore {
                        // Remove the previous document's kinetic/animated targets.
                        // Reuse one egui state slot per tab, not one per document.
                        reset_document_scroll(ui, tab.id);
                        scroll = scroll.scroll_offset(position.display_offset(self.zoom));
                    }
                    let zoom = self.zoom;
                    let output = scroll.show(ui, |ui| {
                        let size = Vec2::new(
                            (page.width * zoom).max(ui.available_width()),
                            (page.height * zoom).max(ui.available_height()),
                        );
                        let (canvas, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        let painter = ui.painter_at(canvas);
                        let page_painter = painter.clone();
                        let mut clips = vec![painter.clip_rect()];
                        painter.rect_filled(canvas, 0., to_color(page.background));
                        let mut graphics_allowed=!graphics_denied;
                        if let Some(scene)=&graphics_scene {
                            let visible=canvas.intersect(painter.clip_rect());
                            if visible.is_positive() {
                                let clip=browser_ipc::Rect{x:(visible.min.x-canvas.min.x)/zoom,y:(visible.min.y-canvas.min.y)/zoom,width:visible.width()/zoom,height:visible.height()/zoom};
                                if let Err(error)=scene.visible_work(clip,ctx.pixels_per_point()*zoom){
                                    self.graphics_error=Some(error);self.notice=Some("Esta página excedeu o limite de trabalho de pintura. Consulte os detalhes do documento.".into());ctx.request_repaint();graphics_allowed=false;
                                }
                            }
                        }
                        if graphics_allowed && canvas.intersect(painter.clip_rect()).is_positive()
                            && let Some(session) = &tab.session
                        {
                            self.displayed_frame_target = Some((tab.id, tab.committed_document, session.clone()));
                        }
                        let use_gpu=graphics_scene.is_some() && graphics_allowed;
                        let mut draw_cursor=0usize;
                        for (item_index, item) in page.items.iter().enumerate() {
                            if !graphics_allowed {break;}
                            match item {
                                DisplayItem::PushClip { rect: r } => {
                                    if clips.len() > 128 { break; }
                                    let rect = egui::Rect::from_min_size(canvas.min + Vec2::new(r.x*zoom,r.y*zoom),Vec2::new(r.width*zoom,r.height*zoom));
                                    clips.push(clips.last().copied().unwrap_or(canvas).intersect(rect));
                                    continue;
                                }
                                DisplayItem::PopClip => {
                                    if clips.len() <= 1 { break; }
                                    clips.pop();
                                    continue;
                                }
                                _ => {}
                            }
                            let painter = page_painter.with_clip_rect(*clips.last().expect("page clip"));
                            let r = match item {
                                DisplayItem::PushClip { .. } | DisplayItem::PopClip => continue,
                                DisplayItem::Rect { rect, .. }
                                | DisplayItem::Text { rect, .. }
                                | DisplayItem::Image { rect, .. }
                                | DisplayItem::Control { rect, .. } => *rect,
                            };
                            let rect = egui::Rect::from_min_size(
                                canvas.min + Vec2::new(r.x * zoom, r.y * zoom),
                                Vec2::new(r.width * zoom, r.height * zoom),
                            );
                            let highlighted=self.show_find && tab.matches.items.contains(&item_index) && matches!(item,DisplayItem::Text{..}) && painter.clip_rect().intersects(rect);
                            if use_gpu && (matches!(item,DisplayItem::Control{..}) || highlighted)
                                && let (Some(driver),Some(scene))=(&gpu,&graphics_scene)
                            {
                                let start=draw_cursor;
                                while draw_cursor<scene.draws.len() && scene.draws[draw_cursor].item<item_index {draw_cursor+=1;}
                                if start<draw_cursor {driver.paint(&page_painter,scene.clone(),start..draw_cursor,canvas,zoom);}
                            }
                            if !painter.clip_rect().intersects(rect) {
                                continue;
                            }
                            if use_gpu && !matches!(item,DisplayItem::Control{..}) {
                                if highlighted {painter.rect_filled(rect,0.,Color32::from_rgba_unmultiplied(219,173,65,90));}
                                continue;
                            }
                            match item {
                                DisplayItem::PushClip { .. } | DisplayItem::PopClip => {},
                                DisplayItem::Control{ id,kind,value,placeholder,form,disabled,appearance,options,.. }=>{
                                    if *kind==ControlKind::Hidden{continue;}
                                    let selected = tab.selected_element.as_deref() == Some(format!("control:{id}").as_str());
                                    let focus = selected && tab.focus_element;
                                    if focus { ui.scroll_to_rect(rect, Some(egui::Align::Center)); tab.focus_element = false; }
                                    if selected { painter.rect_stroke(rect.expand(2.), 5., egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Outside); }
                                    let state=tab.controls.entry(*id).or_insert_with(||value.clone());
                                    let selection_len = state.chars().count();
                                    ui.scope(|ui|{
                                        // Engine geometry must not inherit the native shell's generous widget metrics.
                                        ui.shrink_clip_rect(rect.intersect(painter.clip_rect()));
                                        let background = to_color(appearance.background);
                                        let foreground = to_color(appearance.color);
                                        let stroke = egui::Stroke::new(appearance.border_width * zoom,to_color(appearance.border_color));
                                        let visuals=ui.visuals_mut();
                                        visuals.override_text_color=Some(foreground);
                                        visuals.weak_text_color=Some(foreground.gamma_multiply(0.6));
                                        visuals.text_edit_bg_color=Some(background);
                                        visuals.extreme_bg_color=background;
                                        for widget in [&mut visuals.widgets.inactive,&mut visuals.widgets.hovered,&mut visuals.widgets.active,&mut visuals.widgets.open] {
                                            widget.bg_fill=background; widget.weak_bg_fill=background; widget.bg_stroke=stroke; widget.fg_stroke=egui::Stroke::new(1.,foreground); widget.corner_radius=egui::CornerRadius::same(2);
                                        }
                                        ui.style_mut().override_font_id=Some(FontId::proportional(appearance.font_size * zoom));
                                        if *disabled{ui.disable();}
                                        match kind {
                                            ControlKind::Text|ControlKind::Password|ControlKind::TextArea=>{
                                                let edit=if *kind==ControlKind::TextArea{egui::TextEdit::multiline(state)}else{egui::TextEdit::singleline(state)}.id(egui::Id::unique((tab.id,"control",id))).password(*kind==ControlKind::Password).char_limit(8192).hint_text(placeholder).font(egui::FontId::proportional(appearance.font_size*zoom)).frame(egui::Frame::new().fill(background).stroke(stroke).corner_radius(2).inner_margin(3));
                                                let response=ui.put(rect,edit);
                                                if focus { response.request_focus(); if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) { state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(selection_len)))); state.store(ui.ctx(), response.id); } }
                                                if response.changed(){input_event=Some(Navigation::Input(*id));}else if response.lost_focus(){input_event=Some(Navigation::Change(*id));}
                                                if response.lost_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter))&&*kind!=ControlKind::TextArea&&let Some(form)=form {submitted=Some((form.clone(),*id));}
                                            },
                                            ControlKind::Submit|ControlKind::Button=>{
                                                if page_control_button(ui,rect,value,appearance,zoom).clicked(){if *kind==ControlKind::Submit&&let Some(form)=form{submitted=Some((form.clone(),*id));}else{js_clicked=Some(*id);}}
                                            },
                                            ControlKind::Checkbox=>{let mut checked=!state.is_empty();if ui.put(rect,egui::Checkbox::without_text(&mut checked)).changed(){*state=if checked{if value.is_empty(){"on".into()}else{value.clone()}}else{String::new()};input_event=Some(Navigation::Change(*id));}},
                                            ControlKind::Select=>{
                                                let label=options.iter().find(|option|option.value==*state).map_or("",|option|option.label.as_str());
                                                let mut changed=false;
                                                let response=ui.scope_builder(egui::UiBuilder::new().max_rect(rect),|ui| {
                                                    egui::ComboBox::from_id_salt((tab.id,"select",id)).width(rect.width()).height(220.).selected_text(label).show_ui(ui,|ui|{
                                                        for option in options { if ui.add_enabled(!option.disabled,egui::Button::new(&option.label).selected(option.value==*state)).clicked(){*state=option.value.clone();changed=true;ui.close();} }
                                                    }).response
                                                }).inner;
                                                if focus { response.request_focus(); }
                                                if changed {input_event=Some(Navigation::Change(*id));}
                                            },
                                            ControlKind::Hidden=>{},
                                        }
                                    });
                                },
                                DisplayItem::Rect { color, .. } => {
                                    painter.rect_filled(rect, 0., to_color(*color));
                                }
                                DisplayItem::Text {
                                    text, size, color, ..
                                } => {
                                    if self.show_find && tab.matches.items.contains(&item_index) {
                                        painter.rect_filled(
                                            rect,
                                            0.,
                                            Color32::from_rgba_unmultiplied(219, 173, 65, 90),
                                        );
                                    }
                                    painter.text(
                                        rect.min,
                                        egui::Align2::LEFT_TOP,
                                        text,
                                        egui::FontId::proportional(*size * zoom),
                                        to_color(*color),
                                    );
                                }
                                DisplayItem::Image { asset_id, .. } => {
                                    if !tab.textures.contains_key(asset_id)
                                        && let Some(image) = tab.document.as_ref().and_then(|d| {
                                            d.images.iter().find(|i| i.id == *asset_id)
                                        })
                                    {
                                        let texture = ctx.load_texture(
                                            format!("{}:{}", tab.id, asset_id.chars().take(96).collect::<String>()),
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [image.width as usize, image.height as usize],
                                                &image.rgba,
                                            ),
                                            egui::TextureOptions::LINEAR,
                                        );
                                        tab.textures.insert(asset_id.clone(), texture);
                                    }
                                    if let Some(texture) = tab.textures.get(asset_id) {
                                        painter.image(
                                            texture.id(),
                                            rect,
                                            egui::Rect::from_min_max(
                                                egui::Pos2::ZERO,
                                                egui::pos2(1., 1.),
                                            ),
                                            Color32::WHITE,
                                        );
                                    }
                                }
                            }
                        }

                        if use_gpu && let (Some(driver),Some(scene))=(&gpu,&graphics_scene)
                            && draw_cursor<scene.draws.len()
                        {
                            driver.paint(&page_painter,scene.clone(),draw_cursor..scene.draws.len(),canvas,zoom);
                        }
                        for (i, link) in page.links.iter().enumerate() {
                            if !graphics_allowed {break;}
                            let r = link.rect;
                            let rect = egui::Rect::from_min_size(
                                canvas.min + Vec2::new(r.x * zoom, r.y * zoom),
                                Vec2::new(r.width * zoom, r.height * zoom),
                            );
                            if tab.selected_element.as_deref() == Some(format!("link:{i}").as_str()) {
                                painter.rect_stroke(rect.expand(2.), 4., egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Outside);
                                if tab.focus_element { ui.scroll_to_rect(rect, Some(egui::Align::Center)); tab.focus_element = false; }
                            }
                            let response = ui
                                .interact(
                                    rect,
                                    egui::Id::unique((tab.id, "link", i)),
                                    egui::Sense::click(),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(&link.href);
                            // Keyboard focus belongs to egui, not the AI element selection.
                            // Keep the original hit rectangle and native activation path.
                            if let Some(focus_rect) = page_link_focus_rect(
                                rect, painter.clip_rect(), response.has_focus(),
                            ) {
                                painter.with_clip_rect(focus_rect).rect_stroke(
                                    focus_rect,
                                    2.,
                                    ui.visuals().selection.stroke,
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if response.clicked() {
                                clicked = Some(link.href.clone());
                            }
                        }
                    });
                    // egui reports points; page geometry uses CSS pixels. No extra DPI.
                    let position = HistoryScroll::from_display(
                        output.state.clamped_offset(), output.content_size,
                        output.inner_rect.size(), zoom,
                    );
                    if restore.is_some_and(|requested| requested != position) {
                        // Scrollbar/layout changes can refine the final bounds after painting.
                        ui.ctx().request_repaint();
                    }
                    tab.capture_scroll(position);
                } else if tab.error.is_none() {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                    });
                }
            });
        if let Some(node) = js_clicked {
            self.navigate(
                self.tabs[self.active].url.clone(),
                Navigation::Event(node),
                &ctx,
            );
        } else if let Some((form, submitter)) = submitted {
            let tab = &self.tabs[self.active];
            if let Some(page) = &tab.page {
                match form_navigation(&tab.url, &form, submitter, &page.items, &tab.controls) {
                    Ok(url) => self.navigate(url, Navigation::Push, &ctx),
                    Err(e) => self.notice = Some(e),
                }
            }
        } else if let Some(event) = input_event {
            self.navigate(self.tabs[self.active].url.clone(), event, &ctx);
        } else if let Some(target) = clicked {
            match browser_core::resolve_navigation(&self.tabs[self.active].url, &target) {
                Ok(url) => self.navigate(url, Navigation::Push, &ctx),
                Err(e) => self.notice = Some(e),
            }
        } else if self.tabs[self.active].needs_initial_load {
            self.navigate(self.tabs[self.active].url.clone(), Navigation::Push, &ctx);
        } else if reflow {
            self.navigate(self.tabs[self.active].url.clone(), Navigation::Reflow, &ctx);
        }
    }
}

impl eframe::App for Noir {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.priority_work_this_pass = false;
        self.displayed_frame_target = None;
        ctx.style_mut_of(egui::Theme::Dark, |s| {
            s.animation_time = if self.profile.reduce_motion { 0. } else { 0.22 }
        });
        if let Some(message) = self.backdrop.update(
            frame,
            self.profile.acrylic_enabled,
            self.profile.acrylic_opacity,
        ) {
            self.backdrop_notice = Some(message);
        }
        self.poll(&ctx);
        self.sync_browser_snapshot();
        self.ai.show_settings(&ctx);
        self.shortcuts(&ctx);
        self.tabstrip(ui);
        self.toolbar(ui);
        if self.profile.show_metrics {
            self.status(ui);
        }
        if self.show_ai {
            egui::Panel::right("assistant")
                .default_size(340.)
                .size_range(300.0..=500.)
                .frame(egui::Frame::new().fill(self.glass(PANEL)).inner_margin(20))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icons::mark(ui, Icon::Spark, 20., ACCENT);
                        ui.label(RichText::new("Sua inteligência").size(20.));
                        if icons::button(ui, Icon::Settings, "Configurar IA", true).clicked() {
                            self.open_settings(&ctx);
                            self.settings_section = 2;
                        }
                        if icons::button(ui, Icon::Close, "Fechar assistente", true).clicked() {
                            self.show_ai = false;
                        }
                    });
                    if let Some(page) = &self.tabs[self.active].page
                        && ui.button("Disponibilizar texto desta página").clicked()
                    {
                        let text = page
                            .items
                            .iter()
                            .filter_map(|item| {
                                if let DisplayItem::Text { text, .. } = item {
                                    Some(text.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        self.ai.set_page_context(text);
                    }
                    self.ai.show(ui);
                });
        }
        self.inspector(ui);
        let fade = if self.profile.reduce_motion {
            1.
        } else {
            (self.transition.elapsed().as_secs_f32() / 0.2).clamp(0., 1.)
        };
        ui.scope(|ui| {
            ui.multiply_opacity(0.65 + 0.35 * fade);
            self.content(ui);
        });
        if fade < 1. {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        self.dialogs(&ctx);
        self.sync_browser_snapshot();
        self.process_browser_actions(&ctx);
        resize_window(&ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{} — Noir",
            self.tabs[self.active].title
        )));
        if self.capture.is_some() {
            if self.tabs[self.active].pending.is_none()
                && !self.tabs[self.active].needs_initial_load
                && (self.ai.is_ready() || self.started.elapsed() > Duration::from_secs(60))
            {
                self.capture_ready_frames += 1;
                if self.capture_ready_frames >= 5
                    && let Some(path) = self.capture.take()
                {
                    self.closing = true;
                    let context = ctx.clone();
                    ctx.request_screenshot(move |image| {
                        let rgba: Vec<u8> = image
                            .pixels
                            .iter()
                            .flat_map(|p| p.to_srgba_unmultiplied())
                            .collect();
                        let _ = image::save_buffer_with_format(
                            path,
                            &rgba,
                            image.width() as u32,
                            image.height() as u32,
                            image::ColorType::Rgba8,
                            image::ImageFormat::Png,
                        );
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    });
                }
            }
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        self.offer_animation_frame(&ctx);
    }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0., 0., 0., 0.]
    }
    fn on_exit(&mut self) {
        self.save_profile();
    }
}
// Coordinates are already zoomed egui points. This is paint-only: never use
// the clipped rectangle to replace or expand the link's interaction geometry.
fn page_link_focus_rect(rect: Rect, clip: Rect, has_focus: bool) -> Option<Rect> {
    let visible = rect.intersect(clip);
    (has_focus && visible.is_positive()).then_some(visible)
}

fn resize_window(ctx: &egui::Context) {
    if ctx.input(|i| {
        i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
    }) {
        return;
    }
    let bounds = ctx.viewport_rect();
    let Some(pos) = ctx.pointer_hover_pos() else {
        return;
    };
    let left = pos.x < bounds.left() + 5.;
    let right = pos.x > bounds.right() - 5.;
    let top = pos.y < bounds.top() + 5.;
    let bottom = pos.y > bounds.bottom() - 5.;
    use egui::{CursorIcon as C, ResizeDirection as D};
    let (direction, cursor) = match (left, right, top, bottom) {
        (true, _, true, _) => (D::NorthWest, C::ResizeNwSe),
        (_, true, true, _) => (D::NorthEast, C::ResizeNeSw),
        (true, _, _, true) => (D::SouthWest, C::ResizeNeSw),
        (_, true, _, true) => (D::SouthEast, C::ResizeNwSe),
        (true, _, _, _) => (D::West, C::ResizeHorizontal),
        (_, true, _, _) => (D::East, C::ResizeHorizontal),
        (_, _, true, _) => (D::North, C::ResizeVertical),
        (_, _, _, true) => (D::South, C::ResizeVertical),
        _ => return,
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}
fn poll_tab(tab: &mut Tab) -> bool {
    let result = tab
        .pending
        .as_ref()
        .and_then(|p| match p.receiver.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("A renderização foi interrompida.".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        });
    if let Some(result) = result {
        let pending = tab.pending.take().expect("pending request");
        // Retained work belongs to the already committed document even when a
        // navigation is queued or the final operation fails. Keep its confirmed
        // checkpoint before starting the next loader, without publishing it.
        if let Some(operation) = &pending.committed_storage {
            tab.checkpoint_operation(pending.navigation, operation);
        }
        if tab.queued.is_some() {
            return false;
        }
        if let Some(operation) = &pending.committed_storage
            && (operation.document != tab.committed_document
                || tab
                    .session
                    .as_ref()
                    .is_none_or(|session| !Arc::ptr_eq(session, &operation.session)))
        {
            return false;
        }
        match result {
            Ok(LoadedDocument {
                document,
                page,
                session,
                storage_origin,
                animation_origin,
            }) => {
                if matches!(
                    pending.navigation,
                    Navigation::Tick(_) | Navigation::Frame(_)
                ) && tab.page.as_ref().is_none_or(|old| {
                    old.items != page.items || old.links != page.links || old.title != page.title
                }) {
                    tab.generation = tab.generation.saturating_add(1);
                }
                tab.rendered_width = pending.viewport.width;
                tab.commit(
                    document.url.clone(),
                    if page.title.is_empty() {
                        "Página sem título".into()
                    } else {
                        page.title.clone()
                    },
                    pending.navigation,
                );
                tab.document = Some(document);
                tab.session = Some(session);
                tab.session_origin = storage_origin;
                if !retains_session(pending.navigation) {
                    tab.animation_origin = Some(animation_origin);
                }
                tab.checkpoint_session();
                if matches!(
                    pending.navigation,
                    Navigation::Event(_)
                        | Navigation::Input(_)
                        | Navigation::Change(_)
                        | Navigation::Tick(_)
                        | Navigation::Frame(_)
                ) {
                    tab.controls = page
                        .items
                        .iter()
                        .filter_map(|item| {
                            if let DisplayItem::Control { id, value, .. } = item {
                                Some((*id, value.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                }
                tab.page = Some(page);
                tab.displayed_document = Some(tab.committed_document);
                tab.matches.invalidate();
                return true;
            }
            Err(e) => tab.error = Some(e),
        }
    }
    false
}
fn short_title(title: &str, max: usize) -> String {
    if title.chars().count() > max {
        format!("{}…", title.chars().take(max - 1).collect::<String>())
    } else {
        title.into()
    }
}
fn to_color(c: browser_ipc::Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, c.3)
}

fn page_control_button(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    appearance: &browser_ipc::ControlAppearance,
    zoom: f32,
) -> egui::Response {
    ui.scope(|ui| {
        ui.shrink_clip_rect(rect);
        ui.spacing_mut().button_padding = Vec2::ZERO;
        ui.spacing_mut().interact_size = Vec2::ZERO;
        ui.put(
            rect,
            egui::Button::new(
                RichText::new(label)
                    .font(FontId::proportional(appearance.font_size * zoom))
                    .color(to_color(appearance.color)),
            )
            .truncate()
            .fill(to_color(appearance.background))
            .stroke(egui::Stroke::new(
                appearance.border_width * zoom,
                to_color(appearance.border_color),
            ))
            .corner_radius(2),
        )
    })
    .inner
}

fn form_navigation(
    base: &str,
    form: &FormInfo,
    submitter: u64,
    items: &[DisplayItem],
    values: &HashMap<u64, String>,
) -> Result<String, String> {
    let (source_kind, source_disabled) = items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Control {
                id,
                kind,
                disabled,
                form: Some(owner),
                ..
            } if *id == submitter && owner.id == form.id => Some((*kind, *disabled)),
            _ => None,
        })
        .ok_or("Controle de envio indisponível neste formulário.")?;
    if source_disabled {
        return Err("Este controle de formulário está desativado.".into());
    }
    let (submitter, form) = if source_kind == ControlKind::Submit {
        (Some(submitter), form)
    } else if matches!(source_kind, ControlKind::Text | ControlKind::Password) {
        // Enter activates the first submit button in document order, including
        // its name/value and action/method overrides. A disabled default button
        // blocks implicit submission; it must not fall through to another one.
        let default_submitter = items.iter().find_map(|item| match item {
            DisplayItem::Control {
                id,
                kind: ControlKind::Submit,
                form: Some(owner),
                disabled,
                ..
            } if owner.id == form.id => Some((*id, owner, *disabled)),
            _ => None,
        });
        if let Some((id, owner, disabled)) = default_submitter {
            if disabled {
                return Err("O botão de envio padrão está desativado.".into());
            }
            (Some(id), owner)
        } else {
            let blocking_fields = items
                .iter()
                .filter(|item| {
                    matches!(item, DisplayItem::Control { kind: ControlKind::Text | ControlKind::Password, form: Some(owner), .. } if owner.id == form.id)
                })
                .count();
            if blocking_fields > 1 {
                return Err("Este formulário precisa de um botão de envio para usar Enter.".into());
            }
            (None, form)
        }
    } else {
        return Err("Este controle não envia o formulário.".into());
    };
    if !form.method.eq_ignore_ascii_case("get") {
        return Err("Formulários POST ainda não são suportados nesta versão.".into());
    }
    let destination = if form.action.trim().is_empty() {
        base.to_owned()
    } else {
        browser_core::resolve_navigation(base, &form.action)?
    };
    let mut url = url::Url::parse(&destination).map_err(|_| "Endereço de formulário inválido")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Formulários só podem navegar para um endereço HTTP ou HTTPS.".into());
    }
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        for item in items {
            if let DisplayItem::Control {
                id,
                kind,
                name,
                value,
                form: Some(owner),
                disabled,
                options,
                ..
            } = item
            {
                if owner.id != form.id
                    || *disabled
                    || name.is_empty()
                    || *kind == ControlKind::Button
                    || (*kind == ControlKind::Submit && Some(*id) != submitter)
                {
                    continue;
                }
                let value = values.get(id).unwrap_or(value);
                if *kind == ControlKind::Checkbox && value.is_empty() {
                    continue;
                }
                if *kind == ControlKind::Select
                    && !options
                        .iter()
                        .any(|option| !option.disabled && option.value == *value)
                {
                    continue;
                }
                query.append_pair(name, value);
            }
        }
    }
    if url.as_str().len() > 8192 {
        return Err("O formulário excede o limite de tamanho do endereço.".into());
    }
    Ok(url.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_opportunities_are_visible_root_final_and_once_per_ui_frame() {
        let current = (egui::ViewportId::ROOT, 9);
        for visible in [None, Some(true)] {
            assert!(animation_opportunity_available(
                current, None, visible, false, false, false
            ));
            assert!(animation_opportunity_available(
                current,
                Some((current.0, 8)),
                visible,
                false,
                false,
                false
            ));
        }
        assert!(!animation_opportunity_available(
            current,
            Some(current),
            Some(true),
            false,
            false,
            false
        ));
        assert!(!animation_opportunity_available(
            current,
            None,
            Some(false),
            false,
            false,
            false
        ));
        assert!(!animation_opportunity_available(
            current,
            None,
            Some(true),
            true,
            false,
            false
        ));
        assert!(!animation_opportunity_available(
            current,
            None,
            Some(true),
            false,
            true,
            false
        ));
        assert!(!animation_opportunity_available(
            current,
            None,
            Some(true),
            false,
            false,
            true
        ));
        let secondary = (egui::ViewportId::from_hash_of("secondary"), 9);
        assert!(!animation_opportunity_available(
            secondary,
            None,
            Some(true),
            false,
            false,
            false
        ));
    }

    #[test]
    fn frames_never_queue_or_replace_busy_work_or_its_time_origin() {
        let mut app = app();
        let origin = Instant::now();
        let (sender, receiver) = mpsc::sync_channel(1);
        let tab = &mut app.tabs[0];
        tab.animation_origin = Some(origin);
        tab.pending = Some(Pending {
            receiver,
            committed_storage: None,
            navigation: Navigation::Frame(1.),
            started: origin,
            viewport: Default::default(),
        });
        tab.queued = Some(("https://latest.test/".into(), Navigation::Push));
        app.navigate(
            "https://ignored.test/".into(),
            Navigation::Frame(2.),
            &egui::Context::default(),
        );
        let tab = &mut app.tabs[0];
        assert!(
            matches!(tab.pending.as_ref().unwrap().navigation, Navigation::Frame(time) if time == 1.)
        );
        assert_eq!(tab.queued.as_ref().unwrap().0, "https://latest.test/");
        sender.send(Err("stale frame error".into())).unwrap();
        assert!(!poll_tab(tab));
        assert!(tab.error.is_none());
        assert_eq!(tab.animation_origin, Some(origin));
        assert_eq!(app.frame_opportunity, None);
    }

    #[test]
    fn retained_commits_preserve_document_clock_and_history() {
        let mut tab = Tab::new(1, "https://a.test/".into());
        tab.commit("https://a.test/".into(), "A".into(), Navigation::Push);
        let origin = Instant::now();
        tab.animation_origin = Some(origin);
        let document = tab.committed_document;
        for navigation in [
            Navigation::Reflow,
            Navigation::Event(1),
            Navigation::Tick(3),
            Navigation::Frame(4.),
        ] {
            tab.commit("https://a.test/".into(), "A".into(), navigation);
            assert_eq!(tab.animation_origin, Some(origin));
            assert_eq!(tab.committed_document, document);
            assert_eq!(tab.history.len(), 1);
        }
    }

    #[test]
    fn link_focus_outline_requires_keyboard_focus_and_visible_area() {
        let rect = Rect::from_min_size(egui::pos2(20., 30.), Vec2::new(80., 24.));
        assert_eq!(page_link_focus_rect(rect, rect, true), Some(rect));
        assert_eq!(page_link_focus_rect(rect, rect, false), None);
        let outside = rect.translate(Vec2::new(200., 0.));
        assert_eq!(page_link_focus_rect(rect, outside, true), None);
        let edge = rect.translate(Vec2::new(rect.width(), 0.));
        assert_eq!(page_link_focus_rect(rect, edge, true), None);
    }

    #[test]
    fn link_focus_outline_clips_zoomed_geometry_without_growing_hit_rect() {
        let clip = Rect::from_min_max(egui::pos2(30., 40.), egui::pos2(110., 90.));
        for zoom in [0.5, 1., 2.] {
            let hit_rect =
                Rect::from_min_size(egui::pos2(20., 30.), Vec2::new(80. * zoom, 24. * zoom));
            let original = hit_rect;
            let outline = page_link_focus_rect(hit_rect, clip, true).unwrap();
            assert_eq!(outline, hit_rect.intersect(clip));
            assert!(hit_rect.contains_rect(outline));
            assert!(clip.contains_rect(outline));
            assert_eq!(hit_rect, original);
        }
    }

    #[test]
    fn link_focus_geometry_is_independent_of_ai_selection() {
        let mut tab = Tab::new(7, HOME.into());
        let rect = Rect::from_min_size(egui::pos2(20., 30.), Vec2::new(80., 24.));
        for selection in [None, Some("link:0"), Some("control:1")] {
            tab.selected_element = selection.map(str::to_owned);
            assert_eq!(page_link_focus_rect(rect, rect, true), Some(rect));
            assert_eq!(page_link_focus_rect(rect, rect, false), None);
            assert_eq!(tab.selected_element.as_deref(), selection);
        }
    }

    fn control(
        form: &FormInfo,
        id: u64,
        kind: ControlKind,
        name: &str,
        value: &str,
    ) -> DisplayItem {
        DisplayItem::Control {
            rect: Default::default(),
            id,
            kind,
            name: name.into(),
            value: value.into(),
            placeholder: String::new(),
            form: Some(form.clone()),
            disabled: false,
            appearance: Default::default(),
            options: vec![],
        }
    }

    #[test]
    fn page_buttons_keep_engine_geometry_and_font_under_shell_theme() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let rect = Rect::from_min_size(egui::pos2(20., 30.), Vec2::new(126., 31.));
        let appearance = browser_ipc::ControlAppearance {
            font_size: 15.,
            ..Default::default()
        };
        let output = ctx.run_ui(Default::default(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let response = page_control_button(ui, rect, "Pesquisa Google", &appearance, 1.);
                assert_eq!(response.rect, rect);
            });
        });
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some((shape.clip_rect, text)),
                _ => None,
            })
            .expect("button paints a label");
        assert_eq!(text.0, rect);
        assert_eq!(text.1.galley.rows.len(), 1);
        assert_eq!(text.1.galley.job.sections[0].format.font_id.size, 15.);
        assert!(!text.1.galley.elided);
        output.drop_without_applying_deltas();
    }

    #[test]
    fn narrow_page_buttons_clip_single_line_labels_and_do_not_grow_hit_regions() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let rect = Rect::from_min_size(egui::pos2(20., 30.), Vec2::new(44., 20.));
        let output = ctx.run_ui(Default::default(), |root| {
            egui::CentralPanel::default().show(root, |ui| {
                ui.disable();
                let response =
                    page_control_button(ui, rect, "A long button label", &Default::default(), 0.75);
                assert_eq!(response.rect, rect);
                assert!(!response.enabled());
            });
        });
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some((shape.clip_rect, text)),
                _ => None,
            })
            .expect("button paints a label");
        assert_eq!(text.0, rect);
        assert_eq!(text.1.galley.rows.len(), 1);
        assert!(text.1.galley.elided);
        assert_eq!(text.1.galley.job.sections[0].format.font_id.size, 12.);
        output.drop_without_applying_deltas();
    }

    #[test]
    fn page_control_clip_does_not_clip_select_popup_to_the_closed_control() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let rect = Rect::from_min_size(egui::pos2(20., 30.), Vec2::new(160., 34.));
        let mut painted_popup = false;
        for frame in 0..3 {
            let output = ctx.run_ui(Default::default(), |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    ui.scope(|ui| {
                        ui.shrink_clip_rect(rect);
                        let response = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                egui::ComboBox::from_id_salt("page-select-test")
                                    .width(rect.width())
                                    .selected_text("Selected")
                                    .show_ui(ui, |ui| {
                                        ui.label("Popup option");
                                    })
                                    .response
                            })
                            .inner;
                        if frame == 0 {
                            egui::Popup::open_id(ui.ctx(), response.id.with("popup"));
                        }
                    });
                });
            });
            for shape in &output.shapes {
                if let egui::epaint::Shape::Text(text) = &shape.shape
                    && text.galley.job.text == "Popup option"
                {
                    assert!(text.pos.y >= rect.bottom());
                    assert!(shape.clip_rect.bottom() > rect.bottom());
                    painted_popup = true;
                }
            }
            output.drop_without_applying_deltas();
        }
        assert!(
            painted_popup,
            "select popup remains visible outside its control"
        );
    }

    #[test]
    fn enter_activates_first_submitter_and_its_action_override() {
        let form = FormInfo {
            id: 1,
            action: "/search".into(),
            method: "get".into(),
        };
        let default_action = FormInfo {
            action: "/results".into(),
            ..form.clone()
        };
        let unrelated = FormInfo {
            id: 99,
            ..form.clone()
        };
        let items = vec![
            control(&unrelated, 90, ControlKind::Submit, "wrong", "no"),
            control(&form, 2, ControlKind::Text, "q", "old"),
            control(
                &default_action,
                3,
                ControlKind::Submit,
                "btnG",
                "Pesquisa Google",
            ),
            control(&form, 4, ControlKind::Submit, "btnI", "Estou com sorte"),
        ];
        let url = form_navigation(
            "https://example.com/",
            &form,
            2,
            &items,
            &HashMap::from([(2, "ação local".into())]),
        )
        .unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.path(), "/results");
        assert_eq!(
            parsed.query_pairs().collect::<Vec<_>>(),
            [
                ("q".into(), "ação local".into()),
                ("btnG".into(), "Pesquisa Google".into())
            ]
        );
        let mut disabled = items.clone();
        if let DisplayItem::Control { disabled, .. } = &mut disabled[2] {
            *disabled = true;
        }
        assert!(
            form_navigation("https://example.com/", &form, 2, &disabled, &HashMap::new())
                .unwrap_err()
                .contains("padrão está desativado")
        );
        let mut post = items;
        if let DisplayItem::Control {
            form: Some(owner), ..
        } = &mut post[2]
        {
            owner.method = "post".into();
        }
        assert!(
            form_navigation("https://example.com/", &form, 2, &post, &HashMap::new())
                .unwrap_err()
                .contains("POST")
        );
    }

    #[test]
    fn implicit_submission_without_button_requires_a_single_text_field() {
        let form = FormInfo {
            id: 1,
            action: "/search".into(),
            method: "get".into(),
        };
        let mut items = vec![control(&form, 2, ControlKind::Text, "q", "noir")];
        assert_eq!(
            form_navigation("https://example.com/", &form, 2, &items, &HashMap::new()).unwrap(),
            "https://example.com/search?q=noir"
        );
        items.push(control(
            &form,
            3,
            ControlKind::Password,
            "password",
            "secret",
        ));
        assert!(
            form_navigation("https://example.com/", &form, 2, &items, &HashMap::new()).is_err()
        );
        assert!(
            form_navigation("https://example.com/", &form, 99, &items, &HashMap::new()).is_err()
        );
    }

    fn history_urls(tab: &Tab) -> Vec<&str> {
        tab.history.iter().map(|entry| entry.url.as_str()).collect()
    }
    fn app() -> Noir {
        Noir::from_profile(
            Profile::default(),
            None,
            None,
            None,
            None,
            AiPanel::disabled_for_test(),
        )
    }
    fn snapshot_page(items: Vec<DisplayItem>, links: Vec<browser_ipc::Link>) -> RenderedPage {
        RenderedPage {
            title: String::new(),
            width: 800.,
            height: 600.,
            background: Default::default(),
            items,
            links,
            metrics: Default::default(),
            warnings: vec![],
            next_timer_ms: None,
            animation_frame_pending: false,
        }
    }
    fn snapshot_control(id: u64, kind: ControlKind, value: &str) -> DisplayItem {
        DisplayItem::Control {
            rect: Default::default(),
            id,
            kind,
            name: String::new(),
            value: value.into(),
            placeholder: String::new(),
            form: None,
            disabled: false,
            appearance: Default::default(),
            options: vec![],
        }
    }
    #[test]
    fn snapshot_bounds_unicode_home_and_active_and_parked_tabs_without_mutation() {
        let mut app = app();
        let huge = "🦉".repeat(10_000);
        app.tabs[0].title = huge.clone();
        app.tabs[0].home.query = huge.clone();
        app.tabs[0].generation = 93;
        app.tabs[0].selected_element = Some("home:search".into());
        let mut parked = Vec::new();
        for id in 100..200 {
            let mut tab = Tab::new(id, huge.clone());
            tab.title = huge.clone();
            parked.push(tab);
        }
        // Replace the existing parked group, not the active workspace. Adding
        // another group would make these ordered assertions depend on HashMap order.
        assert_eq!(app.parked.len(), 1);
        let group = *app.parked.keys().next().unwrap();
        assert_ne!(group, app.profile.active_workspace);
        app.profile
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == group)
            .unwrap()
            .name = huge.clone();
        assert!(app.parked.insert(group, (parked, 0)).is_some());
        let snapshot = app.browser_snapshot();
        assert_eq!(snapshot.title, "🦉".repeat(160));
        assert_eq!(snapshot.elements[0].value, "🦉".repeat(160));
        assert_eq!(snapshot.tabs.len(), 24);
        assert_eq!(snapshot.tabs[0].id, app.tabs[0].id);
        assert_eq!(snapshot.tabs[1].id, 100);
        assert_eq!(snapshot.tabs[1].title, format!("[{}", "🦉".repeat(119)));
        assert_eq!(snapshot.tabs[1].url, "🦉".repeat(2048));
        assert_eq!(snapshot.scope.generation, 93);
        assert_eq!(snapshot.scope.tab, app.tabs[0].id);
        assert!(snapshot.selection.ends_with(&"🦉".repeat(160)));
        assert_eq!(app.tabs[0].home.query, huge);
        assert_eq!(app.tabs[0].selected_element.as_deref(), Some("home:search"));
        app.tabs[0].url = huge.clone();
        assert_eq!(app.browser_snapshot().url, "🦉".repeat(2048));
        for id in 200..240 {
            app.tabs.push(Tab::new(id, huge.clone()));
        }
        let snapshot = app.browser_snapshot();
        assert_eq!(snapshot.tabs.len(), 24);
        assert_eq!(snapshot.tabs[23].id, 222);
    }
    #[test]
    fn snapshot_preserves_control_filters_order_scope_and_independent_selection() {
        let mut app = app();
        let huge = "界".repeat(10_000);
        let mut disabled = snapshot_control(3, ControlKind::Text, "private disabled");
        if let DisplayItem::Control { disabled, .. } = &mut disabled {
            *disabled = true;
        }
        let mut items = vec![
            snapshot_control(1, ControlKind::Password, "private password"),
            snapshot_control(2, ControlKind::Hidden, "private hidden"),
            disabled,
        ];
        items.extend((10..60).map(|id| snapshot_control(id, ControlKind::Text, &huge)));
        let links = (0..100)
            .map(|_| browser_ipc::Link {
                rect: Default::default(),
                href: huge.clone(),
            })
            .collect();
        app.tabs[0].page = Some(snapshot_page(items, links));
        app.tabs[0].controls.insert(10, "🦉".repeat(10_000));
        app.tabs[0].generation = 42;
        app.tabs[0].selected_element = Some("control:1".into());
        let snapshot = app.browser_snapshot();
        assert_eq!(snapshot.elements.len(), 64);
        assert_eq!(snapshot.elements[0].id, "control:10");
        assert_eq!(snapshot.elements[39].id, "control:49");
        assert_eq!(snapshot.elements[40].id, "link:0");
        assert_eq!(snapshot.elements[63].id, "link:23");
        assert_eq!(snapshot.elements[0].label, "界".repeat(160));
        assert_eq!(snapshot.elements[0].value, "🦉".repeat(160));
        assert_eq!(snapshot.elements[40].label, "界".repeat(160));
        assert_eq!(snapshot.elements[40].value, "界".repeat(160));
        assert!(snapshot.selection.is_empty());
        assert_eq!(app.tabs[0].selected_element.as_deref(), Some("control:1"));
        assert_eq!(snapshot.scope.generation, 42);
        app.tabs[0].selected_element = Some("control:10".into());
        let selected = app.browser_snapshot();
        assert_eq!(selected.scope, snapshot.scope);
        assert_eq!(
            selected.selection,
            format!("{}: {}", "界".repeat(160), "🦉".repeat(160))
        );
    }
    #[test]
    fn snapshot_select_labels_limit_enabled_options_and_large_parts() {
        let mut app = app();
        let mut select = snapshot_control(1, ControlKind::Select, "");
        if let DisplayItem::Control { name, options, .. } = &mut select {
            *name = "pick".into();
            options.push(browser_ipc::ControlOption {
                label: "excluded".repeat(10_000),
                value: "excluded".into(),
                disabled: true,
            });
            options.extend((0..20).map(|i| browser_ipc::ControlOption {
                label: i.to_string(),
                value: i.to_string(),
                disabled: false,
            }));
        }
        app.tabs[0].page = Some(snapshot_page(vec![select], vec![]));
        assert_eq!(
            app.browser_snapshot().elements[0].label,
            "pick [0=0, 1=1, 2=2, 3=3, 4=4, 5=5, 6=6, 7=7]"
        );
        let page = app.tabs[0].page.as_mut().unwrap();
        if let DisplayItem::Control { options, .. } = &mut page.items[0] {
            options[1].label = "🦉".repeat(10_000);
            options[1].value = "界".repeat(10_000);
        }
        assert_eq!(
            app.browser_snapshot().elements[0].label,
            format!("pick [{}", "🦉".repeat(154))
        );
    }
    #[test]
    fn snapshot_page_and_link_text_stop_at_character_and_fragment_limits() {
        let mut app = app();
        let rect = browser_ipc::Rect {
            x: 0.,
            y: 0.,
            width: 100.,
            height: 20.,
        };
        let text = |value: &str| DisplayItem::Text {
            rect,
            text: value.into(),
            size: 14.,
            color: Default::default(),
            bold: false,
        };
        app.tabs[0].page = Some(snapshot_page(
            vec![text("a"), text("b"), text("c"), text("d"), text("excluded")],
            vec![browser_ipc::Link {
                rect,
                href: "destination".into(),
            }],
        ));
        assert_eq!(app.browser_snapshot().elements[0].label, "a b c d ");
        app.tabs[0].page.as_mut().unwrap().items = vec![text(&"🦉".repeat(10_000))];
        let snapshot = app.browser_snapshot();
        assert_eq!(snapshot.text, "🦉".repeat(4000));
        assert_eq!(snapshot.elements[0].label, "🦉".repeat(160));
    }
    #[test]
    fn workspaces_keep_runtime_history_active_tab_and_home_query() {
        let mut app = app();
        let original = app.tabs[0].id;
        app.tabs[0].home.query = "pesquisa pessoal".into();
        app.tabs[0].commit(
            "https://example.com/".into(),
            "Example".into(),
            Navigation::Push,
        );
        app.new_tab();
        let personal_active = app.tabs[1].id;
        app.switch_workspace(2);
        assert_eq!(app.tabs.len(), 1);
        assert_ne!(app.tabs[0].id, original);
        app.tabs[0].home.query = "outra pesquisa".into();
        app.new_tab();
        app.switch_workspace(1);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[app.active].id, personal_active);
        assert_eq!(app.tabs[0].home.query, "pesquisa pessoal");
        assert_eq!(history_urls(&app.tabs[0]), [HOME, "https://example.com/"]);
        app.switch_workspace(2);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[0].home.query, "outra pesquisa");
    }
    #[test]
    fn moving_tabs_preserves_identity_and_unrelated_selection() {
        let mut app = app();
        app.new_tab();
        app.new_tab();
        app.active = 1;
        let selected = app.tabs[1].id;
        let moved = app.tabs[0].id;
        app.tabs[0].commit(
            "https://example.org/".into(),
            "Example".into(),
            Navigation::Push,
        );
        app.move_tab(0, 2);
        assert_eq!(app.tabs[app.active].id, selected);
        app.switch_workspace(2);
        assert_eq!(app.tabs[app.active].id, moved);
        assert_eq!(
            history_urls(&app.tabs[app.active]),
            [HOME, "https://example.org/"]
        );
        let count = app.total_tabs();
        app.move_tab(app.active, 999);
        assert_eq!(app.total_tabs(), count);
    }
    #[test]
    fn get_form_encodes_unicode_and_only_successful_controls() {
        let form = FormInfo {
            id: 1,
            action: String::new(),
            method: "get".into(),
        };
        let control = |id, kind, name: &str, value: &str, disabled| DisplayItem::Control {
            rect: Default::default(),
            id,
            kind,
            name: name.into(),
            value: value.into(),
            placeholder: String::new(),
            form: Some(form.clone()),
            disabled,
            appearance: Default::default(),
            options: vec![],
        };
        let items = vec![
            control(2, ControlKind::Text, "q", "old", false),
            control(3, ControlKind::Hidden, "hl", "pt-BR", false),
            control(4, ControlKind::Checkbox, "unchecked", "", false),
            control(5, ControlKind::Submit, "submit", "Buscar", false),
            control(6, ControlKind::Submit, "other", "Outro", false),
            control(7, ControlKind::Text, "disabled", "secret", true),
        ];
        let url = form_navigation(
            "https://www.google.com.br/search?discard=1",
            &form,
            5,
            &items,
            &HashMap::from([(2, "ação local".into())]),
        )
        .unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(
            parsed.query_pairs().collect::<Vec<_>>(),
            [
                ("q".into(), "ação local".into()),
                ("hl".into(), "pt-BR".into()),
                ("submit".into(), "Buscar".into())
            ]
        );
        let mut post = form.clone();
        post.method = "post".into();
        assert!(
            form_navigation("https://example.com/", &post, 5, &items, &HashMap::new()).is_err()
        );
    }

    #[test]
    fn scroll_reset_uses_the_actual_egui_slot_and_restore_offset() {
        // In-memory egui only: no application, window, renderer or navigation.
        let ctx = egui::Context::default();
        for offset in [Vec2::new(100., 200.), Vec2::new(70., 90.), Vec2::ZERO] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(400.))),
                ..Default::default()
            };
            let mut frame = ctx.run_ui(input, |ui| {
                let reset_id = reset_document_scroll(ui, 7);
                let output = egui::ScrollArea::both()
                    .id_salt(("page", 7_u64))
                    .auto_shrink([false, false])
                    .scroll_offset(offset)
                    .show(ui, |ui| {
                        ui.allocate_exact_size(Vec2::splat(2000.), Sense::hover());
                    });
                assert_eq!(output.id, reset_id);
                assert_eq!(output.state.clamped_offset(), offset);
                reset_document_scroll(ui, 7);
                let reset = egui::scroll_area::State::load(&ctx, output.id).unwrap();
                assert_eq!(reset.clamped_offset(), Vec2::ZERO);
                assert_eq!(reset.velocity(), Vec2::ZERO);
            });
            // No painter exists in this headless test; explicitly discard GPU work.
            frame.textures_delta.clear();
        }
    }
    fn commit_document(tab: &mut Tab, url: &str, nav: Navigation) {
        tab.commit(url.into(), url.into(), nav);
        tab.displayed_document = Some(tab.committed_document);
    }
    fn position(x: f32, y: f32) -> HistoryScroll {
        HistoryScroll::from_display(Vec2::new(x, y), Vec2::splat(4000.), Vec2::splat(500.), 1.)
    }
    #[test]
    fn history_restores_each_entry_in_both_axes_once() {
        let mut tab = Tab::new(1, HOME.into());
        commit_document(&mut tab, "a", Navigation::Push);
        tab.capture_scroll(position(120., 800.));
        commit_document(&mut tab, "b", Navigation::Push);
        assert_eq!(tab.scroll_to_restore(), Some(HistoryScroll::default()));
        tab.capture_scroll(position(300., 600.));
        commit_document(&mut tab, "a", Navigation::Traverse(1));
        assert_eq!(tab.scroll_to_restore(), Some(position(120., 800.)));
        tab.capture_scroll(position(120., 800.));
        assert_eq!(tab.scroll_to_restore(), None);
        commit_document(&mut tab, "b", Navigation::Traverse(2));
        assert_eq!(tab.scroll_to_restore(), Some(position(300., 600.)));
        // A repeated URL at a different index is an independent entry.
        commit_document(&mut tab, "a", Navigation::Push);
        tab.capture_scroll(position(50., 60.));
        assert_eq!(tab.history[1].scroll, position(120., 800.));
        assert_eq!(tab.history[3].scroll, position(50., 60.));
        commit_document(&mut tab, "a", Navigation::Push);
        assert_eq!(tab.history.len(), 4);
        assert_eq!(tab.history[3].scroll, HistoryScroll::default());
    }
    #[test]
    fn history_branch_limit_and_refresh_reset() {
        let mut tab = Tab::new(1, HOME.into());
        for i in 0..205 {
            commit_document(&mut tab, &i.to_string(), Navigation::Push);
        }
        assert_eq!(tab.history.len(), 200);
        assert_eq!(tab.history[0].url, "5");
        assert_eq!(tab.history_index, 199);
        commit_document(&mut tab, "6", Navigation::Traverse(1));
        tab.capture_scroll(position(10., 20.));
        commit_document(&mut tab, "6", Navigation::Refresh);
        assert_eq!(tab.history[1].scroll, HistoryScroll::default());
        assert_eq!(tab.history.len(), 200);
        commit_document(&mut tab, "branch", Navigation::Push);
        assert_eq!(history_urls(&tab), ["5", "6", "branch"]);
        assert_eq!(tab.history_index, 2);
    }
    #[test]
    fn retained_operations_preserve_document_and_restore() {
        let mut tab = Tab::new(1, "a".into());
        commit_document(&mut tab, "a", Navigation::Push);
        tab.capture_scroll(position(100., 200.));
        commit_document(&mut tab, "b", Navigation::Push);
        commit_document(&mut tab, "a", Navigation::Traverse(0));
        let pending = tab.pending_scroll;
        let document = tab.committed_document;
        for nav in [
            Navigation::Reflow,
            Navigation::Event(1),
            Navigation::Input(1),
            Navigation::Change(1),
            Navigation::Tick(20),
            Navigation::Frame(20.),
        ] {
            tab.commit("a".into(), "A".into(), nav);
            assert_eq!(tab.pending_scroll, pending);
            assert_eq!(tab.committed_document, document);
            assert_eq!(tab.scroll_to_restore(), Some(position(100., 200.)));
        }
        tab.capture_scroll(position(150., 250.));
        tab.commit("a".into(), "A".into(), Navigation::Reflow);
        assert_eq!(tab.pending_scroll, None);
        assert_eq!(tab.history[0].scroll, position(150., 250.));
    }
    #[test]
    fn restore_waits_for_its_document_and_new_commit_replaces_it() {
        let mut tab = Tab::new(1, "a".into());
        commit_document(&mut tab, "a", Navigation::Push);
        tab.capture_scroll(position(10., 200.));
        commit_document(&mut tab, "b", Navigation::Push);
        tab.commit("a".into(), "A".into(), Navigation::Traverse(0));
        let pending = tab.pending_scroll;
        assert_eq!(tab.scroll_to_restore(), None);
        tab.capture_scroll(position(0., 0.));
        assert_eq!(tab.pending_scroll, pending);
        // Parking/moving a Tab keeps both its entries and pending restore.
        let mut parked = HashMap::from([(2, tab)]);
        let mut tab = parked.remove(&2).unwrap();
        tab.displayed_document = Some(tab.committed_document.wrapping_sub(1));
        assert_eq!(tab.scroll_to_restore(), None);
        tab.displayed_document = Some(tab.committed_document);
        assert_eq!(tab.scroll_to_restore(), Some(position(10., 200.)));
        commit_document(&mut tab, "c", Navigation::Push);
        assert_eq!(tab.scroll_to_restore(), Some(HistoryScroll::default()));
        let other = Tab::new(2, "c".into());
        assert!(other.history.is_empty());
        assert_eq!(other.pending_scroll, None);
    }
    #[test]
    fn failure_disconnection_and_queued_result_preserve_history_scroll() {
        for (queued, disconnected) in [(false, false), (false, true), (true, false)] {
            let mut tab = Tab::new(1, "a".into());
            commit_document(&mut tab, "a", Navigation::Push);
            tab.capture_scroll(position(30., 400.));
            commit_document(&mut tab, "b", Navigation::Push);
            commit_document(&mut tab, "a", Navigation::Traverse(0));
            let restore = tab.pending_scroll;
            let (sender, receiver) = mpsc::sync_channel(1);
            tab.pending = Some(Pending {
                receiver,
                committed_storage: None,
                navigation: Navigation::Traverse(1),
                started: Instant::now(),
                viewport: Default::default(),
            });
            if queued {
                tab.queued = Some(("c".into(), Navigation::Push));
            }
            if !disconnected {
                sender.send(Err("failed".into())).unwrap();
            }
            drop(sender);
            assert!(!poll_tab(&mut tab));
            assert_eq!(tab.pending_scroll, restore);
            assert_eq!(tab.history_index, 0);
            assert_eq!(tab.history[0].scroll, position(30., 400.));
            assert_eq!(tab.history[1].scroll, HistoryScroll::default());
            assert_eq!(tab.scroll_to_restore(), None);
            tab.capture_scroll(position(1., 2.));
            assert_eq!(tab.pending_scroll, restore);
        }
    }
    #[test]
    fn scroll_coordinates_validate_zoom_geometry_and_smaller_pages() {
        let css = position(120., 300.);
        for zoom in [0.5, 1., 2., 3.] {
            assert_eq!(
                HistoryScroll::from_display(
                    css.display_offset(zoom),
                    Vec2::splat(4000. * zoom),
                    Vec2::splat(500. * zoom),
                    zoom
                ),
                css
            );
        }
        assert_eq!(
            HistoryScroll::from_display(
                Vec2::new(200., 600.),
                Vec2::new(250., 100.),
                Vec2::splat(200.),
                1.
            ),
            position(50., 0.)
        );
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.] {
            assert_eq!(
                HistoryScroll::from_display(
                    Vec2::splat(invalid),
                    Vec2::splat(4000.),
                    Vec2::splat(500.),
                    1.
                ),
                HistoryScroll::default()
            );
            assert_eq!(
                HistoryScroll::from_display(
                    Vec2::splat(30.),
                    Vec2::splat(invalid),
                    Vec2::splat(500.),
                    1.
                ),
                HistoryScroll::default()
            );
            assert_eq!(
                HistoryScroll::from_display(
                    Vec2::splat(30.),
                    Vec2::splat(4000.),
                    Vec2::splat(invalid),
                    1.
                ),
                HistoryScroll::default()
            );
            assert_eq!(
                HistoryScroll::from_display(
                    Vec2::splat(30.),
                    Vec2::splat(4000.),
                    Vec2::splat(500.),
                    invalid
                ),
                HistoryScroll::default()
            );
        }
        assert_eq!(
            HistoryScroll::from_display(
                Vec2::splat(30.),
                Vec2::splat(4000.),
                Vec2::splat(500.),
                0.
            ),
            HistoryScroll::default()
        );
        let capped = HistoryScroll::from_display(
            Vec2::splat(f32::MAX),
            Vec2::splat(f32::MAX),
            Vec2::splat(100.),
            1.,
        );
        assert_eq!(capped.x, MAX_HISTORY_SCROLL - 100.);
        assert_eq!(capped.y, MAX_HISTORY_SCROLL - 100.);
    }
    #[test]
    fn internal_pages_and_empty_history_do_not_capture_document_scroll() {
        for url in [HOME, SETTINGS] {
            let mut tab = Tab::new(1, url.into());
            commit_document(&mut tab, url, Navigation::Refresh);
            tab.capture_scroll(position(30., 40.));
            assert_eq!(tab.history[0].scroll, HistoryScroll::default());
            assert_eq!(tab.pending_scroll, None);
        }
        let mut tab = Tab::new(1, "a".into());
        tab.capture_scroll(position(30., 40.));
        assert!(tab.history.is_empty());
        assert_eq!(tab.pending_scroll, None);
    }
    #[test]
    fn history_commits_only_successful_navigation() {
        let mut tab = Tab::new(1, HOME.into());
        tab.commit("https://a.test/".into(), "A".into(), Navigation::Push);
        tab.commit("https://b.test/".into(), "B".into(), Navigation::Push);
        tab.commit(
            "https://a.test/".into(),
            "A".into(),
            Navigation::Traverse(1),
        );
        tab.commit("https://c.test/".into(), "C".into(), Navigation::Push);
        assert_eq!(
            history_urls(&tab),
            [HOME, "https://a.test/", "https://c.test/"]
        );
        assert_eq!(tab.history_index, 2);
    }
    #[test]
    fn queued_navigation_discards_obsolete_result() {
        let mut tab = Tab::new(1, HOME.into());
        let (sender, receiver) = mpsc::sync_channel(1);
        tab.pending = Some(Pending {
            receiver,
            committed_storage: None,
            navigation: Navigation::Push,
            started: Instant::now(),
            viewport: Default::default(),
        });
        tab.queued = Some(("https://latest.test".into(), Navigation::Push));
        sender.send(Err("old failure".into())).unwrap();
        assert!(!poll_tab(&mut tab));
        assert!(tab.error.is_none());
        assert_eq!(tab.url, HOME);
        assert!(tab.queued.is_some());
    }
    #[test]
    fn only_exact_committed_operations_can_preserve_queued_checkpoints() {
        for navigation in [
            Navigation::Reflow,
            Navigation::Event(1),
            Navigation::Input(1),
            Navigation::Change(1),
            Navigation::Tick(20),
            Navigation::Frame(20.),
        ] {
            assert!(accepts_committed_checkpoint(navigation, 7, 7, true));
            assert!(!accepts_committed_checkpoint(navigation, 6, 7, true));
            assert!(!accepts_committed_checkpoint(navigation, 7, 7, false));
        }
        // INITIAL results, including refresh/traversal, must not survive queued
        // navigation. This policy is independent of visual/generation changes.
        for navigation in [
            Navigation::Push,
            Navigation::Refresh,
            Navigation::Traverse(1),
        ] {
            assert!(!accepts_committed_checkpoint(navigation, 7, 7, true));
        }
    }
    #[test]
    fn tab_storage_survives_history_home_and_workspace_moves_not_recreation() {
        let mut app = app();
        let values = vec![("draft".into(), "native bounded value".into())];
        app.tabs[0]
            .session_storage
            .commit("https://a.test", &values)
            .unwrap();
        for (url, navigation) in [
            ("https://a.test/one", Navigation::Push),
            ("https://a.test/one", Navigation::Refresh),
            ("https://b.test/two", Navigation::Push),
            ("https://a.test/one", Navigation::Traverse(1)),
            ("https://b.test/two", Navigation::Traverse(2)),
            (HOME, Navigation::Push),
        ] {
            app.tabs[0].commit(url.into(), "page".into(), navigation);
            assert_eq!(
                app.tabs[0]
                    .session_storage
                    .seed_for_url("https://a.test/other?q=1#f")
                    .1
                    .entries,
                values
            );
        }
        app.new_tab();
        assert!(
            app.tabs[1]
                .session_storage
                .seed_for_url("https://a.test")
                .1
                .entries
                .is_empty()
        );
        app.switch_workspace(2);
        assert!(
            app.tabs[0]
                .session_storage
                .seed_for_url("https://a.test")
                .1
                .entries
                .is_empty()
        );
        app.switch_workspace(1);
        assert_eq!(
            app.tabs[0]
                .session_storage
                .seed_for_url("https://a.test")
                .1
                .entries,
            values
        );
        let restored = Noir::from_profile(
            app.profile.clone(),
            None,
            None,
            None,
            None,
            AiPanel::disabled_for_test(),
        );
        assert!(
            restored.tabs[0]
                .session_storage
                .seed_for_url("https://a.test")
                .1
                .entries
                .is_empty()
        );
        let fresh = Tab::new(app.tabs[0].id, "https://a.test".into());
        assert!(
            fresh
                .session_storage
                .seed_for_url("https://a.test")
                .1
                .entries
                .is_empty()
        );
    }
}
