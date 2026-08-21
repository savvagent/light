# Provider Selection & Credential Supply in the Client — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the TUI an in-client surface to see, select, and switch the LLM provider and model
(no restart), and to supply/clear a credential that persists in the OS keyring — masked on input,
redacted everywhere, with env-driven and headless behavior unchanged.

**Architecture:** The `providers` crate's env-only selection is refactored behind an injectable
`Selection` value object and a new `build_provider(&Selection)` entry point; `build_provider_from_env()`
becomes a thin wrapper so its behavior and tests are untouched. The TUI adds a `CredentialStore` trait
(keyring-backed), a composition module that merges env + persisted preferences + keyring into a
`Selection`, and `/provider` / `/model` / `/key` commands. `App` holds a single mutable provider
(source of truth) that `enter_engine` now clones instead of rebuilding from env.

**Tech Stack:** Rust; `keyring` 3.6 (platform-native backends) for the credential store; existing
`ratatui`/`crossterm` for the masked-input mode.

**Spec:** `docs/superpowers/specs/2026-08-21-provider-credentials-ui-design.md` — read it first. This
plan implements it exactly.

## Global Constraints

- No comments unless asked. No AI/Co-Authored-By attribution in commits, comments, or docs.
- Inward dependency flow: `protocol → auth → persistence → server → tui`; `providers` stays a leaf
  (it does **not** gain a `keyring` dependency). `web/` is untouched; `cargo build/test --workspace`
  must never require node.
- Secrets never logged, never in `config.json`, never in the transcript/`EventKind::Error`/status;
  key input is masked and its value never stored on disk in plaintext.
- Run `cargo fmt --all` before every Rust commit. Lint: `cargo clippy --workspace --all-targets -D warnings`.
- Semver: all changes here are additive (new enum, new struct, new struct fields, new free function,
  new default-free trait methods); **no `Cargo.toml` version bump** (Non-Negotiable Rule 6).
- Tests live next to code (`#[cfg(test)] mod tests`), offline-deterministic.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/tui/Cargo.toml` | Add `keyring` dependency |
| Create. `crates/tui/src/credentials.rs` | `CredentialStore` trait, `KeyringStore`, `HashMap` test impl |
| Modify. `crates/tui/src/lib.rs` | Export `credentials` (already exports `i18n`/`engine_view`) |
| Modify. `crates/providers/src/selection.rs` | `Selection`, `SelectedBy`, `build_provider`, `env_key_var`, thread inputs |
| Modify. `crates/providers/src/lib.rs` | Re-export `Selection`, `SelectedBy`, `build_provider`, `env_key_var` |
| Modify. `crates/tui/src/settings.rs` | Extend `Settings` with `provider` + `models`; load/save whole struct |
| Modify. `crates/tui/src/main.rs` | Use the new settings + selection load path |
| Create. `crates/tui/src/selection.rs` | Composition: `resolve_key`, `key_status`, `build_selection`, `rebuild` |
| Modify. `crates/tui/src/provider.rs` | `ProviderInfo` gains `selected_by`; drop `build()` (moves to `selection.rs`) |
| Modify. `crates/tui/src/app.rs` | Commands, masked `/key` mode, single provider source, header reason |
| Modify. `crates/tui/src/i18n.rs` | New EN + ES keys (parity test-enforced) |

## Task Order & Rationale

Credentials store first (isolated, no selection coupling). Then the `providers` selection refactor
(the riskiest, most test-critical change, done while nothing else depends on it). Then the settings
extension (depends only on serde). Then the TUI composition module (depends on both 2 and 3). Then the
UI wiring and i18n (depends on everything). Each task compiles and tests green before the next.

### Task 1: Credential store (`crates/tui/src/credentials.rs`) + `keyring` dependency

**Files:** `crates/tui/Cargo.toml`, `crates/tui/src/credentials.rs`, `crates/tui/src/lib.rs`

**Interfaces:** consumes `keyring`; produces `CredentialStore` trait + `KeyringStore`.

- [ ] Add `keyring = { version = "3.6", features = ["apple-native", "windows-native", "sync-secret-service"] }` to `crates/tui/Cargo.toml`.
- [ ] Add a failing test in `credentials.rs`: `CredentialStore` implemented for `HashMap<String,String>` round-trips `get`/`set`/`delete`; `delete` on a missing key is a no-op; `get` on a missing key returns `None`.
- [ ] Run `cargo test -p light-factory-tui credentials` — expect it to fail to compile (trait/impl not yet written).
- [ ] Implement `pub trait CredentialStore: Send + Sync` (`get`/`set`/`delete`), `pub struct KeyringStore` (service `"light-factory"`, account `<provider>` via `keyring::Entry`), and `impl CredentialStore for HashMap<String, String>`.
- [ ] Run `cargo build -p light-factory-tui` — this validates the `sync-secret-service` backend compiles headlessly (the spec's build-risk check). If it fails to compile here, STOP and escalate per the spec's Risks § (do not weaken storage).
- [ ] Export `pub mod credentials;` from `crates/tui/src/lib.rs`.
- [ ] Run `cargo test -p light-factory-tui credentials` and `cargo fmt --all`, then commit `tui: add a keyring-backed credential store`.

### Task 2: Injectable `Selection` + `SelectedBy` in `providers`

**Files:** `crates/providers/src/selection.rs`, `crates/providers/src/lib.rs`

**Interfaces:** consumes nothing new (stays a leaf); produces `Selection`, `SelectedBy`,
`build_provider(&Selection)`, `env_key_var(provider)`; preserves `build_provider_from_env()`.

- [ ] Add failing tests to `selection.rs` `mod tests` for the new pure surface:
      (a) `build_provider` with `preferred = Some("openai")` + an openai key → `id()=="openai"` and `selected_by == Some(StoredPreference)`;
      (b) `preferred = Some("openai")` with no openai key but an anthropic key → offline with `NamedProviderMissingKey{selector:"openai",..}` (never misroutes), `selected_by == None`;
      (c) a `Selection` whose only key is `gemini` → `selected_by == Some(KeyPrecedence)`;
      (d) `ollama == true` → `selected_by == Some(OllamaEnv)` regardless of keys;
      (e) `env_key_var("openai") == Some("OPENAI_API_KEY")`, `env_key_var("ollama") == None`;
      (f) `build_provider_from_env` still compiles and, with no env configured, yields the offline `LocalProvider` (`offline == Some(NothingConfigured)`).
- [ ] Run `cargo test -p light-factory-providers` — expect failures on the not-yet-added items.
- [ ] Implement: add `pub enum SelectedBy { OllamaEnv, RemoteSelectorEnv, StoredPreference, KeyPrecedence }`; add `pub struct Selection { ollama, selector, preferred, keys, models, base_urls }`; add `pub fn build_provider(&Selection) -> BuiltProvider`; add `selected_by: Option<SelectedBy>` to `BuiltProvider`; rework `build_provider_from_env` to build a `Selection` from env and delegate to `build_provider`; thread the resolved key/model/base-url through `build_remote` instead of `std::env`; add `pub fn env_key_var(provider: &str) -> Option<&'static str>`. Keep the existing pure helpers (`choose`, `select_remote_from`, `resolve_base_url`, `default_model_*`) and their tests unchanged.
- [ ] Re-export the new items from `crates/providers/src/lib.rs`.
- [ ] Run `cargo test -p light-factory-providers`, `cargo clippy -p light-factory-providers --all-targets -D warnings`, `cargo fmt --all`, then commit `providers: add injectable provider selection`.

