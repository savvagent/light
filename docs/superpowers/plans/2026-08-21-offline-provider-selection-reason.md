# Offline Provider Selection Reason — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface *why* provider selection fell back to the offline `LocalProvider` — on the
`BuiltProvider`, in the engine pane on entry, and as a `no_provider_configured` error instead of
`invalid_plan` — and move selection warnings off stderr into the TUI.

**Architecture:** Add an `OfflineReason` enum + `offline`/`warnings` fields to `BuiltProvider`
(providers crate); thread the warnings through the pure selection helpers as return values instead
of `eprintln!`; add a default `Provider::is_offline()` to the engine-core seam (overridden by
`LocalProvider`); guard `run_turn` on it; and surface the reason + warnings in the TUI engine pane
with new i18n strings.

**Tech Stack:** Rust workspace (providers / engine-core / engine / tui). `ScriptedProvider` for
offline engine tests; pure selection helpers for provider tests.

**Spec:** `docs/superpowers/specs/2026-08-21-offline-provider-selection-reason-design.md` — read it
first. This plan implements it exactly.

## Global Constraints

- No comments unless asked; no Claude/AI self-attribution in commits, code, or docs.
- Dependency flow stays inward (`protocol` → `auth` → `persistence` → `server` → `tui`; `engine-core`
  is a leaf; `providers` is a leaf; `engine` → engine-core+protocol+tools+providers; `tui` →
  engine+protocol+providers). No new crate gains/loses a dependency edge.
- All public-surface changes are additive (new enum, new struct fields, new default trait method) —
  semver-minor, **no `Cargo.toml` version bump**.
