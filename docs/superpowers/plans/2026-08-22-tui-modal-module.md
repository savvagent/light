# TUI Modal Module — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the help/connect/models modal machinery out of `crates/tui/src/app.rs` into a new
`crates/tui/src/modal.rs`, behind a single `App` field `modal: ModalHost` that makes "two modals open
at once" unrepresentable — deleting seven `App` fields, the `Mode::Help` variant, four close paths,
two fetch-start functions, two `UiEvent` variants and two transition enums, with **no change to modal
behaviour**.

**Architecture:** One `Modal` enum (`Help` / `Connect(ConnectStep)` / `Models(ModelsStep)`) wrapped
in a `ModalHost { open: Option<Modal>, nonce: u64 }` that owns the stale-fetch counter, so
"closing bumps the nonce" cannot be forgotten at a call site. One `ModalTransition`
(`Step`/`Apply`/`Close`) — the `/models` shape carried forward to `/connect`, which today re-inspects
its step after the fact to decide whether to apply. Four seams in `App` replace the hand-ordered
cascades: `handle_modal_key`, `open_modal`, `apply_and_close_modal`, `draw_modal`.

**Tech Stack:** Rust (edition 2024, toolchain pinned by `rust-toolchain.toml`); existing
`ratatui`/`crossterm`; no new dependencies; no new i18n keys.

**Spec:** `docs/superpowers/specs/2026-08-22-tui-modal-module-design.md` — read it first. This plan
implements it exactly.

## Global Constraints

- **This is a structural refactor. No behaviour change.** Same keys, same strings, same transitions,
  same rendered output. The only two sanctioned deltas are (a) the single-modal invariant becomes
  enforced by construction and (b) `connect_return`/`models_return`/`help_return` are deleted as
  provably inert (spec §7). Anything else that changes behaviour is a defect.
- **Tests are the regression net.** All 63 tests currently in `app.rs`'s `#[cfg(test)] mod tests`
  must still exist and pass at the end. Tests covering moved code move into `modal.rs`'s own test
  module. **Renaming is allowed; deletion is not.** No test's *assertions* may be weakened, and the
  four rendering tests (`models_modal_renders_its_own_header`,
  `a_long_model_list_keeps_the_selection_and_footer_on_screen`,
  `an_empty_list_does_not_advertise_enter`, `the_offline_modal_names_the_actual_reason`) must pass
  with **no expectation edits**.
- No comments unless they explain *why*. **No AI / `Co-Authored-By` / "Generated with" attribution**
  in commits, PR bodies, code comments, or docs.
- Inward dependency flow: every change is inside `crates/tui` (a client leaf). `protocol`, `auth`,
  `persistence`, `server`, `providers`, `engine`, `web/` are untouched. `cargo build/test
  --workspace` must never require node.
- Secrets: the hand-written redacting `Debug for ConnectStep` moves **verbatim**; `mask` still masks;
  `fetch_model_list` still consumes the API key and never returns it; no key reaches a status/error
  string or a log.
- No i18n change. Every `t()`/`t_with()` key is carried verbatim, so `en`/`es` parity in
  `crates/tui/src/i18n.rs` is preserved by construction. If you find yourself adding a key, stop —
  that is a behaviour change.
- Semver (Non-Negotiable Rule 6): no public interface change. `crates/tui/src/lib.rs` (`credentials`,
  `engine_view`, `i18n`) is untouched; `UiEvent` is `pub` inside a private module of the **binary**
  crate and is not a library API. **No `Cargo.toml` version bump.**
- Run `cargo fmt --all` before every Rust commit. Lint gate:
  `cargo clippy --workspace --all-targets -- -D warnings`.
- Tests are offline-deterministic: no live network, no keyring (`MemStore` / `FailingStore`), no real
  terminal (`ratatui::backend::TestBackend`), and each `test_app()` gets its own temp settings path.
- Commit format `<scope>: <subject>` — `tui:` for code, `docs:` for the spec/plan.
- **Every task ends green.** `cargo test -p light-factory-tui` and
  `cargo clippy -p light-factory-tui --all-targets -- -D warnings` pass at each commit. A commit that
  leaves the crate red is not acceptable, because this branch must be replayable across a rebase onto
  #44 and #47.

## File Structure

| File | Responsibility |
|---|---|
| **Create.** `crates/tui/src/modal.rs` | All modal machinery: `ProviderRow`, `ConnectStep` (+ redacting `Debug`), `ModelsStep`, `Modal`, `ModalHost`, `ModalTransition`, `ModalApply`, `ModalView`/`PopupView`/`FullScreenView`/`ModalContext`, `connect_step_next`, `models_step_next`, `models_apply_target`, `help_lines`, `fetch_model_list`, `cycle_index`, `draw_popup`, `draw_full_screen`, `centered_rect`, plus the moved tests for all of it |
| **Modify.** `crates/tui/src/main.rs` | add `mod modal;` |
| **Modify.** `crates/tui/src/app.rs` | delete the moved items; `App.modal: ModalHost` replaces seven fields; `Mode::Help` deleted; `handle_modal_key` / `open_modal` / `apply_and_close_modal` / `draw_modal` / `begin_model_fetch` / merged `handle_models_fetched` replace the per-modal copies; `mask` + `takes_key` become `pub(crate)`; one merged `UiEvent::ModelsFetched` arm in `run` |

No other file changes. In particular: **no** `crates/tui/src/i18n.rs` change, **no** `Cargo.toml`
change, **no** `crates/providers` change, **no** out-of-band surface (`Dockerfile`, `fly.toml`,
`web/`, `crates/persistence/migrations/`, `.github/`) is touched — Phase 5 out-of-band verification
is vacuously satisfied and should be stated as such.

## Task Order & Rationale

Five tasks, each independently green. The order is chosen so that at no point does `app.rs` reference
a type that no longer exists, and so that a rebase onto #44/#47 can be replayed commit by commit.

1. **Task 1 — move the pure machinery.** Create `modal.rs` and move the state types, pure transition
   functions, `help_lines`, `fetch_model_list`, `cycle_index` and the popup renderer, *keeping their
   current signatures and their current `ConnectTransition`/`ModelsTransition` return types*.
   `app.rs` re-imports them. Pure code motion: the moved tests move with it and must pass unchanged.
   Doing this first means every later task edits a small module instead of a 3700-line one.
