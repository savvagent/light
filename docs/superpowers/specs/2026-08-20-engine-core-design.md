# Engine Core — Design

**Date:** 2026-08-20
**Status:** Approved (brainstorm), pending implementation plan

## Context

light-factory has an auth spine and no engine. `crates/protocol/src/wire.rs` is a ping/pong
handshake standing in for the real Command/Event protocol, and nothing agentic exists.

This spec designs the engine core: where the agent loop runs, how a session lives, the
Command/Event vocabulary, and how plan-first approval relates to risk gates.

### Thesis alignment

The product is a blazing fast TUI humans run on distributed machines. The bet is that catchy
user interfaces are unnecessary and agents will increasingly coordinate with each other,
needing little human input. Two consequences bind this design:

- **The protocol is the product.** The TUI is the client a human happens to attach with. Every
  design choice must leave room for a non-human agent to be a first-class peer on the same
  seam.
- **Scope discipline.** otto died of surface area (a Dioxus desktop app, a web app, and a CLI,
  with a 19,794-line engine crate), not of bad design. Deepen the protocol; do not widen the
  product.

## Decisions

1. **Own agent loop through one or more providers**, ported from otto — not a hosted subprocess
   agent.
2. **Port `providers` and the `engine-core` trait seams from otto; leave otto's 19.8k-line
   `engine` crate behind.** Write a new, smaller orchestrator shaped by this protocol. otto is
   the same author under the same MIT/Apache-2.0 dual license, so this is a copy-and-rename,
   not a dependency.
3. **Local-first execution.** The engine runs on the developer's machine where the repo already
   is. Tool calls hit the real filesystem with no network hop. The fly.io server stays small
   and stateless — identity today, the agent-to-agent bus later. Source never leaves the
   machine.
4. **Approving a plan authorizes the whole plan.** Gates catch deviation from the approved
   scope, not each risky step. One exception: the sensitive-path floor always asks.
5. **First slice is a thin vertical one with the engine in-process in the TUI**, behind the
   full Command/Event vocabulary over an mpsc channel. A daemon is a later transport change,
   not a redesign.
6. **The engine is reachable only through Command/Event.** The TUI never calls engine internals.

## Non-goals

Explicitly out of scope for this spec, to be designed separately if and when needed:

- The local daemon, unix socket transport, and detach/reattach. The `seq` primitive that makes
  them cheap is in scope; the transport is not.
- The agent-to-agent message bus and any server-side engine role.
- Remote workspaces. `Workspace`/`WorkspaceRead` is ported as a trait, but only
  `LocalWorkspace` is implemented.
- Server-side provider-key custody (see Assumptions).
- Any web UI surface for the engine. The SPA stays auth-only.
- Token metering as a revenue model. `TokenUsage` events are reported for display only.

## Crate layout

```
crates/protocol      + session.rs  SessionId, Command, Event/EventKind, Plan, Scope, GateReason
                     + sensitive.rs  the sensitive-path floor (ported)
                     auth.rs unchanged; wire.rs Ping/Pong retained for the fly server
crates/engine-core   ported seams: Provider, Tool, ToolRegistry, Workspace/WorkspaceRead,
                     Decision, PermissionGate, Approver, PauseController
crates/providers     ported: AnthropicProvider + ScriptedProvider first
crates/tools         fs.read / fs.list / fs.write / bash
crates/engine        Engine, Session actor, turn state machine, PlanGate
crates/tui           attaches via Command/Event
crates/server        unchanged
```

Dependency flow remains inward: `protocol <- engine-core <- {providers, tools} <- engine <- tui`.

The sensitive-path floor lives in `protocol`, the dependency-free leaf, so a tool that cannot
take an `engine-core` dependency can still enforce it. This follows otto's placement.

**Toolchain:** otto is pinned to Rust 1.97.0; light-factory to 1.95.0. The port requires bumping
`rust-toolchain.toml` and, per repo convention, re-resolving the `dtolnay/rust-toolchain` SHA
pin in `.github/workflows/deploy.yml`.

## Protocol vocabulary

All types live in `crates/protocol/src/session.rs`, serde-serializable, no I/O.

### Commands (client -> engine)

| Command | Meaning |
|---|---|
| `CreateSession { workspace: PathBuf }` | Open a session rooted at a workspace directory. |
| `SendPrompt { session, text }` | Start a turn. |
| `ApprovePlan { session, plan_id, approved }` | Answer a `PlanProposed` gate. |
| `ApproveAction { session, request_id, approved }` | Answer an `ApprovalRequest` gate. |
| `Pause { session }` / `Resume { session }` | Cooperative pause at phase boundaries. |
| `Abort { session }` | End the current turn. |