### Task 3: Extend persisted `Settings` with provider + models

**Files:** `crates/tui/src/settings.rs`, `crates/tui/src/main.rs`

**Interfaces:** consumes serde; produces `Settings { lang, provider, models }` load/save.

- [ ] Add failing tests to `settings.rs`: round-trip `provider` + `models`; a legacy `{"lang":"es"}` file still loads with `provider == None` and empty `models`; a corrupt file loads defaults (or `None` for the whole struct, matching current behavior).
- [ ] Run `cargo test -p light-factory-tui settings` — expect failures.
- [ ] Implement: make `Settings` public with `#[serde(default)] provider: Option<String>` and `#[serde(default)] models: BTreeMap<String,String>`; add `load()` / `save()` for the whole struct; update `main.rs`'s `resolve_locale` to read `load().lang` and keep `--lang` persistence (preserving current `/lang` behavior via a `save_lang` shim or a full `save`).
- [ ] Run `cargo test -p light-factory-tui settings` and `cargo fmt --all`, then commit `tui: persist provider and model preferences`.

### Task 4: TUI composition (`crates/tui/src/selection.rs`) + `ProviderInfo.selected_by`

**Files:** `crates/tui/src/selection.rs`, `crates/tui/src/provider.rs`

**Interfaces:** consumes `providers::{Selection, SelectedBy, build_provider, env_key_var}` and
`credentials::CredentialStore` and `settings::Settings`; produces `resolve_key`, `key_status`,
`build_selection`, `rebuild`.

- [ ] Add failing tests: `resolve_key` returns the env value when set, else the store value, else `None`; `key_status` distinguishes `Env`/`Keyring`/`None`; `build_selection` maps persisted `provider` into `preferred` and merges env models over persisted models; `rebuild` with no keys yields `id()=="local"` + `offline == Some(NothingConfigured)`.
- [ ] Run `cargo test -p light-factory-tui selection` — expect compile failure (module not wired).
- [ ] Implement `selection.rs` (per spec §5), move `build()` out of `provider.rs`, and add `selected_by: Option<SelectedBy>` to `ProviderInfo` with a `reason_suffix(locale)` display helper.
- [ ] Run `cargo test -p light-factory-tui` (full crate), `cargo fmt --all`, then commit `tui: compose provider selection from env, prefs, and keyring`.

### Task 5: Commands, masked `/key` entry, header reason, i18n

**Files:** `crates/tui/src/app.rs`, `crates/tui/src/i18n.rs`

**Interfaces:** consumes `selection::{rebuild, key_status, resolve_key}` and `credentials::KeyringStore`;
produces the `/provider` `/model` `/key` command surface.

- [ ] Add failing i18n/parsing tests first: pure `parse_provider_command` / `parse_key_command` / `parse_model_command` helpers (exact file: `app.rs` `mod tests`), and the EN/ES parity test (already exists; it fails until the ES entries are added).
- [ ] Run `cargo test -p light-factory-tui` — expect failures.
- [ ] Implement in `app.rs`: a `Mode::Key` masked-entry mode with `key_target`/`key_input`/`key_return` fields; `run_command` branches for `/provider`, `/provider <name>`, `/model <id>`, `/key`, `/key <provider>`, `/key <provider> clear`; `rebuild` on each mutation, updating `self.provider`/`self.provider_info`; `enter_engine` clones `self.provider`/`self.provider_info` instead of rebuilding; the connected header shows the `reason_suffix`. Masked rendering shows placeholder characters, never the typed key; status messages never echo the value.
- [ ] Add all new EN + ES keys to `i18n.rs` (parity test-enforced): provider active/available/key-status/reason strings, `status.provider_set`/`provider_invalid`, `status.model_set`/`model_invalid`, `status.key_set`/`key_cleared`/`key_failed`/`key_enter`, `field.key`.
- [ ] Run `cargo test -p light-factory-tui`, then `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt --all`.
- [ ] Commit `tui: add provider, model, and key commands`.
