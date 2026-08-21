# Port otto's LLM providers into light-factory Implementation Plan

For agentic workers: REQUIRED SUB-SKILL — `superpowers:subagent-driven-development` and
`superpowers:executing-plans`. Read both before executing this plan. Check off each `- [ ]` step as
it is completed.

## Goal

Port otto's in-process LLM providers (OpenAI, Anthropic, DeepSeek, Gemini, Ollama, Local,
Scripted, plus the `base_url` trust-boundary module) into a new `crates/providers` leaf crate, add
an env-driven selection entry point, and wire it into the TUI as an active-provider header plus a
`/ask <prompt>` completion command.

## Architecture

A new leaf crate `crates/providers` (depends only on third-party crates) holds the `Provider`
trait, the `CompleteRequest`/`CompleteResponse`/`Usage` types, the seven provider impls, the
`base_url` trust-boundary module, and `build_provider_from_env()` selection. The TUI gains an edge
to it (mirroring its existing edge to the leaf `protocol`) and constructs one provider at startup,
showing its id/model in the connected header and running `/ask` completions off the UI loop. No
change to `protocol`, `auth`, `persistence`, `server`, or `web/`; no wire-type or public-API break.

## Tech Stack

Rust (edition 2024, workspace reqwest 0.13). New dev-deps `wiremock 0.6.5` + `tempfile 3.27.0`
(only in `providers`). `async-trait` promoted to a workspace dependency.

## Spec

`docs/superpowers/specs/2026-08-20-port-llm-providers-design.md` — read it first. This plan
implements it exactly.

## Global Constraints

- All work in the worktree `.worktrees/port-llm-providers`; never commit to `master` directly.
- No AI/Claude attribution in commits, PR bodies, or docs.
- The port is faithful: wire shapes, headers, token-field policy, redirect guards, and tests match
  otto's `otto-providers`; only mechanical renames (`otto_engine_core::*` → `crate::*`,
  `super::base_url` → `crate::base_url`) change.
- `base_url` security invariants are load-bearing and must not be weakened: redirects disabled,
  `http` loopback-only, userinfo/query/fragment rejected, secrets redacted in errors, proxy off for
  `http` bases.
- Inward dependency flow: `providers` is a leaf; only `tui` gains a new edge to it.
- API keys are read from the environment and never logged.
- Run `cargo fmt --all` before every Rust commit; keep clippy clean with `-D warnings`.
- No semver/version bump: this is additive (a new crate + a new client feature); no existing
  interface or wire type changes.
- Out-of-band surfaces (Fly `Dockerfile`/`fly.toml`, `web/` Svelte bundle, DB migrations) are
  untouched — state this explicitly at verification.

## File Structure

| File | Responsibility |
|---|---|
| Create. `crates/providers/Cargo.toml` | Package + deps (anyhow, async-trait, reqwest, serde, serde_json; dev wiremock/tempfile/tokio) |
| Create. `crates/providers/src/lib.rs` | Module decls + re-exports |
| Create. `crates/providers/src/types.rs` | `CompleteRequest`, `CompleteResponse`, `Usage` |
| Create. `crates/providers/src/provider.rs` | `Provider` trait |
| Create. `crates/providers/src/base_url.rs` | Trust-boundary module (validate/redact/build client/reject redirect/join) |
| Create. `crates/providers/src/openai_compatible.rs` | Shared OpenAI-compatible wire impl |
| Create. `crates/providers/src/{openai,deepseek,anthropic,gemini,ollama,local,scripted}.rs` | The seven providers |
| Create. `crates/providers/src/selection.rs` | `build_provider_from_env` + pure selection helpers |
| Modify. `Cargo.toml` | Add `async-trait` workspace dep |
| Modify. `crates/tui/Cargo.toml` | Add `light-factory-providers` dep |
| Create. `crates/tui/src/provider.rs` | Build the provider + `ProviderInfo` for the header |
| Modify. `crates/tui/src/main.rs` | Build the provider and pass it into `run` |
| Modify. `crates/tui/src/app.rs` | Provider field, header display, `/ask`, `/` keybinding, `UiEvent::Completion` |

## Task Order & Rationale

Task 1 establishes the crate skeleton and the `base_url` trust boundary (no external dev-deps, so
it builds and tests offline) — everything else depends on it. Task 2 adds the providers on top
(introducing the `wiremock`/`tempfile` dev-deps). Task 3 adds selection, which composes the
providers from Task 2. Task 4 wires the TUI. Each task ends with its own build/test/fmt/commit so
every commit is independently green and reviewable.

### Task 1: Crate skeleton + `base_url` trust boundary

**Files:** `Cargo.toml` (root), `crates/providers/Cargo.toml`, `crates/providers/src/lib.rs`,
`crates/providers/src/base_url.rs`

**PREREQUISITE:** Task 4 of `docs/superpowers/plans/2026-08-20-engine-core.md` must be complete
first — it creates `crates/engine-core`, which owns the `Provider` trait and the
`CompleteRequest`/`CompleteResponse`/`Usage` types this crate consumes. Do not define them here.