2. **Task 2 — introduce `Modal` + `ModalHost` and migrate connect and models onto it.** The two
   `Option<…>` fields and two nonces become one `modal: ModalHost`; `ConnectTransition`/
   `ModelsTransition` collapse into `ModalTransition` (connect gains the `Apply` arm); the two
   `begin_*_fetch` and two `UiEvent` variants merge; `connect_return`/`models_return` are deleted.
   Connect and models must move together: they share the nonce, the fetch, the `UiEvent` and the
   `handle_key` cascade, so splitting them would leave a half-migrated cascade that cannot compile.
3. **Task 3 — migrate help.** `Mode::Help` and `help_return` are deleted, `Modal::Help` is added,
   and `covers_base()` keeps help's opaque rendering. Separated from Task 2 because it touches a
   different set of call sites (`Mode` match arms, the footer hint) and can be reviewed on its own.
4. **Task 4 — unify the draw tail.** `draw_connect`/`draw_models`/`draw_help` become
   `Modal::view` + one `draw_modal` seam. Last, because it is the only task whose regression net is
   the rendering tests, and it is easiest to bisect when it is alone in a commit.
5. **Task 5 — verify and record.** Line/field/test counts, the full workspace gate, and the plan
   checkboxes.

---

### Task 1: Move the pure modal machinery into `crates/tui/src/modal.rs`

**Files:**
- Create: `crates/tui/src/modal.rs`
- Modify: `crates/tui/src/main.rs` (add `mod modal;`)
- Modify: `crates/tui/src/app.rs` (delete the moved items, import them; `pub(crate)` on `mask` and
  `takes_key`)
- Test: `crates/tui/src/modal.rs` (`#[cfg(test)] mod tests`), `crates/tui/src/app.rs` (remaining tests)

**Interfaces:**
- Consumes: `crate::selection::{resolve_key, REMOTE_IDS}`, `crate::provider::offline_notice`,
  `light_factory_providers::{list_models, list_ollama_models}`,
  `light_factory_tui::credentials::CredentialStore`, `light_factory_tui::i18n::{self, Locale}`,
  and (from `app.rs`) `pub(crate) fn mask(&str) -> String`, `pub(crate) fn takes_key(&str) -> bool`.
- Produces, all `pub(crate)` in `crate::modal`:
  `struct ProviderRow { id: String, connected: bool }`;
  `enum ConnectStep { ProviderList { rows, selected }, KeyEntry { rows, provider, input },
   ModelList { rows, provider, models, selected, fetching, error, from_key } }`;
  `enum ConnectTransition { Step(ConnectStep), Close }`;
  `enum ModelsStep { ModelList { provider, models, selected, fetching }, Manual { provider, input,
   error }, Offline }`;
  `enum ModelsTransition { Step(ModelsStep), Close, Apply }`;
  `fn connect_step_next(&ConnectStep, KeyEvent) -> ConnectTransition`;
  `fn models_step_next(&ModelsStep, KeyEvent) -> ModelsTransition`;
  `fn models_apply_target(&ModelsStep) -> Option<(String, String)>`;
  `fn help_lines(Locale) -> Vec<String>`;
  `async fn fetch_model_list(&str, Option<String>, &dyn CredentialStore, Locale) -> Result<Vec<String>, String>`;
  `fn cycle_index(usize, usize, isize) -> usize`;
  `fn draw_popup(&mut Frame, Rect, String, Vec<Line>, Line, Option<usize>)`;
  `fn centered_rect(u16, u16, Rect) -> Rect`.
  (Signatures are unchanged from `app.rs` — this task is code motion only. They change in Tasks 2–4.)

- [ ] **Step 1: Create the module and wire it in**

Create `crates/tui/src/modal.rs` with the module doc comment:

```rust
//! The modal overlays layered over the TUI's screens: `/connect`, `/models`, and help.
//!
//! Each modal is a state enum plus a pure key-transition function, so the whole state machine is
//! testable without a terminal, a keyring, or the network. `App` owns exactly one of them at a
//! time.
```

Add `mod modal;` to `crates/tui/src/main.rs`, in alphabetical position between `mod config;` and
`mod provider;`.

- [ ] **Step 2: Move the types and pure functions verbatim**

Cut these from `crates/tui/src/app.rs` and paste them into `crates/tui/src/modal.rs`, adding
`pub(crate)` to each item **and to every field of `ProviderRow`, `ConnectStep` and `ModelsStep`**
(they are read from `app.rs`). Do not otherwise alter a single line of their bodies — including the
doc comments and the hand-written `impl std::fmt::Debug for ConnectStep`:

| Item | Current location in `app.rs` |
|---|---|
| `ProviderRow` | 86 |
| `ConnectStep` | 94 |
| `impl std::fmt::Debug for ConnectStep` | 115 |
| `ConnectTransition` | 158 |
| `ModelsStep` | 165 |
| `ModelsTransition` | 183 |
| `cycle_index` | 2202 |
| `connect_step_next` | 2219 |
| `models_apply_target` | 2331 |
| `fetch_model_list` | 2358 |
| `models_step_next` | 2387 |
| `help_lines` | 2489 |
| `draw_popup` | 2549 |
| `centered_rect` | 2610 |

Add to `modal.rs` the imports those bodies need:

```rust
use crossterm::event::{KeyCode, KeyEvent};
use light_factory_providers::{list_models, list_ollama_models};
use light_factory_tui::credentials::CredentialStore;
use light_factory_tui::i18n::{self, Locale};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::takes_key;
```

- [ ] **Step 3: Fix the two cross-module helpers**

In `crates/tui/src/app.rs`, change `fn mask(` to `pub(crate) fn mask(` and `fn takes_key(` to
`pub(crate) fn takes_key(` (they stay in `app.rs`: `mask` is also used by `draw_key`, and
`takes_key` by `begin_key_entry`/`clear_key`). In `app.rs`, add the import of everything that moved:

```rust
use crate::modal::{
    ConnectStep, ConnectTransition, ModelsStep, ModelsTransition, ProviderRow, centered_rect,
    connect_step_next, cycle_index, draw_popup, fetch_model_list, help_lines, models_apply_target,
    models_step_next,
};
```

Delete the now-unused `use light_factory_providers::{CompleteRequest, Provider, list_models,
list_ollama_models};` entries that moved (`list_models`, `list_ollama_models`) — keep
`CompleteRequest` and `Provider`. Likewise drop any `ratatui::widgets` import in `app.rs` that is now
only used by `modal.rs` (`Clear`), and keep the rest.

- [ ] **Step 4: Move the tests that cover the moved code**

Create `#[cfg(test)] mod tests` at the bottom of `crates/tui/src/modal.rs` and **move** these tests
from `app.rs`'s test module into it, unchanged apart from their `use super::…` line. They are pure
(no `App`), so they move cleanly:

`cycle_index_wraps_at_both_ends`, `connect_provider_enter_routes_by_connection_state`,
`connect_ollama_skips_the_key_step_even_when_unconnected`, `connect_esc_closes_from_provider_list`,
`connect_key_entry_enter_blank_stays_and_esc_returns_to_list`,
`connect_key_entry_enter_with_key_fetches_models`,
`connect_model_list_enter_selects_and_esc_routes_back`,
`connect_model_list_enter_is_a_noop_while_fetching`, `connect_up_down_wrap_the_provider_selection`,
`models_offline_closes_on_esc_and_enter_and_ignores_typing`,
`models_while_fetching_cancels_on_esc_and_ignores_enter`,
`models_empty_list_enter_is_a_noop_and_esc_closes`,
`models_list_enter_applies_esc_closes_and_arrows_wrap`,
`models_manual_edits_input_and_applies_only_a_non_blank_id`,
`models_apply_target_reads_the_highlighted_or_typed_id`, `connect_step_debug_redacts_the_key`,
`help_lines_resolve_without_raw_key_fallback`, `help_lines_localize`.

Move the helpers they use with them — `key(code)`, `row(id, connected)`, `model_list_step`,
`models_list_step`, `models_manual_step` — and delete from `app.rs`'s test module any of those
helpers that no longer has a caller there. `modal.rs`'s test module needs:

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use light_factory_tui::i18n::Locale;

use super::*;
```

Leave in `app.rs` every test that constructs an `App` (`handle_connect_models_*`,
`handle_models_fetched_*`, `models_*_persists_*`, the render tests, the help-modal routing tests,
the command parsers, `mask_never_echoes_input`, and the engine/ask tests) — they still belong there.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p light-factory-tui`
Expected: PASS, **86 tests** in the `light-factory` bin target — the same count as before the move
(63 of them from `app.rs`, now split across `app.rs` and `modal.rs`).

Run: `cargo clippy -p light-factory-tui --all-targets -- -D warnings`
Expected: clean. If clippy reports an unused import in `app.rs`, delete that import (do not
`#[allow]` it).

- [ ] **Step 6: Verify the move changed nothing**

Run: `git diff --stat`
Expected: `crates/tui/src/app.rs` shows a large deletion, `crates/tui/src/modal.rs` a matching
addition, `crates/tui/src/main.rs` one line. Then:

Run: `git diff -- crates/tui/src/app.rs | grep '^+' | grep -v '^+++' | grep -vE 'use crate::modal|pub\(crate\) fn (mask|takes_key)'`
Expected: **no output**. This task adds nothing to `app.rs` except the import and the two
`pub(crate)` markers; anything else is a stray edit and must be reverted.

- [ ] **Step 7: Format and commit**

```bash
cargo fmt --all
git add crates/tui/src/modal.rs crates/tui/src/app.rs crates/tui/src/main.rs
git commit -m "tui: move the modal state machines into a modal module"
```

---

### Task 2: Collapse connect + models onto `Modal` / `ModalHost`

**Files:**
- Modify: `crates/tui/src/modal.rs`
- Modify: `crates/tui/src/app.rs`
- Test: both files' `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: Task 1's `ConnectStep`, `ModelsStep`, `connect_step_next`, `models_step_next`,
  `models_apply_target`, `fetch_model_list`.
- Produces, all `pub(crate)` in `crate::modal`:

```rust
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Modal {
    Connect(ConnectStep),
    Models(ModelsStep),
}

#[derive(Debug)]
pub(crate) enum ModalTransition {
    Step(Modal),
    Apply,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModalApply {
    Provider { provider: String, model: String },
    Model { provider: String, model: String },
}

#[derive(Default)]
pub(crate) struct ModalHost { /* open: Option<Modal>, nonce: u64 */ }

impl Modal {
    pub(crate) fn next(&self, key: KeyEvent) -> ModalTransition;
    pub(crate) fn apply_target(&self) -> Option<ModalApply>;
    pub(crate) fn fetch_target(&self) -> Option<&str>;
}

impl ModalHost {
    pub(crate) fn open(&mut self, modal: Modal);
    pub(crate) fn close(&mut self);
    pub(crate) fn replace_step(&mut self, modal: Modal);
    pub(crate) fn current(&self) -> Option<&Modal>;
    pub(crate) fn current_mut(&mut self) -> Option<&mut Modal>;
    pub(crate) fn is_open(&self) -> bool;
    pub(crate) fn nonce(&self) -> u64;
    pub(crate) fn next_fetch_nonce(&mut self) -> u64;
}
```
  and in `app.rs`: `App.modal: ModalHost`, `App::open_modal`, `App::handle_modal_key`,
  `App::apply_and_close_modal`, `App::begin_model_fetch`, merged `App::handle_models_fetched`,
  merged `UiEvent::ModelsFetched`.
- `ConnectTransition` and `ModelsTransition` are **deleted** in this task.

- [ ] **Step 1: Write the failing tests in `modal.rs`**

Append to `crates/tui/src/modal.rs`'s test module:

