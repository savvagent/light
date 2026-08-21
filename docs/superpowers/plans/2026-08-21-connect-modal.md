# `/connect` Modal Flow — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the text-command provider setup (`/provider`, plus the `/key`/`/model` fallback
commands) with a single guided `/connect` modal: pick a provider, supply an API key when it is not
already connected (saved → tested → models fetched), then pick a model — with `Esc`/`Enter`
shortcuts on every step.

**Architecture:** A new `models` module in `crates/providers` lists model ids per provider as free
functions (decoupled from the pinned `Provider`), reusing the crate's base-URL validation and
redirect-disabled HTTP client. The TUI gains a connect modal state machine driven by a pure
`connect_step_next` transition function, an off-loop model fetch via a new `UiEvent::Models`, and a
`resolve_key` helper to read a connected provider's saved key. `/provider` is removed.

**Tech Stack:** Rust; existing `wiremock` (dev) for model-listing tests; existing
`ratatui`/`crossterm` for the centered popup and masked key input; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-21-connect-modal-design.md` — read it first. This plan
implements it exactly.

## Global Constraints

- No comments unless asked. No AI/Co-Authored-By attribution in commits, comments, or docs.
- Inward dependency flow: `providers` stays a leaf (no `keyring`, no `tui` dependency); `web/` is
  untouched; `cargo build/test --workspace` must never require node.
- Secrets never logged, never in `config.json`, never in the transcript/`EventKind::Error`/status;
  key input is masked and its value never stored on disk in plaintext.
- **Base-URL trust boundary must not regress:** every `list_models` request resolves its `*_BASE_URL`
  through `validate_base_url` and uses the redirect-disabled client (`build_http_client` +
  `reject_redirect`). A 3xx or invalid override rejects before any key is sent.
- Run `cargo fmt --all` before every Rust commit. Lint: `cargo clippy --workspace --all-targets -D warnings`.
- Semver: all changes are additive (new module, new free functions, new `pub(crate)` visibility); no
  `Cargo.toml` version bump (Non-Negotiable Rule 6).
- Tests live next to code (`#[cfg(test)] mod tests`), offline-deterministic (wiremock, no live network).

## File Structure

| File | Responsibility |
|---|---|
| Create. `crates/providers/src/models.rs` | `list_models`/`list_models_at`, `list_ollama_models`/`list_ollama_models_at` |
| Modify. `crates/providers/src/lib.rs` | `pub mod models;` + re-export the four functions |
| Modify. `crates/providers/src/selection.rs` | widen base-URL helpers to `pub(crate)` for `models.rs` reuse |
| Modify. `crates/providers/src/ollama.rs` | extract `pub(crate) const LOCAL_BASE` |
| Modify. `crates/tui/src/selection.rs` | add public `resolve_key(provider, store) -> Option<String>` |
| Modify. `crates/tui/src/app.rs` | `ProviderRow`, `ConnectStep`, `ConnectTransition`, `connect_step_next`, `mask`, `UiEvent::Models`, `/connect` command; remove `/provider` + `set_provider`/`list_providers` |
| Modify. `crates/tui/src/i18n.rs` | new EN + ES keys (parity test-enforced) |

## Task Order & Rationale

Model listing first (isolated, pure HTTP, test-critical, nothing depends on it). Then the client
`resolve_key` helper (depends only on `providers` + `credentials`, already stable). Then the modal
state machine + command wiring + i18n (depends on both, and on the existing `set_model`/
`rebuild_provider`/`store` paths). Each task compiles and tests green before the next.

### Task 1: Model listing (`crates/providers/src/models.rs`)

**Files:** `crates/providers/src/models.rs`, `crates/providers/src/lib.rs`, `crates/providers/src/selection.rs`, `crates/providers/src/ollama.rs`

**Interfaces:** consumes `base_url::{validate_base_url, build_http_client, reject_redirect}` and the
crate's base-URL resolution; produces `list_models`, `list_models_at`, `list_ollama_models`,
`list_ollama_models_at`.