**Interfaces:** consumes `light-factory-engine-core` (`Provider`, `CompleteRequest`,
`CompleteResponse`, `Usage`), `async-trait`, `anyhow`, `reqwest`, `serde`, `serde_json`; produces
`validate_base_url`, `BaseUrlError`, and the crate's `pub use` surface.

- [x] Add `async-trait = "0.1"` to `[workspace.dependencies]` in the root `Cargo.toml`, and point
      `auth` and `persistence` at `{ workspace = true }` (they are already direct deps at `0.1`;
      the promotion is behavior-neutral and folds all crates onto one version — spec §5).
- [x] Create `crates/providers/Cargo.toml` (name `light-factory-providers`, `version.workspace`,
      `edition.workspace`, `license.workspace`; deps `light-factory-engine-core = { path =
      "../engine-core" }`, `anyhow`, `async-trait`, `reqwest` (workspace), `serde`, `serde_json`;
      dev-deps `tokio` with `macros`/`rt-multi-thread`, `wiremock = "0.6"`, `tempfile`).
- [x] Delete `crates/providers/src/types.rs` and `crates/providers/src/provider.rs` if they exist
      from an earlier attempt at this task. The `Provider` trait and the
      `CompleteRequest`/`CompleteResponse`/`Usage` types now live in `crates/engine-core` (created
      by Task 4 of the engine-core plan) and must not be duplicated here — two definitions of the
      same seam would not interoperate.
- [x] Create `crates/providers/src/base_url.rs`: port verbatim from otto's
      `providers/src/base_url.rs` (public `validate_base_url` + `BaseUrlError`; `pub(crate)`
      `build_http_client`/`reject_redirect`/`join_url`/`is_loopback_host`) plus its full `#[cfg(test)]`
      module, adjusting nothing except the `pub use` visibility already defined there.
- [x] Create `crates/providers/src/lib.rs`: declare `pub mod base_url;` and re-export
      `pub use base_url::{BaseUrlError, validate_base_url};`. Re-export the seam from engine-core
      for consumer convenience: `pub use light_factory_engine_core::{CompleteRequest,
      CompleteResponse, Provider, Usage};`.
- [x] Build + test the crate: `cargo test -p light-factory-providers` — the `base_url` test matrix
      (loopback carve-out, redaction, userinfo/query/fragment rejection, normalization, join_url)
      must pass.
- [x] `cargo fmt --all` and `cargo clippy -p light-factory-providers --all-targets -D warnings`.
- [x] Format and commit: `git commit -m "providers: add the crate skeleton and the base_url trust boundary"`.

### Task 2: Port the seven provider implementations

**Files:** `crates/providers/src/openai_compatible.rs`, `crates/providers/src/openai.rs`,
`crates/providers/src/deepseek.rs`, `crates/providers/src/anthropic.rs`,
`crates/providers/src/gemini.rs`, `crates/providers/src/ollama.rs`,
`crates/providers/src/local.rs`, `crates/providers/src/scripted.rs`,
`crates/providers/src/lib.rs`

**Interfaces:** consumes `crate::{Provider, CompleteRequest, CompleteResponse, Usage, base_url}`;
produces `OpenAiProvider`, `DeepSeekProvider`, `AnthropicProvider`, `GeminiProvider`,
`OllamaProvider`, `LocalProvider`, `ScriptedProvider`.

- [x] Port `openai_compatible.rs` (shared `OpenAiCompatibleProvider`, `always_max_tokens`,
      `TokenFields`) — change only imports to `crate::{CompleteRequest, CompleteResponse, base_url}`.
- [x] Port `openai.rs` (`OpenAiProvider`, `o_series_token_fields`, `is_o_series`, `api_base_default`)
      and its wiremock tests.
- [x] Port `deepseek.rs` (`DeepSeekProvider` → `openai_compatible` with `/chat/completions` +
      `always_max_tokens`) and its tests.
- [x] Port `anthropic.rs` (`AnthropicProvider`, `x-api-key` + `anthropic-version`) and its tests.
- [x] Port `gemini.rs` (`GeminiProvider`, `x-goog-api-key`, model-interpolated path) and its tests.
- [x] Port `ollama.rs` (`OllamaProvider`, `local_default`) and its tests.
- [x] Port `local.rs` (`LocalProvider`) and `scripted.rs` (`ScriptedProvider`) and their tests.
- [x] Update `lib.rs` to declare and re-export the seven providers (no `candle` module — it is
      out of scope per the spec).
- [x] `cargo test -p light-factory-providers` — all ported provider tests (request shape, auth
      headers, usage parsing, HTTP error surfacing, redirect non-following, trailing-slash base
      URL, empty-choices) must pass.
- [x] `cargo fmt --all` and `cargo clippy -p light-factory-providers --all-targets -D warnings`.
- [x] Format and commit: `git commit -m "providers: port the LLM provider implementations"`.

### Task 3: Env-driven provider selection

