//! The modal overlays layered over the TUI's screens: `/connect`, `/models`, and help.
//!
//! Each modal is a state enum plus a pure key-transition function, so the whole state machine is
//! testable without a terminal, a keyring, or the network. `App` owns exactly one of them at a
//! time.

use crossterm::event::{KeyCode, KeyEvent};
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

/// The result of stepping the connect modal: advance to a new [`ConnectStep`], or close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectTransition {
    Step(ConnectStep),
    Close,
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

/// The result of stepping the models modal: advance to a new [`ModelsStep`], apply the
/// selection, or close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelsTransition {
    Step(ModelsStep),
    Close,
    Apply,
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
pub(crate) fn connect_step_next(step: &ConnectStep, key: KeyEvent) -> ConnectTransition {
    match step {
        ConnectStep::ProviderList { rows, selected } => match key.code {
            KeyCode::Esc => ConnectTransition::Close,
            KeyCode::Up => ConnectTransition::Step(ConnectStep::ProviderList {
                rows: rows.clone(),
                selected: cycle_index(*selected, rows.len(), -1),
            }),
            KeyCode::Down => ConnectTransition::Step(ConnectStep::ProviderList {
                rows: rows.clone(),
                selected: cycle_index(*selected, rows.len(), 1),
            }),
            KeyCode::Enter => match rows.get(*selected) {
                Some(row) if row.connected || row.id == "ollama" => {
                    ConnectTransition::Step(ConnectStep::ModelList {
                        rows: rows.clone(),
                        provider: row.id.clone(),
                        models: Vec::new(),
                        selected: 0,
                        fetching: true,
                        error: None,
                        from_key: false,
                    })
                }
                Some(row) => ConnectTransition::Step(ConnectStep::KeyEntry {
                    rows: rows.clone(),
                    provider: row.id.clone(),
                    input: String::new(),
                }),
                None => ConnectTransition::Step(step.clone()),
            },
            _ => ConnectTransition::Step(step.clone()),
        },
        ConnectStep::KeyEntry {
            rows,
            provider,
            input,
        } => match key.code {
            KeyCode::Esc => ConnectTransition::Step(ConnectStep::ProviderList {
                rows: rows.clone(),
                selected: rows.iter().position(|r| r.id == *provider).unwrap_or(0),
            }),
            KeyCode::Enter if !input.trim().is_empty() => {
                ConnectTransition::Step(ConnectStep::ModelList {
                    rows: rows.clone(),
                    provider: provider.clone(),
                    models: Vec::new(),
                    selected: 0,
                    fetching: true,
                    error: None,
                    from_key: true,
                })
            }
            KeyCode::Backspace => {
                let mut next = input.clone();
                next.pop();
                ConnectTransition::Step(ConnectStep::KeyEntry {
                    rows: rows.clone(),
                    provider: provider.clone(),
                    input: next,
                })
            }
            KeyCode::Char(c) => ConnectTransition::Step(ConnectStep::KeyEntry {
                rows: rows.clone(),
                provider: provider.clone(),
                input: format!("{input}{c}"),
            }),
            _ => ConnectTransition::Step(step.clone()),
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
                    ConnectTransition::Step(ConnectStep::KeyEntry {
                        rows: rows.clone(),
                        provider: provider.clone(),
                        input: String::new(),
                    })
                } else {
                    ConnectTransition::Step(ConnectStep::ProviderList {
                        rows: rows.clone(),
                        selected: rows.iter().position(|r| r.id == *provider).unwrap_or(0),
                    })
                }
            }
            KeyCode::Enter if !*fetching && !models.is_empty() => ConnectTransition::Close,
            KeyCode::Up | KeyCode::Down => {
                let delta = if key.code == KeyCode::Up { -1 } else { 1 };
                ConnectTransition::Step(ConnectStep::ModelList {
                    rows: rows.clone(),
                    provider: provider.clone(),
                    models: models.clone(),
                    selected: cycle_index(*selected, models.len(), delta),
                    fetching: *fetching,
                    error: error.clone(),
                    from_key: *from_key,
                })
            }
            _ => ConnectTransition::Step(step.clone()),
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
pub(crate) fn models_step_next(step: &ModelsStep, key: KeyEvent) -> ModelsTransition {
    match step {
        ModelsStep::Offline => match key.code {
            KeyCode::Esc | KeyCode::Enter => ModelsTransition::Close,
            _ => ModelsTransition::Step(step.clone()),
        },
        ModelsStep::ModelList {
            provider,
            models,
            selected,
            fetching,
        } => {
            if *fetching || models.is_empty() {
                match key.code {
                    KeyCode::Esc => ModelsTransition::Close,
                    _ => ModelsTransition::Step(step.clone()),
                }
            } else {
                match key.code {
                    KeyCode::Esc => ModelsTransition::Close,
                    KeyCode::Enter => ModelsTransition::Apply,
                    KeyCode::Up | KeyCode::Down => {
                        let delta = if key.code == KeyCode::Up { -1 } else { 1 };
                        ModelsTransition::Step(ModelsStep::ModelList {
                            provider: provider.clone(),
                            models: models.clone(),
                            selected: cycle_index(*selected, models.len(), delta),
                            fetching: false,
                        })
                    }
                    _ => ModelsTransition::Step(step.clone()),
                }
            }
        }
        ModelsStep::Manual {
            provider,
            input,
            error,
        } => match key.code {
            KeyCode::Esc => ModelsTransition::Close,
            KeyCode::Enter if !input.trim().is_empty() => ModelsTransition::Apply,
            KeyCode::Backspace => {
                let mut next = input.clone();
                next.pop();
                ModelsTransition::Step(ModelsStep::Manual {
                    provider: provider.clone(),
                    input: next,
                    error: error.clone(),
                })
            }
            KeyCode::Char(c) => ModelsTransition::Step(ModelsStep::Manual {
                provider: provider.clone(),
                input: format!("{input}{c}"),
                error: error.clone(),
            }),
            _ => ModelsTransition::Step(step.clone()),
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
            ConnectTransition::Step(ConnectStep::KeyEntry { provider, .. }) if provider == "openai"
        ));
        let step = ConnectStep::ProviderList {
            rows: rows.clone(),
            selected: 1,
        };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ConnectTransition::Step(ConnectStep::ModelList {
                provider,
                fetching: true,
                ..
            }) if provider == "ollama"
        ));
        let step = ConnectStep::ProviderList { rows, selected: 2 };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ConnectTransition::Step(ConnectStep::ModelList {
                provider,
                fetching: true,
                from_key: false,
                ..
            }) if provider == "gemini"
        ));
    }

    #[test]
    fn connect_ollama_skips_the_key_step_even_when_unconnected() {
        let rows = vec![row("ollama", false)];
        let step = ConnectStep::ProviderList { rows, selected: 0 };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ConnectTransition::Step(ConnectStep::ModelList { provider, .. }) if provider == "ollama"
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
            ConnectTransition::Close
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
            ConnectTransition::Step(ConnectStep::KeyEntry { input, .. }) if input.is_empty()
        ));
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Esc)),
            ConnectTransition::Step(ConnectStep::ProviderList { selected: 0, .. })
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
            ConnectTransition::Step(ConnectStep::ModelList {
                provider,
                fetching: true,
                from_key: true,
                ..
            }) if provider == "openai"
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
        assert_eq!(
            connect_step_next(&step, key(KeyCode::Enter)),
            ConnectTransition::Close
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
            ConnectTransition::Step(ConnectStep::KeyEntry { provider, .. }) if provider == "openai"
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
            ConnectTransition::Step(ConnectStep::ProviderList { .. })
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
            ConnectTransition::Step(ConnectStep::ModelList { fetching: true, .. })
        ));
    }

    #[test]
    fn connect_up_down_wrap_the_provider_selection() {
        let rows = vec![row("a", false), row("b", false), row("c", false)];
        let step = ConnectStep::ProviderList { rows, selected: 0 };
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Up)),
            ConnectTransition::Step(ConnectStep::ProviderList { selected: 2, .. })
        ));
        assert!(matches!(
            connect_step_next(&step, key(KeyCode::Down)),
            ConnectTransition::Step(ConnectStep::ProviderList { selected: 1, .. })
        ));
    }

    #[test]
    fn models_offline_closes_on_esc_and_enter_and_ignores_typing() {
        let step = ModelsStep::Offline;
        assert_eq!(
            models_step_next(&step, key(KeyCode::Esc)),
            ModelsTransition::Close
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Enter)),
            ModelsTransition::Close
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Char('x'))),
            ModelsTransition::Step(ModelsStep::Offline)
        );
    }

    #[test]
    fn models_while_fetching_cancels_on_esc_and_ignores_enter() {
        let step = models_list_step(vec![], true);
        assert_eq!(
            models_step_next(&step, key(KeyCode::Esc)),
            ModelsTransition::Close
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Enter)),
            ModelsTransition::Step(step.clone())
        );
    }

    #[test]
    fn models_empty_list_enter_is_a_noop_and_esc_closes() {
        let step = models_list_step(vec![], false);
        assert_eq!(
            models_step_next(&step, key(KeyCode::Enter)),
            ModelsTransition::Step(step.clone())
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Esc)),
            ModelsTransition::Close
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
            ModelsTransition::Apply
        );
        assert_eq!(
            models_step_next(&step, key(KeyCode::Esc)),
            ModelsTransition::Close
        );
        assert!(matches!(
            models_step_next(&step, key(KeyCode::Up)),
            ModelsTransition::Step(ModelsStep::ModelList { selected: 2, .. })
        ));
        assert!(matches!(
            models_step_next(&step, key(KeyCode::Down)),
            ModelsTransition::Step(ModelsStep::ModelList { selected: 1, .. })
        ));
    }

    #[test]
    fn models_manual_edits_input_and_applies_only_a_non_blank_id() {
        let blank = models_manual_step("   ");
        assert_eq!(
            models_step_next(&blank, key(KeyCode::Enter)),
            ModelsTransition::Step(blank.clone())
        );
        assert_eq!(
            models_step_next(&blank, key(KeyCode::Esc)),
            ModelsTransition::Close
        );

        let typed = models_step_next(&models_manual_step("gpt-4"), key(KeyCode::Char('o')));
        assert!(matches!(
            &typed,
            ModelsTransition::Step(ModelsStep::Manual { input, .. }) if input == "gpt-4o"
        ));

        let popped = models_step_next(&models_manual_step("gpt-4o"), key(KeyCode::Backspace));
        assert!(matches!(
            &popped,
            ModelsTransition::Step(ModelsStep::Manual { input, .. }) if input == "gpt-4"
        ));

        assert_eq!(
            models_step_next(&models_manual_step("gpt-4o"), key(KeyCode::Enter)),
            ModelsTransition::Apply
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
}
