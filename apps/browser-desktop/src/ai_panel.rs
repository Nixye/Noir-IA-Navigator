//! Native assistant UI backed by real local inference or an explicitly selected provider.
use crate::theme::{ACCENT, AMBER, LINE, MUTED, PANEL, SURFACE, TEXT};
use ai_context::{BrowserSnapshot, BrowserTurn, DocumentScope, ScopedAction};
use ai_runtime::{Backend, Config, Message, Mode, ShutdownHandle};
use eframe::egui::{self, RichText};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

enum PanelResponse {
    Chat(String),
    Browser(BrowserTurn, DocumentScope),
}

pub struct AiPanel {
    config: Config,
    draft: Config,
    draft_key: String,
    status: Arc<Mutex<String>>,
    backend: Option<Arc<Mutex<Backend>>>,
    loading: Option<mpsc::Receiver<Result<Backend, String>>>,
    response: Option<mpsc::Receiver<Result<PanelResponse, String>>>,
    input: String,
    messages: Vec<Message>,
    error: Option<String>,
    page_context: Option<String>,
    use_context: bool,
    cancel_start: Arc<AtomicBool>,
    start_thread: Option<std::thread::JoinHandle<()>>,
    shutdown: Option<ShutdownHandle>,
    browser_mode: bool,
    browser_allowed: [bool; 6],
    full_access: bool,
    browser_snapshot: Option<BrowserSnapshot>,
    pending_action: Option<ScopedAction>,
    authorized_action: Option<ScopedAction>,
    authorized_once: bool,
    executing_action: bool,
    action_steps: usize,
    browser_goal: String,
    cancelled: bool,
    read_once_scope: Option<DocumentScope>,
}

impl AiPanel {
    #[cfg(test)]
    pub fn disabled_for_test() -> Self {
        let config = Config {
            enabled: false,
            ..Config::default()
        };
        Self {
            config: config.clone(),
            draft: config,
            draft_key: String::new(),
            status: Arc::new(Mutex::new("IA desativada".into())),
            backend: None,
            loading: None,
            response: None,
            input: String::new(),
            messages: vec![],
            error: None,
            page_context: None,
            use_context: false,
            cancel_start: Arc::new(AtomicBool::new(false)),
            start_thread: None,
            shutdown: None,
            browser_mode: false,
            browser_allowed: [true, false, false, false, false, false],
            full_access: false,
            browser_snapshot: None,
            pending_action: None,
            authorized_action: None,
            authorized_once: false,
            executing_action: false,
            action_steps: 0,
            browser_goal: String::new(),
            cancelled: false,
            read_once_scope: None,
        }
    }
    pub fn new(ctx: &egui::Context) -> Self {
        let (config, error) = match ai_runtime::load_config() {
            Ok(config) => (config, None),
            Err(e) => (
                Config {
                    enabled: false,
                    ..Config::default()
                },
                Some(e),
            ),
        };
        let mut panel = Self {
            draft: config.clone(),
            config,
            draft_key: String::new(),
            status: Arc::new(Mutex::new("Iniciando IA local".into())),
            backend: None,
            loading: None,
            response: None,
            input: String::new(),
            messages: vec![],
            error,
            page_context: None,
            use_context: false,
            cancel_start: Arc::new(AtomicBool::new(false)),
            start_thread: None,
            shutdown: None,
            browser_mode: false,
            browser_allowed: [true, false, false, false, false, false],
            full_access: false,
            browser_snapshot: None,
            pending_action: None,
            authorized_action: None,
            authorized_once: false,
            executing_action: false,
            action_steps: 0,
            browser_goal: String::new(),
            cancelled: false,
            read_once_scope: None,
        };
        let config_error = panel.error.clone();
        panel.start(ctx);
        if let Some(error) = config_error {
            panel.error = Some(error);
        }
        panel
    }

