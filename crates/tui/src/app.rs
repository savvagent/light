//! The ratatui application: auth forms plus the connected WebSocket screen.

use std::collections::VecDeque;
use std::io;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use light_factory_protocol::auth::AuthResponse;
use light_factory_protocol::wire::{ClientMessage, ServerMessage};
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
use crate::i18n::{self, Locale};
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
    error: Option<String>,
    status: String,
    log: VecDeque<String>,
    nonce: u64,
    pongs: u64,
}

impl App {
    fn new(
        config: Config,
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
            },
            KeyCode::Char('/')
                if matches!(
                    self.mode,
                    Mode::SignIn | Mode::Register | Mode::RegisterCode
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
            KeyCode::Char(c) => self.type_char(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Tab | KeyCode::Up | KeyCode::Down => self.cycle_focus(),
            KeyCode::Enter => self.submit().await,
            _ => {}
        }
        false
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
        match command.trim() {
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
                    self.status = self.t_with("status.disconnected", &[("reason", &message)]);
                    self.session = None;
                    self.mode = Mode::SignIn;
                    self.focus = Focus::Email;
                    self.error = Some(self.error_text(&code, &message));
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
        }

        let hints = if self.command_mode {
            format!("> {}", self.command)
        } else {
            match self.mode {
                Mode::Connected => self.t("hint.connected").to_string(),
                Mode::Device => self.t("hint.device_cancel").to_string(),
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
                self.t_with(
                    "info.connected",
                    &[
                        ("name", &s.display_name),
                        ("email", &s.email),
                        ("pongs", &pongs),
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
}

/// Run the terminal UI until the user quits.
pub async fn run(config: Config, prefilled_email: Option<String>) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (events, mut event_rx) = mpsc::unbounded_channel::<UiEvent>();
    let mut app = App::new(config, prefilled_email, events.clone());

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