```rust
#[test]
fn opening_a_modal_replaces_whatever_was_open_and_invalidates_its_fetch() {
    let mut host = ModalHost::default();
    host.open(Modal::Connect(ConnectStep::ProviderList {
        rows: vec![row("anthropic", true)],
        selected: 0,
    }));
    let first = host.nonce();
    host.open(Modal::Models(models_list_step(vec!["a".into()], true)));
    assert!(host.nonce() > first, "opening must invalidate the outgoing modal's fetch");
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
    assert_eq!(host.nonce(), before, "stepping must not discard the result being awaited");
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
        Some(ModalApply::Provider { provider: "openai".into(), model: "gpt-5-mini".into() })
    );
}

#[test]
fn connect_esc_from_the_provider_list_closes_without_applying() {
    let modal = Modal::Connect(ConnectStep::ProviderList {
        rows: vec![row("openai", true)],
        selected: 0,
    });
    assert!(matches!(modal.next(key(KeyCode::Esc)), ModalTransition::Close));
    assert_eq!(modal.apply_target(), None);
}

#[test]
fn models_enter_applies_the_active_provider_not_a_new_preference() {
    let modal = Modal::Models(models_list_step(vec!["m1".into()], false));
    assert!(matches!(modal.next(key(KeyCode::Enter)), ModalTransition::Apply));
    assert_eq!(
        modal.apply_target(),
        Some(ModalApply::Model { provider: "openai".into(), model: "m1".into() })
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
        Modal::Connect(ConnectStep::ProviderList { rows: vec![], selected: 0 }).fetch_target(),
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
```

`models_list_step(models, fetching)` is the existing helper moved in Task 1; confirm it builds a
`ModelsStep::ModelList` with `provider: "openai"`. If it uses a different provider id, use that id
in the assertions above rather than editing the helper.

- [ ] **Step 2: Run them and watch them fail to compile**

Run: `cargo test -p light-factory-tui modal::`
Expected: FAIL — `cannot find type Modal`, `ModalHost`, `ModalTransition`, `ModalApply`.

- [ ] **Step 3: Implement `Modal` and `ModalHost` in `modal.rs`**

Add the types from the Interfaces block above, plus:

```rust
impl Modal {
    /// Pure: maps a key press in the current state to the next state, apply, or close.
    /// No keyring, terminal, or network state.
    pub(crate) fn next(&self, key: KeyEvent) -> ModalTransition {
        match self {
            Modal::Connect(step) => connect_step_next(step, key),
            Modal::Models(step) => models_step_next(step, key),
        }
    }

    /// What `ModalTransition::Apply` should commit, or `None` when this state carries no usable
    /// selection (still fetching, an empty list, or a blank manual entry).
    pub(crate) fn apply_target(&self) -> Option<ModalApply> {
        match self {
            Modal::Connect(ConnectStep::ModelList {
                provider, models, selected, fetching: false, ..
            }) => models.get(*selected).map(|model| ModalApply::Provider {
                provider: provider.clone(),
                model: model.clone(),
            }),
            Modal::Connect(_) => None,
            Modal::Models(step) => models_apply_target(step)
                .map(|(provider, model)| ModalApply::Model { provider, model }),
        }
    }

    /// The provider whose model list this state is waiting on, or `None`.
    pub(crate) fn fetch_target(&self) -> Option<&str> {
        match self {
            Modal::Connect(ConnectStep::ModelList { provider, fetching: true, .. })
            | Modal::Models(ModelsStep::ModelList { provider, fetching: true, .. }) => {
                Some(provider.as_str())
            }
            _ => None,
        }
    }
}
```

Change `connect_step_next` and `models_step_next` to return `ModalTransition`, then delete
`ConnectTransition` and `ModelsTransition`. The rewrite is mechanical:

- `ConnectTransition::Step(s)` → `ModalTransition::Step(Modal::Connect(s))`
- `ModelsTransition::Step(s)` → `ModalTransition::Step(Modal::Models(s))`
- `ModelsTransition::Apply` → `ModalTransition::Apply`, `ModelsTransition::Close` →
  `ModalTransition::Close`
- `ConnectTransition::Close` **splits** — this is the one semantic edit in the file, and the whole
  point of the issue's "carry forward the `Close`/`Apply`/`Step` shape from `/models`":
  - `ProviderList` + `KeyCode::Esc` → `ModalTransition::Close` (applies nothing, as today)
  - `ModelList` + `KeyCode::Enter if !*fetching && !models.is_empty()` → `ModalTransition::Apply`
    (today this returned `Close`, and `apply_and_close_connect` re-inspected the step to discover it
    should apply)

  **Every other arm of `connect_step_next` keeps its exact key mapping.** In particular the
  `ModelList` + `Esc` arm still routes back to `KeyEntry` when
  `!*fetching && takes_key(provider) && (*from_key || error.is_some())` and to `ProviderList`
  otherwise, and `KeyEntry` still appends `KeyCode::Char(c)` to `input`.

Update the moved transition tests in `modal.rs` to the new enum (`ConnectTransition::Step(x)` →
`ModalTransition::Step(Modal::Connect(x))`, etc.). Their **inputs and expected steps must not
change**; only the wrapper does. The one exception is
`connect_model_list_enter_selects_and_esc_routes_back`, whose `Enter` expectation becomes
`ModalTransition::Apply` — that is the sanctioned split, and the new test
`connect_enter_on_a_model_list_applies_instead_of_closing` above pins the equivalent outcome.

Now `ModalHost`:

```rust
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

    pub(crate) fn current(&self) -> Option<&Modal> { self.open.as_ref() }
    pub(crate) fn current_mut(&mut self) -> Option<&mut Modal> { self.open.as_mut() }
    pub(crate) fn is_open(&self) -> bool { self.open.is_some() }
    pub(crate) fn nonce(&self) -> u64 { self.nonce }

    /// Claim a nonce for a newly-spawned fetch, invalidating any earlier one.
    pub(crate) fn next_fetch_nonce(&mut self) -> u64 {
        self.nonce += 1;
        self.nonce
    }
}
```

- [ ] **Step 4: Run the `modal.rs` tests**

Run: `cargo test -p light-factory-tui modal::`
Expected: PASS for `modal.rs`'s module. `app.rs` will not compile yet — that is Step 5. If you
cannot get a clean run because `app.rs` is broken, proceed to Step 5 and run them together at
Step 6.

- [ ] **Step 5: Migrate `App` in `crates/tui/src/app.rs`**

