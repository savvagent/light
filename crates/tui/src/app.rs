//! The ratatui application: auth forms plus the connected WebSocket screen.

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use light_factory_engine::Engine;
use light_factory_protocol::auth::AuthResponse;
use light_factory_protocol::session::{Command, Event as EngineEvent, EventKind, SessionId};
use light_factory_protocol::wire::{ClientMessage, ServerMessage};
use light_factory_providers::{CompleteRequest, Provider};
use light_factory_tui::credentials::CredentialStore;
use light_factory_tui::engine_view::{describe_event, pending_prompt};
use light_factory_tui::i18n::{self, Locale};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::api::{Api, ApiError};
use crate::browser;
use crate::config::Config;
use crate::modal::{
    ConnectStep, Modal, ModalApply, ModalHost, ModalTransition, ModelsStep, ProviderRow,
    centered_rect, draw_popup, fetch_model_list, help_lines,
};
use crate::provider::ProviderInfo;
use crate::session::Session;
use crate::settings::{Settings, SettingsHandle};
use crate::ws;

/// Events flowing into the single UI loop.
pub enum UiEvent {
    Key(KeyEvent),
    Server(ServerMessage),
    Device {
        nonce: u64,
        result: Result<AuthResponse, ApiError>,
    },
    Completion(Result<String, String>),
    Engine(EngineEvent),
    EngineDropped(u64),
    /// A provider's model list, fetched for the `/connect` modal. `nonce` discards a result that
    /// outlived the modal that asked for it.
    ConnectModels {
        nonce: u64,
        provider: String,
        result: Result<Vec<String>, String>,
    },
    /// A provider's model list, fetched for the `/models` modal. Kept separate from
    /// `ConnectModels` because the two modals render a failure differently.
    ModelsFetched {
        nonce: u64,
        provider: String,
        result: Result<Vec<String>, String>,
    },
}

/// Which field currently owns keyboard input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Email,
    Name,
    Code,
}

/// The screen currently shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    SignIn,
    Register,
    RegisterCode,
    Device,
    Connected,
    Engine,
    Key,
    Help,
}

const LOG_CAPACITY: usize = 200;
const KEEPALIVE_SECONDS: u64 = 30;

pub struct App {
    config: Config,
    api: Api,
    events: mpsc::UnboundedSender<UiEvent>,
    mode: Mode,
    focus: Focus,
    email: String,
    name: String,
    code: String,
    command: String,
    command_mode: bool,
    device_nonce: u64,
    device_user_code: Option<String>,
    device_verification_uri: Option<String>,
    setup_token: Option<String>,
    secret: Option<String>,
    otpauth_url: Option<String>,
    session: Option<Session>,
    ws_tx: Option<mpsc::UnboundedSender<ClientMessage>>,
    provider: Arc<dyn Provider>,
    provider_info: ProviderInfo,
    store: Arc<dyn CredentialStore>,
    settings: Settings,
    settings_path: PathBuf,
    key_target: Option<String>,
    key_input: String,
    key_return: Mode,
    help_return: Mode,
    modal: ModalHost,
    engine: Option<Engine>,
    engine_session: Option<SessionId>,
    engine_forward_task: Option<tokio::task::JoinHandle<()>>,
    engine_log: Vec<String>,
    engine_prompt: String,
    pending: Option<(EventKind, String)>,
    error: Option<String>,
    status: String,
    log: VecDeque<String>,
    nonce: u64,
    pongs: u64,
}

impl App {
    fn new(
        config: Config,
        provider: Arc<dyn Provider>,
        provider_info: ProviderInfo,
        store: Arc<dyn CredentialStore>,
        settings: SettingsHandle,
        prefilled_email: Option<String>,
        events: mpsc::UnboundedSender<UiEvent>,
    ) -> Self {
        let api = Api::new(&config.http_base);
        let status = i18n::t(config.lang, "status.not_signed_in").to_string();
        Self {
            config,
            api,
            events,
            mode: Mode::SignIn,
            focus: Focus::Email,
            email: prefilled_email.unwrap_or_default(),
            name: String::new(),
            code: String::new(),
            command: String::new(),
            command_mode: false,
            device_nonce: 0,
            device_user_code: None,
            device_verification_uri: None,
            setup_token: None,
            secret: None,
            otpauth_url: None,
            session: None,
            ws_tx: None,
            provider,
            provider_info,
            store,
            settings: settings.settings,
            settings_path: settings.path,
            key_target: None,
            key_input: String::new(),
            key_return: Mode::SignIn,
            help_return: Mode::SignIn,
            modal: ModalHost::default(),
            engine: None,
            engine_session: None,
            engine_forward_task: None,
            engine_log: Vec::new(),
            engine_prompt: String::new(),
            pending: None,
            error: None,
            status,
            log: VecDeque::new(),
            nonce: 0,
            pongs: 0,
        }
    }

