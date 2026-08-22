//! The modal overlays layered over the TUI's screens: `/connect`, `/models`, and help.
//!
//! Each modal is a state enum plus a pure key-transition function, so the whole state machine is
//! testable without a terminal, a keyring, or the network. `App` owns exactly one of them at a
//! time.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use light_factory_providers::{OfflineReason, list_models, list_ollama_models};
use light_factory_tui::credentials::CredentialStore;
use light_factory_tui::i18n::{self, Locale};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

// `takes_key` also gates the `/key` command; `mask` (below) also renders the `Mode::Key` screen.
// Both point away from `app.rs`, so this module has no edge back into the one that owns it.
use crate::selection::takes_key;

/// Mask a secret for rendering: one `*` per character, never the input value.
pub(crate) fn mask(input: &str) -> String {
    "*".repeat(input.chars().count())
}

/// One row of the connect modal's provider list. Self-contained (id + connected flag) so the pure
/// transition function needs no store/keyring/network state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderRow {
    pub(crate) id: String,
    pub(crate) connected: bool,
}

/// The connect modal's step. `rows` is carried through every step so "back" navigation can
/// reconstruct the provider list without re-querying the keyring.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ConnectStep {
    ProviderList {
        rows: Vec<ProviderRow>,
        selected: usize,
    },
    KeyEntry {
        rows: Vec<ProviderRow>,
        provider: String,
        input: String,
    },
    ModelList {
        rows: Vec<ProviderRow>,
        provider: String,
        models: Vec<String>,
        selected: usize,
        fetching: bool,
        error: Option<String>,
        from_key: bool,
    },
}

impl std::fmt::Debug for ConnectStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `KeyEntry.input` holds a plaintext API key and must never appear in a Debug rendering.
        match self {
            ConnectStep::ProviderList { rows, selected } => f
                .debug_struct("ProviderList")
                .field("rows", rows)
                .field("selected", selected)
                .finish(),
            ConnectStep::KeyEntry {
                rows,
                provider,
                input: _,
            } => f
                .debug_struct("KeyEntry")
                .field("rows", rows)
                .field("provider", provider)
                .field("input", &"<redacted>")
                .finish(),
            ConnectStep::ModelList {
                rows,
                provider,
                models,
                selected,
                fetching,
                error,
                from_key,
            } => f
                .debug_struct("ModelList")
                .field("rows", rows)
                .field("provider", provider)
                .field("models", models)
                .field("selected", selected)
                .field("fetching", fetching)
                .field("error", error)
                .field("from_key", from_key)
                .finish(),
        }
    }
}

/// The `/models` modal's step.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum ModelsStep {
    ModelList {
        provider: String,
        models: Vec<String>,
        selected: usize,
        fetching: bool,
    },
    Manual {
        provider: String,
        input: String,
        error: Option<String>,
    },
    Offline,
}

/// The one overlay that owns the keyboard, if any.
///
/// One variant per modal is what makes "two modals open at once" unrepresentable: `App` holds a
/// single [`ModalHost`], not one `Option` per modal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Modal {
    Help,
    Connect(ConnectStep),
    Models(ModelsStep),
}

/// What a [`ModalTransition::Apply`] commits. The two modals apply different things, at different
/// privilege: `/connect` adopts a new preferred provider — changing which service receives the
/// user's prompts and key — while `/models` only re-pins the model of the provider already active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModalApply {
    /// `/connect`: adopt `provider` as the preferred provider and pin `model` to it.
    Provider { provider: String, model: String },
    /// `/models`: pin `model` to the already-active `provider`.
    Model { provider: String, model: String },
}

/// Which `UiEvent` a spawned model-list fetch reports back on.
///
/// Carried out of [`Modal::fetch_target`] by the state that asked for the fetch, so the sink is
/// decided by the same match that decided there was a fetch at all — never re-derived from `App`'s
/// state after the fact, which would be correct only for as long as nothing else had moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchSink {
    /// `UiEvent::ConnectModels` — filled into the `/connect` modal's list.
    Connect,
    /// `UiEvent::ModelsFetched` — filled into the `/models` modal's list.
    Models,
}

/// The result of stepping any modal: advance to a new state, commit a [`ModalApply`], or close.
///
/// `Apply` carries what it commits rather than naming it for the caller to look up again. That
/// makes "a step with nothing to commit cannot produce an `Apply`" structural — the rule is stated
/// once, in the transition arm that has the selection in scope, and there is no second read of the
/// modal (after the caller has already begun mutating state) that could disagree with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModalTransition {
    Step(Modal),
    Apply(ModalApply),
    Close,
}

impl Modal {
    /// Pure: maps a key press in the current state to the next state, apply, or close. No keyring,
    /// terminal, or network state.
    pub(crate) fn next(&self, key: KeyEvent) -> ModalTransition {
        match self {
            Modal::Help => help_step_next(key),
            Modal::Connect(step) => connect_step_next(step, key),
            Modal::Models(step) => models_step_next(step, key),
        }
    }

    /// The provider whose model list this state is waiting on, and where the result must be
    /// delivered. Comparing this across a transition is what starts a fetch exactly once, on the
    /// step that begins waiting.
    ///
    /// Every variant is spelled out rather than falling through a `_ => None`: a modal or step
    /// added later must say here whether it fetches, and the compiler asks. A catch-all would
    /// answer "no fetch" on its behalf, and the failure would be a list that loads forever.
    pub(crate) fn fetch_target(&self) -> Option<(&str, FetchSink)> {
        match self {
            Modal::Connect(ConnectStep::ModelList {
                provider,
                fetching: true,
                ..
            }) => Some((provider.as_str(), FetchSink::Connect)),
            Modal::Models(ModelsStep::ModelList {
                provider,
                fetching: true,
                ..
            }) => Some((provider.as_str(), FetchSink::Models)),
            Modal::Help
            | Modal::Connect(
                ConnectStep::ProviderList { .. }
                | ConnectStep::KeyEntry { .. }
                | ConnectStep::ModelList {
                    fetching: false, ..
                },
            )
            | Modal::Models(
                ModelsStep::ModelList {
                    fetching: false, ..
                }
                | ModelsStep::Manual { .. }
                | ModelsStep::Offline,
            ) => None,
        }
    }

