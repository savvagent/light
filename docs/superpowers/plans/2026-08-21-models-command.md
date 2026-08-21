# `/models` Command — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add a `/models` command that opens a "Select a model" modal listing the models of the
**currently active provider**, pre-highlights the current model, and lets the user pick one — with a
manual model-id fallback when the list cannot be fetched. Reuses #35's fetch plumbing
(`list_models`/`list_ollama_models`/`resolve_key`) and modal semantics (Esc/Enter, off-loop fetch).

**Architecture:** A standalone `ModelsStep` state machine in `crates/tui/src/app.rs`, driven by a
pure `models_step_next` transition function returning an explicit `Close`/`Apply`/`Step` outcome, an
off-loop fetch posting a new `UiEvent::Picker` guarded by a `models_nonce` stale-result counter
(distinct from the connect modal's existing `UiEvent::Models`, which routes via `connect_nonce`), and
a shared `draw_popup` helper extracted from the connect modal's centered-popup rendering. No changes
outside `crates/tui`; `crates/providers` is reused as-is.

**Tech Stack:** Rust; existing `ratatui`/`crossterm` centered popup and list selection; existing
`list_models`/`list_ollama_models`/`resolve_key`; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-21-models-command-design.md` — read it first. This plan
implements it exactly.

## Global Constraints

- No comments unless asked. No AI/Co-Authored-By attribution in commits, comments, or docs.
- Inward dependency flow: changes stay in `crates/tui` (a client leaf); `protocol`/`auth`/
  `persistence`/`server`/`providers`/`web/` are untouched; `cargo build/test --workspace` must never
  require node.
- Secrets never logged, never in `config.json`, never in a status/error string. Model ids are not
  secrets; API keys never enter this modal (fetched via `resolve_key`, never rendered).
- Run `cargo fmt --all` before every Rust commit. Lint: `cargo clippy --workspace --all-targets -D warnings`.
- Semver: all changes are TUI-internal (new private enum/event/command/strings); no public interface,
  wire type, or crate boundary changes; no `Cargo.toml` version bump (Non-Negotiable Rule 6).
- Tests live next to code (`#[cfg(test)] mod tests` in `app.rs`), offline-deterministic (no live
  network, no keyring, no terminal; `MemStore` where a store is needed).

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/tui/src/app.rs` | `ModelsStep`/`ModelsTransition`/`models_step_next`; `App` fields `models`/`models_return`/`models_nonce`; `UiEvent::Picker`; `enter_models`/`close_models`/`apply_and_close_models`/`handle_models_key`/`handle_picker`/`begin_models_fetch`; `draw_models` + extracted `draw_popup`; `parse_models_command` + `run_command` branch + `handle_key` routing/Ctrl-P guard + `run` loop arm |
| Modify. `crates/tui/src/i18n.rs` | new EN + ES keys (parity test-enforced) |

## Task Order & Rationale

Single task: the modal's types, transition function, and i18n are only exercised once the command
wiring, fetch, and apply path exist, and a split would leave private `ModelsStep`/`ModelsTransition`
types unused between commits (dead-code under `clippy -D warnings`). All of it lands in
`app.rs` + `i18n.rs` in one cohesive change, TDD-ordered (tests → compile-fail → implement → green →
commit).

### Task 1: `/models` modal, command, fetch, apply, i18n

**Files:** `crates/tui/src/app.rs`, `crates/tui/src/i18n.rs`

**Interfaces:** consumes `light_factory_providers::{list_models, list_ollama_models}` and
`crate::selection::resolve_key` (both already imported/available); produces the `/models` command
and modal surface. No public API change.

- [ ] Add failing tests in `crates/tui/src/app.rs` `#[cfg(test)] mod tests` (reusing the existing
      `test_app()`/`test_app_with_store`/`key(code)`/`model_list_step`-style helpers):
      - `models_step_next`: Offline Esc→`Close` + Enter→`Close` + a char→`Step(Offline)`; fetching
        ModelList Esc→`Close` + Enter→`Step` (stays fetching); non-empty ModelList Esc→`Close`,
        Enter→`Apply`, Up/Down wrap (via `cycle_index`); empty ModelList Enter→`Step` (no-op);
        Manual Esc→`Close`, Enter-with-id→`Apply`, blank-Enter→`Step`, Backspace pops, Char appends.
      - `parse_models_command`: `/models` and `/models   ` → true; `/modelsx`, `/model gpt-5`,
        `/connect` → false.
      - `handle_picker`: stale nonce ignored; matching nonce fills `models` and pre-highlights the
        current model (index 0 when the current model is absent from the list); matching nonce `Err`
        → `Some(ModelsStep::Manual { error: Some(_), .. })`.
      - apply path via `handle_models_key`: Enter on a non-fetching ModelList persists
        `settings.models[<provider>]` and rebuilds (assert `settings.models` and
        `app.provider_info.model`); Esc leaves `settings.models` unchanged; Manual Enter applies the
        trimmed id.