a. **Fields.** Delete `connect`, `connect_return`, `connect_nonce`, `models`, `models_return`,
   `models_nonce` from the `App` struct and from `App::new`'s initializer. Add `modal: ModalHost` in
   their place, initialized `modal: ModalHost::default()`.

b. **`UiEvent`.** Delete the `ConnectModels { .. }` variant. Keep `ModelsFetched { nonce: u64,
   provider: String, result: Result<Vec<String>, String> }`. In `run`'s `match ev`, delete the
   `UiEvent::ConnectModels` arm; the surviving `UiEvent::ModelsFetched` arm still calls
   `app.handle_models_fetched(nonce, provider, result)`.

c. **Fetch.** Delete `begin_fetch` and `begin_models_fetch`; add:

```rust
/// Fetch a provider's model list off the UI loop for the open modal. The nonce claimed here is
/// what makes a result that outlives its modal discardable.
fn begin_model_fetch(&mut self, provider: String, key: Option<String>) {
    let nonce = self.modal.next_fetch_nonce();
    let events = self.events.clone();
    let store = self.store.clone();
    let lang = self.config.lang;
    tokio::spawn(async move {
        let result = fetch_model_list(&provider, key, store.as_ref(), lang).await;
        let _ = events.send(UiEvent::ModelsFetched { nonce, provider, result });
    });
}
```

d. **Open.** Delete `enter_connect`'s and `enter_models`' direct fetch calls and their
   `*_return` writes; add the shared opener and rewrite the two enters through it:

```rust
/// Open `modal`, replacing any other, and start its model-list fetch if it is waiting on one.
fn open_modal(&mut self, modal: Modal, key: Option<String>) {
    let target = modal.fetch_target().map(str::to_string);
    self.modal.open(modal);
    if let Some(provider) = target {
        self.begin_model_fetch(provider, key);
    }
}

fn enter_connect(&mut self) {
    let rows = self.build_provider_rows();
    self.open_modal(Modal::Connect(ConnectStep::ProviderList { rows, selected: 0 }), None);
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
```

e. **Close.** Delete `close_connect` and `close_models`. Rewrite `dismiss_modals` to delegate,
   keeping its doc comment (the *why* is not obvious at its two call sites):

```rust
/// Tear down any open modal and invalidate its in-flight fetch. Called when the session goes
/// away, so a modal cannot float over the sign-in screen or swallow its keys.
fn dismiss_modals(&mut self) {
    self.modal.close();
}
```

f. **Apply.** Delete `apply_and_close_connect` and `apply_and_close_models`; add:

```rust
fn apply_and_close_modal(&mut self) {
    let apply = self.modal.current().and_then(Modal::apply_target);
    self.modal.close();
    match apply {
        Some(ModalApply::Provider { provider, model }) => {
            let previous = self.settings.provider.replace(provider.clone());
            if !self.persist_model(provider, model) {
                self.settings.provider = previous;
            }
        }
        Some(ModalApply::Model { provider, model }) => {
            self.persist_model(provider, model);
        }
        None => {}
    }
}
```

g. **Key routing.** Delete `handle_connect_key` and `handle_models_key`; add:

```rust
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

    let before = current.fetch_target().map(str::to_string);
    let transition = current.next(key);

    // Storing the typed key needs `self.store`, so it cannot live in the pure transition.
    let mut fetch_key = None;
    if let (
        Modal::Connect(ConnectStep::KeyEntry { provider, input, .. }),
        ModalTransition::Step(Modal::Connect(ConnectStep::ModelList { from_key: true, .. })),
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
```

In `handle_key`, replace the two trailing cascade arms

```rust
if self.connect.is_some() { return self.handle_connect_key(key); }
if self.models.is_some() { return self.handle_models_key(key); }
```

with a single check moved to the **top** of the function, above the Ctrl-P arm:

```rust
if self.modal.is_open() {
    return self.handle_modal_key(key);
}
```

and drop `&& self.connect.is_none() && self.models.is_none()` from the Ctrl-P guard (dead by
construction now). Leave the `self.mode == Mode::Help` check where it is — Task 3 removes it.

h. **Fetch result.** Delete `handle_connect_models`; rewrite `handle_models_fetched` as the merged
   handler, keeping each modal's fill **exactly** as it is today (connect shows the error inline in
   its model-list step; models falls back to a manual entry step):

```rust
fn handle_models_fetched(
    &mut self,
    nonce: u64,
    provider: String,
    result: Result<Vec<String>, String>,
) {
    if nonce != self.modal.nonce() {
        return;
    }
    match self.modal.current() {
        Some(Modal::Connect(ConnectStep::ModelList { provider: p, fetching: true, .. }))
            if *p == provider => self.fill_connect_models(result),
        Some(Modal::Models(ModelsStep::ModelList { provider: p, fetching: true, .. }))
            if *p == provider => self.fill_models(provider, result),
        _ => {}
    }
}
```

`fill_connect_models` is the body of today's `handle_connect_models` from its `err_msg` line
onward; `fill_models` is the body of today's `handle_models_fetched` from its `match result`
onward. Both patch through `self.modal.current_mut()`. Copy them verbatim apart from the field
access.

i. **Draw.** For now, keep `draw_connect`/`draw_models` and change only their guard to read
   `self.modal.current()`:

```rust
if let Some(Modal::Connect(_)) = self.modal.current() {
    self.draw_connect(frame, chunks[1]);
}
if let Some(Modal::Models(_)) = self.modal.current() {
    self.draw_models(frame, chunks[1]);
}
```

Task 4 collapses these. Splitting it that way keeps this task's diff about state, not rendering.

- [ ] **Step 6: Update the `app.rs` tests to the new field**

