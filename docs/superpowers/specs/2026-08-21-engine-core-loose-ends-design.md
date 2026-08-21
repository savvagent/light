# Engine Core Loose Ends — Design

**Date:** 2026-08-21
**Status:** DRAFT — resolves the five loose ends from the engine core slice review

> **Implements:** GitHub issue #21

## Context

The engine core vertical slice left five loose ends, none of which break the slice but each of
which is a papercut or a correctness gap. This design resolves them together because they share
one theme: the turn machine and its TUI surface should not carry dead vocabulary, leak
resources, or silently drop user input.

## Decisions

1. **Remove the dead step events.** `StepStarted` / `StepFinished` are defined in
   `crates/protocol/src/session.rs`, rendered by `crates/tui/src/engine_view.rs`, and translated
   in `crates/tui/src/i18n.rs`, but the turn machine never emits them — and cannot, because the
   model's tool calls carry no step id, so execution has no step boundaries to bracket. Rather
   than emit false step-level progress, the variants (and their TUI rendering + i18n keys) are
   removed. This is a **breaking** protocol change: the affected crate (`light-factory-protocol`)
   is bumped `0.1.1` → `0.2.0` per the repo's semver rule.
2. **Remove `Session::approved`.** It is set in `run_turn` and never read; a second `SendPrompt`
   re-plans from scratch with a fresh `PlanGate` regardless. The field is deleted.
3. **Tear down engine mode fully.** `leave_engine` aborts the forwarding task (whose `JoinHandle`
   is now stored on `App`), drops the `Engine` (dropping the command sender so the session actor
   fails closed and exits), and clears `engine_session`. Re-entering builds a fresh `Engine` and a
   fresh broadcast channel, so an abandoned session's events can no longer replay into a new one.
4. **Reject mid-turn prompts visibly.** The decision loops in the turn machine
   (`await_plan_decision`, `await_action_decision`, `wait_if_paused`) currently drop a
   `SendPrompt` via a catch-all arm after the TUI has echoed it. They now emit
   `EventKind::Error { code: "turn_in_progress" }` so the user sees the rejection instead of
   silence.
5. **Error on non-UTF-8 `fs.read`.** `FsReadTool` used `String::from_utf8_lossy`, silently
   corrupting binary files into the transcript. It now errors (`fs.read: <path> is not valid
   UTF-8`), which the model sees as a tool error it can react to.

## Scope

**In:** the five fixes above, their tests, and the semver bump for the protocol crate.

**Out:** emitting real step-level progress (requires a step-tagged tool-call protocol, not built
here); persisting sessions; a queue for mid-turn prompts (rejection is chosen over queueing).

## Assumptions

- **Rejection over queueing** for mid-turn prompts: queueing would require buffering and a
  re-dispatch order that is unspecified; a visible rejection is simpler and leaves the user in
  control.
- **Removal over fake emission** for step events: emitting `StepStarted`/`StepFinished` without a
  real step boundary would be dishonest progress reporting.

## Error handling

- A mid-turn `SendPrompt` is answered by a stable `turn_in_progress` `Error` (translated via a new
  `error.turn_in_progress` i18n key in both locales), never by silence.
- A non-UTF-8 `fs.read` surfaces as a tool error, fed back to the model as a transcript entry.

## Risks & Open Questions

- Removing the step events is semver-major; it is the honest choice but should be called out to
  reviewers explicitly.
- `leave_engine` relies on `JoinHandle::abort` plus dropping the `Engine` (dropping the command
  sender) to stop the session actor; a session mid-provider-call stops only after that call
  returns, which is bounded by the provider timeout from issue #19.