- [ ] Run `cargo test -p light-factory-tui` — expect compile failures (the new types/function/variant
      are not yet defined).
- [ ] Implement in `app.rs`:
      - Add `ModelsStep` (derive `Clone, PartialEq, Eq, Debug`), `ModelsTransition` (derive `Debug,
        Clone, PartialEq, Eq`), and the pure `models_step_next(&ModelsStep, KeyEvent) ->
        ModelsTransition` exactly per spec §5.1.
      - Add `models: Option<ModelsStep>`, `models_return: Mode`, `models_nonce: u64` to `App` and
        initialize them in `App::new` (mirroring `connect`/`connect_return`/`connect_nonce`).
      - Add `UiEvent::Picker { nonce: u64, provider: String, result: Result<Vec<String>, String> }`.
      - Add `enter_models` (offline → `Offline`; else `ModelList { fetching: true }` + fetch),
        `begin_models_fetch` (same body as `begin_fetch`, posting `UiEvent::Picker`), `handle_picker`,
        `handle_models_key`, `close_models`, `apply_and_close_models` (persist to
        `settings.models[provider]` + save + `rebuild_provider()` + `status.model_set`; **do not**
        set `settings.provider`).
      - Add `parse_models_command` (mirroring `parse_connect_command`) and a `run_command` branch
        gated to `Mode::Connected` (else `status.models_not_connected`), placed after the `/connect`
        branch.
      - Route in `handle_key`: extend the Ctrl-P help guard to also require `self.models.is_none()`,
        and add `if self.models.is_some() { return self.handle_models_key(key); }` after the connect
        block.
      - Add the `UiEvent::Picker` match arm in the `run` loop's `tokio::select!` →
        `app.handle_picker(nonce, provider, result)`.
      - Extract the centered-popup tail of `draw_connect` into `fn draw_popup(frame, area, title,
        lines)` (same width-60/dynamic-height rect + `Clear` + bordered `Paragraph`), call it from
        `draw_connect` (preserving its exact current layout), and add `draw_models` using it: title
        `models.title`; Offline → `models.offline` line + `models.footer_offline`; ModelList
        fetching → `connect.fetching` + `connect.footer_fetching`; empty → `connect.no_models` +
        `models.footer_list`; else the model rows (reusing the `> ` selected marker styling) +
        `models.footer_list`; Manual → error line (red) + `models.manual` hint + reversed input +
        `models.footer_manual`. Draw the modal over the connected screen when `self.models.is_some()`
        in `draw` (mirroring the `self.connect.is_some()` block).
      - Add the `help.commands.models` entry to `help_lines`'s commands section.
- [ ] Add the new EN + ES keys to `i18n.rs` together (parity test-enforced): `status.models_not_connected`,
      `models.title` ("Select a model"), `models.offline`, `models.manual`, `models.footer_list`,
      `models.footer_manual`, `models.footer_offline`, `help.commands.models`. Reuse the existing
      `connect.fetching`/`connect.fetch_error`/`connect.no_models`/`connect.footer_fetching`.
- [ ] Run `cargo test -p light-factory-tui`, then `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt --all`.
- [ ] Commit `tui: add a /models modal for model selection`.

## Known Plan Gaps

- None recorded. The spec's open question (Ollama) is resolved in favor of coverage (§2.3); the
  manual fallback reading of AC 6 is the documented interpretation (§2.4).
