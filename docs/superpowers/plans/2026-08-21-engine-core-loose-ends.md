# Engine Core Loose Ends — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve the five engine-core loose ends from issue #21: remove dead step events, remove
the write-only `Session::approved`, tear engine mode down on leave, reject mid-turn prompts
visibly, and error on non-UTF-8 `fs.read`.

**Architecture:** Dead protocol vocabulary is deleted (with the semver-major bump it requires);
the TUI stores and aborts the forwarder handle; the turn machine's decision loops answer a
mid-turn `SendPrompt` with a stable error; `FsReadTool` rejects non-UTF-8 bytes.

**Tech Stack:** Rust, tokio, the existing i18n catalogs.

**Spec:** `docs/superpowers/specs/2026-08-21-engine-core-loose-ends-design.md`

## Global Constraints

- No comments unless asked.
- Any new user-facing string goes through `crates/tui/src/i18n.rs` `EN`/`ES` (key sets identical).
- Removing protocol enum variants is semver-major: bump `light-factory-protocol` `0.1.1` → `0.2.0`.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/protocol/src/session.rs` | Remove `StepStarted`/`StepFinished` |
| Modify. `crates/protocol/Cargo.toml` | Bump version to `0.2.0` |
| Modify. `crates/engine/src/session.rs` | Remove `approved` field |
| Modify. `crates/engine/src/turn.rs` | Remove `approved` write; reject mid-turn prompts |
| Modify. `crates/engine/tests/turn.rs` | Mid-turn prompt rejection test |
| Modify. `crates/tui/src/app.rs` | Store/abort the forwarder; drop engine on leave |
| Modify. `crates/tui/src/engine_view.rs` | Remove step-event rendering |
| Modify. `crates/tui/src/i18n.rs` | Remove step keys; add `error.turn_in_progress` |
| Modify. `crates/tools/src/fs.rs` | Error on non-UTF-8 read |
| Create. `crates/tools/tests/fs.rs` | `fs.read` binary test |

### Task 1: Remove dead step events (semver-major)

- [x] Remove `StepStarted`/`StepFinished` from `EventKind`.
- [x] Remove their arms from `engine_view::describe_event`.
- [x] Remove `engine.step_started`/`engine.step_done`/`engine.step_failed` from `EN`/`ES`.
- [x] Bump `light-factory-protocol` to `0.2.0`.

### Task 2: Remove the write-only `approved` field

- [x] Delete `Session::approved` and its initialization; drop the `Plan` import.
- [x] Delete the `self.approved = …` write in `run_turn`.

### Task 3: Tear engine mode down on leave

- [x] Store the forwarder `JoinHandle` in `App`.
- [x] In `leave_engine`, abort the forwarder and drop `engine`/`engine_session`.

### Task 4: Reject mid-turn prompts visibly

- [x] Emit `Error { code: "turn_in_progress" }` from `await_plan_decision`,
      `await_action_decision`, and `wait_if_paused` on a `SendPrompt`.
- [x] Add `error.turn_in_progress` to `EN`/`ES`.
- [x] Add a turn test asserting the rejection then a normal plan approval.

### Task 5: Error on non-UTF-8 `fs.read`

- [x] Replace `from_utf8_lossy` with `from_utf8(...)` + error.
- [x] Add `crates/tools/tests/fs.rs` covering binary and UTF-8 reads.

### Task 6: Format and commit

- [x] Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`.
- [x] Commit.