### Events (engine -> client)

Every event is wrapped as `Event { seq: u64, session: SessionId, kind: EventKind }`. `seq` is
monotonic per session.

| EventKind | Meaning |
|---|---|
| `PlanProposed { plan_id, plan }` | A plan awaits approval. |
| `PlanDecided { plan_id, approved }` | Echo of the decision, so a late attacher can reconstruct state. |
| `StepStarted { step_id, description }` | Execution entered a plan step. |
| `StepFinished { step_id, ok }` | Step concluded. |
| `FileEdit { path, bytes_written }` | A write was applied. |
| `CommandRun { command, exit_code }` | A command was executed. |
| `ApprovalRequest { request_id, reason, detail }` | A gate is asking. |
| `Log { message }` | Human-readable progress. |
| `TokenUsage { input_tokens, output_tokens }` | Cumulative usage for the turn, display only. |
| `TurnComplete { ok }` | The turn ended. |
| `Error { code, message }` | Non-fatal or terminal error. |

`seq` exists so a reattaching client can replay from `last_seq`. Nothing detaches in this
slice; the counter is included because it is expensive to retrofit and free to add now.

`PlanDecided` is an event rather than private state for the same reason: a client attaching
mid-turn must be able to reconstruct where things stand from the event stream alone.

### Plan and scope

```rust
struct Plan { id: Uuid, summary: String, steps: Vec<PlanStep>, scope: Scope }
struct PlanStep { id: Uuid, description: String }
struct Scope { write_paths: Vec<String>, commands: Vec<CommandPattern> }
struct CommandPattern { program: String, args: Vec<ArgPattern> }
enum ArgPattern { Exact(String), Any }
enum GateReason { OutsideScope { what: String }, SensitiveFloor { path: PathBuf } }
```

The plan is structured data, not prose, because the gate enforces it mechanically.
`write_paths` are globs relative to the workspace root. `ArgPattern::Any` matches one argument
in that position, so `cargo test <any>` is expressible without permitting `cargo publish`. A
provider response that does not parse into a `Plan` is a turn error, not a free-form fallback.

There is deliberately no `network` field. Any program may egress, so a network flag could not
be enforced mechanically, and an unenforceable field in a security-relevant struct is worse
than none. Egress is governed by which programs `commands` admits.

**On "risk tiers".** The project's stated model was "plan-first approval + risk-tiered gates."
This design subsumes tiers into two mechanisms: declared scope, and the sensitive floor. A
separate per-tool tier ladder would be a second thing to keep in sync with scope and would not
change any outcome, since approving a plan already authorizes everything inside it. Tiers are
therefore not implemented, and the phrase is retired from ARCHITECTURE.md.

## Session lifetime

`Session` owns: `SessionId`, a `LocalWorkspace` rooted at a path, conversation history, the
currently approved `Plan`, the `seq` counter, a `ToolRegistry`, and a provider handle.

It runs on its own tokio task. Inbound `Command` arrives on an mpsc receiver. Outbound `Event`
goes to a `tokio::sync::broadcast` channel plus a bounded replay ring buffer. Broadcast rather
than mpsc because it costs nothing now and is what allows a human client and an agent peer to
observe one session later.

`Engine` holds `HashMap<SessionId, SessionHandle>`, so multiple concurrent sessions work from
day one and swapping mpsc for a socket touches only the transport.

## Turn state machine

```
SendPrompt
   |
   v
[Plan] ---- provider call; response must parse into Plan; emit PlanProposed
   |
   v
[AwaitingApproval] ---- park until ApprovePlan
   |                     rejected or Abort -> TurnComplete { ok: false }
   v
[Execute] ---- loop: provider proposes tool calls
   |             each call -> PlanGate -> Allow (dispatch) | Ask (park on ApprovalRequest)
   |             denial is fed back to the model; 3 consecutive denials -> abort
   v
[TurnComplete]
```

`PauseController` is consulted at phase boundaries and between tool calls, ported unchanged
from otto. Its default (`NeverPause`) keeps non-interactive runs unaffected.

Every gate fails closed. `Approver` implementations return `false` when they cannot obtain an
answer — for example a client that disconnects while an `ApprovalRequest` is parked. This is
otto's documented `Approver` contract and is preserved.