The `app.rs` tests that reach into `app.connect` / `app.models` must be updated to
`app.modal.current()` / `app.modal.open(...)` — **assertions unchanged**. Affected:
`handle_connect_models_ignores_stale_nonces`, `handle_connect_models_fills_models_for_a_matching_nonce`,
`handle_connect_models_surfaces_a_fetch_error`, `models_fetch_result_is_ignored_when_the_provider_does_not_match`,
`models_fetch_result_does_not_clobber_manual_entry`, `an_empty_but_successful_fetch_stays_a_list`,
`models_command_opens_a_fetching_list_for_the_active_provider`,
`closing_the_modal_returns_to_the_mode_it_was_opened_from`,
`losing_the_session_dismisses_an_open_models_modal`, `a_failed_save_rolls_back_the_staged_model`,
`handle_models_fetched_*` (four), `models_enter_persists_the_highlighted_model_and_rebuilds`,
`models_manual_enter_persists_the_trimmed_id`, `models_apply_rebuilds_the_active_provider`,
`models_esc_closes_without_touching_settings`,
`models_blank_manual_enter_stays_open_without_touching_settings`,
`closing_the_models_modal_invalidates_an_in_flight_fetch`,
`models_command_requires_a_connected_session`,
`models_command_opens_the_modal_offline_when_no_provider_is_active`,
`handle_connect_key_blank_key_stays_on_key_entry`,
`handle_connect_key_keyring_failure_sets_error_and_stays`, and the render tests.

Tests that set up state via `app.connect = Some(step)` become
`app.modal.open(Modal::Connect(step))`; tests that assert `app.models.is_none()` become
`assert!(app.modal.current().is_none())`; tests that call `app.handle_models_key(k)` /
`app.handle_connect_key(k)` become `app.handle_modal_key(k)`.

**Two nonce-sensitive tests need care, and their assertions must not be weakened:**

- `closing_the_models_modal_invalidates_an_in_flight_fetch` — capture `app.modal.nonce()` before
  the close and assert a `handle_models_fetched` call carrying the old nonce is ignored, exactly
  as today.
- `handle_models_fetched_ignores_stale_nonces` / `handle_connect_models_ignores_stale_nonces` —
  drive the nonce through `app.modal` rather than the deleted per-modal counters.

Where a test previously set up a modal by hand *and* relied on the old counter starting at 0,
open the modal through `app.modal.open(...)` and read `app.modal.nonce()` back rather than
hardcoding a number.

Also add the two `App`-level tests that pin the sanctioned invariant and the new opener:

```rust
#[test]
fn opening_the_models_modal_replaces_an_open_connect_modal() {
    let mut app = test_app();
    app.modal.open(Modal::Connect(ConnectStep::ProviderList { rows: vec![], selected: 0 }));
    app.enter_models();
    assert!(
        matches!(app.modal.current(), Some(Modal::Models(_))),
        "only one modal can be open at a time"
    );
}

#[test]
fn opening_a_second_modal_invalidates_the_first_modals_fetch() {
    let mut app = test_app();
    app.enter_models();
    let stale = app.modal.nonce();
    app.enter_connect();
    app.handle_models_fetched(stale, "openai".to_string(), Ok(vec!["m".to_string()]));
    assert!(
        matches!(app.modal.current(), Some(Modal::Connect(ConnectStep::ProviderList { .. }))),
        "a fetch from the replaced modal must not reach the new one"
    );
}
```

- [ ] **Step 7: Run the full crate**

Run: `cargo test -p light-factory-tui`
Expected: PASS. Test count is **88** in the `light-factory` bin target (86 + the two new `App`
tests) plus the seven new `modal.rs` tests from Step 1 = **95**. If any count is *lower* than 86 + 9,
a test was lost — find it and restore it before continuing.

Run: `cargo clippy -p light-factory-tui --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Format and commit**

```bash
cargo fmt --all
git add crates/tui/src/modal.rs crates/tui/src/app.rs
git commit -m "tui: collapse the connect and models modals onto one modal host"
```

---

### Task 3: Migrate help onto `Modal` and delete `Mode::Help`

**Files:**
- Modify: `crates/tui/src/modal.rs`
- Modify: `crates/tui/src/app.rs`
- Test: both files' `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: Task 2's `Modal`, `ModalHost`, `ModalTransition`, `App::open_modal`,
  `App::handle_modal_key`.
- Produces: `Modal::Help` variant; `Modal::covers_base(&self) -> bool`;
  `ModalHost::covers_base(&self) -> bool`. Deletes `Mode::Help` and `App.help_return`,
  `App::open_help`'s `help_return` write, `App::close_help`, `App::handle_help_key`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/tui/src/modal.rs`'s test module:

```rust
#[test]
fn help_closes_on_esc_and_ctrl_p_and_ignores_other_keys() {
    assert!(matches!(Modal::Help.next(key(KeyCode::Esc)), ModalTransition::Close));
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
    assert!(!Modal::Connect(ConnectStep::ProviderList { rows: vec![], selected: 0 }).covers_base());
    assert!(!ModalHost::default().covers_base());
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p light-factory-tui modal::`
Expected: FAIL — `no variant named Help`, `no method named covers_base`.

- [ ] **Step 3: Implement in `modal.rs`**

Add `Help` as the first variant of `Modal`. Add to `Modal::next` a `Modal::Help => help_step_next(key)`
arm, with:

```rust
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
```

(`use crossterm::event::KeyModifiers;` joins the existing crossterm import.)

Add `Modal::Help => None` arms to `apply_target` (it already falls through `_ => None` in
`fetch_target`; make sure `apply_target`'s `Modal::Connect(_) => None` arm is joined by
`Modal::Help => None` rather than a catch-all, so a fourth modal cannot be silently swallowed).

Add:

```rust
impl Modal {
    /// Whether this modal replaces the screen underneath it (help) or floats over it
    /// (connect, models). Help renders a full-area pane and has never drawn over a screen.
    pub(crate) fn covers_base(&self) -> bool {
        matches!(self, Modal::Help)
    }
}

impl ModalHost {
    pub(crate) fn covers_base(&self) -> bool {
        self.open.as_ref().is_some_and(Modal::covers_base)
    }
}
```

- [ ] **Step 4: Migrate `App`**

In `crates/tui/src/app.rs`:

a. Delete the `Help` variant from `enum Mode`, and the `Mode::Help => {}` arm from `handle_key`'s
   `KeyCode::Esc` match.
b. Delete the `help_return` field and its `App::new` initializer.
c. Delete `close_help` and `handle_help_key`. Rewrite `open_help`:

```rust
fn open_help(&mut self) {
    self.open_modal(Modal::Help, None);
}
```

d. In `handle_key`, delete the leading `if self.mode == Mode::Help { return self.handle_help_key(key); }`
   — `if self.modal.is_open() { return self.handle_modal_key(key); }` from Task 2 now covers it and
   must be the first statement in the function.
e. In `draw`, delete the `Mode::Help => self.draw_help(frame, chunks[1]),` arm and guard the screen
   match:

```rust
// Help replaces the screen underneath it; connect and models float over it.
if !self.modal.covers_base() {
    match self.mode {
        Mode::SignIn => self.draw_signin(frame, chunks[1]),
        Mode::Register => self.draw_register(frame, chunks[1]),
        Mode::RegisterCode => self.draw_register_code(frame, chunks[1]),
        Mode::Device => self.draw_device(frame, chunks[1]),
        Mode::Connected => self.draw_connected(frame, chunks[1]),
        Mode::Engine => self.draw_engine(frame, chunks[1]),
        Mode::Key => self.draw_key(frame, chunks[1]),
    }
}
if matches!(self.modal.current(), Some(Modal::Help)) {
    self.draw_help(frame, chunks[1]);
}
```

   (Task 4 folds that last block into the unified seam.)
f. In `draw`'s footer hint, replace `} else if self.mode == Mode::Help {` with
   `} else if matches!(self.modal.current(), Some(Modal::Help)) {`, keeping the `hint.help_close`
   key and the branch's position between `command_mode` and `Mode::Device`.

- [ ] **Step 5: Update the three help tests in `app.rs`**

`help_modal_opens_and_restores_the_prior_mode`, `help_modal_returns_to_the_mode_it_was_opened_from`
and `esc_and_ctrl_p_close_help_but_ctrl_c_quits` currently assert on `app.mode == Mode::Help`.
Rewrite them to assert on the modal while **keeping every `app.mode` assertion they already make** —
those are the regression net for the `help_return` deletion (spec §4.5) and must not be dropped:

```rust
#[tokio::test]
async fn help_modal_opens_and_restores_the_prior_mode() {
    let mut app = test_app();
    app.mode = Mode::Connected;
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)).await;
    assert!(matches!(app.modal.current(), Some(Modal::Help)));
    // The base mode is never disturbed, so there is nothing to restore.
    assert_eq!(app.mode, Mode::Connected);
    app.handle_key(key(KeyCode::Esc)).await;
    assert!(app.modal.current().is_none());
    assert_eq!(app.mode, Mode::Connected);
}
```

Apply the same shape to the other two (the third must keep asserting that Ctrl-C returns `true` and
that Ctrl-P closes). If any of the three is currently a sync `#[test]`, keep it sync and drive it
through the same entry point it uses today.

- [ ] **Step 6: Run**

Run: `cargo test -p light-factory-tui`
Expected: PASS, with two more `modal.rs` tests than Task 2 left behind and **no** loss in `app.rs`.

Run: `cargo clippy -p light-factory-tui --all-targets -- -D warnings`
Expected: clean. `Mode` now has seven variants and every `match self.mode` must still be exhaustive
without a wildcard.

- [ ] **Step 7: Format and commit**

```bash
cargo fmt --all
git add crates/tui/src/modal.rs crates/tui/src/app.rs
git commit -m "tui: move the help overlay onto the modal host"
```

---

### Task 4: Unify the three draw tails behind one seam

**Files:**
- Modify: `crates/tui/src/modal.rs`
- Modify: `crates/tui/src/app.rs`
- Test: `crates/tui/src/app.rs` (the four existing rendering tests, unchanged)

**Interfaces:**
- Consumes: Task 3's `Modal`, `ModalHost::covers_base`, and Task 1's `draw_popup` / `centered_rect`.
- Produces, `pub(crate)` in `crate::modal`:

```rust
pub(crate) struct ModalContext<'a> {
    pub(crate) locale: Locale,
    pub(crate) error: Option<&'a str>,
    pub(crate) offline: Option<&'a OfflineReason>,
}

pub(crate) struct PopupView {
    pub(crate) title: String,
    pub(crate) body: Vec<Line<'static>>,
    pub(crate) footer: String,
    pub(crate) focus: Option<usize>,
}

pub(crate) struct FullScreenView {
    pub(crate) title: String,
    pub(crate) body: Vec<Line<'static>>,
}

pub(crate) enum ModalView {
    Popup(PopupView),
    FullScreen(FullScreenView),
}

impl Modal {
    pub(crate) fn view(&self, ctx: &ModalContext<'_>) -> ModalView;
}

pub(crate) fn draw_modal(frame: &mut Frame, area: Rect, view: ModalView);
```
  `draw_popup`'s signature changes to `draw_popup(&mut Frame, Rect, PopupView)`; `centered_rect`
  is unchanged. `App::draw_connect`, `App::draw_models` and `App::draw_help` are deleted.

- [ ] **Step 1: Move the view construction into `modal.rs`**

Add to `modal.rs`:

```rust
impl Modal {
    /// The rendering of this modal, decoupled from the frame so it can be built from `&App`
    /// without borrowing the terminal.
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
```

`connect_view` is the body of today's `App::draw_connect` with these mechanical substitutions,
**and no other change**: `self.t(k)` → `i18n::t(ctx.locale, k)`, `self.t_with(k, p)` →
`i18n::t_with(ctx.locale, k, p)`, `self.error` → `ctx.error` (a `&str`, so `err.clone()` becomes
`err.to_string()`), and the trailing `draw_popup(frame, area, title, lines, Line::from(...), focus)`
call replaced by `PopupView { title, body: lines, footer: footer.to_string(), focus }`. The
footer-selection `match` and the `focus` `match` move across verbatim.

`models_view` is the body of today's `App::draw_models` with the same substitutions, plus
`self.provider_info.offline` → `ctx.offline` and
`crate::provider::offline_notice(self.config.lang, reason)` →
`crate::provider::offline_notice(ctx.locale, reason)`.

