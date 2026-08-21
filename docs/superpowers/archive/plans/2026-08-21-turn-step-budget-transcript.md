# Turn Step Budget + Transcript Truncation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bound the execute loop with a per-turn step budget and truncate transcript entries so a
large `fs.read` result cannot grow the prompt without bound.

**Architecture:** Add `MAX_STEPS_PER_TURN` (counted alongside the consecutive-denial cap) that
ends the turn with `TurnComplete { ok: false }` and a `step_budget_exceeded` `Error`; add a
`transcript_entry` helper that caps each entry at `MAX_TRANSCRIPT_ENTRY_CHARS`.

**Tech Stack:** Rust, `ScriptedProvider` for offline turn tests.

**Spec:** n/a — fast-path bug fix (one source file + tests; no public interface, constants are
internal).

## Global Constraints

- No comments unless asked.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/engine/src/turn.rs` | Step budget + transcript truncation |
| Modify. `crates/engine/tests/turn.rs` | Step-budget exit test |

### Task 1: Bound the execute loop and the transcript

- [x] Add `MAX_STEPS_PER_TURN`; increment per iteration and emit a `step_budget_exceeded`
      `Error` + `TurnComplete { ok: false }` when hit.
- [x] Add `transcript_entry` truncating to `MAX_TRANSCRIPT_ENTRY_CHARS`, applied to all pushes.
- [x] Add a unit test for `transcript_entry` and an integration test for the budget exit.
- [x] Run `cargo fmt --all` and commit.
