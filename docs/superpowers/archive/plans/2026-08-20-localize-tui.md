# Localize the TUI (ratatui client) Implementation Plan

For agentic workers: REQUIRED SUB-SKILL — `superpowers:subagent-driven-development` /
`executing-plans`. Each task is a `- [ ]` checklist; work failing-test-first, check boxes as you
complete steps, commit at the final step of each task.

## Goal

Externalize every user-facing string in the ratatui client to an `en`/`es` catalog, add locale
resolution (`--lang` → saved config → `LANG`/`LC_ALL` → `en`) with `config.json` persistence and
English fallback, and translate server errors by stable code.

## Architecture

- `crates/tui/src/i18n.rs` — pure catalog + lookup: `Locale`, `EN`/`ES` static tables, `t`,
  `t_with`, `error_message`, `resolve_locale` (pure, testable).
- `crates/tui/src/settings.rs` — `config.json` persistence with `_at(path)` variants for tests.
- `Config` gains `lang: Locale`; `main.rs` resolves and passes it; `app.rs` consumes it via
  `self.t(...)` helpers and a `/lang` command.

## Tech Stack

Rust (edition 2024, rust-toolchain 1.95.0), ratatui, clap, serde/serde_json (already dependencies
of `light-factory-tui`). No new dependencies.

## Spec

`docs/superpowers/specs/2026-08-20-localize-tui-design.md` — read it first. This plan implements
it exactly.

## Global Constraints

- No AI/Claude self-attribution anywhere (comments, docs, strings).
- No new dependency or crate; inward dependency flow unchanged (`tui` remains a leaf client).
- No wire-type / public-API change → no semver bump (additive CLI flag only).
- Do not change the auth spine, device-grant flow, session handling, or WS auth.
- Fallback never panics; unknown error codes surface verbatim.
- `cargo fmt --all` before every Rust commit; `cargo clippy --workspace --all-targets -D warnings`
  and `cargo test --workspace` must be green.

## File Structure

| File | Responsibility |
|---|---|
| Create. `crates/tui/src/i18n.rs` | `Locale`, catalogs, `t`/`t_with`, `error_message`, `resolve_locale` |
| Create. `crates/tui/src/settings.rs` | `config.json` load/save |
| Modify. `crates/tui/src/main.rs` | `--lang`, resolution, localized logout, `mod` decls |
| Modify. `crates/tui/src/config.rs` | `lang: Locale` + `with_lang` |
| Modify. `crates/tui/src/app.rs` | externalize all strings, `/lang`, error translation |

## Task Order & Rationale

1. **i18n core + tests** first — everything depends on the catalog API; tests lock catalog
   completeness, fallback, resolution, and error-code contracts.
2. **settings** — independent; its `_at` round-trip test needs no i18n.
3. **config + main wiring** — plumbs the resolved locale into `Config` and the CLI.
4. **app.rs externalization** — the mechanical bulk; depends on all of the above.
5. **full-workspace verification** — fmt, clippy, tests.

## Task 1: i18n core module + tests

**Files:** `crates/tui/src/i18n.rs`
**Interfaces:** produces `Locale`, `t`, `t_with`, `error_message`, `resolve_locale`.

- [x] Write the `#[cfg(test)] mod tests` first (failing): assert (a) every `ES` key exists in `EN`
  and key sets are identical; (b) `Locale::parse` maps `en`, `EN`, `en-US`, `en_US.UTF-8` → `En`,
  `es-419` → `Es`, `fr`/`C.UTF-8` → `None`; (c) `resolve_locale` precedence
  (`Some("es"), _, _, _` → `Es`; `None, Some("es"), _, _` → `Es`; `None, None, Some("es"), _` →
  `Es`; `None, None, None, Some("es")` → `Es`; `None, None, None, None` → `En`; `LC_ALL` beats
  `LANG`); (d) `t(Locale::Es, "screen.sign_in")` != `t(Locale::En, "screen.sign_in")`; (e)
  `t(Locale::Es, "definitely.missing")` == `t(Locale::En, "definitely.missing")`; (f) `t_with`
  interpolation; (g) `error_message(Locale::Es, "invalid_credentials")` is `Some` and
  `error_message(Locale::En, "no_such_code")` is `None`.
- [x] Run `cargo test -p light-factory-tui i18n` — expect compile failure (module missing).
- [x] Implement `i18n.rs` with full `EN`/`ES` catalogs (keys listed in §1/§3 of the spec).
- [x] Run `cargo test -p light-factory-tui i18n` — expect green.
- [x] `cargo fmt --all` and commit: `git commit -m "tui: add i18n catalog, lookup, and locale resolution"`.

## Task 2: settings persistence

**Files:** `crates/tui/src/settings.rs`
**Interfaces:** produces `load_lang`, `save_lang`, and `_at(path)` variants.

- [x] Write the `#[cfg(test)] mod tests` first (failing): `save_lang_at`/`load_lang_at` round-trip
  against a temp path (use `std::env::temp_dir()` + unique suffix), and `load_lang_at` on a
  nonexistent path returns `None`.
- [x] Run `cargo test -p light-factory-tui settings` — expect compile failure.
- [x] Implement `settings.rs` (JSON `{ "lang": … }` at `~/.config/light-factory/config.json`).
- [x] Run `cargo test -p light-factory-tui settings` — expect green.
- [x] `cargo fmt --all` and commit: `git commit -m "tui: add locale settings persistence"`.

## Task 3: config + CLI wiring

**Files:** `crates/tui/src/config.rs`, `crates/tui/src/main.rs`
**Interfaces:** `Config::with_lang`, `Config.lang`; `--lang` flag; `mod i18n; mod settings;`.

- [x] Add `lang: Locale` to `Config`, default `Locale::En` in `from_url`, plus `with_lang`.
- [x] Add `--lang <LANG>` to `Cli`; resolve via `resolve_locale` (CLI → `settings::load_lang` →
  `LC_ALL` → `LANG`), persist when `--lang` was passed, localize the `"Logged out."` path, and pass
  the resolved locale into `app::run`.
- [x] Run `cargo test -p light-factory-tui` — expect green.
- [x] `cargo fmt --all` and commit: `git commit -m "tui: wire locale resolution through config and CLI"`.

## Task 4: externalize app.rs strings

**Files:** `crates/tui/src/app.rs`
**Interfaces:** consumes `i18n::{t, t_with, error_message}` via `self.t`/`self.t_with`/`self.error_text`.

- [x] Add `self.t`/`self.t_with`/`self.error_text` helpers on `App`.
- [x] Replace every literal status line, screen title, field label, hint, footer bar, log line, and
  validation/error message with the localized helper.
- [x] Add the `/lang en|es` command to `run_command` (mutate `config.lang`, persist, localized
  confirmation / invalid-language error).
- [x] Update `handle_server`'s `ws_closed` path to use localized status + `error_text`.
- [x] Run `cargo test -p light-factory-tui` — expect green.
- [x] `cargo fmt --all` and commit: `git commit -m "tui: externalize UI strings to the i18n catalog"`.

## Task 5: full verification

**Files:** none (verification only)
**Reminders:** this change touches only `crates/tui` (no Fly image, no web bundle, no DB migration).
Out-of-band check is vacuous — state it in the PR.

- [x] `cargo fmt --all --check` — clean.
- [x] `cargo clippy --workspace --all-targets -D warnings` — clean.
- [x] `cargo test --workspace` — green.
- [x] Grep `crates/tui/src/app.rs` for remaining hardcoded English literals — none in the UI path.