- `cargo fmt --all` before every Rust commit. Verify with `cargo test -p <crate>`, `cargo clippy
  --workspace --all-targets -D warnings`, `cargo fmt --all --check`.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/providers/src/selection.rs` | `OfflineReason`, `BuiltProvider` fields, warning collection, reason wiring |
| Modify. `crates/providers/src/lib.rs` | Re-export `OfflineReason` |
| Modify. `crates/providers/src/local.rs` | Override `is_offline() -> true` |
| Modify. `crates/engine-core/src/traits.rs` | Default `Provider::is_offline()` |
| Modify. `crates/engine/src/turn.rs` | `no_provider_configured` guard in `run_turn` |
| Modify. `crates/engine/tests/turn.rs` | Offline-provider turn test |
| Modify. `crates/tui/src/provider.rs` | `ProviderInfo` fields + `offline_notice` helper |
| Modify. `crates/tui/src/app.rs` | Surface warnings + notice on engine entry |
| Modify. `crates/tui/src/i18n.rs` | EN + ES entries |

## Task Order & Rationale

Bottom-up, matching the dependency flow: the `OfflineReason`/`BuiltProvider` shape (Task 1) is the
contract the engine-core seam (Task 2) and the TUI (Task 4) consume; the engine guard (Task 3)
depends only on the seam. Each task is independently compilable and testable.

### Task 1: `OfflineReason` + warning collection in `providers`

**Files:** `crates/providers/src/selection.rs`, `crates/providers/src/lib.rs`.
**Interfaces:** consumes `validate_base_url`; produces `OfflineReason`, `BuiltProvider { offline,
warnings }`, re-exported from `lib.rs`.

- [ ] Add `OfflineReason` enum (derives `Debug, Clone, PartialEq, Eq`; variants `NothingConfigured`,
      `NamedProviderMissingKey { selector, key }`, `BaseUrlRejected { var }`).
- [ ] Add `pub offline: Option<OfflineReason>` and `pub warnings: Vec<String>` to `BuiltProvider`.
- [ ] Introduce internal `RemoteSelection { choice, offline, warnings }` and `BaseUrlRejection {
      var, warning }`; change `select_remote_from` to return `RemoteSelection`, `present_or_warn`
      to return `RemoteSelection`, `resolve_base_url` to return `Result<String, BaseUrlRejection>`,
      `env_base_url` to return `Result<String, BaseUrlRejection>`, and `build_remote` to return a
      `RemoteBuild { provider, offline, warnings }`; remove all four `eprintln!` calls.
- [ ] Rewire `build_provider_from_env` to populate `offline` + `warnings`.
- [ ] Update existing selection tests (they now assert against `RemoteSelection`/`Result`) and add
      tests: `nothing_configured` reason, `named_provider_missing_key` reason + warning, unknown
      selector warning with a live provider, base-url rejection → `BaseUrlRejection` + warning,
      `BuiltProvider.offline` is `None` for Ollama/remote.
- [ ] Run `cargo test -p light-factory-providers` (expect green).
- [ ] Run `cargo fmt --all` and commit `providers: carry offline selection reason and warnings`.

### Task 2: `is_offline()` default method on the engine-core seam

**Files:** `crates/engine-core/src/traits.rs`, `crates/providers/src/local.rs`.
**Interfaces:** adds a defaulted `Provider::is_offline(&self) -> bool { false }`; `LocalProvider`
overrides it.

- [ ] Add `fn is_offline(&self) -> bool { false }` to `Provider` (default body) with a doc comment.
- [ ] Override `is_offline() -> true` in `LocalProvider` (`crates/providers/src/local.rs`).
- [ ] Add a unit test in `local.rs` asserting `LocalProvider::new().is_offline()` is `true` and a
      `ScriptedProvider` is `false` (scripted's existing test module or selection tests).
- [ ] Run `cargo test -p light-factory-engine-core -p light-factory-providers` (expect green).
- [ ] Run `cargo fmt --all` and commit `engine-core: add Provider::is_offline seam`.

### Task 3: `no_provider_configured` guard in the engine

**Files:** `crates/engine/src/turn.rs`, `crates/engine/tests/turn.rs`.
**Interfaces:** consumes `Provider::is_offline`; emits `EventKind::Error { code:
"no_provider_configured" }` + `TurnComplete { ok: false }`.

- [ ] Add the guard at the top of `Session::run_turn` (`turn.rs:49`), before `propose_plan`,
      emitting the error then returning (fail-closed, never calls the provider).
- [ ] Add a test in `crates/engine/tests/turn.rs` that spawns a `Session` with `LocalProvider`,
      sends `SendPrompt`, and asserts the first matching event is
      `Error { code: "no_provider_configured" }` (never `invalid_plan`) followed by
      `TurnComplete { ok: false }`.
- [ ] Run `cargo test -p light-factory-engine` (expect green).
- [ ] Run `cargo fmt --all` and commit `engine: report no_provider_configured for offline turns`.

### Task 4: Surface reason + warnings in the TUI

**Files:** `crates/tui/src/provider.rs`, `crates/tui/src/app.rs`, `crates/tui/src/i18n.rs`.
**Interfaces:** consumes `OfflineReason`; produces localized strings.

- [ ] Extend `ProviderInfo` with `offline: Option<OfflineReason>` and `warnings: Vec<String>`;
      populate from `BuiltProvider`.
- [ ] Add pure `offline_notice(locale, &OfflineReason) -> String` in `provider.rs` mapping the three
      variants to `provider.offline.*` keys.
- [ ] In `app.rs::enter_engine`, capture `info` (not `_info`) and, **after**
      `self.engine_log.clear()`, push every `info.warnings` line then the `offline_notice` when
      offline.
- [ ] Add EN + ES entries (`provider.offline.nothing`, `provider.offline.missing_key`,
      `provider.offline.base_url`, `error.no_provider_configured`), ES mirroring EN.
- [ ] Add a unit test for `offline_notice` (three reasons) in `provider.rs`; the i18n EN/ES parity
      test already enforces mirroring.
- [ ] Run `cargo test -p light-factory-tui` (expect green).
- [ ] Run `cargo fmt --all` and commit `tui: surface offline reason and selection warnings`.

### Task 5: Workspace verification

**Files:** none (verification only).

- [ ] Run `cargo test --workspace` (persistence PG test skips without `DATABASE_URL`).
- [ ] Run `cargo clippy --workspace --all-targets -D warnings`.
- [ ] Run `cargo fmt --all --check`.
- [ ] Out-of-band: no `Dockerfile`/`fly.toml`/`web/`/`migrations/`/`.github/` touched — state
      "none" in the PR.