    pub fn status_label(&self) -> String {
        self.status
            .lock()
            .map_or_else(|_| "IA indisponível".into(), |s| s.clone())
    }
    pub fn is_ready(&self) -> bool {
        self.backend.is_some() && self.loading.is_none()
    }
    pub fn browser_busy(&self) -> bool {
        self.browser_mode && self.busy()
    }
    pub fn has_full_access(&self) -> bool {
        self.browser_mode && self.full_access
    }
    pub fn browser_snapshot_scope(&self) -> Option<DocumentScope> {
        self.browser_snapshot
            .as_ref()
            .map(|snapshot| snapshot.scope)
    }
    fn allowed_tools(&self) -> [bool; 6] {
        if self.full_access {
            [true; 6]
        } else {
            self.browser_allowed
        }
    }
    fn action_limit(&self) -> usize {
        if self.full_access { 32 } else { 8 }
    }
    fn interrupt_sequence(&mut self) {
        let was_busy = self.busy();
        self.response = None;
        self.pending_action = None;
        self.authorized_action = None;
        self.authorized_once = false;
        self.read_once_scope = None;
        self.cancelled = true;
        if was_busy {
            self.error = Some("Sequência interrompida. Uma ação já iniciada pode terminar; nenhuma nova ação será enviada.".into());
        }
    }
    /// Call only after the browser UI authorized this document. Replacing context
    /// revokes the attachment toggle; document text is never attached by default.
    pub fn set_page_context(&mut self, context: String) {
        let context = context.chars().take(6000).collect::<String>();
        if self.page_context.as_ref() != Some(&context) {
            self.page_context = Some(context);
            self.use_context = false;
        }
    }
    pub fn clear_page_context(&mut self) {
        self.page_context = None;
        self.use_context = false;
        self.browser_snapshot = None;
        self.pending_action = None;
        self.authorized_action = None;
        self.authorized_once = false;
        self.read_once_scope = None;
    }

    /// The shell replaces observations after every committed page operation.
    pub fn set_browser_snapshot(&mut self, snapshot: BrowserSnapshot) {
        if self
            .read_once_scope
            .is_some_and(|scope| scope != snapshot.scope)
        {
            self.read_once_scope = None;
        }
        if self
            .pending_action
            .as_ref()
            .is_some_and(|a| a.scope != snapshot.scope)
        {
            self.pending_action = None;
            self.error = Some("A página mudou. A ação proposta foi descartada.".into());
        }
        self.browser_snapshot = Some(snapshot.bounded());
    }

    pub fn take_browser_action(&mut self) -> Option<ScopedAction> {
        let action = self.authorized_action.take()?;
        if !self.browser_mode
            || (!self.authorized_once && !self.allowed_tools()[action.action.tool_index()])
            || !self
                .browser_snapshot
                .as_ref()
                .is_some_and(|s| s.scope == action.scope)
        {
            self.error = Some("Ação descartada: a página ou a permissão mudou.".into());
            return None;
        }
        self.executing_action = true;
        if self.authorized_once && matches!(action.action, ai_context::BrowserAction::ReadPage) {
            self.read_once_scope = Some(action.scope);
        }
        self.authorized_once = false;
        Some(action)
    }

