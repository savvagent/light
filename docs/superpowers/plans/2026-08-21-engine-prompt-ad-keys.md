# Engine Prompt a/d Keys — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the TUI engine prompt so the letters `a` and `d` approve/deny only while an
approval is pending, and are typed into the prompt otherwise.

**Architecture:** `handle_engine_key` matched `a`/`d` unconditionally, colliding with the
free-text `engine_prompt` arm added on top of the plan's approval keys. Route every letter key
through a pure helper that returns an approval answer only when `self.pending` is set.

**Tech Stack:** Rust, crossterm key handling, the existing `i18n` catalogs.

**Spec:** n/a — fast-path bug fix (single logical source file + hint strings; no public
interface, no behavior change on covered paths beyond the bug fix).

## Global Constraints

- No comments unless asked.
- Any new user-facing string goes through `crates/tui/src/i18n.rs` `EN`/`ES` (the key sets must
  stay identical).
- `cargo fmt`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace`.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/tui/src/app.rs` | Route engine letter keys through the new helper |
| Modify. `crates/tui/src/i18n.rs` | Clarify `hint.engine` for both locales |

### Task 1: Guard the approval keys on pending state

**Files:** `crates/tui/src/app.rs`, `crates/tui/src/i18n.rs`

- [x] Add `engine_approval_key(c, pending) -> Option<bool>` returning `Some(true/false)` for
      `a`/`d` only when `pending`, else `None`.
- [x] Update `handle_engine_key` to push non-approval chars into `engine_prompt`.
- [x] Update `hint.engine` in `EN` and `ES` to say the keys approve/deny only while pending.
- [x] Add a unit test for `engine_approval_key` (fires while pending, typed otherwise).
- [x] Run `cargo fmt --all` and commit.
