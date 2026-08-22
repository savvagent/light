//! The modal overlays layered over the TUI's screens: `/connect`, `/models`, and help.
//!
//! Each modal is a state enum plus a pure key-transition function, so the whole state machine is
//! testable without a terminal, a keyring, or the network. `App` owns exactly one of them at a
//! time.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use light_factory_providers::{list_models, list_ollama_models};
use light_factory_tui::credentials::CredentialStore;
use light_factory_tui::i18n::{self, Locale};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::takes_key;

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

/// The result of stepping any modal: advance to a new state, commit its selection, or close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModalTransition {
    Step(Modal),
    Apply,
    Close,
}

/// What a [`ModalTransition::Apply`] commits. The two modals apply different things: `/connect`
/// adopts a new preferred provider, `/models` only re-pins the model of the provider already
/// active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModalApply {
    /// `/connect`: adopt `provider` as the preferred provider and pin `model` to it.
    Provider { provider: String, model: String },
    /// `/models`: pin `model` to the already-active `provider`.
    Model { provider: String, model: String },
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

    /// What [`ModalTransition::Apply`] should commit, or `None` when this state carries no usable
    /// selection (still fetching, an empty list, or a blank manual entry).
    pub(crate) fn apply_target(&self) -> Option<ModalApply> {
        match self {
            Modal::Connect(ConnectStep::ModelList {
                provider,
                models,
                selected,
                fetching: false,
                ..
            }) => models.get(*selected).map(|model| ModalApply::Provider {
                provider: provider.clone(),
                model: model.clone(),
            }),
            Modal::Help | Modal::Connect(_) => None,
            Modal::Models(step) => models_apply_target(step)
                .map(|(provider, model)| ModalApply::Model { provider, model }),
        }
    }

    /// The provider whose model list this state is waiting on, or `None`. Comparing this across a
    /// transition is what starts a fetch exactly once, on the step that begins waiting.
    pub(crate) fn fetch_target(&self) -> Option<&str> {
        match self {
            Modal::Connect(ConnectStep::ModelList {
                provider,
                fetching: true,
                ..
            })
            | Modal::Models(ModelsStep::ModelList {
                provider,
                fetching: true,
                ..
            }) => Some(provider.as_str()),
            _ => None,
        }
    }

    /// Whether this modal replaces the screen underneath it, or floats over it. Help renders a
    /// full-area pane and has never drawn over a screen; the popup modals always have.
    pub(crate) fn covers_base(&self) -> bool {
        matches!(self, Modal::Help)
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
/// The nonce outlives the modal on purpose: closing bumps it so a fetch already in flight is
/// discarded when it lands. Keeping it here — rather than as a second `App` field beside an
/// `Option<Modal>` — is what makes "closing invalidates the fetch" impossible to forget at a call
/// site.
#[derive(Default)]
pub(crate) struct ModalHost {
    open: Option<Modal>,
    nonce: u64,
}

impl ModalHost {
    /// Open `modal`, replacing whatever was open and invalidating its in-flight fetch.
    pub(crate) fn open(&mut self, modal: Modal) {
        self.nonce += 1;
        self.open = Some(modal);
    }

    /// Close whatever is open and invalidate its in-flight fetch.
    pub(crate) fn close(&mut self) {
        if self.open.take().is_some() {
            self.nonce += 1;
        }
    }

    /// Swap the open modal's state without invalidating the fetch it is awaiting.
    pub(crate) fn replace_step(&mut self, modal: Modal) {
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

    pub(crate) fn covers_base(&self) -> bool {
        self.open.as_ref().is_some_and(Modal::covers_base)
    }

    /// Claim a nonce for a newly-spawned fetch, invalidating any earlier one.
    pub(crate) fn next_fetch_nonce(&mut self) -> u64 {
        self.nonce += 1;
        self.nonce
    }
}

/// Move a list selection up (`-1`) or down (`+1`), wrapping at the ends.
pub(crate) fn cycle_index(current: usize, len: usize, delta: isize) -> usize {
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
pub(crate) fn connect_step_next(step: &ConnectStep, key: KeyEvent) -> ModalTransition {
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
            KeyCode::Enter if !*fetching && !models.is_empty() => ModalTransition::Apply,
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

/// The `(provider, model)` pair a models-modal step would persist, or `None` when the step
/// carries no usable selection (still fetching, an empty list, or a blank manual entry).
pub(crate) fn models_apply_target(step: &ModelsStep) -> Option<(String, String)> {
    match step {
        ModelsStep::ModelList {
            provider,
            models,
            selected,
            fetching: false,
        } => models
            .get(*selected)
            .map(|model| (provider.clone(), model.clone())),
        ModelsStep::Manual {
            provider, input, ..
        } => {
            let id = input.trim();
            if id.is_empty() {
                None
            } else {
                Some((provider.clone(), id.to_string()))
            }
        }
        _ => None,
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
pub(crate) fn models_step_next(step: &ModelsStep, key: KeyEvent) -> ModalTransition {
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
                    KeyCode::Enter => ModalTransition::Apply,
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
            KeyCode::Enter if !input.trim().is_empty() => ModalTransition::Apply,
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
pub(crate) fn help_lines(locale: Locale) -> Vec<String> {
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
/// Render a centered, bordered popup titled `title`, clearing what is underneath. `footer` is
/// pinned to the bottom so it stays visible, and `focus` names a `body` row that must remain on
/// screen — the body scrolls to keep it visible when the list is taller than the terminal.
pub(crate) fn draw_popup(
    frame: &mut Frame,
    area: Rect,
    title: String,
    body: Vec<Line>,
    footer: Line,
    focus: Option<usize>,
) {
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
pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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
            ModalTransition::Apply
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
            ModalTransition::Apply
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
            ModalTransition::Apply
        );
    }

    #[test]
    fn models_apply_target_reads_the_highlighted_or_typed_id() {
        assert_eq!(
            models_apply_target(&models_list_step(
                vec!["gpt-4o".to_string(), "o3".to_string()],
                false
            )),
            Some(("openai".to_string(), "gpt-4o".to_string()))
        );
        assert_eq!(
            models_apply_target(&models_list_step(vec!["gpt-4o".to_string()], true)),
            None
        );
        assert_eq!(models_apply_target(&models_list_step(vec![], false)), None);
        assert_eq!(
            models_apply_target(&models_manual_step("  o3-mini  ")),
            Some(("openai".to_string(), "o3-mini".to_string()))
        );
        assert_eq!(models_apply_target(&models_manual_step("   ")), None);
        assert_eq!(models_apply_target(&ModelsStep::Offline), None);
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
        host.replace_step(Modal::Models(models_list_step(vec!["a".into()], false)));
        assert_eq!(
            host.nonce(),
            before,
            "stepping must not discard the result being awaited"
        );
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
        assert!(matches!(
            Modal::Connect(step.clone()).next(key(KeyCode::Enter)),
            ModalTransition::Apply
        ));
        assert_eq!(
            Modal::Connect(step).apply_target(),
            Some(ModalApply::Provider {
                provider: "openai".into(),
                model: "gpt-5-mini".into()
            })
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
        assert_eq!(modal.apply_target(), None);
    }

    #[test]
    fn models_enter_applies_the_active_provider_not_a_new_preference() {
        let modal = Modal::Models(models_list_step(vec!["m1".into()], false));
        assert!(matches!(
            modal.next(key(KeyCode::Enter)),
            ModalTransition::Apply
        ));
        assert_eq!(
            modal.apply_target(),
            Some(ModalApply::Model {
                provider: "openai".into(),
                model: "m1".into()
            })
        );
    }

    #[test]
    fn only_a_fetching_list_names_a_fetch_target() {
        assert_eq!(
            Modal::Models(models_list_step(vec![], true)).fetch_target(),
            Some("openai")
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
            Some("openai")
        );
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
        assert_eq!(Modal::Help.apply_target(), None);
        assert_eq!(Modal::Help.fetch_target(), None);
    }

    #[test]
    fn only_help_hides_the_screen_underneath_it() {
        assert!(Modal::Help.covers_base());
        assert!(!Modal::Models(models_list_step(vec![], true)).covers_base());
        assert!(
            !Modal::Connect(ConnectStep::ProviderList {
                rows: vec![],
                selected: 0
            })
            .covers_base()
        );
        assert!(!ModalHost::default().covers_base());
    }
}