    /// Invoke after the operation has actually committed, with a fresh snapshot.
    pub fn complete_browser_action(&mut self, result: Result<String, String>, ctx: &egui::Context) {
        self.executing_action = false;
        if self.cancelled {
            return;
        }
        match result {
            Ok(summary) => {
                self.messages.push(Message::user(format!(
                    "Resultado da ferramenta do navegador: {}",
                    summary.chars().take(1000).collect::<String>()
                )));
                let limit = self.action_limit();
                if self.browser_mode && self.action_steps < limit {
                    self.generate(ctx);
                } else if self.action_steps >= limit {
                    self.error = Some(format!(
                        "Limite de {limit} ações por pedido. Envie outra mensagem para continuar."
                    ));
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        self.cancel_start.store(true, Ordering::Release);
        if let Some(handle) = self.shutdown.take() {
            handle.shutdown();
        }
        if let Some(thread) = self.start_thread.take() {
            let _ = thread.join();
        }
        self.cancel_start = Arc::new(AtomicBool::new(false));
        self.backend = None;
        self.error = None;
        self.messages.clear();
        self.browser_mode = false;
        self.full_access = false;
        self.browser_allowed = [true, false, false, false, false, false];
        self.response = None;
        self.authorized_once = false;
        self.read_once_scope = None;
        self.cancelled = true;
        self.use_context = false;
        self.pending_action = None;
        self.authorized_action = None;
        self.executing_action = false;
        if !self.config.enabled {
            if let Ok(mut status) = self.status.lock() {
                *status = "IA desativada".into();
            }
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        self.loading = Some(receiver);
        let config = self.config.clone();
        let status = self.status.clone();
        if let Ok(mut value) = status.lock() {
            *value = if matches!(config.mode, Mode::Bundled | Mode::CustomGguf) {
                "Carregando modelo local"
            } else {
                "Preparando assistente"
            }
            .into();
        }
        let ctx = ctx.clone();
        let cancelled = self.cancel_start.clone();
        self.start_thread = Some(std::thread::spawn(move || {
            let result = Backend::start_cancellable(&config, &cancelled);
            if let Ok(mut value) = status.lock() {
                *value = match &result {
                    Ok(backend) => if backend.is_local() {
                        "IA local pronta"
                    } else {
                        "Provedor configurado"
                    }
                    .into(),
                    Err(_) => "IA requer atenção".into(),
                };
            }
            let _ = sender.send(result);
            ctx.request_repaint();
        }));
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(result) = self.loading.as_ref().and_then(|r| match r.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Inicialização da IA interrompida".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }) {
            self.loading = None;
            match result {
                Ok(backend) => {
                    self.shutdown = backend.shutdown_handle();
                    self.backend = Some(Arc::new(Mutex::new(backend)));
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(result) = self.response.as_ref().and_then(|r| match r.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Geração interrompida".into())),
            Err(mpsc::TryRecvError::Empty) => None,
        }) {
            self.response = None;
            match result {
                Ok(PanelResponse::Chat(text)) => self.messages.push(Message::assistant(text)),
                Ok(PanelResponse::Browser(turn, scope)) => {
                    if !self.browser_mode
                        || !self
                            .browser_snapshot
                            .as_ref()
                            .is_some_and(|s| s.scope == scope)
                    {
                        self.error = Some(
                            "A página mudou durante a resposta; nenhuma ação foi executada.".into(),
                        );
                    } else {
                        if !turn.message.trim().is_empty() {
                            self.messages.push(Message::assistant(turn.message));
                        }
                        if let Some(action) = turn.action {
                            self.action_steps += 1;
                            let allowed = self.allowed_tools()[action.tool_index()];
                            let always_review = !self.full_access
                                && matches!(action, ai_context::BrowserAction::Click { .. });
                            let scoped = ScopedAction { scope, action };
                            if allowed && !always_review {
                                self.authorized_action = Some(scoped);
                                self.authorized_once = false;
                            } else {
                                self.pending_action = Some(scoped);
                            }
                        }
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
        if self.loading.is_some() || self.response.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn send(&mut self, ctx: &egui::Context) {
        let text = self.input.trim().chars().take(2400).collect::<String>();
        if text.is_empty() || self.busy() || self.loading.is_some() {
            return;
        }
        self.error = None;
        self.input.clear();
        self.messages.push(Message::user(text));
        self.browser_goal = self
            .messages
            .last()
            .map_or_else(String::new, |m| m.content.clone());
        self.action_steps = 0;
        self.cancelled = false;
        self.generate(ctx);
    }

    fn busy(&self) -> bool {
        self.response.is_some()
            || self.pending_action.is_some()
            || self.authorized_action.is_some()
            || self.executing_action
    }

    fn generate(&mut self, ctx: &egui::Context) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let snapshot = if self.browser_mode {
            let Some(snapshot) = self.browser_snapshot.clone() else {
                self.error = Some(
                    "Aguarde a página terminar de carregar para usar a navegação assistida.".into(),
                );
                return;
            };
            Some(snapshot)
        } else {
            None
        };
        let mut allowed = self.allowed_tools();
        let full_access = self.full_access;
        if let Some(scope) = self.read_once_scope.take()
            && snapshot.as_ref().is_some_and(|s| s.scope == scope)
        {
            allowed[0] = true;
        }
        if self.messages.len() > 24 {
            self.messages.drain(..self.messages.len() - 24);
        }
        let mut budget = 3600;
        let mut messages = Vec::new();
        for message in self.messages.iter().rev().take(8) {
            let len = message.content.len();
            if len > budget && !messages.is_empty() {
                break;
            }
            budget = budget.saturating_sub(len);
            messages.push(message.clone());
        }
        messages.reverse();
        if self.browser_mode
            && !messages
                .iter()
                .any(|m| m.role == "user" && m.content == self.browser_goal)
        {
            messages.insert(0, Message::user(self.browser_goal.clone()));
        }
        let context = if self.use_context {
            self.page_context.clone()
        } else {
            None
        };
        self.use_context = false;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.response = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = backend
                .lock()
                .map_err(|_| "Assistente indisponível".to_string())
                .and_then(|backend| {
                    if let Some(snapshot) = snapshot {
                        backend
                            .browser_turn(&messages, &snapshot, allowed, full_access)
                            .map(|turn| PanelResponse::Browser(turn, snapshot.scope))
                    } else {
                        backend
                            .chat(&messages, context.as_deref())
                            .map(PanelResponse::Chat)
                    }
                });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        ui.horizontal(|ui| {
            if self.loading.is_some() {
                ui.spinner();
            }
            let color = if self.is_ready() { ACCENT } else { MUTED };
            ui.label(RichText::new(self.status_label()).size(11.).color(color));
        });
        ui.add_space(14.);
        if self.config.mode == Mode::Provider {
            ui.label(
                RichText::new("As mensagens são enviadas ao provedor escolhido.")
                    .size(11.)
                    .color(MUTED),
            );
        } else if self.config.mode == Mode::LocalServer {
            ui.label(
                RichText::new("As mensagens seguem para o servidor local configurado.")
                    .size(11.)
                    .color(MUTED),
            );
        } else {
            ui.label(
                RichText::new("Uma conversa privada, processada no seu dispositivo.")
                    .size(11.)
                    .color(MUTED),
            );
        }
        ui.add_space(12.);
        let browser_changed = ui
            .checkbox(&mut self.browser_mode, "Assistente de navegação")
            .changed();
        if browser_changed {
            self.interrupt_sequence();
            if !self.browser_mode {
                self.full_access = false;
            }
        }
        if self.browser_mode {
            ui.label(RichText::new("A observação da página e das abas é enviada ao modelo escolhido. Senhas e campos ocultos são excluídos.").size(11.).color(MUTED));
            if ui
                .checkbox(&mut self.full_access, "Permissão total")
                .changed()
            {
                self.interrupt_sequence();
            }
            if self.full_access {
                egui::Frame::new().fill(ACCENT.gamma_multiply(0.07)).corner_radius(10).inner_margin(11).show(ui, |ui| {
                    ui.label(RichText::new("Permissão total ativa").strong().color(ACCENT));
                    ui.label(RichText::new("Todas as ferramentas do navegador sem confirmações por ação, incluindo cliques e formulários suportados. Até 32 ações por pedido.").size(11.).color(MUTED));
                    if ui.button("Revogar permissão total").clicked() {
                        self.full_access = false;
                        self.interrupt_sequence();
                    }
                });
            } else {
                let mut permissions_changed = false;
                ui.collapsing("Permissões nesta conversa", |ui| {
                    for (index, label) in [
                        "Ler a página",
                        "Navegar",
                        "Abrir e alternar abas",
                        "Selecionar elementos",
                        "Digitar em campos",
                        "Solicitar cliques",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        permissions_changed |= ui
                            .checkbox(&mut self.browser_allowed[index], label)
                            .changed();
                    }
                    ui.label(
                        RichText::new(
                            "Neste modo, cliques exigem revisão. Até 8 ações por pedido.",
                        )
                        .size(10.)
                        .color(MUTED),
                    );
                });
                if permissions_changed {
                    self.interrupt_sequence();
                }
            }
            ui.add_space(8.);
        }
        egui::ScrollArea::vertical()
            .id_salt("ai_conversation")
            .max_height((ui.available_height() - 188.).max(120.))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.messages.is_empty() {
                    ui.add_space(22.);
                    ui.label(RichText::new("Como posso ajudar?").size(19.).color(TEXT));
                    ui.label(
                        RichText::new("Pergunte, desenvolva uma ideia ou esclareça uma dúvida.")
                            .size(13.)
                            .color(MUTED),
                    );
                    ui.add_space(18.);
                    for suggestion in [
                        "Ajude-me a organizar meu trabalho",
                        "Explique um conceito de forma simples",
                    ] {
                        if ui
                            .add_enabled(
                                self.is_ready() && !self.busy(),
                                egui::Button::new(suggestion).wrap(),
                            )
                            .clicked()
                        {
                            self.input = suggestion.into();
                            self.send(&ctx);
                        }
                    }
                }
                for message in &self.messages {
                    let user = message.role == "user";
                    let fill = if user {
                        SURFACE.gamma_multiply(0.84)
                    } else {
                        PANEL.gamma_multiply(0.72)
                    };
                    egui::Frame::new()
                        .fill(fill)
                        .stroke(egui::Stroke::new(1., LINE.gamma_multiply(0.36)))
                        .corner_radius(10)
                        .inner_margin(13)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(if user { "Você" } else { "Noir" })
                                    .size(10.)
                                    .strong()
                                    .color(if user { MUTED } else { ACCENT }),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&message.content).size(13.).color(TEXT),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        });
                    ui.add_space(10.);
                }
                if self.response.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Pensando…").size(12.).color(MUTED));
                    });
                }
                if self.executing_action {
                    ui.horizontal(|ui| { ui.spinner(); ui.label("Executando ação no navegador…"); });
                }
                if let Some(action) = self.pending_action.clone() {
                    egui::Frame::new().fill(SURFACE.gamma_multiply(0.88)).stroke(egui::Stroke::new(1., ACCENT.gamma_multiply(0.3))).corner_radius(12).inner_margin(14).show(ui, |ui| {
                        ui.label(RichText::new("Revisar ação").strong().color(ACCENT));
                        ui.label(action.action.description());
                        if let ai_context::BrowserAction::Click { element }
                            | ai_context::BrowserAction::Select { element }
                            | ai_context::BrowserAction::Type { element, .. } = &action.action
                            && let Some(target) = self.browser_snapshot.as_ref().and_then(|s| s.elements.iter().find(|e| &e.id == element))
                        {
                            ui.label(RichText::new(format!("Elemento da página: “{}”", target.label)).strong());
                            if !target.value.is_empty() {
                                let label = if target.kind == "link" { "Destino informado pela página" } else { "Valor informado pela página" };
                                ui.label(RichText::new(format!("{label}: “{}”", target.value)).size(11.).color(MUTED));
                            }
                        }
                        if matches!(action.action, ai_context::BrowserAction::Click { .. }) {
                            ui.label(RichText::new("O clique pode enviar dados ou confirmar uma operação na página.").size(11.).color(MUTED));
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Executar uma vez").clicked() {
                                self.pending_action = None;
                                self.authorized_action = Some(action.clone());
                                self.authorized_once = true;
                            }
                            if ui.button("Recusar").clicked() {
                                self.pending_action = None;
                                self.messages.push(Message::assistant("Ação recusada. Nenhuma ação adicional será executada para este pedido."));
                            }
                        });
                    });
                }
                if let Some(error) = &self.error {
                    ui.label(
                        RichText::new(error)
                            .size(12.)
                            .color(AMBER),
                    );
                }
            });
        ui.add_space(12.);
        if self.page_context.is_some() {
            ui.checkbox(&mut self.use_context, "Anexar página nesta mensagem");
        }
        let edit = ui.add(
            egui::TextEdit::multiline(&mut self.input)
                .hint_text("Escreva uma mensagem…")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        let submit =
            edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl);
        ui.horizontal(|ui| {
            if self.busy() && ui.button("Interromper").clicked() {
                self.interrupt_sequence();
            }
            if ui
                .add_enabled(
                    self.is_ready() && !self.busy() && !self.input.trim().is_empty(),
                    egui::Button::new("Enviar"),
                )
                .clicked()
                || submit
            {
                self.send(&ctx);
            }
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Nova conversa"))
                .clicked()
            {
                self.messages.clear();
                self.browser_goal.clear();
                self.read_once_scope = None;
                self.error = None;
                self.browser_allowed = [true, false, false, false, false, false];
                self.full_access = false;
            }
        });
        ui.add_space(7.);
        ui.label(
            RichText::new("O modelo pode errar. Confirme informações importantes.")
                .size(10.)
                .color(MUTED),
        );
    }