    /// The i18n key for the status-bar hint this modal overrides, or `None` to leave the hint to
    /// the screen underneath. Only help takes the line over; the popups float and let the base
    /// screen keep saying what it was saying.
    pub(crate) fn hint_key(&self) -> Option<&'static str> {
        match self {
            Modal::Help => Some("hint.help_close"),
            Modal::Connect(_) | Modal::Models(_) => None,
        }
    }
}

/// Pure step-transition for the help modal. Esc and Ctrl-P close it; every other key is ignored,
/// so the screen underneath cannot be driven from behind the overlay.
fn help_step_next(key: KeyEvent) -> ModalTransition {
    match key.code {
        KeyCode::Esc => ModalTransition::Close,
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ModalTransition::Close
        }
        _ => ModalTransition::Step(Modal::Help),
    }
}

/// Owns the open modal together with the counter that invalidates its in-flight fetch.
///
/// The nonce outlives the modal on purpose: every mutation that walks away from an awaited model
/// list bumps it, so a fetch already in flight is discarded when it lands. Keeping it here — rather
/// than as a second `App` field beside an `Option<Modal>` — is what makes "abandoning a fetch
/// invalidates it" impossible to forget at a call site.
///
/// **The nonce is monotonic, and must stay so.** One counter serves every modal that has ever been
/// open, which is sound only because no value is ever reused. Resetting it to `0` on close — the
/// obvious-looking tidy-up — would let a later modal's fetch collide with an earlier one's nonce
/// and accept its result.
///
/// **One invalidation seam.** [`Self::invalidate_fetch`] is the only place the nonce moves, and
/// every mutator that abandons an in-flight fetch routes through it: [`Self::open`],
/// [`Self::close`], [`Self::replace_step`] when the awaited target changes, and
/// [`Self::next_fetch_nonce`]. Anything else that must happen when a fetch is abandoned — aborting
/// its `JoinHandle`, say — belongs in that one method, never at the four call sites.
#[derive(Default)]
pub(crate) struct ModalHost {
    open: Option<Modal>,
    nonce: u64,
}

impl ModalHost {
    /// Abandon whatever fetch this host is awaiting: bump the nonce so a result already in flight
    /// is discarded when it lands. The one seam — see the type docs.
    fn invalidate_fetch(&mut self) {
        self.nonce += 1;
    }

    /// Open `modal`, replacing whatever was open and invalidating its in-flight fetch.
    pub(crate) fn open(&mut self, modal: Modal) {
        self.invalidate_fetch();
        self.open = Some(modal);
    }

    /// Close whatever is open and invalidate its in-flight fetch.
    ///
    /// Unconditional on purpose. A fetch outlives the modal that started it — `Apply` closes a
    /// connect list whose request may still be in flight — so "nothing is open" is not evidence
    /// that nothing needs invalidating. Bumping with nothing open costs one increment and keeps
    /// this a plain fetch-generation counter rather than a modal-state-conditional one.
    pub(crate) fn close(&mut self) {
        self.open = None;
        self.invalidate_fetch();
    }

    /// Swap the open modal's state, invalidating the fetch it was awaiting only if the new state is
    /// no longer waiting on the same thing.
    ///
    /// A step that keeps the same [`Modal::fetch_target`] — moving the cursor inside a list that is
    /// still loading — must keep its result, which is why this is not an unconditional bump. A step
    /// that walks away from one — Esc out of a fetching list, back to the provider list — must not,
    /// which is why it is not an unconditional skip either. Without the bump the abandoned result
    /// is stopped only by the receiver-side shape guard, a second copy of the rule maintained
    /// somewhere else.
    pub(crate) fn replace_step(&mut self, modal: Modal) {
        let target_changed =
            self.open.as_ref().and_then(|m| m.fetch_target()) != modal.fetch_target();
        if target_changed {
            self.invalidate_fetch();
        }
        self.open = Some(modal);
    }

    pub(crate) fn current(&self) -> Option<&Modal> {
        self.open.as_ref()
    }

    pub(crate) fn current_mut(&mut self) -> Option<&mut Modal> {
        self.open.as_mut()
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub(crate) fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Claim a nonce for a newly-spawned fetch, invalidating any earlier one. The returned value is
    /// what that fetch's result must carry back to be accepted.
    pub(crate) fn next_fetch_nonce(&mut self) -> u64 {
        self.invalidate_fetch();
        self.nonce
    }
}

/// Move a list selection up (`-1`) or down (`+1`), wrapping at the ends.
fn cycle_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as isize + delta;
    if next < 0 {
        len - 1
    } else if next >= len as isize {
        0
    } else {
        next as usize
    }
}