When the agent needs a genuinely different direction rather than one out-of-scope action, it
proposes an amended plan: a second `PlanProposed` with a new `plan_id`, approved once, which
replaces the session's approved scope. This avoids grinding a human through per-action prompts
and uses vocabulary that already exists.

## The gate

`PlanGate` implements `engine_core::PermissionGate`. It is deterministic and involves no LLM:

1. Path or command matches the **sensitive floor** -> `Ask` with `GateReason::SensitiveFloor`.
   Plan approval cannot silently unlock it; it is always surfaced to a human. It is `Ask` and
   not `Deny` so that legitimately editing a `.env` remains possible with one deliberate answer.
2. A **read** (`fs.read`, `fs.list`) inside the workspace root -> `Allow`. Reads are not
   scope-checked: an agent must be able to explore a repository to work in it, a plan cannot
   name in advance every file it needs to consult, and the floor already covers what must not
   be read. Only mutation and execution are scope-checked.
3. A **write or command** inside the approved `Scope` -> `Allow`.
4. Otherwise -> `Ask` with `GateReason::OutsideScope`.

### Enforcement details

These two decide whether the guardrail is real or decorative:

**Path canonicalization.** Scope globs are matched against canonicalized paths, with any path
escaping the workspace root rejected outright. Without this, `src/../../../etc/passwd`
satisfies a `src/**` scope.

**No shell strings.** The `bash` tool takes a program and an args vector and performs no shell
interpretation. A gate cannot meaningfully evaluate a shell string: `cargo test; rm -rf ~`
matches any pattern that permits `cargo test`. Shell features (pipes, redirection, chaining,
globbing, substitution) are unavailable through the tool. If a task genuinely needs them, that
is an explicit out-of-scope action that asks. This is accepted as occasionally inconvenient in
exchange for a gate that holds.

## Tools

| Tool | Risk surface | Gate interaction |
|---|---|---|
| `fs.read` | Reads a file. | Floor-checked only; confined to the workspace root. |
| `fs.list` | Lists paths by glob. | Floor-checked only; confined to the workspace root. |
| `fs.write` | Full-file write. | Floor-checked; scope-checked; emits `FileEdit`. |
| `bash` | Runs program + args, no shell. | Matched against `Scope::commands` by program and args; emits `CommandRun`. |

Agents receive the read-only `WorkspaceRead` view. Only the gated `fs.write` tool and the
orchestrator hold the writable `Workspace`, preserving otto's separation.

## Providers

`AnthropicProvider` and `ScriptedProvider` are ported first; OpenAI, Gemini, DeepSeek, and
Ollama follow unchanged from otto and are not part of the first slice. The `Provider` trait
(`id()`, `complete()`) ports as-is.

## Error handling

- Provider transport failures surface as `EventKind::Error` and end the turn with
  `TurnComplete { ok: false }`. They do not kill the session.
- A provider response that fails to parse into a `Plan` is a turn error with a stable code.
- Tool failures are returned to the model as tool results so it can adapt; they are not turn
  errors.
- Gate denials are fed back to the model, capped at three consecutive denials before abort.
- Session-fatal conditions (workspace unreadable, provider unconfigured) end the session with a
  terminal `Error`.

## Testing strategy

- **Gate:** table-driven unit tests over `(tool, args, scope)` triples, including the traversal
  escape and the shell-string cases named above. No I/O.
- **Turn state machine:** driven by `ScriptedProvider` so plan -> approve -> execute ->
  deviation-stop is deterministic and offline. This mirrors the existing `MemStore` pattern in
  `crates/auth`.
- **Tools:** against a `tempfile` workspace.
- **No network in tests.** The Anthropic provider is exercised against a `wiremock` server, as
  in otto.

## Assumptions

Stated explicitly because they were not settled during the brainstorm:

1. **Provider keys are read from local configuration** in this slice (environment variable or
   the existing TUI config file), not from the fly.io server. Server-side key custody was named
   as a future server role but is not built here.
2. **The consecutive-denial cap is three.** Chosen as a starting value; cheap to tune once the
   loop is real.
3. **Conversation history is in memory only.** Sessions do not survive an engine restart.
   Persisting them is a follow-on, and `seq` plus the event stream is the natural substrate.

## Follow-on work

In rough order: the local daemon and socket transport (detach/reattach, using `seq`); session
persistence; the remaining providers; the agent-to-agent bus on the fly.io server; Slack/JIRA/
GitHub integrations.