    fn t<'a>(&self, key: &'a str) -> &'a str {
        i18n::t(self.config.lang, key)
    }

    fn t_with(&self, key: &str, params: &[(&str, &str)]) -> String {
        i18n::t_with(self.config.lang, key, params)
    }

    fn error_text(&self, code: &str, message: &str) -> String {
        i18n::error_message(self.config.lang, code)
            .map(str::to_string)
            .unwrap_or_else(|| message.to_string())
    }

    async fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.mode == Mode::Help {
            return self.handle_help_key(key);
        }
        if self.modal.is_open() {
            return self.handle_modal_key(key);
        }
        if key.code == KeyCode::Char('p')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !self.command_mode
        {
            self.open_help();
            return false;
        }
        if self.command_mode {
            return self.handle_command_key(key).await;
        }
        if self.mode == Mode::Engine {
            return self.handle_engine_key(key).await;
        }
        if self.mode == Mode::Key {
            return self.handle_key_entry(key);
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => match self.mode {
                Mode::SignIn => return true,
                Mode::Register => {
                    self.mode = Mode::SignIn;
                    self.code.clear();
                    self.error = None;
                }
                Mode::RegisterCode => {
                    self.mode = Mode::Register;
                    self.code.clear();
                    self.error = None;
                }
                Mode::Device => {
                    self.device_nonce += 1;
                    self.device_user_code = None;
                    self.device_verification_uri = None;
                    self.mode = Mode::SignIn;
                    self.status = self.t("status.device_cancelled").to_string();
                    self.error = None;
                }
                Mode::Connected => {}
                Mode::Engine => {}
                Mode::Key => {}
                Mode::Help => {}
            },
            KeyCode::Char('/')
                if matches!(
                    self.mode,
                    Mode::SignIn | Mode::Register | Mode::RegisterCode | Mode::Connected
                ) =>
            {
                self.command_mode = true;
                self.command = "/".to_string();
                self.error = None;
            }
            KeyCode::Char('q') => {
                if self.mode == Mode::Connected {
                    return true;
                }
                self.type_char('q');
            }
            KeyCode::Char('p') if self.mode == Mode::Connected => self.ping(),
            KeyCode::Char('o') if self.mode == Mode::Connected => self.sign_out().await,
            KeyCode::Char('e') if self.mode == Mode::Connected => {
                if let Err(e) = self.enter_engine() {
                    self.error = Some(e.to_string());
                }
            }
            KeyCode::Char(c) => self.type_char(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Tab | KeyCode::Up | KeyCode::Down => self.cycle_focus(),
            KeyCode::Enter => self.submit().await,
            _ => {}
        }
        false
    }

    async fn handle_engine_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => self.leave_engine(),
            KeyCode::Enter => self.engine_send_prompt(),
            KeyCode::Backspace => {
                self.engine_prompt.pop();
            }
            KeyCode::Char(c) => match engine_approval_key(c, self.pending.is_some()) {
                Some(approved) => self.engine_answer(approved),
                None => self.engine_prompt.push(c),
            },
            _ => {}
        }
        false
    }

    fn open_help(&mut self) {
        self.help_return = self.mode;
        self.mode = Mode::Help;
    }

    fn close_help(&mut self) {
        self.mode = self.help_return;
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.close_help()
            }
            KeyCode::Esc => self.close_help(),
            _ => {}
        }
        false
    }

    fn enter_engine(&mut self) -> anyhow::Result<()> {
        let provider = self.provider.clone();
        let info = self.provider_info.clone();

        let mut engine = Engine::new(provider);
        let session = engine.create_session(std::env::current_dir()?)?;

        let mut events = engine.handle(session).expect("just created").subscribe();
        let tx = self.events.clone();
        let forwarder = tokio::spawn(async move {
            loop {
                match engine_forward_step(events.recv().await) {
                    EngineForward::Event(event) => {
                        if tx.send(UiEvent::Engine(event)).is_err() {
                            break;
                        }
                    }
                    EngineForward::Dropped(n) => {
                        if tx.send(UiEvent::EngineDropped(n)).is_err() {
                            break;
                        }
                    }
                    EngineForward::Stop => break,
                }
            }
        });

        self.engine = Some(engine);
        self.engine_session = Some(session);
        self.engine_forward_task = Some(forwarder);
        self.engine_log.clear();
        for warning in info.warnings {
            self.engine_log.push(warning);
        }
        if let Some(reason) = &info.offline {
            self.engine_log
                .push(crate::provider::offline_notice(self.config.lang, reason));
        }
        self.engine_prompt.clear();
        self.pending = None;
        self.mode = Mode::Engine;
        self.status = self.t("status.engine_started").to_string();
        Ok(())
    }

    fn leave_engine(&mut self) {
        if let Some(forwarder) = self.engine_forward_task.take() {
            forwarder.abort();
        }
        self.engine = None;
        self.engine_session = None;
        self.mode = Mode::Connected;
        self.engine_prompt.clear();
        self.pending = None;
        if let Some(s) = &self.session {
            self.status = self.t_with("status.connected_as", &[("email", &s.email)]);
        }
    }

    fn engine_answer(&mut self, approved: bool) {
        let (Some(engine), Some(session)) = (self.engine.as_mut(), self.engine_session) else {
            return;
        };
        let command = match self.pending.as_ref().map(|(kind, _)| kind) {
            Some(EventKind::PlanProposed { plan_id, .. }) => Some(Command::ApprovePlan {
                session,
                plan_id: *plan_id,
                approved,
            }),
            Some(EventKind::ApprovalRequest { request_id, .. }) => Some(Command::ApproveAction {
                session,
                request_id: *request_id,
                approved,
            }),
            _ => None,
        };
        if let Some(command) = command {
            let _ = engine.dispatch(command);
            self.pending = None;
        }
    }

    fn engine_send_prompt(&mut self) {
        let text = self.engine_prompt.trim().to_string();
        if text.is_empty() {
            return;
        }
        let (Some(engine), Some(session)) = (self.engine.as_mut(), self.engine_session) else {
            return;
        };
        let _ = engine.dispatch(Command::SendPrompt {
            session,
            text: text.clone(),
        });
        self.engine_log.push(format!("> {text}"));
        self.engine_prompt.clear();
    }

    fn handle_engine_event(&mut self, event: EngineEvent) {
        let line = describe_event(self.config.lang, &event.kind);
        self.engine_log.push(line);
        while self.engine_log.len() > LOG_CAPACITY {
            self.engine_log.remove(0);
        }
        if let Some(prompt) = pending_prompt(self.config.lang, &event.kind) {
            self.pending = Some((event.kind, prompt));
        }
    }

    fn handle_engine_dropped(&mut self, n: u64) {
        let count = n.to_string();
        let line = i18n::t_with(
            self.config.lang,
            "engine.dropped_events",
            &[("count", &count)],
        );
        self.engine_log.push(line);
        while self.engine_log.len() > LOG_CAPACITY {
            self.engine_log.remove(0);
        }
    }

    async fn handle_command_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => {
                self.command_mode = false;
                self.command.clear();
            }
            KeyCode::Enter => {
                let command = self.command.clone();
                self.command_mode = false;
                self.command.clear();
                self.run_command(&command).await;
            }
            KeyCode::Backspace => {
                self.command.pop();
            }
            KeyCode::Char(c) => self.command.push(c),
            _ => {}
        }
        false
    }

    fn handle_key_entry(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Esc => self.cancel_key_entry(),
            KeyCode::Enter => self.submit_key_entry(),
            KeyCode::Backspace => {
                self.key_input.pop();
            }
            KeyCode::Char(c) => self.key_input.push(c),
            _ => {}
        }
        false
    }

    fn cancel_key_entry(&mut self) {
        self.key_target = None;
        self.key_input.clear();
        self.mode = self.key_return;
    }

    fn submit_key_entry(&mut self) {
        let Some(provider) = self.key_target.clone() else {
            self.mode = self.key_return;
            return;
        };
        let key = self.key_input.trim().to_string();
        self.key_target = None;
        self.key_input.clear();
        self.mode = self.key_return;
        if key.is_empty() {
            self.status = self.t("status.key_empty").to_string();
            return;
        }
        match self.store.set(&provider, &key) {
            Ok(()) => {
                self.rebuild_provider();
                self.status = self.t_with("status.key_set", &[("provider", &provider)]);
            }
            Err(e) => {
                let error = e.to_string();
                self.error = Some(self.t_with(
                    "status.key_failed",
                    &[("provider", &provider), ("error", &error)],
                ));
            }
        }
    }

    /// Tear down any open modal and invalidate its in-flight fetch. Called when the session goes
    /// away, so a modal cannot float over the sign-in screen or swallow its keys.
    fn dismiss_modals(&mut self) {
        self.modal.close();
    }

    /// Persist the settings, surfacing a failure as a user-visible error rather than swallowing
    /// it. Returns whether the write landed, so callers can roll back the change they staged.
    fn persist_settings(&mut self) -> bool {
        match crate::settings::save_at(&self.settings_path, &self.settings) {
            Ok(()) => true,
            Err(e) => {
                // `{:#}` keeps anyhow's source chain; `to_string` would drop the actual cause.
                let err = format!("{e:#}");
                self.error = Some(self.t_with("status.settings_save_failed", &[("error", &err)]));
                false
            }
        }
    }

    /// Stage a model for a provider and persist it, rolling the in-memory map back if the write
    /// fails so a later unrelated save cannot silently resurrect it.
    fn persist_model(&mut self, provider: String, model: String) -> bool {
        let previous = self.settings.models.insert(provider.clone(), model.clone());
        if self.persist_settings() {
            self.rebuild_provider();
            self.status = self.t_with("status.model_set", &[("model", &model)]);
            return true;
        }
        match previous {
            Some(old) => self.settings.models.insert(provider, old),
            None => self.settings.models.remove(&provider),
        };
        false
    }

    fn rebuild_provider(&mut self) {
        let (provider, info) = crate::selection::rebuild(&self.settings, self.store.as_ref());
        self.provider = provider;
        self.provider_info = info;
    }

    async fn run_command(&mut self, command: &str) {
        self.error = None;
        let trimmed = command.trim();
        if let Some(prompt) = parse_ask_command(trimmed) {
            if self.mode == Mode::Connected {
                self.ask(prompt);
            } else {
                self.error = Some(self.t("status.ask_not_connected").to_string());
            }
            return;
        }
        if trimmed.starts_with("/ask") {
            self.error = Some(self.t("status.ask_empty").to_string());
            return;
        }
        if parse_connect_command(trimmed) {
            if self.mode == Mode::Connected {
                self.enter_connect();
            } else {
                self.error = Some(self.t("status.connect_not_connected").to_string());
            }
            return;
        }
        if parse_models_command(trimmed) {
            if self.mode == Mode::Connected {
                self.enter_models();
            } else {
                self.error = Some(self.t("status.models_not_connected").to_string());
            }
            return;
        }
        if let Some(model) = parse_model_command(trimmed) {
            match model {
                Some(id) => self.set_model(id),
                None => self.error = Some(self.t("status.model_empty").to_string()),
            }
            return;
        }
        if let Some(key_command) = parse_key_command(trimmed) {
            match key_command {
                KeyCommand::List => self.list_keys(),
                KeyCommand::Set(provider) => self.begin_key_entry(provider),
                KeyCommand::Clear(provider) => self.clear_key(&provider),
            }
            return;
        }
        match trimmed {
            "/auth/login" => self.start_device_login().await,
            "/auth/logout" => self.sign_out().await,
            "" => {}
            other if other.starts_with("/lang ") => {
                let arg = other["/lang ".len()..].trim();
                if let Some(locale) = Locale::parse(arg) {
                    let previous_lang = self.config.lang;
                    let previous_saved =
                        std::mem::replace(&mut self.settings.lang, locale.as_str().to_string());
                    self.config.lang = locale;
                    if self.persist_settings() {
                        self.status = self.t_with("status.lang_set", &[("lang", locale.as_str())]);
                    } else {
                        self.config.lang = previous_lang;
                        self.settings.lang = previous_saved;
                    }
                } else {
                    self.error = Some(self.t("status.lang_invalid").to_string());
                }
            }
            other => {
                self.error = Some(self.t_with("status.unknown_command", &[("command", other)]))
            }
        }
    }

    fn enter_connect(&mut self) {
        let rows = self.build_provider_rows();
        self.open_modal(
            Modal::Connect(ConnectStep::ProviderList { rows, selected: 0 }),
            None,
        );
    }

    /// Open `modal`, replacing any other, and start its model-list fetch if it is waiting on one.
    fn open_modal(&mut self, modal: Modal, key: Option<String>) {
        let target = modal.fetch_target().map(str::to_string);
        self.modal.open(modal);
        if let Some(provider) = target {
            self.begin_model_fetch(provider, key);
        }
    }

    fn build_provider_rows(&self) -> Vec<ProviderRow> {
        PROVIDER_NAMES
            .iter()
            .map(|id| {
                let connected = if *id == "ollama" {
                    std::env::var("LIGHT_OLLAMA").as_deref() == Ok("1")
                } else {
                    crate::selection::key_status(id, self.store.as_ref())
                        != crate::selection::KeyStatus::None
                };
                ProviderRow {
                    id: id.to_string(),
                    connected,
                }
            })
            .collect()
    }

    /// Commit whatever the open modal selected, then close it. Closing first is the `/models`
    /// ordering; `persist_model` never touches the modal, so the order is not observable.
    fn apply_and_close_modal(&mut self) {
        let apply = self.modal.current().and_then(Modal::apply_target);
        self.modal.close();
        match apply {
            Some(ModalApply::Provider { provider, model }) => {
                let previous_provider = self.settings.provider.replace(provider.clone());
                if !self.persist_model(provider, model) {
                    self.settings.provider = previous_provider;
                }
            }
            Some(ModalApply::Model { provider, model }) => {
                self.persist_model(provider, model);
            }
            None => {}
        }
    }

    /// Fetch a provider's model list off the UI loop for the open modal. The nonce claimed here is
    /// what makes a result that outlives its modal discardable; the open modal decides which event
    /// carries the answer back.
    fn begin_model_fetch(&mut self, provider: String, key: Option<String>) {
        let nonce = self.modal.next_fetch_nonce();
        let for_connect = matches!(self.modal.current(), Some(Modal::Connect(_)));
        let events = self.events.clone();
        let store = self.store.clone();
        let lang = self.config.lang;
        tokio::spawn(async move {
            let result = fetch_model_list(&provider, key, store.as_ref(), lang).await;
            let event = if for_connect {
                UiEvent::ConnectModels {
                    nonce,
                    provider,
                    result,
                }
            } else {
                UiEvent::ModelsFetched {
                    nonce,
                    provider,
                    result,
                }
            };
            let _ = events.send(event);
        });
    }

    /// Fill the connect modal's model list, or surface the fetch error inline in it. The nonce
    /// discards a result that outlived its modal; the provider and step shape discard one the open
    /// modal has already stepped past.
    fn handle_connect_models(
        &mut self,
        nonce: u64,
        provider: String,
        result: Result<Vec<String>, String>,
    ) {
        if nonce != self.modal.nonce() {
            return;
        }
        if !matches!(
            self.modal.current(),
            Some(Modal::Connect(ConnectStep::ModelList {
                provider: p,
                fetching: true,
                ..
            })) if *p == provider
        ) {
            return;
        }
        let err_msg = result
            .as_ref()
            .err()
            .map(|e| self.t_with("connect.fetch_error", &[("error", e)]));
        if let Some(Modal::Connect(ConnectStep::ModelList {
            models,
            selected,
            fetching,
            error,
            ..
        })) = self.modal.current_mut()
        {
            match result {
                Ok(list) => {
                    *models = list;
                    *selected = 0;
                    *fetching = false;
                    *error = None;
                }
                Err(_) => {
                    *fetching = false;
                    *error = err_msg;
                }
            }
        }
    }

    fn enter_models(&mut self) {
        let provider = self.provider_info.id.clone();
        if self.provider_info.offline.is_some() {
            self.open_modal(Modal::Models(ModelsStep::Offline), None);
            return;
        }
        self.open_modal(
            Modal::Models(ModelsStep::ModelList {
                provider,
                models: vec![],
                selected: 0,
                fetching: true,
            }),
            None,
        );
    }

    /// Fill the models modal's list, or fall back to manual entry when the fetch failed. Guarded
    /// exactly like `handle_connect_models`.
    fn handle_models_fetched(
        &mut self,
        nonce: u64,
        provider: String,
        result: Result<Vec<String>, String>,
    ) {
        if nonce != self.modal.nonce() {
            return;
        }
        if !matches!(
            self.modal.current(),
            Some(Modal::Models(ModelsStep::ModelList {
                provider: p,
                fetching: true,
                ..
            })) if *p == provider
        ) {
            return;
        }
        match result {
            Ok(list) => {
                let current = self.provider_info.model.clone();
                let selected = current
                    .as_ref()
                    .and_then(|m| list.iter().position(|x| x == m))
                    .unwrap_or(0);
                if let Some(Modal::Models(ModelsStep::ModelList {
                    models,
                    selected: sel,
                    fetching,
                    ..
                })) = self.modal.current_mut()
                {
                    *models = list;
                    *sel = selected;
                    *fetching = false;
                }
            }
            Err(e) => {
                let err_msg = self.t_with("connect.fetch_error", &[("error", &e)]);
                self.modal.replace_step(Modal::Models(ModelsStep::Manual {
                    provider,
                    input: String::new(),
                    error: Some(err_msg),
                }));
            }
        }
    }

    /// The single key seam for every modal: Ctrl-C quits, the modal's own pure transition decides
    /// the rest, and a step that begins waiting on a model list starts exactly one fetch.
    fn handle_modal_key(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        // Cloning releases the borrow on `self.modal` so the arms below can take `&mut self`.
        let Some(current) = self.modal.current().cloned() else {
            return false;
        };

        // Blank Enter on the connect key-entry step is a status message, not a transition.
        if matches!(&current, Modal::Connect(ConnectStep::KeyEntry { input, .. })
            if input.trim().is_empty())
            && key.code == KeyCode::Enter
        {
            self.status = self.t("status.key_empty").to_string();
            return false;
        }

        let transition = current.next(key);

        // Storing the typed key needs `self.store`, so it cannot live in the pure transition.
        let mut fetch_key = None;
        if let (
            Modal::Connect(ConnectStep::KeyEntry {
                provider, input, ..
            }),
            ModalTransition::Step(Modal::Connect(ConnectStep::ModelList {
                from_key: true, ..
            })),
        ) = (&current, &transition)
        {
            let key_value = input.trim().to_string();
            if let Err(e) = self.store.set(provider, &key_value) {
                let err = e.to_string();
                self.error = Some(self.t_with(
                    "status.key_failed",
                    &[("provider", provider.as_str()), ("error", &err)],
                ));
                return false;
            }
            fetch_key = Some(key_value);
        }

        let before = current.fetch_target().map(str::to_string);
        match transition {
            ModalTransition::Close => self.modal.close(),
            ModalTransition::Apply => self.apply_and_close_modal(),
            ModalTransition::Step(next) => {
                let after = next.fetch_target().map(str::to_string);
                self.modal.replace_step(next);
                if let Some(provider) = after.filter(|a| Some(a) != before.as_ref()) {
                    self.begin_model_fetch(provider, fetch_key);
                }
            }
        }
        false
    }

    fn set_model(&mut self, model: &str) {
        let active = self.provider_info.id.clone();
        if !is_valid_provider(&active) {
            self.error = Some(self.t("status.model_unsupported").to_string());
            return;
        }
        self.persist_model(active, model.to_string());
    }

    fn list_keys(&mut self) {
        let mut parts = Vec::new();
        for name in REMOTE_IDS {
            parts.push(format!("{name}: {}", self.key_status_label(name)));
        }
        self.push_log(self.t_with("key.list", &[("list", &parts.join(", "))]));
    }

    fn begin_key_entry(&mut self, provider: String) {
        if !takes_key(&provider) {
            self.error = Some(self.t_with("status.key_unsupported", &[("provider", &provider)]));
            return;
        }
        self.key_return = self.mode;
        self.key_target = Some(provider);
        self.key_input.clear();
        self.mode = Mode::Key;
    }

    fn clear_key(&mut self, provider: &str) {
        if !takes_key(provider) {
            self.error = Some(self.t_with("status.key_unsupported", &[("provider", provider)]));
            return;
        }
        match self.store.delete(provider) {
            Ok(()) => {
                self.rebuild_provider();
                self.status = self.t_with("status.key_cleared", &[("provider", provider)]);
            }
            Err(e) => {
                let error = e.to_string();
                self.error = Some(self.t_with(
                    "status.key_failed",
                    &[("provider", provider), ("error", &error)],
                ));
            }
        }
    }

    fn key_status_label(&self, provider: &str) -> String {
        match crate::selection::key_status(provider, self.store.as_ref()) {
            crate::selection::KeyStatus::Env => self.t("provider.key.env").to_string(),
            crate::selection::KeyStatus::Keyring => self.t("provider.key.keyring").to_string(),
            crate::selection::KeyStatus::None => self.t("provider.key.none").to_string(),
        }
    }

    /// Run an `/ask` completion off the UI loop so a slow provider never blocks input.
    fn ask(&mut self, prompt: &str) {
        let provider = self.provider.clone();
        let events = self.events.clone();
        let prompt = prompt.to_string();
        tokio::spawn(async move {
            let result = provider.complete(CompleteRequest { prompt }).await;
            let message = match result {
                Ok(resp) => Ok(resp.text),
                Err(e) => Err(e.to_string()),
            };
            let _ = events.send(UiEvent::Completion(message));
        });
    }

    /// Begin the browser-based device login and poll for approval.
    async fn start_device_login(&mut self) {
        self.status = self.t("status.requesting_device_code").to_string();
        match self.api.device().await {
            Ok(resp) => {
                self.device_nonce += 1;
                let nonce = self.device_nonce;
                self.device_user_code = Some(resp.user_code);
                self.device_verification_uri = Some(resp.verification_uri);
                self.mode = Mode::Device;
                self.status = self.t("status.waiting_approval").to_string();

                let _ = browser::open_browser(&resp.verification_uri_complete);

                let api = self.api.clone();
                let events = self.events.clone();
                let device_code = resp.device_code;
                let interval = std::time::Duration::from_secs(resp.interval.max(1));
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(interval).await;
                        match api.device_token(&device_code).await {
                            Ok(auth) => {
                                let _ = events.send(UiEvent::Device {
                                    nonce,
                                    result: Ok(auth),
                                });
                                return;
                            }
                            Err(e) if e.code == "authorization_pending" => continue,
                            Err(e) => {
                                let _ = events.send(UiEvent::Device {
                                    nonce,
                                    result: Err(e),
                                });
                                return;
                            }
                        }
                    }
                });
            }
            Err(e) => {
                self.error = Some(self.error_text(&e.code, &e.message));
                self.status = self.t("status.device_failed").to_string();
            }
        }
    }

    async fn handle_device_result(&mut self, nonce: u64, result: Result<AuthResponse, ApiError>) {
        if nonce != self.device_nonce {
            return;
        }
        self.device_user_code = None;
        self.device_verification_uri = None;
        match result {
            Ok(auth) => {
                let session = Session {
                    token: auth.token,
                    expires_at: auth.expires_at,
                    email: auth.user.email,
                    display_name: auth.user.display_name,
                };
                let _ = session.save();
                self.enter(session).await;
            }
            Err(e) => {
                self.mode = Mode::SignIn;
                self.error = Some(self.error_text(&e.code, &e.message));
                self.status = self.t("status.device_failed").to_string();
            }
        }
    }

    async fn submit(&mut self) {
        self.error = None;
        match self.mode {
            Mode::SignIn => match self.focus {
                Focus::Email => {
                    if !self.email.is_empty() {
                        self.focus = Focus::Code;
                    }
                }
                Focus::Code => {
                    if self.email.is_empty() {
                        self.error = Some(self.t("status.email_required").to_string());
                        return;
                    }
                    if self.code.is_empty() {
                        self.error = Some(self.t("status.code_required").to_string());
                        return;
                    }
                    self.status = self.t("status.signing_in").to_string();
                    match self.api.login(&self.email, &self.code).await {
                        Ok(auth) => {
                            let session = Session {
                                token: auth.token,
                                expires_at: auth.expires_at,
                                email: auth.user.email,
                                display_name: auth.user.display_name,
                            };
                            let _ = session.save();
                            self.enter(session).await;
                        }
                        Err(e) => self.error = Some(self.error_text(&e.code, &e.message)),
                    }
                }
                Focus::Name => {}
            },
            Mode::Register => match self.focus {
                Focus::Email => {
                    if self.email.is_empty() {
                        self.error = Some(self.t("status.email_required").to_string());
                        return;
                    }
                    self.focus = Focus::Name;
                }
                Focus::Name => {
                    if self.email.is_empty() {
                        self.error = Some(self.t("status.email_required").to_string());
                        return;
                    }
                    self.status = self.t("status.creating_account").to_string();
                    match self.api.register(&self.email, Some(&self.name)).await {
                        Ok(resp) => {
                            self.setup_token = Some(resp.setup_token);
                            self.secret = Some(resp.secret);
                            self.otpauth_url = Some(resp.otpauth_url);
                            self.mode = Mode::RegisterCode;
                            self.focus = Focus::Code;
                            self.code.clear();
                            self.status = self.t("status.scan_confirm").to_string();
                        }
                        Err(e) => self.error = Some(self.error_text(&e.code, &e.message)),
                    }
                }
                Focus::Code => {}
            },
            Mode::RegisterCode => {
                if self.code.is_empty() {
                    self.error = Some(self.t("status.code_required").to_string());
                    return;
                }
                let setup_token = self.setup_token.clone().unwrap_or_default();
                self.status = self.t("status.confirming").to_string();
                match self.api.register_confirm(&setup_token, &self.code).await {
                    Ok(auth) => {
                        let session = Session {
                            token: auth.token,
                            expires_at: auth.expires_at,
                            email: auth.user.email,
                            display_name: auth.user.display_name,
                        };
                        let _ = session.save();
                        self.enter(session).await;
                    }
                    Err(e) => self.error = Some(self.error_text(&e.code, &e.message)),
                }
            }
            Mode::Device => {}
            Mode::Connected => {}
            Mode::Engine => {}
            Mode::Key => {}
            Mode::Help => {}
        }
    }

    /// Move into the connected state and open the WebSocket.
    async fn enter(&mut self, session: Session) {
        self.session = Some(session.clone());
        self.mode = Mode::Connected;
        self.focus = Focus::Code;
        self.code.clear();
        self.name.clear();
        self.setup_token = None;
        self.secret = None;
        self.otpauth_url = None;
        self.error = None;
        self.status = self.t("status.connecting").to_string();

        let events = self.events.clone();
        let config = self.config.clone();
        match ws::connect(&config, &session.token, &events).await {
            Ok(tx) => {
                self.ws_tx = Some(tx);
                self.status = self.t_with("status.connected_as", &[("email", &session.email)]);
            }
            Err(e) => {
                let error = e.to_string();
                self.error = Some(self.t_with("status.connect_failed", &[("error", &error)]));
                self.status = self.t("status.ws_failed").to_string();
            }
        }
    }

    async fn sign_out(&mut self) {
        if let Some(session) = &self.session {
            let _ = self.api.logout(&session.token).await;
        }
        let _ = Session::clear();
        self.session = None;
        self.ws_tx = None;
        self.mode = Mode::SignIn;
        self.focus = Focus::Email;
        self.dismiss_modals();
        self.code.clear();
        self.log.clear();
        self.pongs = 0;
        self.status = self.t("status.signed_out").to_string();
        self.error = None;
    }

    fn handle_server(&mut self, msg: ServerMessage) {
        match msg {
            ServerMessage::Ready { user } => {
                self.push_log(self.t_with(
                    "status.ready",
                    &[("name", &user.display_name), ("email", &user.email)],
                ));
            }
            ServerMessage::Pong { nonce } => {
                self.pongs += 1;
                let nonce = nonce.to_string();
                self.push_log(self.t_with("status.pong", &[("nonce", &nonce)]));
            }
            ServerMessage::Error { code, message } => {
                self.push_log(format!("[{code}] {message}"));
                if code == "ws_closed" {
                    self.ws_tx = None;
                    let text = self.error_text(&code, &message);
                    self.status = self.t_with("status.disconnected", &[("reason", &text)]);
                    self.session = None;
                    self.mode = Mode::SignIn;
                    self.focus = Focus::Email;
                    self.dismiss_modals();
                    self.error = Some(text);
                }
            }
        }
    }

    fn ping(&mut self) {
        if let Some(tx) = self.ws_tx.clone() {
            self.nonce += 1;
            let nonce = self.nonce.to_string();
            self.push_log(self.t_with("status.ping", &[("nonce", &nonce)]));
            let _ = tx.send(ClientMessage::Ping { nonce });
        }
    }

    fn handle_completion(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => self.push_log(text),
            Err(e) => self.push_log(format!("[ask] {e}")),
        }
    }

    fn push_log(&mut self, line: String) {
        self.log.push_back(line);
        while self.log.len() > LOG_CAPACITY {
            self.log.pop_front();
        }
    }

    fn type_char(&mut self, c: char) {
        self.error = None;
        match (self.mode, self.focus) {
            (Mode::SignIn, Focus::Email) => self.email.push(c),
            (Mode::SignIn, Focus::Code) => self.code.push(c),
            (Mode::Register, Focus::Email) => self.email.push(c),
            (Mode::Register, Focus::Name) => self.name.push(c),
            (Mode::RegisterCode, Focus::Code) => self.code.push(c),
            _ => {}
        }
    }

    fn backspace(&mut self) {
        match (self.mode, self.focus) {
            (Mode::SignIn, Focus::Email) => {
                self.email.pop();
            }
            (Mode::SignIn, Focus::Code) => {
                self.code.pop();
            }
            (Mode::Register, Focus::Email) => {
                self.email.pop();
            }
            (Mode::Register, Focus::Name) => {
                self.name.pop();
            }
            (Mode::RegisterCode, Focus::Code) => {
                self.code.pop();
            }
            _ => {}
        }
    }

    fn cycle_focus(&mut self) {
        match self.mode {
            Mode::SignIn => {
                self.focus = match self.focus {
                    Focus::Email => Focus::Code,
                    _ => Focus::Email,
                }
            }
            Mode::Register => {
                self.focus = match self.focus {
                    Focus::Email => Focus::Name,
                    _ => Focus::Email,
                }
            }
            Mode::RegisterCode => self.focus = Focus::Code,
            Mode::Device => {}
            Mode::Connected => {}
            Mode::Engine => {}
            Mode::Key => {}
            Mode::Help => {}
        }
    }

    fn field(label: &str, value: &str, focused: bool) -> Line<'static> {
        let marker = if focused { "> " } else { "  " };
        let label_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let mut spans = vec![
            Span::styled(format!("{marker}{label}: "), label_style),
            Span::raw(value.to_string()),
        ];
        if focused {
            spans.push(Span::styled("_", label_style));
        }
        Line::from(spans)
    }

    fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(frame.area());

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" light-factory · {}", self.status),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            chunks[0],
        );

        match self.mode {
            Mode::SignIn => self.draw_signin(frame, chunks[1]),
            Mode::Register => self.draw_register(frame, chunks[1]),
            Mode::RegisterCode => self.draw_register_code(frame, chunks[1]),
            Mode::Device => self.draw_device(frame, chunks[1]),
            Mode::Connected => self.draw_connected(frame, chunks[1]),
            Mode::Engine => self.draw_engine(frame, chunks[1]),
            Mode::Key => self.draw_key(frame, chunks[1]),
            Mode::Help => self.draw_help(frame, chunks[1]),
        }

        if let Some(Modal::Connect(_)) = self.modal.current() {
            self.draw_connect(frame, chunks[1]);
        }

        if let Some(Modal::Models(_)) = self.modal.current() {
            self.draw_models(frame, chunks[1]);
        }

        let hints = if self.command_mode {
            format!("> {}", self.command)
        } else if self.mode == Mode::Help {
            self.t("hint.help_close").to_string()
        } else if self.mode == Mode::Device {
            self.t("hint.device_cancel").to_string()
        } else {
            self.t("hint.help").to_string()
        };
        frame.render_widget(
            Paragraph::new(hints).style(Style::default().fg(Color::DarkGray)),
            chunks[2],
        );
    }

    fn draw_signin(&self, frame: &mut Frame, area: Rect) {
        let mut lines = vec![Line::from(Span::styled(
            self.t("screen.sign_in"),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(""));
        lines.push(Self::field(
            self.t("field.email"),
            &self.email,
            self.focus == Focus::Email,
        ));
        lines.push(Self::field(
            self.t("field.code"),
            &self.code,
            self.focus == Focus::Code,
        ));
        lines.push(Line::from(""));
        lines.push(match &self.error {
            Some(err) => Line::from(Span::styled(err.clone(), Style::default().fg(Color::Red))),
            None => Line::from(Span::styled(
                self.t("hint.sign_in"),
                Style::default().fg(Color::DarkGray),
            )),
        });
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" light-factory "),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }

    fn draw_register(&self, frame: &mut Frame, area: Rect) {
        let mut lines = vec![Line::from(Span::styled(
            self.t("screen.create_account"),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(""));
        lines.push(Self::field(
            self.t("field.email"),
            &self.email,
            self.focus == Focus::Email,
        ));
        lines.push(Self::field(
            self.t("field.name"),
            &self.name,
            self.focus == Focus::Name,
        ));
        lines.push(Line::from(""));
        lines.push(match &self.error {
            Some(err) => Line::from(Span::styled(err.clone(), Style::default().fg(Color::Red))),
            None => Line::from(Span::styled(
                self.t("hint.register"),
                Style::default().fg(Color::DarkGray),
            )),
        });
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" light-factory "),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }

    fn draw_register_code(&self, frame: &mut Frame, area: Rect) {
        let mut lines = vec![Line::from(Span::styled(
            self.t("screen.complete_registration"),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(""));
        lines.push(Line::from(Span::raw(
            self.t_with("hint.account", &[("email", &self.email)]),
        )));
        if let Some(url) = &self.otpauth_url {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                self.t("hint.open_url"),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::raw(url.clone())));
        }
        if let Some(secret) = &self.secret {
            lines.push(Line::from(vec![
                Span::styled(
                    self.t("hint.manual_secret"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(secret.clone()),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Self::field(
            self.t("field.code"),
            &self.code,
            self.focus == Focus::Code,
        ));
        lines.push(Line::from(""));
        if let Some(err) = &self.error {
            lines.push(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" light-factory "),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }

    fn draw_device(&self, frame: &mut Frame, area: Rect) {
        let mut lines = vec![Line::from(Span::styled(
            self.t("screen.device_login"),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(""));
        lines.push(Line::from(self.t("hint.device_line1")));
        lines.push(Line::from(self.t("hint.device_line2")));
        lines.push(Line::from(""));
        lines.push(Line::from(self.t("hint.device_visit")));
        if let Some(url) = &self.device_verification_uri {
            lines.push(Line::from(Span::styled(
                url.clone(),
                Style::default().fg(Color::Cyan),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            self.t("hint.device_code_filled"),
            Style::default().fg(Color::DarkGray),
        )));
        if let Some(code) = &self.device_user_code {
            lines.push(Line::from(Span::styled(
                code.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            self.t("hint.device_waiting"),
            Style::default().fg(Color::DarkGray),
        )));
        if let Some(err) = &self.error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" light-factory "),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }

    fn draw_connected(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let info = match &self.session {
            Some(s) => {
                let pongs = self.pongs.to_string();
                let mut provider = self.provider_info.display();
                let reason = self.provider_info.reason(self.config.lang);
                if !reason.is_empty() {
                    provider = format!("{provider} · {reason}");
                }
                self.t_with(
                    "info.connected",
                    &[
                        ("name", &s.display_name),
                        ("email", &s.email),
                        ("pongs", &pongs),
                        ("provider", &provider),
                    ],
                )
            }
            None => self.t("hint.not_signed_in").to_string(),
        };
        frame.render_widget(
            Paragraph::new(info).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" light-factory "),
            ),
            chunks[0],
        );

        let items: Vec<ListItem> = if self.log.is_empty() {
            vec![ListItem::new(Span::styled(
                self.t("hint.no_messages"),
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            self.log.iter().map(|l| ListItem::new(l.clone())).collect()
        };
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", self.t("title.activity"))),
        );
        frame.render_widget(list, chunks[1]);
    }

    fn draw_engine(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(area);

        let items: Vec<ListItem> = if self.engine_log.is_empty() {
            vec![ListItem::new(Span::styled(
                self.t("hint.no_messages"),
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            self.engine_log
                .iter()
                .map(|l| ListItem::new(l.clone()))
                .collect()
        };
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", self.t("title.engine"))),
        );
        frame.render_widget(list, chunks[0]);

        let footer_text = match &self.pending {
            Some((_, prompt)) => prompt.clone(),
            None => format!("> {}", self.engine_prompt),
        };
        frame.render_widget(
            Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL)),
            chunks[1],
        );
    }

    fn draw_key(&self, frame: &mut Frame, area: Rect) {
        let provider = self.key_target.as_deref().unwrap_or("");
        let masked = mask(&self.key_input);
        let mut lines = vec![
            Line::from(Span::styled(
                self.t_with("status.key_enter", &[("provider", provider)]),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Self::field(self.t("field.key"), &masked, true),
            Line::from(""),
            Line::from(Span::styled(
                self.t("hint.key"),
                Style::default().fg(Color::DarkGray),
            )),
        ];
        if let Some(err) = &self.error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" light-factory "),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let modal = centered_rect(80, 90, area);
        let lines: Vec<Line> = help_lines(self.config.lang)
            .into_iter()
            .map(Line::from)
            .collect();
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.t("title.help"))),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, modal);
    }

    fn draw_connect(&self, frame: &mut Frame, area: Rect) {
        let Some(Modal::Connect(step)) = self.modal.current() else {
            return;
        };
        let mut lines: Vec<Line> = Vec::new();
        let title: String;
        match step {
            ConnectStep::ProviderList { rows, selected } => {
                title = self.t("connect.title").to_string();
                for (i, row) in rows.iter().enumerate() {
                    let marker = if i == *selected { "> " } else { "  " };
                    let style = if i == *selected {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default()
                    };
                    let suffix = if row.connected {
                        format!(" ({})", self.t("connect.connected"))
                    } else {
                        String::new()
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{marker}{}{suffix}", row.id),
                        style,
                    )));
                }
            }
            ConnectStep::KeyEntry {
                provider, input, ..
            } => {
                title = self.t("connect.key_heading").to_string();
                lines.push(Line::from(
                    self.t_with("status.key_enter", &[("provider", provider)]),
                ));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    mask(input),
                    Style::default().add_modifier(Modifier::REVERSED),
                )));
                if let Some(err) = &self.error {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        err.clone(),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
            ConnectStep::ModelList {
                provider,
                models,
                selected,
                fetching,
                error,
                ..
            } => {
                title = self.t_with("connect.models_heading", &[("provider", provider)]);
                if *fetching {
                    lines.push(Line::from(Span::styled(
                        self.t("connect.fetching"),
                        Style::default().fg(Color::DarkGray),
                    )));
                } else if let Some(err) = error {
                    lines.push(Line::from(Span::styled(
                        err.clone(),
                        Style::default().fg(Color::Red),
                    )));
                } else if models.is_empty() {
                    lines.push(Line::from(Span::styled(
                        self.t("connect.no_models"),
                        Style::default().fg(Color::DarkGray),
                    )));
                } else {
                    for (i, model) in models.iter().enumerate() {
                        let marker = if i == *selected { "> " } else { "  " };
                        let style = if i == *selected {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default()
                        };
                        lines.push(Line::from(Span::styled(format!("{marker}{model}"), style)));
                    }
                }
            }
        }

        let footer = match step {
            ConnectStep::ProviderList { .. } => self.t("connect.footer_list"),
            ConnectStep::KeyEntry { .. } => self.t("connect.footer_key"),
            ConnectStep::ModelList { fetching, .. } => {
                if *fetching {
                    self.t("connect.footer_fetching")
                } else {
                    self.t("connect.footer_models")
                }
            }
        };
        let focus = match step {
            ConnectStep::ProviderList { selected, .. } => Some(*selected),
            ConnectStep::ModelList {
                selected,
                fetching: false,
                models,
                ..
            } if !models.is_empty() => Some(*selected),
            _ => None,
        };
        draw_popup(
            frame,
            area,
            title,
            lines,
            Line::from(Span::styled(footer, Style::default().fg(Color::DarkGray))),
            focus,
        );
    }

    fn draw_models(&self, frame: &mut Frame, area: Rect) {
        let Some(Modal::Models(step)) = self.modal.current() else {
            return;
        };
        let title = self.t("models.title").to_string();
        let mut lines: Vec<Line> = Vec::new();
        let footer: &str;
        match step {
            ModelsStep::Offline => {
                if let Some(reason) = &self.provider_info.offline {
                    lines.push(Line::from(Span::styled(
                        crate::provider::offline_notice(self.config.lang, reason),
                        Style::default().fg(Color::Yellow),
                    )));
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(Span::styled(
                    self.t("models.offline"),
                    Style::default().fg(Color::DarkGray),
                )));
                footer = self.t("models.footer_offline");
            }
            ModelsStep::ModelList {
                models,
                selected,
                fetching,
                ..
            } => {
                if *fetching {
                    lines.push(Line::from(Span::styled(
                        self.t("connect.fetching"),
                        Style::default().fg(Color::DarkGray),
                    )));
                    footer = self.t("connect.footer_fetching");
                } else if models.is_empty() {
                    lines.push(Line::from(Span::styled(
                        self.t("connect.no_models"),
                        Style::default().fg(Color::DarkGray),
                    )));
                    // Enter is a no-op with nothing to select, so don't advertise it.
                    footer = self.t("models.footer_offline");
                } else {
                    for (i, model) in models.iter().enumerate() {
                        let marker = if i == *selected { "> " } else { "  " };
                        let style = if i == *selected {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default()
                        };
                        lines.push(Line::from(Span::styled(format!("{marker}{model}"), style)));
                    }
                    footer = self.t("models.footer_list");
                }
            }
            ModelsStep::Manual { input, error, .. } => {
                if let Some(err) = error {
                    lines.push(Line::from(Span::styled(
                        err.clone(),
                        Style::default().fg(Color::Red),
                    )));
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(Span::styled(
                    self.t("models.manual"),
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(Span::styled(
                    input.clone(),
                    Style::default().add_modifier(Modifier::REVERSED),
                )));
                footer = self.t("models.footer_manual");
            }
        }
        let focus = match step {
            ModelsStep::ModelList {
                selected,
                fetching: false,
                models,
                ..
            } if !models.is_empty() => Some(*selected),
            _ => None,
        };
        draw_popup(
            frame,
            area,
            title,
            lines,
            Line::from(Span::styled(footer, Style::default().fg(Color::DarkGray))),
            focus,
        );
    }
}

/// Run the terminal UI until the user quits.
pub async fn run(
    config: Config,
    provider: Arc<dyn Provider>,
    provider_info: ProviderInfo,
    store: Arc<dyn CredentialStore>,
    settings: SettingsHandle,
    prefilled_email: Option<String>,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (events, mut event_rx) = mpsc::unbounded_channel::<UiEvent>();
    let mut app = App::new(
        config,
        provider,
        provider_info,
        store,
        settings,
        prefilled_email,
        events.clone(),
    );

    // Restore a saved session if it is still valid; otherwise go straight into
    // the browser-based device login.
    let restored = match Session::load() {
        Some(session) => match app.api.me(&session.token).await {
            Ok(_) => {
                app.enter(session).await;
                true
            }
            Err(_) => {
                let _ = Session::clear();
                false
            }
        },
        None => false,
    };
    if !restored {
        app.start_device_login().await;
    }

    let input_events = events.clone();
    tokio::spawn(async move {
        let mut stream = crossterm::event::EventStream::new();
        while let Some(Ok(ev)) = stream.next().await {
            if let Event::Key(key) = ev
                && input_events.send(UiEvent::Key(key)).is_err()
            {
                break;
            }
        }
    });

    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut ticks: u64 = 0;

    let result: anyhow::Result<()> = loop {
        tokio::select! {
            Some(ev) = event_rx.recv() => {
                match ev {
                    UiEvent::Key(key) => {
                        if app.handle_key(key).await {
                            break Ok(());
                        }
                    }
                    UiEvent::Server(msg) => app.handle_server(msg),
                    UiEvent::Device { nonce, result } => {
                        app.handle_device_result(nonce, result).await
                    }
                    UiEvent::Completion(result) => app.handle_completion(result),
                    UiEvent::Engine(event) => app.handle_engine_event(event),
                    UiEvent::EngineDropped(n) => app.handle_engine_dropped(n),
                    UiEvent::ConnectModels {
                        nonce,
                        provider,
                        result,
                    } => app.handle_connect_models(nonce, provider, result),
                    UiEvent::ModelsFetched {
                        nonce,
                        provider,
                        result,
                    } => app.handle_models_fetched(nonce, provider, result),
                }
            }
            _ = tick.tick() => {
                ticks += 1;
                if app.ws_tx.is_some() && ticks.is_multiple_of(KEEPALIVE_SECONDS) {
                    app.ping();
                }
            }
        }
        terminal.draw(|f| app.draw(f))?;
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Map an engine-mode letter key to an approval answer while an approval is pending.
/// `a`/`d` approve/deny only when `pending` is set; otherwise they are ordinary input.
fn engine_approval_key(c: char, pending: bool) -> Option<bool> {
    if !pending {
        return None;
    }
    match c {
        'a' => Some(true),
        'd' => Some(false),
        _ => None,
    }
}

/// One step of the engine-event forwarding loop, decoded from a `broadcast::Receiver::recv`
/// result. `Lagged(n)` continues the loop (surfacing a "dropped n events" notice); only a
/// closed channel ends forwarding.
enum EngineForward {
    Event(EngineEvent),
    Dropped(u64),
    Stop,
}

fn engine_forward_step(
    result: Result<EngineEvent, tokio::sync::broadcast::error::RecvError>,
) -> EngineForward {
    match result {
        Ok(event) => EngineForward::Event(event),
        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => EngineForward::Dropped(n),
        Err(tokio::sync::broadcast::error::RecvError::Closed) => EngineForward::Stop,
    }
}

/// Parse an `/ask <prompt>` command into the prompt, or `None` when the command is not an
/// `/ask` with a non-empty prompt. `/ask` and `/ask   ` (empty prompt) are `None` so the caller
/// can show a usage hint; `/askhello` (no word boundary) and other commands are also `None`.
fn parse_ask_command(command: &str) -> Option<&str> {
    let rest = command.trim().strip_prefix("/ask")?;
    let boundary = rest.chars().next().map(char::is_whitespace).unwrap_or(true);
    if !boundary {
        return None;
    }
    let prompt = rest.trim();
    if prompt.is_empty() {
        None
    } else {
        Some(prompt)
    }
}

use crate::selection::REMOTE_IDS;

/// Every provider the connect modal can offer.
const PROVIDER_NAMES: [&str; 5] = ["anthropic", "openai", "gemini", "deepseek", "ollama"];

fn is_valid_provider(name: &str) -> bool {
    PROVIDER_NAMES.contains(&name)
}

pub(crate) fn takes_key(provider: &str) -> bool {
    light_factory_providers::env_key_var(provider).is_some()
}

/// A parsed `/key` command.
enum KeyCommand {
    List,
    Set(String),
    Clear(String),
}

/// Parse a `/connect` command: `true` for `/connect` (optionally followed by whitespace), `false`
/// otherwise — including `/connectX` (no word boundary), mirroring `/ask`.
fn parse_connect_command(command: &str) -> bool {
    command
        .trim()
        .strip_prefix("/connect")
        .map(word_boundary)
        .unwrap_or(false)
}

/// Parse a `/models` command: `true` for `/models` (optionally followed by whitespace), `false`
/// otherwise — including `/modelsX` (no word boundary), mirroring `/connect`.
fn parse_models_command(command: &str) -> bool {
    command
        .trim()
        .strip_prefix("/models")
        .map(word_boundary)
        .unwrap_or(false)
}

/// Mask a secret for rendering: one `*` per character, never the input value.
pub(crate) fn mask(input: &str) -> String {
    "*".repeat(input.chars().count())
}

/// Parse a `/model` command: `Some(Some(id))` for `/model <id>`, `Some(None)` for a bare `/model`
/// (empty arg), or `None` when the command is not `/model`.
fn parse_model_command(command: &str) -> Option<Option<&str>> {
    let rest = command.strip_prefix("/model")?;
    if !word_boundary(rest) {
        return None;
    }
    let arg = rest.trim();
    if arg.is_empty() {
        Some(None)
    } else {
        Some(Some(arg))
    }
}

/// Parse a `/key` command: bare → `List`, `/key <provider>` → `Set`, `/key <provider> clear` →
/// `Clear`. `None` when the command is not `/key`.
fn parse_key_command(command: &str) -> Option<KeyCommand> {
    let rest = command.strip_prefix("/key")?;
    if !word_boundary(rest) {
        return None;
    }
    let arg = rest.trim();
    if arg.is_empty() {
        return Some(KeyCommand::List);
    }
    if let Some(provider) = arg.strip_suffix(" clear").map(str::trim)
        && !provider.is_empty()
    {
        return Some(KeyCommand::Clear(provider.to_string()));
    }
    Some(KeyCommand::Set(arg.to_string()))
}

/// True when `rest` begins at a word boundary (empty, or starts with whitespace) — so `/askhello`,
/// `/providerX`, etc. do not match their commands.
fn word_boundary(rest: &str) -> bool {
    rest.chars().next().map(char::is_whitespace).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{
        App, ConnectStep, EngineForward, KeyCommand, Modal, Mode, ModelsStep, UiEvent,
        engine_approval_key, engine_forward_step, help_lines, mask, parse_ask_command,
        parse_connect_command, parse_key_command, parse_model_command, parse_models_command,
    };
    use crate::config::Config;
    use crate::provider::ProviderInfo;
    use crate::settings::{Settings, SettingsHandle};
    use light_factory_protocol::session::{Event as EngineEvent, EventKind, SessionId};
    use light_factory_protocol::wire::ServerMessage;
    use light_factory_providers::{LocalProvider, OfflineReason, Provider};
    use light_factory_tui::credentials::{CredentialStore, MemStore};
    use light_factory_tui::i18n::Locale;
    use ratatui::Terminal;
    use tokio::sync::broadcast::error::RecvError;
    use tokio::sync::mpsc;

    fn test_app_with_store(store: Arc<dyn CredentialStore>) -> App {
        let config = Config::from_url("http://localhost:8080").unwrap();
        let provider: Arc<dyn Provider> = Arc::new(LocalProvider::new());
        let provider_info = ProviderInfo {
            id: "local".to_string(),
            model: None,
            offline: None,
            selected_by: None,
            warnings: Vec::new(),
        };
        let (events, _rx) = mpsc::unbounded_channel::<UiEvent>();
        // Isolation by construction: no test may ever write the developer's real config.json.
        App::new(
            config,
            provider,
            provider_info,
            store,
            SettingsHandle {
                settings: Settings::default(),
                path: temp_settings_path(),
            },
            None,
            events,
        )
    }

    fn test_app() -> App {
        test_app_with_store(Arc::new(MemStore::new()))
    }

    /// A unique settings file under the temp dir, so no test ever touches the developer's real
    /// `config.json`. Every `test_app*` gets its own, so parallel tests cannot collide.
    fn temp_settings_path() -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("light-factory-app-{}-{n}.json", std::process::id()))
    }

    /// Removes the settings file it names when the test ends, panic or not.
    struct TempSettings(std::path::PathBuf);

    impl Drop for TempSettings {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// A store whose `set` always fails, for exercising the keyring-failure branch.
    struct FailingStore;

    impl CredentialStore for FailingStore {
        fn get(&self, _provider: &str) -> anyhow::Result<Option<String>> {
            Ok(None)
        }

        fn set(&self, _provider: &str, _key: &str) -> anyhow::Result<()> {
            anyhow::bail!("keyring unavailable")
        }

        fn delete(&self, _provider: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Open a modal directly on the host, bypassing `App::open_modal` (which would spawn a fetch
    /// outside a runtime), and return the nonce a fetch started for it would carry.
    fn open(app: &mut App, modal: Modal) -> u64 {
        app.modal.open(modal);
        app.modal.nonce()
    }

    /// The open connect step, for tests that assert on it.
    fn connect_step(app: &App) -> Option<&ConnectStep> {
        match app.modal.current() {
            Some(Modal::Connect(step)) => Some(step),
            _ => None,
        }
    }

    /// The open models step, for tests that assert on it.
    fn models_step(app: &App) -> Option<&ModelsStep> {
        match app.modal.current() {
            Some(Modal::Models(step)) => Some(step),
            _ => None,
        }
    }

    #[test]
    fn approval_keys_only_fire_while_a_prompt_is_pending() {
        assert_eq!(engine_approval_key('a', true), Some(true));
        assert_eq!(engine_approval_key('d', true), Some(false));
        assert_eq!(engine_approval_key('a', false), None);
        assert_eq!(engine_approval_key('d', false), None);
        assert_eq!(engine_approval_key('x', true), None);
    }

    #[test]
    fn a_lagged_broadcast_continues_instead_of_stopping() {
        assert!(matches!(
            engine_forward_step(Err(RecvError::Lagged(7))),
            EngineForward::Dropped(7)
        ));
        assert!(matches!(
            engine_forward_step(Err(RecvError::Closed)),
            EngineForward::Stop
        ));
    }

    #[test]
    fn a_received_event_is_forwarded() {
        let event = EngineEvent {
            seq: 1,
            session: SessionId::new(),
            kind: EventKind::Log {
                message: "hi".into(),
            },
        };
        assert!(matches!(
            engine_forward_step(Ok(event)),
            EngineForward::Event(_)
        ));
    }

    #[test]
    fn parses_an_ask_prompt() {
        assert_eq!(parse_ask_command("/ask hello"), Some("hello"));
    }

    #[test]
    fn rejects_an_empty_ask() {
        assert_eq!(parse_ask_command("/ask"), None);
        assert_eq!(parse_ask_command("/ask   "), None);
    }

    #[test]
    fn rejects_other_commands() {
        assert_eq!(parse_ask_command("/auth/login"), None);
        assert_eq!(parse_ask_command("/askhello"), None);
    }

    #[test]
    fn parses_connect_command() {
        assert!(parse_connect_command("/connect"));
        assert!(parse_connect_command("/connect   "));
        assert!(!parse_connect_command("/connectx"));
        assert!(!parse_connect_command("/provider"));
        assert!(!parse_connect_command("/ask hello"));
    }

    #[test]
    fn mask_never_echoes_input() {
        assert_eq!(mask(""), "");
        assert_eq!(mask("abc"), "***");
        assert_eq!(mask("sk-secret"), "*********");
    }

    fn model_list_step(models: Vec<String>, fetching: bool) -> ConnectStep {
        ConnectStep::ModelList {
            rows: vec![],
            provider: "openai".to_string(),
            models,
            selected: 0,
            fetching,
            error: None,
            from_key: false,
        }
    }

    #[test]
    fn handle_connect_models_ignores_stale_nonces() {
        let mut app = test_app();
        let nonce = open(&mut app, Modal::Connect(model_list_step(vec![], true)));
        app.handle_connect_models(
            nonce - 1,
            "openai".to_string(),
            Ok(vec!["gpt-4o".to_string()]),
        );
        assert!(matches!(
            connect_step(&app),
            Some(ConnectStep::ModelList {
                fetching: true,
                models,
                ..
            }) if models.is_empty()
        ));
    }

    #[test]
    fn handle_connect_models_fills_models_for_a_matching_nonce() {
        let mut app = test_app();
        let nonce = open(&mut app, Modal::Connect(model_list_step(vec![], true)));
        app.handle_connect_models(
            nonce,
            "openai".to_string(),
            Ok(vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]),
        );
        assert!(matches!(
            connect_step(&app),
            Some(ConnectStep::ModelList {
                fetching: false,
                error: None,
                models,
                ..
            }) if *models == vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]
        ));
    }

    #[test]
    fn handle_connect_models_surfaces_a_fetch_error() {
        let mut app = test_app();
        let nonce = open(&mut app, Modal::Connect(model_list_step(vec![], true)));
        app.handle_connect_models(nonce, "openai".to_string(), Err("bad key".to_string()));
        assert!(matches!(
            connect_step(&app),
            Some(ConnectStep::ModelList {
                fetching: false,
                error: Some(_),
                ..
            })
        ));
    }

    fn models_list_step(models: Vec<String>, fetching: bool) -> ModelsStep {
        ModelsStep::ModelList {
            provider: "openai".to_string(),
            models,
            selected: 0,
            fetching,
        }
    }

    fn models_manual_step(input: &str) -> ModelsStep {
        ModelsStep::Manual {
            provider: "openai".to_string(),
            input: input.to_string(),
            error: None,
        }
    }

    /// Render the whole app to an off-screen terminal and return it as text, so modal rendering
    /// can be asserted without a real terminal.
    fn render(app: &mut App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(width as usize)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn models_modal_renders_its_own_header() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        open(
            &mut app,
            Modal::Models(models_list_step(vec!["gpt-4o".to_string()], false)),
        );
        let screen = render(&mut app, 80, 20);
        assert!(screen.contains("Select a model"), "{screen}");
        assert!(screen.contains("gpt-4o"), "{screen}");
    }

    #[test]
    fn a_long_model_list_keeps_the_selection_and_footer_on_screen() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        let models: Vec<String> = (0..80).map(|i| format!("model-{i:03}")).collect();
        open(
            &mut app,
            Modal::Models(ModelsStep::ModelList {
                provider: "openai".to_string(),
                models,
                selected: 60,
                fetching: false,
            }),
        );

        let screen = render(&mut app, 80, 24);

        assert!(
            screen.contains("> model-060"),
            "the highlighted row scrolled off screen:\n{screen}"
        );
        assert!(
            screen.contains("Enter: select"),
            "the footer scrolled off screen:\n{screen}"
        );
    }

    #[test]
    fn an_empty_list_does_not_advertise_enter() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        open(&mut app, Modal::Models(models_list_step(vec![], false)));
        let screen = render(&mut app, 80, 20);
        assert!(
            !screen.contains("Enter: select"),
            "Enter is a no-op with nothing to select:\n{screen}"
        );
        assert!(screen.contains("Esc: close"), "{screen}");
    }

    #[test]
    fn the_offline_modal_names_the_actual_reason() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        app.provider_info.offline = Some(OfflineReason::NamedProviderMissingKey {
            selector: "openai".to_string(),
            key: "OPENAI_API_KEY".to_string(),
        });
        open(&mut app, Modal::Models(ModelsStep::Offline));
        let screen = render(&mut app, 80, 20);
        assert!(screen.contains("OPENAI_API_KEY"), "{screen}");
    }

    #[test]
    fn a_popup_on_a_tiny_terminal_does_not_panic() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        open(
            &mut app,
            Modal::Models(models_list_step(vec!["gpt-4o".to_string()], false)),
        );
        for (w, h) in [(4u16, 3u16), (10, 4), (80, 5)] {
            let _ = render(&mut app, w, h);
        }
    }

    #[test]
    fn models_fetch_result_is_ignored_when_the_provider_does_not_match() {
        let mut app = test_app();
        let nonce = open(&mut app, Modal::Models(models_list_step(vec![], true)));
        app.handle_models_fetched(
            nonce,
            "anthropic".to_string(),
            Ok(vec!["claude".to_string()]),
        );
        assert!(
            matches!(models_step(&app), Some(ModelsStep::ModelList { fetching: true, models, .. }) if models.is_empty()),
            "a result for another provider must not populate this modal"
        );
    }

    #[test]
    fn models_fetch_result_does_not_clobber_manual_entry() {
        let mut app = test_app();
        open(
            &mut app,
            Modal::Models(ModelsStep::Manual {
                provider: "openai".to_string(),
                input: "gpt-4".to_string(),
                error: None,
            }),
        );
        app.handle_models_fetched(
            app.modal.nonce(),
            "openai".to_string(),
            Ok(vec!["gpt-4o".to_string()]),
        );
        assert!(
            matches!(models_step(&app), Some(ModelsStep::Manual { input, .. }) if input == "gpt-4"),
            "a late result must not discard what the user typed"
        );
    }

    #[test]
    fn an_empty_but_successful_fetch_stays_a_list() {
        let mut app = test_app();
        let nonce = open(&mut app, Modal::Models(models_list_step(vec![], true)));
        app.handle_models_fetched(nonce, "openai".to_string(), Ok(vec![]));
        assert!(
            matches!(
                models_step(&app),
                Some(ModelsStep::ModelList {
                    fetching: false,
                    models,
                    ..
                }) if models.is_empty()
            ),
            "an empty list is not a fetch failure and must not route to manual entry"
        );
    }

    #[tokio::test]
    async fn models_command_opens_a_fetching_list_for_the_active_provider() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        // `local` resolves no key, so the spawned fetch fails offline instead of hitting network.
        app.provider_info.id = "local".to_string();
        app.run_command("/models").await;
        assert!(
            matches!(
                models_step(&app),
                Some(ModelsStep::ModelList { provider, fetching: true, .. }) if provider == "local"
            ),
            "expected a fetching list scoped to provider_info.id, got {:?}",
            models_step(&app)
        );
        assert_ne!(app.modal.nonce(), 0, "the fetch nonce must be bumped");
    }

    #[test]
    fn closing_the_modal_returns_to_the_mode_it_was_opened_from() {
        let mut app = test_app();
        app.mode = Mode::Engine;
        open(
            &mut app,
            Modal::Models(models_list_step(vec!["gpt-4o".to_string()], false)),
        );
        app.handle_modal_key(key(KeyCode::Esc));
        assert!(app.modal.current().is_none());
        assert!(
            app.mode == Mode::Engine,
            "a modal never disturbs the mode it is opened over, so there is nothing to restore"
        );
    }

    #[test]
    fn losing_the_session_dismisses_an_open_models_modal() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        open(
            &mut app,
            Modal::Models(models_list_step(vec!["gpt-4o".to_string()], false)),
        );

        app.handle_server(ServerMessage::Error {
            code: "ws_closed".to_string(),
            message: "server closed the connection".to_string(),
        });

        assert!(
            app.modal.current().is_none(),
            "modal must not survive a sign-out"
        );
        assert!(app.mode == Mode::SignIn);
    }

    #[test]
    fn a_failed_save_rolls_back_the_staged_model() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        // A path under a non-directory can never be created, so the write always fails.
        app.settings_path = std::path::PathBuf::from("/dev/null/nope/config.json");
        app.provider_info.model = Some("stale-sentinel".to_string());
        open(
            &mut app,
            Modal::Models(ModelsStep::Manual {
                provider: "openai".to_string(),
                input: "o3".to_string(),
                error: None,
            }),
        );

        app.handle_modal_key(key(KeyCode::Enter));

        assert!(
            app.settings.models.is_empty(),
            "a model that failed to save must not linger and be persisted by a later write"
        );
        assert!(app.error.is_some(), "the failure must be surfaced");
        assert_eq!(
            app.provider_info.model.as_deref(),
            Some("stale-sentinel"),
            "a failed save must not activate the model"
        );
    }

    #[tokio::test]
    async fn models_command_error_names_the_models_command() {
        let mut app = test_app();
        app.mode = Mode::SignIn;
        let expected = app.t("status.models_not_connected").to_string();
        app.run_command("/models").await;
        assert_eq!(app.error.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn parses_models_command() {
        assert!(parse_models_command("/models"));
        assert!(parse_models_command("/models   "));
        assert!(!parse_models_command("/modelsx"));
        assert!(!parse_models_command("/model gpt-5"));
        assert!(!parse_models_command("/connect"));
    }

    #[test]
    fn handle_models_fetched_ignores_stale_nonces() {
        let mut app = test_app();
        let nonce = open(&mut app, Modal::Models(models_list_step(vec![], true)));
        app.handle_models_fetched(
            nonce - 1,
            "openai".to_string(),
            Ok(vec!["gpt-4o".to_string()]),
        );
        assert!(matches!(
            models_step(&app),
            Some(ModelsStep::ModelList { fetching: true, models, .. }) if models.is_empty()
        ));
    }

    #[test]
    fn handle_models_fetched_pre_highlights_the_current_model() {
        let mut app = test_app();
        app.provider_info.model = Some("o3".to_string());
        let nonce = open(&mut app, Modal::Models(models_list_step(vec![], true)));
        app.handle_models_fetched(
            nonce,
            "openai".to_string(),
            Ok(vec!["gpt-4o".to_string(), "o3".to_string()]),
        );
        assert!(matches!(
            models_step(&app),
            Some(ModelsStep::ModelList { fetching: false, selected: 1, models, .. })
                if models.len() == 2
        ));
    }

    #[test]
    fn handle_models_fetched_falls_back_to_the_first_row_when_the_model_is_absent() {
        let mut app = test_app();
        app.provider_info.model = Some("not-listed".to_string());
        let nonce = open(&mut app, Modal::Models(models_list_step(vec![], true)));
        app.handle_models_fetched(
            nonce,
            "openai".to_string(),
            Ok(vec!["gpt-4o".to_string(), "o3".to_string()]),
        );
        assert!(matches!(
            models_step(&app),
            Some(ModelsStep::ModelList {
                fetching: false,
                selected: 0,
                ..
            })
        ));
    }

    #[test]
    fn handle_models_fetched_falls_back_to_manual_entry_on_a_fetch_error() {
        let mut app = test_app();
        let nonce = open(&mut app, Modal::Models(models_list_step(vec![], true)));
        app.handle_models_fetched(nonce, "openai".to_string(), Err("bad key".to_string()));
        assert!(matches!(
            models_step(&app),
            Some(ModelsStep::Manual {
                provider,
                input,
                error: Some(_),
            }) if provider == "openai" && input.is_empty()
        ));
    }

    #[test]
    fn models_enter_persists_the_highlighted_model_and_rebuilds() {
        let mut app = test_app();
        let _cleanup = TempSettings(app.settings_path.clone());
        app.mode = Mode::Connected;
        open(
            &mut app,
            Modal::Models(ModelsStep::ModelList {
                provider: "openai".to_string(),
                models: vec!["gpt-4o".to_string(), "o3".to_string()],
                selected: 1,
                fetching: false,
            }),
        );

        app.handle_modal_key(key(KeyCode::Enter));

        assert!(app.modal.current().is_none());
        assert_eq!(
            app.settings.models.get("openai").map(String::as_str),
            Some("o3")
        );
        assert!(
            app.settings.provider.is_none(),
            "/models must not activate a provider"
        );
        let saved = crate::settings::load_at(&app.settings_path).expect("settings were saved");
        assert_eq!(saved.models.get("openai").map(String::as_str), Some("o3"));
    }

    #[test]
    fn models_manual_enter_persists_the_trimmed_id() {
        let mut app = test_app();
        let _cleanup = TempSettings(app.settings_path.clone());
        app.mode = Mode::Connected;
        open(
            &mut app,
            Modal::Models(ModelsStep::Manual {
                provider: "openai".to_string(),
                input: "  o3-mini  ".to_string(),
                error: None,
            }),
        );

        app.handle_modal_key(key(KeyCode::Enter));

        assert!(app.modal.current().is_none());
        assert_eq!(
            app.settings.models.get("openai").map(String::as_str),
            Some("o3-mini")
        );
        assert!(app.settings.provider.is_none());
    }

    /// A successful apply must re-derive `provider_info` from the updated settings. Asserting the
    /// resulting model directly would depend on the ambient `LIGHT_*`/API-key environment, so this
    /// plants a sentinel that only a real `rebuild_provider()` call can clear.
    #[test]
    fn models_apply_rebuilds_the_active_provider() {
        let mut app = test_app();
        let _cleanup = TempSettings(app.settings_path.clone());
        app.mode = Mode::Connected;
        app.provider_info.model = Some("stale-sentinel".to_string());
        open(
            &mut app,
            Modal::Models(ModelsStep::Manual {
                provider: "ollama".to_string(),
                input: "llama3".to_string(),
                error: None,
            }),
        );

        app.handle_modal_key(key(KeyCode::Enter));

        assert_eq!(
            app.settings.models.get("ollama").map(String::as_str),
            Some("llama3")
        );
        assert_ne!(
            app.provider_info.model.as_deref(),
            Some("stale-sentinel"),
            "apply must rebuild the active provider"
        );
    }

    #[test]
    fn models_esc_closes_without_touching_settings() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        open(
            &mut app,
            Modal::Models(models_list_step(vec!["gpt-4o".to_string()], false)),
        );
        app.handle_modal_key(key(KeyCode::Esc));
        assert!(app.modal.current().is_none());
        assert!(app.mode == Mode::Connected);
        assert!(app.settings.models.is_empty());
    }

    #[test]
    fn models_blank_manual_enter_stays_open_without_touching_settings() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        open(&mut app, Modal::Models(models_manual_step("   ")));
        app.handle_modal_key(key(KeyCode::Enter));
        assert!(matches!(models_step(&app), Some(ModelsStep::Manual { .. })));
        assert!(app.settings.models.is_empty());
    }

    #[test]
    fn opening_the_models_modal_replaces_an_open_connect_modal() {
        let mut app = test_app();
        open(
            &mut app,
            Modal::Connect(ConnectStep::ProviderList {
                rows: vec![],
                selected: 0,
            }),
        );
        app.provider_info.offline = Some(OfflineReason::NothingConfigured);
        app.enter_models();
        assert!(
            matches!(app.modal.current(), Some(Modal::Models(_))),
            "only one modal can be open at a time"
        );
    }

    #[test]
    fn opening_a_second_modal_invalidates_the_first_modals_fetch() {
        let mut app = test_app();
        let stale = open(&mut app, Modal::Models(models_list_step(vec![], true)));
        app.enter_connect();
        app.handle_models_fetched(stale, "openai".to_string(), Ok(vec!["m".to_string()]));
        assert!(
            matches!(
                app.modal.current(),
                Some(Modal::Connect(ConnectStep::ProviderList { .. }))
            ),
            "a fetch from the replaced modal must not reach the new one"
        );
    }

    #[test]
    fn closing_the_models_modal_invalidates_an_in_flight_fetch() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        let nonce = open(&mut app, Modal::Models(models_list_step(vec![], true)));
        app.handle_modal_key(key(KeyCode::Esc));
        assert!(app.modal.current().is_none());
        assert_ne!(
            app.modal.nonce(),
            nonce,
            "the in-flight fetch must be invalidated"
        );
    }

    #[tokio::test]
    async fn models_command_requires_a_connected_session() {
        let mut app = test_app();
        app.mode = Mode::SignIn;
        app.run_command("/models").await;
        assert!(app.modal.current().is_none());
        assert!(app.error.is_some());
    }

    #[tokio::test]
    async fn models_command_opens_the_modal_offline_when_no_provider_is_active() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        app.provider_info.offline = Some(OfflineReason::NothingConfigured);
        app.run_command("/models").await;
        assert_eq!(models_step(&app), Some(&ModelsStep::Offline));
        assert!(app.settings.models.is_empty());
    }

    #[test]
    fn help_lists_the_models_command() {
        let lines = help_lines(Locale::En);
        assert!(lines.iter().any(|l| l.contains("/models")));
    }

    #[test]
    fn handle_connect_key_blank_key_stays_on_key_entry() {
        let mut app = test_app();
        open(
            &mut app,
            Modal::Connect(ConnectStep::KeyEntry {
                rows: vec![],
                provider: "openai".to_string(),
                input: "  ".to_string(),
            }),
        );
        app.handle_modal_key(key(KeyCode::Enter));
        assert!(matches!(
            connect_step(&app),
            Some(ConnectStep::KeyEntry { .. })
        ));
    }

    #[test]
    fn handle_connect_key_keyring_failure_sets_error_and_stays() {
        let mut app = test_app_with_store(Arc::new(FailingStore));
        open(
            &mut app,
            Modal::Connect(ConnectStep::KeyEntry {
                rows: vec![],
                provider: "openai".to_string(),
                input: "sk-x".to_string(),
            }),
        );
        app.handle_modal_key(key(KeyCode::Enter));
        assert!(matches!(
            connect_step(&app),
            Some(ConnectStep::KeyEntry { .. })
        ));
        assert!(app.error.is_some());
    }

    #[test]
    fn parses_model_commands() {
        assert_eq!(parse_model_command("/model gpt-5"), Some(Some("gpt-5")));
        assert_eq!(parse_model_command("/model"), Some(None));
        assert_eq!(parse_model_command("/model   "), Some(None));
        assert_eq!(parse_model_command("/modelx"), None);
    }

    #[test]
    fn parses_key_commands() {
        assert!(matches!(parse_key_command("/key"), Some(KeyCommand::List)));
        assert!(matches!(
            parse_key_command("/key openai"),
            Some(KeyCommand::Set(p)) if p == "openai"
        ));
        assert!(matches!(
            parse_key_command("/key openai clear"),
            Some(KeyCommand::Clear(p)) if p == "openai"
        ));
        assert!(
            matches!(parse_key_command("/key clear"), Some(KeyCommand::Set(p)) if p == "clear")
        );
        assert!(parse_key_command("/keyx").is_none());
    }

    #[test]
    fn help_modal_opens_and_restores_the_prior_mode() {
        let mut app = test_app();
        assert!(matches!(app.mode, Mode::SignIn));
        app.open_help();
        assert!(matches!(app.mode, Mode::Help));
        assert!(matches!(app.help_return, Mode::SignIn));
        app.close_help();
        assert!(matches!(app.mode, Mode::SignIn));
    }

    #[test]
    fn help_modal_returns_to_the_mode_it_was_opened_from() {
        let mut app = test_app();
        app.mode = Mode::Connected;
        app.open_help();
        assert!(matches!(app.help_return, Mode::Connected));
        app.close_help();
        assert!(matches!(app.mode, Mode::Connected));
    }

    #[test]
    fn esc_and_ctrl_p_close_help_but_ctrl_c_quits() {
        let mut app = test_app();

        app.open_help();
        assert!(!app.handle_help_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())));
        assert!(matches!(app.mode, Mode::SignIn));

        app.open_help();
        assert!(!app.handle_help_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)));
        assert!(matches!(app.mode, Mode::SignIn));

        app.open_help();
        assert!(app.handle_help_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    }
}