    /// Call every frame so background loading completes even with the panel closed.
    pub fn show_settings(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
    }

    pub fn settings_page(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.label(
            RichText::new("Seu assistente, do seu jeito.")
                .size(20.)
                .color(TEXT),
        );
        ui.add_space(12.);
        ui.checkbox(&mut self.draft.enabled, "Ativar inteligência artificial");
        ui.add_space(8.);
        egui::ComboBox::from_label("Executar com")
            .selected_text(self.draft.mode.label())
            .show_ui(ui, |ui| {
                for mode in [
                    Mode::Bundled,
                    Mode::CustomGguf,
                    Mode::LocalServer,
                    Mode::Provider,
                ] {
                    ui.selectable_value(&mut self.draft.mode, mode, mode.label());
                }
            });
        match self.draft.mode {
            Mode::Bundled => {
                ui.add_space(10.);
                ui.label("Qwen2.5 · 1.5B · Q4_K_M");
                ui.label(RichText::new("Modelo incluído no aplicativo. Inferência em CPU, sem enviar suas mensagens à internet.").size(12.).color(MUTED));
            }
            Mode::CustomGguf => {
                let mut path = self.draft.custom_model.to_string_lossy().into_owned();
                ui.label("Arquivo do modelo GGUF");
                if ui.text_edit_singleline(&mut path).changed() {
                    self.draft.custom_model = path.into();
                }
                if ui.button("Escolher modelo…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Modelo GGUF", &["gguf"])
                        .pick_file()
                {
                    self.draft.custom_model = path;
                }
                ui.label(
                    RichText::new("O runtime incluído carregará o modelo selecionado localmente.")
                        .size(12.)
                        .color(MUTED),
                );
            }
            Mode::LocalServer | Mode::Provider => {
                ui.label("Endereço base compatível com chat/completions");
                ui.text_edit_singleline(&mut self.draft.endpoint);
                ui.label("Identificador do modelo");
                ui.text_edit_singleline(&mut self.draft.model);
                ui.label(if self.draft.mode == Mode::Provider {
                    "Chave de API"
                } else {
                    "Chave de API (opcional)"
                });
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft_key)
                        .password(true)
                        .hint_text("Deixe vazio para usar a chave já guardada"),
                );
                if self.draft.mode == Mode::Provider {
                    ui.label(RichText::new("A chave fica no cofre de credenciais do sistema. Mensagens serão enviadas ao provedor configurado.").size(11.).color(MUTED));
                } else {
                    ui.label(
                        RichText::new(
                            "Aceita servidores locais como Ollama ou llama.cpp em localhost.",
                        )
                        .size(11.)
                        .color(MUTED),
                    );
                }
            }
        }
        ui.add_space(15.);
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).size(12.).color(AMBER));
        }
        if ui
            .add_enabled(
                self.loading.is_none() && !self.busy(),
                egui::Button::new("Salvar e ativar"),
            )
            .clicked()
        {
            let result = (|| {
                ai_runtime::validate_config(&self.draft)?;
                if matches!(self.draft.mode, Mode::Provider | Mode::LocalServer)
                    && !self.draft_key.is_empty()
                {
                    ai_runtime::store_api_key(&self.draft.endpoint, &self.draft_key)?;
                }
                ai_runtime::save_config(&self.draft)
            })();
            match result {
                Ok(()) => {
                    self.config = self.draft.clone();
                    self.draft_key.clear();
                    self.start(&ctx);
                }
                Err(e) => self.error = Some(e),
            }
        }
    }
}

