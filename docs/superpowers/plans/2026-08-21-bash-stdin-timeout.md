# BashTool stdin + timeout — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent a `bash` tool call from hanging the turn: redirect the child's stdin to null
and bound its wall-clock runtime, killing the child on expiry and returning a non-zero result.

**Architecture:** Add `BashTool::new` / `with_timeout` and a `timeout` field; spawn with
`Stdio::null()` stdin and `kill_on_drop`, and wrap `Command::output()` in
`tokio::time::timeout`.

**Tech Stack:** Rust, tokio process + time.

**Spec:** n/a — fast-path bug fix (one source file + tests; additive constructor/method, no
public-interface break).

## Global Constraints

- No comments unless asked.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/tools/src/bash.rs` | stdin null + timeout + kill-on-drop |
| Modify. `crates/engine/src/turn.rs` | Use `BashTool::new` |
| Modify. `crates/tools/tests/bash.rs` | stdin-EOF and timeout tests |

### Task 1: Bound command execution

- [x] Redirect stdin to `Stdio::null()`.
- [x] Add `DEFAULT_COMMAND_TIMEOUT` + `timeout` field + `new`/`with_timeout` constructors.
- [x] Wrap `output()` in `tokio::time::timeout`; on expiry return a non-zero result with a
      "timed out" stderr message.
- [x] Update `turn.rs` to construct `BashTool` via `BashTool::new`.
- [x] Add tests: `cat` reaches EOF (stdin null) and `sleep 60` is killed on a short timeout.
- [x] Run `cargo fmt --all` and commit.