/// Pure step-transition for the connect modal: maps a key press in the current step to the next
/// step (or close). No keyring, terminal, or network state — the `rows` carried in each step make
/// "back" navigation total.
fn connect_step_next(step: &ConnectStep, key: KeyEvent) -> ModalTransition {
    match step {
        ConnectStep::ProviderList { rows, selected } => match key.code {
            KeyCode::Esc => ModalTransition::Close,
            KeyCode::Up => ModalTransition::Step(Modal::Connect(ConnectStep::ProviderList {
                rows: rows.clone(),
                selected: cycle_index(*selected, rows.len(), -1),
            })),
            KeyCode::Down => ModalTransition::Step(Modal::Connect(ConnectStep::ProviderList {
                rows: rows.clone(),
                selected: cycle_index(*selected, rows.len(), 1),
            })),
            KeyCode::Enter => match rows.get(*selected) {
                Some(row) if row.connected || row.id == "ollama" => {
                    ModalTransition::Step(Modal::Connect(ConnectStep::ModelList {
                        rows: rows.clone(),
                        provider: row.id.clone(),
                        models: Vec::new(),
                        selected: 0,
                        fetching: true,
                        error: None,
                        from_key: false,
                    }))
                }
                Some(row) => ModalTransition::Step(Modal::Connect(ConnectStep::KeyEntry {
                    rows: rows.clone(),
                    provider: row.id.clone(),
                    input: String::new(),
                })),
                None => ModalTransition::Step(Modal::Connect(step.clone())),
            },
            _ => ModalTransition::Step(Modal::Connect(step.clone())),
        },
        ConnectStep::KeyEntry {
            rows,
            provider,
            input,
        } => match key.code {
            KeyCode::Esc => ModalTransition::Step(Modal::Connect(ConnectStep::ProviderList {
                rows: rows.clone(),
                selected: rows.iter().position(|r| r.id == *provider).unwrap_or(0),
            })),
            KeyCode::Enter if !input.trim().is_empty() => {
                ModalTransition::Step(Modal::Connect(ConnectStep::ModelList {
                    rows: rows.clone(),
                    provider: provider.clone(),
                    models: Vec::new(),
                    selected: 0,
                    fetching: true,
                    error: None,
                    from_key: true,
                }))
            }
            KeyCode::Backspace => {
                let mut next = input.clone();
                next.pop();
                ModalTransition::Step(Modal::Connect(ConnectStep::KeyEntry {
                    rows: rows.clone(),
                    provider: provider.clone(),
                    input: next,
                }))
            }
            KeyCode::Char(c) => ModalTransition::Step(Modal::Connect(ConnectStep::KeyEntry {
                rows: rows.clone(),
                provider: provider.clone(),
                input: format!("{input}{c}"),
            })),
            _ => ModalTransition::Step(Modal::Connect(step.clone())),
        },
        ConnectStep::ModelList {
            rows,
            provider,
            models,
            selected,
            fetching,
            error,
            from_key,
        } => match key.code {
            KeyCode::Esc => {
                if !*fetching && takes_key(provider) && (*from_key || error.is_some()) {
                    ModalTransition::Step(Modal::Connect(ConnectStep::KeyEntry {
                        rows: rows.clone(),
                        provider: provider.clone(),
                        input: String::new(),
                    }))
                } else {
                    ModalTransition::Step(Modal::Connect(ConnectStep::ProviderList {
                        rows: rows.clone(),
                        selected: rows.iter().position(|r| r.id == *provider).unwrap_or(0),
                    }))
                }
            }
            // `models.get` subsumes the emptiness check and hands the payload straight to `Apply`,
            // so there is no state in which this returns `Apply` with nothing to commit.
            KeyCode::Enter if !*fetching => match models.get(*selected) {
                Some(model) => ModalTransition::Apply(ModalApply::Provider {
                    provider: provider.clone(),
                    model: model.clone(),
                }),
                None => ModalTransition::Step(Modal::Connect(step.clone())),
            },
            KeyCode::Up | KeyCode::Down => {
                let delta = if key.code == KeyCode::Up { -1 } else { 1 };
                ModalTransition::Step(Modal::Connect(ConnectStep::ModelList {
                    rows: rows.clone(),
                    provider: provider.clone(),
                    models: models.clone(),
                    selected: cycle_index(*selected, models.len(), delta),
                    fetching: *fetching,
                    error: error.clone(),
                    from_key: *from_key,
                }))
            }
            _ => ModalTransition::Step(Modal::Connect(step.clone())),
        },
    }
}

/// Fetch a provider's model list off the UI loop. `key_override` is a just-typed key that wins
/// over the stored one; otherwise the key is resolved from the environment or the keyring. The key
/// is consumed here and never returned to the caller.
pub(crate) async fn fetch_model_list(
    provider: &str,
    key_override: Option<String>,
    store: &dyn CredentialStore,
    locale: Locale,
) -> Result<Vec<String>, String> {
    // `{:#}` keeps anyhow's source chain — `to_string` reports only the outermost message, which
    // hides the actual cause (connection refused, DNS failure, TLS error, 401, ...).
    if provider == "ollama" {
        return list_ollama_models().await.map_err(|e| format!("{e:#}"));
    }
    let key = match key_override {
        Some(k) => Some(k),
        None => crate::selection::resolve_key(provider, store),
    };
    match key {
        Some(k) => list_models(provider, &k)
            .await
            .map_err(|e| format!("{e:#}")),
        None => Err(i18n::t_with(
            locale,
            "connect.no_key",
            &[("provider", provider)],
        )),
    }
}