impl Drop for AiPanel {
    fn drop(&mut self) {
        self.cancel_start.store(true, Ordering::Release);
        if let Some(handle) = self.shutdown.take() {
            handle.shutdown();
        }
        if let Some(thread) = self.start_thread.take() {
            let _ = thread.join();
        }
        // A completed startup may still be in the channel if the panel was never
        // shown. Dropping the receiver drops that owned backend and reaps it.
        self.loading.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_context::{BrowserAction, BrowserElement};
    fn click_proposal(full: bool) -> AiPanel {
        let mut panel = AiPanel::disabled_for_test();
        panel.browser_mode = true;
        panel.full_access = full;
        panel.browser_allowed = if full { [false; 6] } else { [true; 6] };
        let scope = DocumentScope {
            tab: 1,
            generation: 7,
        };
        panel.set_browser_snapshot(BrowserSnapshot {
            scope,
            title: "Fixture".into(),
            url: "https://example.com/".into(),
            text: "Save".into(),
            selection: String::new(),
            tabs: vec![],
            elements: vec![BrowserElement {
                id: "control:42".into(),
                kind: "button".into(),
                label: "Save".into(),
                value: String::new(),
            }],
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        panel.response = Some(receiver);
        sender
            .send(Ok(PanelResponse::Browser(
                BrowserTurn {
                    message: String::new(),
                    action: Some(BrowserAction::Click {
                        element: "control:42".into(),
                    }),
                },
                scope,
            )))
            .unwrap();
        panel.poll(&egui::Context::default());
        panel
    }
    #[test]
    fn full_access_dispatches_click_without_review_but_granular_mode_does_not() {
        let mut full = click_proposal(true);
        assert!(full.pending_action.is_none());
        assert!(
            matches!(full.take_browser_action().map(|a|a.action),Some(BrowserAction::Click{element}) if element=="control:42")
        );
        let mut granular = click_proposal(false);
        assert!(granular.pending_action.is_some());
        assert!(granular.take_browser_action().is_none());
    }
    #[test]
    fn revocation_and_changed_document_prevent_dispatch_of_an_old_proposal() {
        let mut revoked = click_proposal(true);
        revoked.full_access = false;
        revoked.interrupt_sequence();
        assert!(revoked.take_browser_action().is_none());
        let mut changed = click_proposal(true);
        let mut snapshot = changed.browser_snapshot.clone().unwrap();
        snapshot.scope.generation += 1;
        changed.set_browser_snapshot(snapshot);
        assert!(changed.take_browser_action().is_none());
        assert!(!changed.executing_action);
    }
}