**Files:** `crates/providers/src/selection.rs`, `crates/providers/src/lib.rs`

**Interfaces:** consumes the providers from Task 2; produces `pub fn build_provider_from_env() ->
BuiltProvider` (where `BuiltProvider { provider: Box<dyn Provider>, model: Option<String> }`).

- [x] Write the failing tests first in `selection.rs` (`#[cfg(test)]`), asserting the pure helpers
      (injectable, no process env) mirroring otto's `select_remote_from`:
      - `LIGHT_OLLAMA=1` wins over any remote (via the pure `choose` helper).
      - a valid selector whose key is present selects that provider;
      - a valid selector whose key is **absent** selects `None` (offline), NOT another keyed provider;
      - an unknown selector falls through to key precedence `Anthropic > OpenAI > Gemini > DeepSeek`;
      - `resolve_base_url` accepts a valid override (normalized) and rejects a bad one → `None`;
      - default models match otto constants when the `*_MODEL` var is unset.
      Run `cargo test -p light-factory-providers selection` to see them fail (no `selection.rs`
      yet — expected compile failure, which is the red state).
- [x] Implement `selection.rs`: `RemoteChoice` enum; `select_remote_from`; `has_key`;
      `default_model_for`; `resolve_base_url` (using `crate::validate_base_url`); a local-slot
      branch (`LIGHT_OLLAMA`); and `build_provider_from_env()` that reads the environment and
      returns `BuiltProvider`, degrading to `LocalProvider` (offline) with stderr warnings on the
      exact otto `present_or_warn` semantics (named-but-no-key → offline; unknown selector → key
      precedence; rejected base URL → no provider for that slot).
- [x] Re-run `cargo test -p light-factory-providers selection` → green.
- [x] Re-export `build_provider_from_env` (and `BuiltProvider`) from `lib.rs`.
- [x] `cargo test -p light-factory-providers` (whole crate) and `cargo fmt --all`.
- [x] Format and commit: `git commit -m "providers: add env-driven provider selection"`.

### Task 4: Wire providers into the TUI

**Files:** `crates/tui/Cargo.toml`, `crates/tui/src/provider.rs`, `crates/tui/src/main.rs`,
`crates/tui/src/app.rs`

**Interfaces:** consumes `light_factory_providers::{build_provider_from_env, Provider,
CompleteRequest}`; the TUI gains a provider-backed `/ask` command (no change to the wire protocol
or the server).

- [x] Add `light-factory-providers = { path = "../providers" }` to `crates/tui/Cargo.toml`.
- [x] Write the failing test first in `app.rs` (or `provider.rs`) for the pure `/ask` parsing
      helper (extract a `fn parse_ask_command(&str) -> Option<&str>`): `/ask hello` → `Some("hello")`,
      `/ask` → `None` (empty), `/ask  ` → `None`, `/auth/login` → `None`. Run
      `cargo test -p light-factory-tui` to see the red state.
- [x] Create `crates/tui/src/provider.rs`: `ProviderInfo { id: String, model: Option<String> }` and
      `fn build() -> (Arc<dyn Provider>, ProviderInfo)` wrapping
      `light_factory_providers::build_provider_from_env()` (id from `provider.id()`).
- [x] In `main.rs`, build the provider via `crate::provider::build()` and pass `Arc<dyn Provider>`
      into `app::run` (new parameter), storing it in `App` (add the field + constructor param in
      `app.rs`).
- [x] In `app.rs`: extend the `/` keybinding to include `Mode::Connected`; add `UiEvent::Completion`
      (text/error); implement `run_command` `/ask <prompt>`: guard to `Connected` mode, reject empty
      prompts with a hint, spawn a task that awaits `provider.complete(CompleteRequest { prompt })`
      and sends `UiEvent::Completion` back; handle the event in the main loop by appending the
      result (or `[ask] <error>`) to the activity log; show `· provider: <id>[ (<model>)]` in the
      connected header.
- [x] Localize the new strings in `i18n.rs` (EN + ES must mirror — `es_mirrors_en_exactly` is
      test-enforced): add a `{provider}` param to `info.connected`, extend `hint.connected` to
      advertise `/ask`, and add `status.ask_empty` (empty-prompt usage hint). The `[ask]` log
      prefix is the literal command name (not translated).
- [x] `cargo test -p light-factory-tui` → green (parse tests), then
      `cargo test --workspace`, `cargo fmt --all`,
      `cargo clippy --workspace --all-targets -D warnings`.
- [x] Smoke: `cargo run -p light-factory-tui` (no keys set) → header shows `provider: local`; a
      `/ask hello` returns the deterministic offline completion in the log.
- [x] Format and commit: `git commit -m "tui: wire providers in with a /ask completion command"`.

## Out-of-band verification

No `Dockerfile`/`fly.toml`, `web/`, or `crates/persistence/migrations/` change in this plan — the
Fly image, Svelte bundle, and DB migrations are untouched (vacuously satisfied; state this in the
PR). The relevant in-repo verification is the Rust suite above plus the manual TUI smoke (Task 4).