/// Pure step-transition for the models modal: maps a key press in the current step to the next
/// step, apply, or close. No network/keyring/terminal state.
fn models_step_next(step: &ModelsStep, key: KeyEvent) -> ModalTransition {
    match step {
        ModelsStep::Offline => match key.code {
            KeyCode::Esc | KeyCode::Enter => ModalTransition::Close,
            _ => ModalTransition::Step(Modal::Models(step.clone())),
        },
        ModelsStep::ModelList {
            provider,
            models,
            selected,
            fetching,
        } => {
            if *fetching || models.is_empty() {
                match key.code {
                    KeyCode::Esc => ModalTransition::Close,
                    _ => ModalTransition::Step(Modal::Models(step.clone())),
                }
            } else {
                match key.code {
                    KeyCode::Esc => ModalTransition::Close,
                    KeyCode::Enter => match models.get(*selected) {
                        Some(model) => ModalTransition::Apply(ModalApply::Model {
                            provider: provider.clone(),
                            model: model.clone(),
                        }),
                        None => ModalTransition::Step(Modal::Models(step.clone())),
                    },
                    KeyCode::Up | KeyCode::Down => {
                        let delta = if key.code == KeyCode::Up { -1 } else { 1 };
                        ModalTransition::Step(Modal::Models(ModelsStep::ModelList {
                            provider: provider.clone(),
                            models: models.clone(),
                            selected: cycle_index(*selected, models.len(), delta),
                            fetching: false,
                        }))
                    }
                    _ => ModalTransition::Step(Modal::Models(step.clone())),
                }
            }
        }
        ModelsStep::Manual {
            provider,
            input,
            error,
        } => match key.code {
            KeyCode::Esc => ModalTransition::Close,
            KeyCode::Enter if !input.trim().is_empty() => {
                ModalTransition::Apply(ModalApply::Model {
                    provider: provider.clone(),
                    model: input.trim().to_string(),
                })
            }
            KeyCode::Backspace => {
                let mut next = input.clone();
                next.pop();
                ModalTransition::Step(Modal::Models(ModelsStep::Manual {
                    provider: provider.clone(),
                    input: next,
                    error: error.clone(),
                }))
            }
            KeyCode::Char(c) => ModalTransition::Step(Modal::Models(ModelsStep::Manual {
                provider: provider.clone(),
                input: format!("{input}{c}"),
                error: error.clone(),
            })),
            _ => ModalTransition::Step(Modal::Models(step.clone())),
        },
    }
}
/// Assemble the help modal body for `locale`: section headers followed by their indented entries,
/// with a blank line between sections. Pure and unit-testable (no ratatui types).
fn help_lines(locale: Locale) -> Vec<String> {
    const SECTIONS: &[(&str, &[&str])] = &[
        (
            "help.section.global",
            &["help.global.help", "help.global.quit"],
        ),
        (
            "help.section.forms",
            &[
                "help.forms.navigate",
                "help.forms.submit",
                "help.forms.command",
                "help.forms.back",
            ],
        ),
        (
            "help.section.connected",
            &[
                "help.connected.ping",
                "help.connected.signout",
                "help.connected.engine",
                "help.connected.quit",
            ],
        ),
        (
            "help.section.engine",
            &[
                "help.engine.send",
                "help.engine.back",
                "help.engine.approve",
            ],
        ),
        (
            "help.section.commands",
            &[
                "help.commands.ask",
                "help.commands.connect",
                "help.commands.model",
                "help.commands.models",
                "help.commands.key",
                "help.commands.auth",
                "help.commands.lang",
            ],
        ),
    ];

    let mut lines = Vec::new();
    for (section, entries) in SECTIONS {
        lines.push(i18n::t(locale, section).to_string());
        for entry in *entries {
            lines.push(format!("  {}", i18n::t(locale, entry)));
        }
        lines.push(String::new());
    }
    lines
}
/// The slice of `App` a modal needs to render itself, so a view can be built from `&App` without
/// borrowing the terminal.
pub(crate) struct ModalContext<'a> {
    pub(crate) locale: Locale,
    /// The app-level error line, rendered inside the connect key-entry step.
    pub(crate) error: Option<&'a str>,
    /// Why the active provider is offline, rendered by the models modal's offline step.
    pub(crate) offline: Option<&'a OfflineReason>,
}

/// A modal rendered as a centered popup over the screen underneath it.
pub(crate) struct PopupView {
    pub(crate) title: String,
    pub(crate) body: Vec<Line<'static>>,
    pub(crate) footer: String,
    /// A `body` row that must stay on screen; the body scrolls to keep it visible.
    pub(crate) focus: Option<usize>,
}

/// A modal rendered as a full-area pane, replacing the screen underneath it.
pub(crate) struct FullScreenView {
    pub(crate) title: String,
    pub(crate) body: Vec<Line<'static>>,
}

pub(crate) enum ModalView {
    Popup(PopupView),
    FullScreen(FullScreenView),
}

impl ModalView {
    /// Whether this modal replaces the screen underneath it, or floats over it.
    ///
    /// Read off the view rather than stated separately per modal, because it is a property of how
    /// the modal is *painted*, not of which modal it is: [`draw_full_screen`] deliberately does not
    /// `Clear`, so a modal that claimed to cover the base while rendering a `Popup` would paint
    /// over a screen nobody drew.
    pub(crate) fn covers_base(&self) -> bool {
        matches!(self, ModalView::FullScreen(_))
    }
}

impl Modal {
    /// This modal's rendering, decoupled from the frame. The one place a modal's body, footer and
    /// focus are decided; [`draw_modal`] is the one place they are painted.
    pub(crate) fn view(&self, ctx: &ModalContext<'_>) -> ModalView {
        match self {
            Modal::Help => ModalView::FullScreen(FullScreenView {
                title: i18n::t(ctx.locale, "title.help").to_string(),
                body: help_lines(ctx.locale).into_iter().map(Line::from).collect(),
            }),
            Modal::Connect(step) => ModalView::Popup(connect_view(step, ctx)),
            Modal::Models(step) => ModalView::Popup(models_view(step, ctx)),
        }
    }
}