The footer styling that both functions applied at the call site
(`Line::from(Span::styled(footer, Style::default().fg(Color::DarkGray)))`) moves into `draw_popup`,
so it is applied once instead of twice. Add to `modal.rs`'s imports:
`use light_factory_providers::OfflineReason;` and
`use ratatui::style::{Color, Modifier, Style};` and `use ratatui::text::Span;`.

- [ ] **Step 2: Change `draw_popup` and add the renderers**

```rust
/// Render a centered, bordered popup, clearing what is underneath. The footer is pinned to the
/// bottom so it stays visible, and `view.focus` names a body row that must remain on screen — the
/// body scrolls to keep it visible when the list is taller than the terminal.
fn draw_popup(frame: &mut Frame, area: Rect, view: PopupView) { … }
```

The body is today's `draw_popup` with `title` → `view.title`, `body` → `view.body`, `focus` →
`view.focus`, and `footer` → `Line::from(Span::styled(view.footer, Style::default().fg(Color::DarkGray)))`
built inside. **Do not change the geometry, the `CHROME` constant, the `u16::try_from` clamp, or the
scroll arithmetic.**

```rust
/// Render a full-area modal pane. Unlike `draw_popup` this does not clear behind itself, because
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

pub(crate) fn draw_modal(frame: &mut Frame, area: Rect, view: ModalView) {
    match view {
        ModalView::Popup(v) => draw_popup(frame, area, v),
        ModalView::FullScreen(v) => draw_full_screen(frame, area, v),
    }
}
```

`draw_full_screen`'s body is today's `App::draw_help` verbatim (`centered_rect(80, 90, area)`,
bordered `Block` titled `" {title} "`, `Wrap { trim: false }`) — the geometry and the absence of a
`Clear` are load-bearing for byte-identical output.

- [ ] **Step 3: Collapse `App::draw`**

Delete `App::draw_connect`, `App::draw_models` and `App::draw_help` from `app.rs`. Replace the two
blocks Task 3 left behind with:

```rust
if let Some(modal) = self.modal.current() {
    let ctx = ModalContext {
        locale: self.config.lang,
        error: self.error.as_deref(),
        offline: self.provider_info.offline.as_ref(),
    };
    crate::modal::draw_modal(frame, chunks[1], modal.view(&ctx));
}
```

Update `app.rs`'s `use crate::modal::{…}` list: drop `draw_popup`, `centered_rect` and `help_lines`
if `app.rs` no longer names them; add `ModalContext`. Delete any `ratatui` import in `app.rs` that
clippy now reports as unused.

- [ ] **Step 4: Run the rendering tests with no expectation edits**

Run: `cargo test -p light-factory-tui`
Expected: PASS. In particular `models_modal_renders_its_own_header`,
`a_long_model_list_keeps_the_selection_and_footer_on_screen`, `an_empty_list_does_not_advertise_enter`,
`the_offline_modal_names_the_actual_reason` and `a_popup_on_a_tiny_terminal_does_not_panic` must pass
**without touching their expected strings**. If one fails, the rendering changed — fix the code, not
the test.

Run: `cargo clippy -p light-factory-tui --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt --all
git add crates/tui/src/modal.rs crates/tui/src/app.rs
git commit -m "tui: render every modal through one draw seam"
```

---

### Task 5: Verify the reduction and record it

**Files:**
- Modify: `docs/superpowers/plans/2026-08-22-tui-modal-module.md` (check the boxes)
- Test: the whole workspace

**Interfaces:** none — this task adds no code.

- [ ] **Step 1: Measure**

```bash
wc -l crates/tui/src/app.rs crates/tui/src/modal.rs
git show HEAD~4:crates/tui/src/app.rs | wc -l
```

Expected: `app.rs` sheds at least 800 lines from its 3736-line starting point; `modal.rs` holds the
difference plus the new seam.

```bash
awk '/^pub struct App \{/,/^\}/' crates/tui/src/app.rs | grep -cE '^    [a-z_]+:'
```

Expected: **38** (from 44 — seven fields removed, one added).

```bash
grep -rn 'connect_return\|models_return\|help_return\|connect_nonce\|models_nonce\|Mode::Help\|ConnectTransition\|ModelsTransition\|handle_connect_key\|handle_models_key\|close_connect\|close_models\|begin_models_fetch\|handle_connect_models\|UiEvent::ConnectModels' crates/tui/src/
```

Expected: **no output**. Any hit is a leftover.

- [ ] **Step 2: Count the tests**

```bash
cargo test -p light-factory-tui 2>&1 | grep 'test result'
```

Expected: the `light-factory` bin target reports **at least 95** tests (86 before + 9 added across
Tasks 2–3), 0 failed. A number below 86 means a test was lost — restore it.

```bash
git show HEAD~4:crates/tui/src/app.rs | awk 'NR>2725 && /^    (async )?fn /' | sed 's/[({].*//;s/^    //' | sort > /tmp/before.txt
awk '/^#\[cfg\(test\)\]/,0' crates/tui/src/app.rs crates/tui/src/modal.rs | grep -E '^    (async )?fn ' | sed 's/[({].*//;s/^    //' | sort > /tmp/after.txt
comm -23 /tmp/before.txt /tmp/after.txt
```

Expected: any name printed is a test that no longer exists under that name. For each one, confirm it
was **renamed**, not deleted, and record the rename in the PR body. A genuine deletion is a plan
failure.

- [ ] **Step 3: Full workspace gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all three clean. (`crates/persistence/tests/pg_store.rs` skips without `DATABASE_URL`;
that is expected and pre-existing.)

- [ ] **Step 4: Confirm nothing out-of-band moved**

```bash
git diff --name-only origin/master...HEAD
```

Expected: only `crates/tui/src/{app.rs,modal.rs,main.rs}` and the two `docs/superpowers/` files.
No `Cargo.toml`, no `crates/tui/src/i18n.rs`, no `Dockerfile`/`fly.toml`/`web/`/
`crates/persistence/migrations/`/`.github/`. Phase 5 out-of-band verification is therefore
**vacuously satisfied** — state it explicitly rather than skipping it.

- [ ] **Step 5: Commit the plan checkboxes**

```bash
git add docs/superpowers/plans/2026-08-22-tui-modal-module.md
git commit -m "docs: mark the tui-modal-module plan complete"
```
