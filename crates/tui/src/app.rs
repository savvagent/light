//! The ratatui application: auth forms plus the connected WebSocket screen.

use std::collections::VecDeque;
use std::io;
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
use crate::provider::ProviderInfo;
use crate::session::Session;
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
    engine: Option<Engine>,
    engine_session: Option<SessionId>,
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
            engine: None,
            engine_session: None,
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
        if self.command_mode {
            return self.handle_command_key(key).await;
        }
        if self.mode == Mode::Engine {
            return self.handle_engine_key(key).await;
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
            KeyCode::Char('a') => self.engine_answer(true),
            KeyCode::Char('d') => self.engine_answer(false),
            KeyCode::Enter => self.engine_send_prompt(),
            KeyCode::Backspace => {
                self.engine_prompt.pop();
            }
            KeyCode::Char(c) => self.engine_prompt.push(c),
            _ => {}
        }
        false
    }

    fn enter_engine(&mut self) -> anyhow::Result<()> {
        let (provider, _info) = crate::provider::build();

        let mut engine = Engine::new(provider);
        let session = engine.create_session(std::env::current_dir()?)?;

        let mut events = engine.handle(session).expect("just created").subscribe();
        let tx = self.events.clone();
        tokio::spawn(async move {
            while let Ok(event) = events.recv().await {
                let _ = tx.send(UiEvent::Engine(event));
            }
        });

        self.engine = Some(engine);
        self.engine_session = Some(session);
        self.engine_log.clear();
        self.engine_prompt.clear();
        self.pending = None;
        self.mode = Mode::Engine;
        self.status = self.t("status.engine_started").to_string();
        Ok(())
    }

    fn leave_engine(&mut self) {
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
        match trimmed {
            "/auth/login" => self.start_device_login().await,
            "/auth/logout" => self.sign_out().await,
            "" => {}
            other if other.starts_with("/lang ") => {
                let arg = other["/lang ".len()..].trim();
                if let Some(locale) = Locale::parse(arg) {
                    self.config.lang = locale;
                    let _ = crate::settings::save_lang(locale.as_str());
                    self.status = self.t_with("status.lang_set", &[("lang", locale.as_str())]);
                } else {
                    self.error = Some(self.t("status.lang_invalid").to_string());
                }
            }
            other => {
                self.error = Some(self.t_with("status.unknown_command", &[("command", other)]))
            }
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
        }

        let hints = if self.command_mode {
            format!("> {}", self.command)
        } else {
            match self.mode {
                Mode::Connected => self.t("hint.connected").to_string(),
                Mode::Device => self.t("hint.device_cancel").to_string(),
                Mode::Engine => self.t("hint.engine").to_string(),
                _ => self.t("hint.default").to_string(),
            }
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
                let provider = self.provider_info.display();
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
}

/// Run the terminal UI until the user quits.
pub async fn run(
    config: Config,
    provider: Arc<dyn Provider>,
    provider_info: ProviderInfo,
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

#[cfg(test)]
mod tests {
    use super::parse_ask_command;

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
}
