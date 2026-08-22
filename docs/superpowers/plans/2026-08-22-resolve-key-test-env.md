# resolve_key Test Env Independence — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `selection::tests::resolve_key_reads_a_stored_keyring_key` deterministic so
`cargo test -p light-factory-tui` is reproducibly green on a developer machine with
`OPENAI_API_KEY` (or any other provider key) exported.

**Architecture:** `resolve_key` reads the process environment through `sources`, so the test's
outcome depends on the ambient shell. Introduce a private env-reader seam — `sources_with` and
`resolve_key_with`, both taking an `impl Fn(&str) -> Option<String>` env lookup — and have the
existing `sources` / `resolve_key` delegate to them with `std::env::var`. The test then drives
`resolve_key_with` with a stub environment. No production behavior changes; the public API
(`key_status`, `resolve_key`, `apply_preferences`, `build_selection`, `rebuild`) is untouched.

Mutating the process environment in the test is deliberately rejected: `std::env::set_var` /
`remove_var` are `unsafe` in edition 2024 and Rust runs tests in parallel threads within one
process, so it would trade an env dependency for a data race.

**Tech Stack:** Rust (edition 2024), the existing `CredentialStore`/`MemStore` seam.

**Spec:** n/a — fast-path per light-factory-development trivial-task criteria: one logical source
file (`crates/tui/src/selection.rs`), no new public interface (both new fns are private), no
behavior change, no auth-spine / dependency-flow / deploy-shape impact.

**Source:** GitHub issue savvagent/light#45.

## Global Constraints

- Change stays confined to `crates/tui/src/selection.rs` plus this plan doc. Do not touch
  `crates/tui/src/app.rs` (parallel work in flight).
- No new public interface: the new seams are private to the module.
- No AI/self-attribution anywhere (commits, PR body, comments, docs).
- Doc comments follow the file's existing `///` style.
- `cargo fmt --all` before every Rust commit; `cargo test -p light-factory-tui` and
  `cargo clippy --workspace --all-targets -- -D warnings` must pass.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/tui/src/selection.rs` | Private env-reader seam + env-independent wiring test |
| Create. `docs/superpowers/plans/2026-08-22-resolve-key-test-env.md` | This plan |

## Task Order & Rationale

Single task: the failing test and the seam it needs land together, because the test cannot be
written to fail-for-the-right-reason until the seam exists.

### Task 1: Make the `resolve_key` wiring test env-independent

**Files:** `crates/tui/src/selection.rs`

**Interfaces:** consumes `light_factory_providers::env_key_var`,
`light_factory_tui::credentials::CredentialStore`; produces no new public items.

- [ ] Confirm the reported failure reproduces:
      `OPENAI_API_KEY=sk-test cargo test -p light-factory-tui resolve_key` — expect
      `resolve_key_reads_a_stored_keyring_key` to FAIL with `left: Some("sk-test"), right: Some("sk-ring")`.
- [ ] Add private `fn sources_with(provider, store, env: impl Fn(&str) -> Option<String>)` holding
      the current body of `sources`, with the env lookup supplied by the caller.
- [ ] Reduce `sources` to `sources_with(provider, store, |var| std::env::var(var).ok())`.
- [ ] Add private `fn resolve_key_with(provider, store, env: impl Fn(&str) -> Option<String>)`
      holding the current body of `resolve_key`, and reduce `resolve_key` to a delegation with the
      real env lookup.
- [ ] Rewrite `resolve_key_reads_a_stored_keyring_key` to call `resolve_key_with` with a stub env,
      asserting: (a) an absent env yields the keyring value, (b) an empty store yields `None`, and
      (c) the lookup is made against the provider's declared var name (`OPENAI_API_KEY`), whose
      value wins over the keyring.
- [ ] Run `cargo test -p light-factory-tui resolve_key` — expect PASS.
- [ ] Run `OPENAI_API_KEY=sk-test cargo test -p light-factory-tui resolve_key` — expect PASS
      (the acceptance criterion).
- [ ] Run `cargo test -p light-factory-tui` and
      `OPENAI_API_KEY=sk-test cargo test -p light-factory-tui` — expect both green.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings` — expect clean.
- [ ] Run `cargo fmt --all` and commit as `tui: make the resolve_key test independent of the ambient env`.

## Out-of-band verification

Vacuous — this change touches no `Dockerfile`/`fly.toml`, no `web/`, no
`crates/persistence/migrations/`, and no `.github/`.
