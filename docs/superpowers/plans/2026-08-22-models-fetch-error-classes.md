# Model-list Fetch Error Classes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Branch the `/models` modal on *why* the model-list fetch failed. A missing key or a
401/403 renders a terminal `Credentials` step naming `/connect` and `/key <provider>` and offers no
input box; every other failure keeps the manual fallback (AC 6 of #36) but adds a `Ctrl+R` retry and
labels the box as an unverified fallback. A model id that was typed rather than picked reports
"Model set to {model} — not verified against {provider}".

**Architecture:** A private `FetchFailure` classification plus a `FetchError { class, message }`
carrier, computed at `crates/tui/src/app.rs`'s `fetch_model_list` boundary by walking the `anyhow`
chain for a `reqwest::Error` status. The `/models` state machine gains a `ModelsStep::Credentials`
terminal step and a `ModelsTransition::Retry` outcome; `models_apply_target` returns a `ModelChoice`
carrying a `verified` flag that `persist_model` turns into one of two status strings. Everything is
private to `crates/tui`; `crates/providers`, the connect modal's event payload, and the help modal
are untouched.

**Tech Stack:** Rust (edition 2024, toolchain pinned in `rust-toolchain.toml`); existing
`ratatui`/`crossterm` popup rendering; existing `reqwest` (already a `crates/tui` dependency);
`wiremock` added as a `crates/tui` **dev**-dependency (already in `Cargo.lock` via
`crates/providers`). No new runtime dependency.

**Spec:** `docs/superpowers/specs/2026-08-22-models-fetch-error-classes-design.md` — read it first.
This plan implements it exactly.

## Global Constraints

- No comments unless the surrounding code already carries them in that style. **No AI /
  Co-Authored-By / "Generated with" attribution** in commits, PR bodies, code comments, or docs.
- Inward dependency flow: all changes stay in `crates/tui` (a client leaf). `protocol`, `auth`,
  `persistence`, `server`, `providers` and `web/` are untouched; `cargo build/test --workspace` must
  never require node.
- Secrets never logged, never rendered, never in a status/error string. API keys never enter this
  modal (resolved inside `fetch_model_list` and consumed there); model ids are not secrets.
- Every new user-facing string goes in **both** the `EN` and `ES` catalogs in
  `crates/tui/src/i18n.rs`. The existing `es_mirrors_en_exactly` test enforces parity and must stay
  green.
- Semver: all new types are private to `crates/tui` (`FetchFailure`, `FetchError`, `ModelChoice`,
  the new `ModelsStep`/`ModelsTransition` variants). No public interface, wire type, route, or
  `Store` method changes ⇒ **no `Cargo.toml` version bump** (Non-Negotiable Rule 6).
- Parallel-work constraints: do **not** implement #44's fetch bounds (timeout / byte cap /
  truncation / `JoinHandle`), and do **not** restructure the modals or rename
  `ModelsStep`/`ModelsState` (#46). Keep the diff to the models-modal error path, the transition
  function, the `draw_models` tail, and the i18n catalogs.
- Run `cargo fmt --all` before every Rust commit. Lint with
  `cargo clippy --workspace --all-targets -- -D warnings`.
- Tests live next to the code (`#[cfg(test)] mod tests` in `app.rs`) and must be
  offline-deterministic: no live network beyond a local `wiremock` server, no keyring, no terminal.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/tui/src/app.rs` | `FetchFailure` + `FetchError` + `classify_fetch_error`/`class_for_status`; `fetch_model_list` return type; `UiEvent::ModelsFetched.result` type; `begin_fetch` maps to `String` for the connect modal; `ModelsStep::Credentials` + `ModelsStep::provider()`; `ModelsTransition::Retry`; `models_step_next` arms; `handle_models_fetched` branch + `fetch_error_message`; `handle_models_key` retry arm + `retry_models_fetch`; `ModelChoice` + `models_apply_target`; `persist_model(verified)` + its three call sites; `draw_models` `Credentials` arm and relabelled `Manual` arm; new tests |
| Modify. `crates/tui/src/i18n.rs` | `status.model_set_unverified`, `models.auth_rejected`, `models.credentials_hint`, `models.credentials_remedy`, `models.manual_unverified` (replacing `models.manual`), updated `models.footer_manual` value — in **both** `EN` and `ES` |
| Modify. `crates/tui/Cargo.toml` | new `[dev-dependencies]` section with `wiremock = "0.6"` (test targets only) |

## Task Order & Rationale

**Single task.** The classification type, the new step, the retry transition and the verified flag
are one cohesive change: splitting them would leave a private enum variant unconstructed or a
private struct unused between commits, which `clippy -D warnings` rejects as dead code, and would
leave the modal in a half-branched state that no test can meaningfully assert. The whole change is
two source files plus one manifest line, TDD-ordered inside the task.

### Task 1: classify model-list fetch failures and branch the `/models` modal

**Files:** `crates/tui/src/app.rs`, `crates/tui/src/i18n.rs`, `crates/tui/Cargo.toml`

**Interfaces:** consumes `light_factory_providers::{list_models, list_ollama_models}` (unchanged
signatures) and `reqwest::Error::status()`; produces the `/models` modal's credential step, retry
key, and unverified status string. **No public API change.**

- [x] Add `[dev-dependencies]` to `crates/tui/Cargo.toml` with `wiremock = "0.6"` (matching
      `crates/providers/Cargo.toml`). Verify no lockfile churn: `cargo metadata --offline >/dev/null`
      then `git diff --stat Cargo.lock` — expect an empty diff.
- [x] Write the failing tests first in `crates/tui/src/app.rs`'s `#[cfg(test)] mod tests`, reusing
      the existing `test_app()` / `key(code)` / `models_list_step` / `models_manual_step` /
      `TempSettings` helpers. The existing `key()` hardcodes `KeyModifiers::NONE`, so **add a
      `ctrl_key(code)` helper** (`KeyEvent::new(code, KeyModifiers::CONTROL)`). Extend the explicit
      `use super::{...}` list at the top of `mod tests` with `FetchError`, `FetchFailure`,
      `ModelChoice`, `class_for_status`, `classify_fetch_error`, and add `use anyhow::Context;` plus
      the `wiremock` imports inside the tests that need them:
      - `class_for_status`: `Some(401)` → `Auth`, `Some(403)` → `Auth`, `Some(400)`/`Some(404)`/
        `Some(429)`/`Some(500)`/`None` → `Fetch`.
      - `classify_fetch_error` (`#[tokio::test]`, wiremock): a 401 response → `Auth`; a 500 → `Fetch`;
        a 401 wrapped with `anyhow::Context` → `Auth` (proves `err.chain()` traversal); a request to a
        closed port → `Fetch`.
      - `models_step_next` on `ModelsStep::Credentials`: `Esc` → `Close`, `Enter` → `Close`,
        `Char('x')` → `Step` with the step unchanged.
      - `models_step_next` on `ModelsStep::Manual`: `Ctrl+R` → `Retry`; a bare `Char('r')` still
        appends `r` to the input (regression guard on arm ordering).
      - `handle_models_fetched` with `FetchError { class: FetchFailure::Auth, .. }` → the step is
        `Credentials { provider: "openai", error }` whose `error` contains `"openai"`; with
        `FetchFailure::MissingKey` → `Credentials` whose `error` is the message verbatim; with
        `FetchFailure::Fetch` → `Manual { input: "", error: Some(_) }`.
      - `handle_models_key` with `Ctrl+R` from `Manual` (`#[tokio::test]` — `begin_models_fetch`
        spawns): the step becomes `ModelList { provider: "openai", fetching: true }` with an empty
        list and `models_nonce` strictly greater than before.
      - `models_apply_target`: `ModelList` → `ModelChoice { verified: true }`; `Manual` →
        `ModelChoice { verified: false }` with the id trimmed; `Credentials` → `None`.
      - Manual `Enter` sets `app.status` to the `status.model_set_unverified` rendering (assert it
        contains the model id **and** the provider id, and differs from the verified string); list
        `Enter` sets the unchanged `status.model_set` rendering.
      - Two `TestBackend` render assertions (mirroring `models_modal_renders_its_own_header`): the
        `Credentials` step shows `/connect` and `/key openai` and advertises neither "save" nor
        "retry"; the `Manual` step shows the whole unverified prompt on one line and advertises
        `Ctrl+R: retry` and "save unverified".
- [x] Run `cargo test -p light-factory-tui` — expect **compile failure** (the new items do not exist
      yet). That is the failing-test signal for this task.
- [x] Add the i18n entries to **both** catalogs in `crates/tui/src/i18n.rs`:
      `status.model_set_unverified`, `models.auth_rejected`, `models.credentials_hint`,
      `models.credentials_remedy`, `models.manual_unverified`; update the `models.footer_manual`
      value to advertise `Ctrl+R`; delete the now-unreferenced `models.manual` from both. Values per
      spec §5.5. **Every string must fit the popup's ~58-column inner width** — `draw_popup` sizes
      the box from the line count, so a wrapping line pushes the last row (the input field) out of
      view.
- [x] Run `cargo test -p light-factory-tui i18n::tests::es_mirrors_en_exactly` — expect **pass**
      (key parity holds).
- [x] Implement in `crates/tui/src/app.rs` per spec §5.1–§5.4:
      1. `FetchFailure` (`MissingKey`/`Auth`/`Fetch`, `Copy`) with `needs_credentials()`; `FetchError
         { class, message }`; `class_for_status(Option<u16>) -> FetchFailure`;
         `classify_fetch_error(&anyhow::Error) -> FetchFailure` walking `err.chain()` for the first
         `reqwest::Error` and reading `.status()`.
      2. `fetch_model_list` returns `Result<Vec<String>, FetchError>`; the `None` key arm builds
         `MissingKey` with the existing `connect.no_key` message; provider errors build
         `classify_fetch_error(&e)` with `format!("{e:#}")` as the message (keep `{:#}` — it carries
         anyhow's source chain).
      3. `UiEvent::ModelsFetched.result` becomes `Result<Vec<String>, FetchError>`;
         `UiEvent::ConnectModels.result` stays `Result<Vec<String>, String>` and `begin_fetch` maps
         with `.map_err(|e| e.message)`.
      4. `ModelsStep::Credentials { provider, error }` + `ModelsStep::provider(&self) -> Option<&str>`
         (`None` only for `Offline`).
      5. `ModelsTransition::Retry`; `models_step_next` gains the `Credentials` arm and the `Ctrl+R`
         arm on `Manual`, ordered **before** `Enter`/`Backspace`/`Char(c)`.
      6. `handle_models_fetched`'s `Err` arm calls a new `fetch_error_message(&provider, &err)`
         (`MissingKey` → message verbatim; `Auth` → `models.auth_rejected`; `Fetch` →
         `connect.fetch_error`) and routes on `err.class.needs_credentials()`.
      7. `handle_models_key` maps `Retry` to `retry_models_fetch()`, which reads the provider via
         `ModelsStep::provider()`, returns early on `None`, resets the step to
         `ModelList { models: vec![], selected: 0, fetching: true }` and calls
         `begin_models_fetch(provider)`.
      8. `ModelChoice { provider, model, verified }`; `models_apply_target` returns
         `Option<ModelChoice>`; `apply_and_close_models` forwards the flag.
      9. `persist_model(&mut self, provider: String, model: String, verified: bool)` selects
         `status.model_set` vs `status.model_set_unverified`. Update all three call sites:
         `apply_and_close_connect` → `true`, `apply_and_close_models` → the choice's flag,
         `set_model` (`/model <id>`) → `false`.
      10. `draw_models`: add the `Credentials` arm (red error line, blank line, dark-gray
          `models.credentials_hint`, footer `models.footer_offline`, no focus) and bind `provider` in
          the `Manual` arm so it can render `models.manual_unverified`.
- [x] Update the two pre-existing tests whose **constructors** changed shape — never their
      assertions' intent:
      - `handle_models_fetched_falls_back_to_manual_entry_on_a_fetch_error` passes
        `Err("bad key".to_string())`; it must pass a
        `FetchError { class: FetchFailure::Fetch, message: "bad key".into() }` and keep asserting the
        `Manual` outcome (it is now the *transport* case, so rename it accordingly).
      - `models_apply_target_reads_the_highlighted_or_typed_id` compares against
        `Some(("openai", "gpt-4o"))` tuples; it must compare against `ModelChoice` values.
- [x] Run `cargo test -p light-factory-tui` — expect **all green**, including every other
      pre-existing `/models` test unchanged.
- [x] Run `cargo test --workspace` — expect green (the `crates/persistence` PostgreSQL integration
      test skips without `DATABASE_URL`; that is not a failure).
- [x] Run `cargo clippy --workspace --all-targets -- -D warnings` — expect clean.
- [x] **Out-of-band surfaces:** this task touches none. `Dockerfile`/`fly.toml`, `web/`, and
      `crates/persistence/migrations/` are unchanged — state this explicitly at Phase 5 step 14
      rather than skipping it.
- [x] Format and commit: `cargo fmt --all` then
      `git commit -m "tui: branch the /models modal on the fetch error class"`.
