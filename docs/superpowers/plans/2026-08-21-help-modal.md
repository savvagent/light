# Help Modal — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The TUI's bottom status line is filling up with long per-mode help hints. Move those
help items (keybindings + slash commands) into a help modal raised with **Ctrl-P**, and show a
short **"Ctrl-P: help"** indicator in the status line instead.

**Architecture:** Add a `Mode::Help` screen to the TUI state machine, alongside the existing
`Mode::Key` masked-entry pattern. `App` gains a `help_return: Mode` field so the modal is a
non-destructive overlay — opening it saves the current mode, closing it (Esc / Ctrl-P) restores
it. The help body is assembled by a pure `help_lines(locale)` function (unit-testable, like the
existing `parse_*` helpers), and rendered as a centered bordered block. The status line collapses
the four long per-mode hints (`hint.connected` / `hint.engine` / `hint.default`) to a single short
`hint.help` ("Ctrl-P: help"), keeping `hint.device_cancel` (Esc cancels a pending device login —
a safety-relevant action) and the `> command` prompt.

**Tech Stack:** Rust; existing `ratatui`/`crossterm`. No new dependencies.

**Spec:** none — fast-path per light-factory-development trivial-task criteria (TUI-only, 2 source
files, no new public interface, no auth-spine change, no dependency-flow change, one-sentence AC).

## Global Constraints

- No comments unless asked. No AI/Co-Authored-By attribution in commits, comments, or docs.
- Inward dependency flow unchanged; `web/` untouched; `cargo build/test --workspace` never requires node.
- Every new user-facing string is added to both `EN` and `ES` catalogs (parity test-enforced in
  `i18n.rs`).
- Run `cargo fmt --all` before every Rust commit. Lint: `cargo clippy --workspace --all-targets -D warnings`.
- No semver impact (no public interface or wire-type change; i18n string keys are internal).

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/tui/src/app.rs` | `Mode::Help`, `help_return`, Ctrl-P/Esc handling, `draw_help`, `help_lines`, status-line hints |
| Modify. `crates/tui/src/i18n.rs` | New EN + ES help/hint keys; drop the three long per-mode hints |

## Task Order & Rationale

A single task: the modal and its content are one coherent unit. i18n keys and the `app.rs`
state-machine changes land together so the crate stays compiling and the parity test stays green.

### Task 1: Help modal, Ctrl-P keybinding, and shortened status line

**Files:** `crates/tui/src/app.rs`, `crates/tui/src/i18n.rs`

**Interfaces:** consumes `i18n::{self, Locale}`; produces `Mode::Help`, `help_lines(locale)`.

- [ ] Add failing tests first in `app.rs` `mod tests`: a `help_lines` test asserting the EN body is
      non-empty and no line contains the raw-key sentinel `"help."` (catches a typo'd key that would
      fall through `i18n::t`'s key-name fallback); a localization test asserting
      `help_lines(Locale::En)` and `help_lines(Locale::Es)` are non-empty, equal length, and differ.
      Run `cargo test -p light-factory-tui help_lines` — expect compile failure (`help_lines` not yet
      defined).
- [ ] Add the EN help/hint keys to `i18n.rs` (`hint.help`, `hint.help_close`, `title.help`, the
      `help.section.*` / `help.global.*` / `help.forms.*` / `help.connected.*` / `help.engine.*` /
      `help.commands.*` keys), and remove `hint.connected` / `hint.engine` / `hint.default`.
- [ ] Add the matching ES keys (parity) so `es_mirrors_en_exactly` passes.
- [ ] Implement in `app.rs`: `Mode::Help` variant; `help_return: Mode` field (init `Mode::SignIn`);
      `open_help`/`close_help`/`handle_help_key`; a global Ctrl-P branch at the top of `handle_key`
      (opens help in every mode except while typing a command); `help_lines` free function; a
      `centered_rect` helper; `draw_help`; the `Mode::Help =>` arm in `draw`; no-op `Mode::Help` arms
      in the `Esc`, `submit`, and `cycle_focus` matches; and the collapsed status-line hint
      (`command_mode` → `> <cmd>`, `Help` → `hint.help_close`, `Device` → `hint.device_cancel`,
      else `hint.help`).
- [ ] Run `cargo test -p light-factory-tui`, then `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt --all`.
- [ ] Commit `tui: add a Ctrl-P help modal`.
