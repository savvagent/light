# TUI Broadcast Lag — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the engine-event forwarding loop so a `RecvError::Lagged(n)` continues the loop
(and surfaces a "dropped n events" notice) instead of terminating the forwarder permanently.

**Architecture:** Decode each `broadcast::Receiver::recv` result through a pure helper into one
of three actions (`Event`, `Dropped(n)`, `Stop`). Only a closed channel ends the loop.

**Tech Stack:** Rust, tokio broadcast, the existing `i18n` catalogs.

**Spec:** n/a — fast-path bug fix (single logical source file + hint strings; no public
interface).

## Global Constraints

- No comments unless asked.
- Any new user-facing string goes through `crates/tui/src/i18n.rs` `EN`/`ES` (key sets identical).
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/tui/src/app.rs` | `EngineForward` helper + loop + dropped-event handling |
| Modify. `crates/tui/src/i18n.rs` | `engine.dropped_events` string for both locales |

### Task 1: Continue on lag, stop only on close

**Files:** `crates/tui/src/app.rs`, `crates/tui/src/i18n.rs`

- [x] Add `UiEvent::EngineDropped(u64)`.
- [x] Add `EngineForward` + `engine_forward_step` mapping `Ok`/`Lagged`/`Closed`.
- [x] Rewrite the forwarder loop to use it, breaking only on `Stop` or a closed UI channel.
- [x] Add `handle_engine_dropped` and wire it into the run loop.
- [x] Add `engine.dropped_events` to `EN`/`ES`.
- [x] Add unit tests for `engine_forward_step` (lag continues, close stops, event forwards).
- [x] Run `cargo fmt --all` and commit.