- [ ] Widen the base-URL helpers in `selection.rs` (`base_url_var`, `default_base`, `resolve_base_url`, and `RemoteChoice` incl. its `parse`/`id` methods, plus `BaseUrlRejection` where needed) from private to `pub(crate)` so `models.rs` reuses the exact env→default resolution instead of duplicating it. Ollama is **not** a `RemoteChoice` and has **no** `*_BASE_URL` override: extract a `pub(crate) const LOCAL_BASE: &str = "http://127.0.0.1:11434";` in `ollama.rs` and reference it from both `OllamaProvider::local_default` and the model-listing path, so the localhost base is never duplicated/diverged.
- [ ] Factor the base-URL resolution into a **pure, directly-testable helper** `pub(crate) fn resolve_base_url_for(id: &str, override_value: Option<String>) -> Result<String, …>` that is **total**: `Err` for an unknown `id` (only the four remotes + `"ollama"` are legal), `Err` for an invalid override (via `validate_base_url`), else the normalized override or the production default (Ollama → `LOCAL_BASE`). `list_models(provider, key)` reads the env override in exactly one place, calls the pure helper, then delegates to `list_models_at`. This keeps the helper testable **without** `std::env` (which is `unsafe`/racy under `edition = "2024"` and avoided by this workspace).
- [ ] Declare `pub mod models;` in `lib.rs` (module declaration only — no re-exports yet) so the test file is actually compiled, then add failing tests in `models.rs` (wiremock `MockServer::start().await`): for each of anthropic/openai/gemini/deepseek a `list_models_at(server.uri(), key)` mock asserting path + auth header (incl. `anthropic-version: 2023-06-01`) and parsing the body into ids; Gemini strips the `models/` prefix; Ollama `list_ollama_models_at` extracts `name` from `{"models":[{"name":…}]}`; dedup + stable sort; an auth (401) response → `Err`; a 3xx response → `Err` (redirect rejected, no key forwarded). Separately unit-test the pure `resolve_base_url_for`: valid override → normalized, invalid override → `Err` from `validate_base_url`, `None` → the production default (and Ollama → the localhost constant). No test touches the live network.
- [ ] Run `cargo test -p light-factory-providers models` — expect compile failure (the `list_models_*` functions are not yet defined).
- [ ] Implement `models.rs`: a per-provider GET that builds the URL with the same `join_url` helper the providers use, sets the auth header(s), calls `build_http_client` (redirects off), `reject_redirect`s any 3xx, and `error_for_status`s non-2xx. Parse per §5.1 of the spec (stable-sort, dedup, Gemini prefix strip, Ollama name extraction). The `_at` variants take an explicit, already-resolved base URL and **deliberately do not re-validate it** (the wrapper is the trust boundary; note this in a one-line doc comment mirroring the crate's existing trust-boundary style). The non-`_at` wrappers resolve via the pure helper and delegate.
- [ ] Re-export from `lib.rs`: re-export **only** the validating wrappers `pub use models::{list_models, list_ollama_models};`. The `_at` variants stay `pub(crate)` so no external caller can hit them with an unvalidated base URL (the trust boundary stays enforced at the only public entry points). Note: the spec §5.1 sketches the `_at` variants as `pub`; `pub(crate)` is the deliberate, trust-boundary-correct deviation recorded here.
- [ ] Run `cargo test -p light-factory-providers`, `cargo clippy -p light-factory-providers --all-targets -D warnings`, `cargo fmt --all`, then commit `providers: add model listing for providers`.

### Task 2: Client key resolution (`crates/tui/src/selection.rs`)

**Files:** `crates/tui/src/selection.rs`

**Interfaces:** consumes `providers::env_key_var` + `credentials::CredentialStore`; produces
`resolve_key`.

- [ ] Add failing test: a **pure** `resolve_key_from(env_key: Option<String>, keyring_key: Option<String>) -> Option<String>` (mirroring the existing `classify`/`sources` split) returns the env value when set, else the keyring value, else `None`; an **empty** env value (`Some("")`) is treated as absent (mirrors the existing `classify` empty-string rule) so the modal never fetches with an empty key.
- [ ] Run `cargo test -p light-factory-tui selection` — expect compile failure.
- [ ] Implement `resolve_key_from` (pure) plus `pub fn resolve_key(provider: &str, store: &dyn CredentialStore) -> Option<String>` as the env-reading wrapper: it calls the existing private `sources` helper then delegates to `resolve_key_from`. Only `resolve_key_from` is unit-tested (no `std::env::set_var`, consistent with the repo's no-process-env test convention).
- [ ] Run `cargo test -p light-factory-tui selection` and `cargo fmt --all`, then commit `tui: expose resolved provider key for the connect flow`.

### Task 3: Connect modal, `/connect` command, i18n

**Files:** `crates/tui/src/app.rs`, `crates/tui/src/i18n.rs`

**Interfaces:** consumes `providers::models::{list_models, list_ollama_models}`,
`selection::{key_status, resolve_key, rebuild}`; produces the `/connect` modal surface and drops
`/provider`.

- [ ] Add failing tests first: `connect_step_next` pure transitions (ProviderList Enter → ModelList for connected/ollama, → KeyEntry for unconnected keyed; Esc → Close from ProviderList; KeyEntry Enter/blank-key/Esc; ModelList Enter → Close, Esc → KeyEntry only when `takes_key && (from_key || error)`), `mask` never echoes input, `parse_connect_command` symmetry, and the EN/ES parity test (fails until ES entries added). Note: the "keyring save fails → stay on `KeyEntry`" edge (§6) lives in the impure `handle_connect_key` (it branches on `store.set`'s result) and is intentionally not covered by the pure `connect_step_next` tests.
- [ ] Run `cargo test -p light-factory-tui` — expect failures.
- [ ] Implement in `app.rs`: add `ProviderRow`, `ConnectStep`, `ConnectTransition`, and `connect_step_next`; add `connect: Option<ConnectStep>` + `connect_return: Mode` to `App`; add `UiEvent::Models { nonce, provider, result }` using a new `connect_nonce: u64` field (a third counter alongside `nonce`/`device_nonce`) as the stale-result guard; add a `UiEvent::Models` match arm + handler in the `run` loop's `tokio::select!`; add `handle_connect_key`/`enter_connect`/`close_connect`; draw the centered `Clear`+`Block` popup for each step with a localized footer; extract `mask` from `draw_key` and use it in `KeyEntry`.
- [ ] Gate `/connect` to `Mode::Connected` (like `/ask`): when typed outside the connected screen, show a localized `status.connect_not_connected` error instead of opening the modal.
- [ ] Wire the fetch: on entering a `fetching` `ModelList`, spawn `list_models(provider, key)` (key = typed key, or `resolve_key` for a connected provider) / `list_ollama_models()` off-loop and post `UiEvent::Models`. On success, fill `models` and clear `fetching`/`error`; on failure, set `error` and clear `fetching`.
- [ ] Wire model application: ModelList Enter sets `settings.models[provider] = model` **and** `settings.provider = Some(provider)`, saves settings, calls `rebuild_provider()`, and closes the modal (this supersedes `set_model`, which keys off the active id and never sets `settings.provider`).
- [ ] Replace the `/provider` branch in `run_command` with `/connect` (add `parse_connect_command`); **delete the now-dead code** (clippy `-D warnings` treats it as an error): `set_provider`, `list_providers`, `parse_provider_command` and its test `parses_provider_commands`, plus the now-unused i18n keys `status.provider_set`, `status.provider_invalid`, `provider.list_active`, `provider.list_available`. **Keep** `is_valid_provider` (still used by `set_model`), `key_status_label`, and `provider.key.*` (still used by the retained `/key` path). Update `hint.connected` to advertise `/connect`. (Every removed i18n key must be dropped from **both** `EN` and `ES` — `es_mirrors_en_exactly` enforces parity — and the `hint.connected` edit must also be mirrored in `ES`.)
- [ ] Add all new EN + ES keys to `i18n.rs` (parity test-enforced): connect modal title/hints, provider list labels, connected/unconnected markers, "API Key" heading, key-entry hint, fetching/error/no-models messages, and the Esc/Enter footer strings.
- [ ] Run `cargo test -p light-factory-tui`, then `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt --all`.
- [ ] Commit `tui: add the /connect provider modal flow`.

## Known Plan Gaps

- None recorded; the spec's open question ("keep `/provider` as an alias?") is intentionally not
  built — add the one-line alias only if requested after the modal lands.