/// The connect modal's rendering.
fn connect_view(step: &ConnectStep, ctx: &ModalContext<'_>) -> PopupView {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let title: String;
    match step {
        ConnectStep::ProviderList { rows, selected } => {
            title = i18n::t(ctx.locale, "connect.title").to_string();
            for (i, row) in rows.iter().enumerate() {
                let marker = if i == *selected { "> " } else { "  " };
                let style = if i == *selected {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                };
                let suffix = if row.connected {
                    format!(" ({})", i18n::t(ctx.locale, "connect.connected"))
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
            title = i18n::t(ctx.locale, "connect.key_heading").to_string();
            lines.push(Line::from(i18n::t_with(
                ctx.locale,
                "status.key_enter",
                &[("provider", provider)],
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                mask(input),
                Style::default().add_modifier(Modifier::REVERSED),
            )));
            if let Some(err) = ctx.error {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    err.to_string(),
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
            title = i18n::t_with(
                ctx.locale,
                "connect.models_heading",
                &[("provider", provider)],
            );
            if *fetching {
                lines.push(Line::from(Span::styled(
                    i18n::t(ctx.locale, "connect.fetching"),
                    Style::default().fg(Color::DarkGray),
                )));
            } else if let Some(err) = error {
                lines.push(Line::from(Span::styled(
                    err.clone(),
                    Style::default().fg(Color::Red),
                )));
            } else if models.is_empty() {
                lines.push(Line::from(Span::styled(
                    i18n::t(ctx.locale, "connect.no_models"),
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
        ConnectStep::ProviderList { .. } => i18n::t(ctx.locale, "connect.footer_list"),
        ConnectStep::KeyEntry { .. } => i18n::t(ctx.locale, "connect.footer_key"),
        ConnectStep::ModelList { fetching, .. } => {
            if *fetching {
                i18n::t(ctx.locale, "connect.footer_fetching")
            } else {
                i18n::t(ctx.locale, "connect.footer_models")
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
    PopupView {
        title,
        body: lines,
        footer: footer.to_string(),
        focus,
    }
}

/// The models modal's rendering.
fn models_view(step: &ModelsStep, ctx: &ModalContext<'_>) -> PopupView {
    let title = i18n::t(ctx.locale, "models.title").to_string();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let footer: &str;
    match step {
        ModelsStep::Offline => {
            if let Some(reason) = &ctx.offline {
                lines.push(Line::from(Span::styled(
                    crate::provider::offline_notice(ctx.locale, reason),
                    Style::default().fg(Color::Yellow),
                )));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                i18n::t(ctx.locale, "models.offline"),
                Style::default().fg(Color::DarkGray),
            )));
            footer = i18n::t(ctx.locale, "models.footer_offline");
        }
        ModelsStep::ModelList {
            models,
            selected,
            fetching,
            ..
        } => {
            if *fetching {
                lines.push(Line::from(Span::styled(
                    i18n::t(ctx.locale, "connect.fetching"),
                    Style::default().fg(Color::DarkGray),
                )));
                footer = i18n::t(ctx.locale, "connect.footer_fetching");
            } else if models.is_empty() {
                lines.push(Line::from(Span::styled(
                    i18n::t(ctx.locale, "connect.no_models"),
                    Style::default().fg(Color::DarkGray),
                )));
                // Enter is a no-op with nothing to select, so don't advertise it.
                footer = i18n::t(ctx.locale, "models.footer_offline");
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
                footer = i18n::t(ctx.locale, "models.footer_list");
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
                i18n::t(ctx.locale, "models.manual"),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                input.clone(),
                Style::default().add_modifier(Modifier::REVERSED),
            )));
            footer = i18n::t(ctx.locale, "models.footer_manual");
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
    PopupView {
        title,
        body: lines,
        footer: footer.to_string(),
        focus,
    }
}

/// Paint a modal. The single seam every modal's rendering passes through.
pub(crate) fn draw_modal(frame: &mut Frame, area: Rect, view: ModalView) {
    match view {
        ModalView::Popup(view) => draw_popup(frame, area, view),
        ModalView::FullScreen(view) => draw_full_screen(frame, area, view),
    }
}

/// Render a full-area modal pane. Unlike [`draw_popup`] this does not clear behind itself, because
/// the caller does not draw the screen underneath a modal that covers it.
fn draw_full_screen(frame: &mut Frame, area: Rect, view: FullScreenView) {
    let pane = centered_rect(80, 90, area);
    let paragraph = Paragraph::new(view.body)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", view.title)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, pane);
}

/// Render a centered, bordered popup, clearing what is underneath. The footer is pinned to the
/// bottom so it stays visible, and `view.focus` names a body row that must remain on screen — the
/// body scrolls to keep it visible when the list is taller than the terminal.
///
/// The one place a popup's geometry is decided, reached from the one call site in [`draw_modal`].
fn draw_popup(frame: &mut Frame, area: Rect, view: PopupView) {
    let PopupView {
        title,
        body,
        footer,
        focus,
    } = view;
    let footer = Line::from(Span::styled(footer, Style::default().fg(Color::DarkGray)));
    // Borders take two rows and the pinned footer one.
    const CHROME: u16 = 3;
    let available = area.height.saturating_sub(2);
    // Clamp in `usize` first: a remote-supplied list long enough to overflow `u16` must not wrap.
    let wanted = body.len().saturating_add(CHROME as usize);
    let height = u16::try_from(wanted).unwrap_or(u16::MAX).min(available);
    let width = 60u16.min(area.width.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let body_height = inner.height.saturating_sub(1);
    if body_height > 0 {
        // Scroll just far enough to bring the focused row into view.
        let offset = match focus {
            Some(row) => (row as u16).saturating_sub(body_height.saturating_sub(1)),
            None => 0,
        };
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .scroll((offset, 0)),
            Rect {
                height: body_height,
                ..inner
            },
        );
    }
    frame.render_widget(
        Paragraph::new(footer),
        Rect {
            y: inner.y + body_height,
            height: 1,
            ..inner
        },
    );
}

/// Center a rectangle of `percent_x` by `percent_y` within `area` (a standard ratatui modal
/// helper).
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use light_factory_tui::i18n::Locale;

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn row(id: &str, connected: bool) -> ProviderRow {
        ProviderRow {
            id: id.to_string(),
            connected,
        }
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
    fn model_apply(provider: &str, model: &str) -> ModalTransition {
        ModalTransition::Apply(ModalApply::Model {
            provider: provider.to_string(),
            model: model.to_string(),
        })
    }

    /// A minimal render context: no app-level error, no offline reason.
    fn ctx() -> ModalContext<'static> {
        ModalContext {
            locale: Locale::En,
            error: None,
            offline: None,
        }
    }

    #[test]
    fn cycle_index_wraps_at_both_ends() {
        assert_eq!(cycle_index(0, 3, -1), 2);
        assert_eq!(cycle_index(2, 3, 1), 0);
        assert_eq!(cycle_index(1, 3, 1), 2);
        assert_eq!(cycle_index(0, 0, 1), 0);
    }

    #[test]
    fn connect_provider_enter_routes_by_connection_state() {
        let rows = vec![
            row("openai", false),
            row("ollama", true),
            row("gemini", true),
        ];
        let step = ConnectStep::ProviderList {
            rows: rows.clone(),
            selected: 0,
        };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Connect(ConnectStep::KeyEntry { provider, .. })) if provider == "openai"
        ));
        let step = ConnectStep::ProviderList {
            rows: rows.clone(),
            selected: 1,
        };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ModelList {
                provider,
                fetching: true,
                ..
            })) if provider == "ollama"
        ));
        let step = ConnectStep::ProviderList { rows, selected: 2 };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ModelList {
                provider,
                fetching: true,
                from_key: false,
                ..
            })) if provider == "gemini"
        ));
    }

    #[test]
    fn connect_ollama_skips_the_key_step_even_when_unconnected() {
        let rows = vec![row("ollama", false)];
        let step = ConnectStep::ProviderList { rows, selected: 0 };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ModelList { provider, .. })) if provider == "ollama"
        ));
    }

    #[test]
    fn connect_esc_closes_from_provider_list() {
        let step = ConnectStep::ProviderList {
            rows: vec![row("openai", false)],
            selected: 0,
        };
        assert_eq!(
            connect_step_next(&step, key(KeyCode::Esc)),
            ModalTransition::Close
        );
    }

    #[test]
    fn connect_key_entry_enter_blank_stays_and_esc_returns_to_list() {
        let rows = vec![row("openai", false)];
        let step = ConnectStep::KeyEntry {
            rows: rows.clone(),
            provider: "openai".to_string(),
            input: String::new(),
        };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Connect(ConnectStep::KeyEntry { input, .. })) if input.is_empty()
        ));
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Esc)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ProviderList {
                selected: 0,
                ..
            }))
        ));
    }

    #[test]
    fn connect_key_entry_enter_with_key_fetches_models() {
        let rows = vec![row("openai", false)];
        let step = ConnectStep::KeyEntry {
            rows,
            provider: "openai".to_string(),
            input: "sk-x".to_string(),
        };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ModelList {
                provider,
                fetching: true,
                from_key: true,
                ..
            })) if provider == "openai"
        ));
    }

    #[test]
    fn connect_model_list_enter_selects_and_esc_routes_back() {
        let step = ConnectStep::ModelList {
            rows: vec![row("openai", false)],
            provider: "openai".to_string(),
            models: vec!["gpt-4o".to_string()],
            selected: 0,
            fetching: false,
            error: None,
            from_key: true,
        };
        // Enter on a usable list is `Apply`, not `Close`: the modal no longer has to be
        // re-inspected after the fact to discover it had something to commit.
        assert_eq!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Apply(ModalApply::Provider {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
            })
        );

        let step = ConnectStep::ModelList {
            rows: vec![row("openai", false)],
            provider: "openai".to_string(),
            models: vec![],
            selected: 0,
            fetching: false,
            error: Some("bad key".to_string()),
            from_key: false,
        };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Esc)),
            ModalTransition::Step(Modal::Connect(ConnectStep::KeyEntry { provider, .. })) if provider == "openai"
        ));

        let step = ConnectStep::ModelList {
            rows: vec![row("ollama", false)],
            provider: "ollama".to_string(),
            models: vec![],
            selected: 0,
            fetching: false,
            error: Some("unreachable".to_string()),
            from_key: false,
        };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Esc)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ProviderList { .. }))
        ));
    }

    #[test]
    fn connect_model_list_enter_is_a_noop_while_fetching() {
        let step = ConnectStep::ModelList {
            rows: vec![row("openai", false)],
            provider: "openai".to_string(),
            models: vec![],
            selected: 0,
            fetching: true,
            error: None,
            from_key: false,
        };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ModelList {
                fetching: true,
                ..
            }))
        ));
    }

    #[test]
    fn connect_up_down_wrap_the_provider_selection() {
        let rows = vec![row("a", false), row("b", false), row("c", false)];
        let step = ConnectStep::ProviderList { rows, selected: 0 };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Up)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ProviderList {
                selected: 2,
                ..
            }))
        ));
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Down)),
            ModalTransition::Step(Modal::Connect(ConnectStep::ProviderList {
                selected: 1,
                ..
            }))
        ));
    }

    #[test]
    fn models_offline_closes_on_esc_and_enter_and_ignores_typing() {
        let step = ModelsStep::Offline;
        assert_eq!(
            models_step_next(&step, key(KeyCode::Esc)),
            ModalTransition::Close
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Close
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Char('x'))),
            ModalTransition::Step(Modal::Models(ModelsStep::Offline))
        );
    }

    #[test]
    fn models_while_fetching_cancels_on_esc_and_ignores_enter() {
        let step = models_list_step(vec![], true);
        assert_eq!(
            models_step_next(&step, key(KeyCode::Esc)),
            ModalTransition::Close
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Models(step.clone()))
        );
    }

    #[test]
    fn models_empty_list_enter_is_a_noop_and_esc_closes() {
        let step = models_list_step(vec![], false);
        assert_eq!(
            models_step_next(&step, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Models(step.clone()))
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Esc)),
            ModalTransition::Close
        );
    }

    #[test]
    fn models_list_enter_applies_esc_closes_and_arrows_wrap() {
        let step = models_list_step(
            vec![
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
                "o3".to_string(),
            ],
            false,
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Enter)),
            model_apply("openai", "gpt-4o")
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Esc)),
            ModalTransition::Close
        );
        assert!(matches!(
            models_step_next(&step, key(KeyCode::Up)),
            ModalTransition::Step(Modal::Models(ModelsStep::ModelList { selected: 2, .. }))
        ));
        assert!(matches!(
            models_step_next(&step, key(KeyCode::Down)),
            ModalTransition::Step(Modal::Models(ModelsStep::ModelList { selected: 1, .. }))
        ));
    }

    #[test]
    fn models_manual_edits_input_and_applies_only_a_non_blank_id() {
        let blank = models_manual_step("   ");
        assert_eq!(
            models_step_next(&blank, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Models(blank.clone()))
        );
        assert_eq!(
            models_step_next(&blank, key(KeyCode::Esc)),
            ModalTransition::Close
        );

        let typed = models_step_next(&models_manual_step("gpt-4"), key(KeyCode::Char('o')));
        assert!(matches!(
            &typed,
            ModalTransition::Step(Modal::Models(ModelsStep::Manual { input, .. })) if input == "gpt-4o"
        ));

        let popped = models_step_next(&models_manual_step("gpt-4o"), key(KeyCode::Backspace));
        assert!(matches!(
            &popped,
            ModalTransition::Step(Modal::Models(ModelsStep::Manual { input, .. })) if input == "gpt-4"
        ));

        assert_eq!(
            models_step_next(&models_manual_step("gpt-4o"), key(KeyCode::Enter)),
            model_apply("openai", "gpt-4o")
        );
    }

    /// Was `models_apply_target_reads_the_highlighted_or_typed_id`; the target is no longer a
    /// separate function, so the same cases are asserted on the payload `Enter` carries.
    #[test]
    fn models_enter_applies_the_highlighted_or_typed_id_and_nothing_else() {
        assert_eq!(
            models_step_next(
                &models_list_step(vec!["gpt-4o".to_string(), "o3".to_string()], false),
                key(KeyCode::Enter)
            ),
            model_apply("openai", "gpt-4o")
        );
        let fetching = models_list_step(vec!["gpt-4o".to_string()], true);
        assert_eq!(
            models_step_next(&fetching, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Models(fetching.clone())),
            "a list still loading has nothing to commit"
        );
        let empty = models_list_step(vec![], false);
        assert_eq!(
            models_step_next(&empty, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Models(empty.clone())),
            "an empty list has nothing to commit"
        );
        assert_eq!(
            models_step_next(&models_manual_step("  o3-mini  "), key(KeyCode::Enter)),
            model_apply("openai", "o3-mini"),
            "a manual id is trimmed before it is committed"
        );
        let blank = models_manual_step("   ");
        assert_eq!(
            models_step_next(&blank, key(KeyCode::Enter)),
            ModalTransition::Step(Modal::Models(blank.clone())),
            "a blank manual entry has nothing to commit"
        );
        assert_eq!(
            models_step_next(&ModelsStep::Offline, key(KeyCode::Enter)),
            ModalTransition::Close,
            "the offline step commits nothing at all"
        );
    }

    #[test]
    fn connect_step_debug_redacts_the_key() {
        let step = ConnectStep::KeyEntry {
            rows: vec![],
            provider: "openai".to_string(),
            input: "sk-super-secret".to_string(),
        };
        let rendered = format!("{step:?}");
        assert!(!rendered.contains("sk-super-secret"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn help_lines_resolve_without_raw_key_fallback() {
        let lines = help_lines(Locale::En);
        assert!(!lines.is_empty(), "help body must not be empty");
        for line in &lines {
            assert!(
                !line.contains("help."),
                "untranslated key leaked into help body: {line:?}"
            );
        }
    }

    #[test]
    fn help_lists_the_models_command() {
        let lines = help_lines(Locale::En);
        assert!(lines.iter().any(|l| l.contains("/models")));
    }

    #[test]
    fn help_lines_localize() {
        let en = help_lines(Locale::En);
        let es = help_lines(Locale::Es);
        assert_eq!(
            en.len(),
            es.len(),
            "EN and ES help bodies must have equal length"
        );
        assert_ne!(en, es, "EN and ES help bodies must differ");
    }

    #[test]
    fn opening_a_modal_replaces_whatever_was_open_and_invalidates_its_fetch() {
        let mut host = ModalHost::default();
        host.open(Modal::Connect(ConnectStep::ProviderList {
            rows: vec![row("anthropic", true)],
            selected: 0,
        }));
        let first = host.nonce();
        host.open(Modal::Models(models_list_step(vec!["a".into()], true)));
        assert!(
            host.nonce() > first,
            "opening must invalidate the outgoing modal's fetch"
        );
        assert!(matches!(host.current(), Some(Modal::Models(_))));
    }

    #[test]
    fn closing_bumps_the_nonce_and_clears_the_modal() {
        let mut host = ModalHost::default();
        host.open(Modal::Models(models_list_step(vec![], true)));
        let before = host.nonce();
        host.close();
        assert!(host.current().is_none());
        assert!(host.nonce() > before);
    }

    #[test]
    fn a_step_transition_does_not_invalidate_its_own_fetch() {
        let mut host = ModalHost::default();
        host.open(Modal::Models(models_list_step(vec![], true)));
        let before = host.nonce();
        // Same provider, still fetching: the cursor moved, the request did not change.
        host.replace_step(Modal::Models(ModelsStep::ModelList {
            provider: "openai".to_string(),
            models: vec![],
            selected: 3,
            fetching: true,
        }));
        assert_eq!(
            host.nonce(),
            before,
            "stepping must not discard the result being awaited"
        );
    }

    /// The counterpart of the above, and the reason `replace_step` is not an unconditional skip:
    /// Esc out of a loading list abandons the request, so its result must not be accepted if it
    /// lands afterwards.
    #[test]
    fn a_step_that_walks_away_from_a_fetch_invalidates_it() {
        let mut host = ModalHost::default();
        host.open(Modal::Connect(ConnectStep::ModelList {
            rows: vec![row("openai", true)],
            provider: "openai".to_string(),
            models: vec![],
            selected: 0,
            fetching: true,
            error: None,
            from_key: false,
        }));
        let awaiting = host.nonce();
        host.replace_step(Modal::Connect(ConnectStep::ProviderList {
            rows: vec![row("openai", true)],
            selected: 0,
        }));
        assert!(
            host.nonce() > awaiting,
            "stepping back out of a fetching list must discard its result"
        );

        // So does switching which provider is being awaited.
        let switched = host.nonce();
        host.replace_step(Modal::Connect(ConnectStep::ModelList {
            rows: vec![row("openai", true)],
            provider: "gemini".to_string(),
            models: vec![],
            selected: 0,
            fetching: true,
            error: None,
            from_key: false,
        }));
        assert!(
            host.nonce() > switched,
            "a fetch for another provider must not fill this list"
        );
    }

    /// `close` bumps even with nothing open: a fetch outlives the modal that started it (`Apply`
    /// closes the list first), so modal state is not evidence about fetch state.
    #[test]
    fn closing_invalidates_a_fetch_that_outlived_its_modal() {
        let mut host = ModalHost::default();
        host.open(Modal::Models(models_list_step(vec![], true)));
        let in_flight = host.next_fetch_nonce();
        host.close();
        let after_apply = host.nonce();
        assert!(after_apply > in_flight);
        host.close();
        assert!(
            host.nonce() > after_apply,
            "closing with nothing open must still invalidate"
        );
    }

    /// Merging the two per-modal counters into one is sound only because no value is ever reused.
    #[test]
    fn the_fetch_nonce_never_repeats_a_value() {
        let mut host = ModalHost::default();
        let mut seen = vec![host.nonce()];
        host.open(Modal::Help);
        seen.push(host.nonce());
        seen.push(host.next_fetch_nonce());
        host.replace_step(Modal::Models(models_list_step(vec![], true)));
        seen.push(host.nonce());
        seen.push(host.next_fetch_nonce());
        host.close();
        seen.push(host.nonce());
        let mut sorted = seen.clone();
        sorted.dedup();
        assert_eq!(sorted, seen, "a repeated nonce would alias two fetches");
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "must be monotonic");
    }

    #[test]
    fn connect_enter_on_a_model_list_applies_instead_of_closing() {
        let step = ConnectStep::ModelList {
            rows: vec![row("openai", true)],
            provider: "openai".into(),
            models: vec!["gpt-5".into(), "gpt-5-mini".into()],
            selected: 1,
            fetching: false,
            error: None,
            from_key: false,
        };
        assert_eq!(
            Modal::Connect(step).next(key(KeyCode::Enter)),
            ModalTransition::Apply(ModalApply::Provider {
                provider: "openai".into(),
                model: "gpt-5-mini".into()
            }),
            "/connect adopts a provider as well as pinning its model"
        );
    }

    #[test]
    fn connect_esc_from_the_provider_list_closes_without_applying() {
        let modal = Modal::Connect(ConnectStep::ProviderList {
            rows: vec![row("openai", true)],
            selected: 0,
        });
        assert!(matches!(
            modal.next(key(KeyCode::Esc)),
            ModalTransition::Close
        ));
    }

    #[test]
    fn models_enter_applies_the_active_provider_not_a_new_preference() {
        let modal = Modal::Models(models_list_step(vec!["m1".into()], false));
        assert_eq!(
            modal.next(key(KeyCode::Enter)),
            ModalTransition::Apply(ModalApply::Model {
                provider: "openai".into(),
                model: "m1".into()
            }),
            "/models re-pins a model and must never adopt a provider"
        );
    }

    #[test]
    fn only_a_fetching_list_names_a_fetch_target() {
        assert_eq!(
            Modal::Models(models_list_step(vec![], true)).fetch_target(),
            Some(("openai", FetchSink::Models)),
            "the models modal names its own sink, not the connect one"
        );
        assert_eq!(
            Modal::Models(models_list_step(vec!["m".into()], false)).fetch_target(),
            None
        );
        assert_eq!(
            Modal::Connect(ConnectStep::ProviderList {
                rows: vec![],
                selected: 0
            })
            .fetch_target(),
            None
        );
        assert_eq!(
            Modal::Connect(ConnectStep::KeyEntry {
                rows: vec![],
                provider: "openai".into(),
                input: String::new(),
            })
            .fetch_target(),
            None
        );
        assert_eq!(
            Modal::Connect(ConnectStep::ModelList {
                rows: vec![],
                provider: "openai".into(),
                models: vec![],
                selected: 0,
                fetching: true,
                error: None,
                from_key: false,
            })
            .fetch_target(),
            Some(("openai", FetchSink::Connect)),
            "the connect modal names its own sink, not the models one"
        );
        assert_eq!(Modal::Models(models_manual_step("m")).fetch_target(), None);
        assert_eq!(Modal::Models(ModelsStep::Offline).fetch_target(), None);
    }

    #[test]
    fn help_closes_on_esc_and_ctrl_p_and_ignores_other_keys() {
        assert!(matches!(
            Modal::Help.next(key(KeyCode::Esc)),
            ModalTransition::Close
        ));
        assert!(matches!(
            Modal::Help.next(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
            ModalTransition::Close
        ));
        assert!(matches!(
            Modal::Help.next(key(KeyCode::Char('x'))),
            ModalTransition::Step(Modal::Help)
        ));
        assert_eq!(Modal::Help.fetch_target(), None);
    }

    #[test]
    fn only_help_hides_the_screen_underneath_it() {
        assert!(Modal::Help.view(&ctx()).covers_base());
        assert!(
            !Modal::Models(models_list_step(vec![], true))
                .view(&ctx())
                .covers_base()
        );
        assert!(
            !Modal::Connect(ConnectStep::ProviderList {
                rows: vec![],
                selected: 0
            })
            .view(&ctx())
            .covers_base()
        );
    }

    /// The status line is help's alone to take over: a popup floats, so the screen underneath keeps
    /// saying what it was saying.
    #[test]
    fn only_help_overrides_the_status_hint() {
        assert_eq!(Modal::Help.hint_key(), Some("hint.help_close"));
        assert_eq!(Modal::Models(ModelsStep::Offline).hint_key(), None);
        assert_eq!(
            Modal::Connect(ConnectStep::ProviderList {
                rows: vec![],
                selected: 0
            })
            .hint_key(),
            None
        );
    }

    #[test]
    fn mask_never_echoes_input() {
        assert_eq!(mask(""), "");
        assert_eq!(mask("abc"), "***");
        assert_eq!(mask("sk-secret"), "*********");
    }
}
