# PlanGate Sensitive-Argument Floor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `bash` arm of `PlanGate` apply the sensitive-path floor to every command
argument, so an approved scope matching `{ program: "cat", args: [Any] }` cannot silently read
`.env` or overwrite `.git/config`.

**Architecture:** The `bash` arm checked scope without consulting `is_sensitive`. Add the floor
check on `args` before the scope match, mirroring the `fs.read`/`fs.write` arms.

**Tech Stack:** Rust, `light_factory_protocol::is_sensitive`.

**Spec:** n/a — fast-path bug fix (one source file + test + docs wording; no public interface).

## Global Constraints

- No comments unless asked.
- Sensitive-path floor is `Ask`, never silent `Allow`; plan approval cannot bypass it.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/engine/src/gate.rs` | Floor-check `args` in the `bash` arm |
| Modify. `crates/engine/tests/gate.rs` | Test that `cat` + `.env` asks despite `ArgPattern::Any` |
| Modify. `docs/superpowers/archive/specs/2026-08-20-engine-core-design.md` | Explicit floor coverage of command arguments |
| Modify. `docs/superpowers/archive/plans/2026-08-20-engine-core.md` | Explicit floor coverage in global constraints |
| Modify. `ARCHITECTURE.md` | Explicit floor coverage of command arguments |

### Task 1: Floor-check command arguments

- [x] In the `bash` arm, return `Ask(SensitiveFloor { path })` when any `args` element is
      sensitive under `is_sensitive`, before consulting scope.
- [x] Add a test: `{"program":"cat","args":[".env"]}` asks even when scope authorizes `cat`
      with `ArgPattern::Any`.
- [x] Correct the spec/plan/architecture wording to make the floor's coverage of command
      arguments explicit.
- [x] Run `cargo fmt --all` and commit.
