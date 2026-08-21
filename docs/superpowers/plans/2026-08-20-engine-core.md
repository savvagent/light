# Engine Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a thin vertical slice of the light-factory engine — prompt → structured plan → human approval → gated execution — running in-process in the TUI behind the full Command/Event protocol.

**Architecture:** Port otto's proven trait seams (`engine-core`) and provider implementations, then write a new, small orchestrator shaped by light-factory's Command/Event vocabulary. The engine runs locally where the repo is. The TUI reaches it only by sending `Command` and receiving `Event` over an mpsc/broadcast pair, so a later daemon is a transport change rather than a redesign. Approving a plan authorizes everything inside its declared scope; a deterministic, non-LLM gate catches deviation and always asks on sensitive paths.

**Tech Stack:** Rust 1.97.0 (edition 2024), tokio, serde, async-trait, reqwest (rustls), ratatui, anyhow, thiserror. Tests: tokio-test, tempfile, wiremock.

**Spec:** `docs/superpowers/specs/2026-08-20-engine-core-design.md`

## Global Constraints

- **Rust 1.97.0**, edition 2024. Workspace deps are declared in the root `Cargo.toml` and referenced as `{ workspace = true }` — never version-pinned per crate.
- **Dependency flow is inward and must not be violated:** `protocol ← engine-core ← {providers, tools} ← engine ← tui`. `protocol` depends on no other workspace crate.
- **No comments unless asked.** Doc comments (`///`, `//!`) on public items are encouraged and expected.
- **Every gate fails closed.** A gate that cannot obtain an answer resolves to denial. A disconnected client, a closed command channel, and an abort are all denials — never approvals.
- **The `bash` tool takes a program and an args vector and performs no shell interpretation.** No `sh -c`, no pipes, no redirection, no glob expansion, no command chaining.
- **Sensitive-path floor is `Ask`, never silent `Allow`.** Plan approval cannot bypass it.
- **No network in tests.** Providers are tested against `wiremock`; engine turns against `ScriptedProvider`.
- `cargo fmt`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace` must pass before any task is considered done.
- Ported files come from `/home/robhicks/dev/otto` (same author, same MIT/Apache-2.0 dual license). Porting means copy, rename `otto_*` → `light_factory_*`, strip anything the spec's non-goals exclude.
- **Naming:** crate packages are `light-factory-<name>`; the crate identifier in code is `light_factory_<name>`.

### Execution order across two plans

This plan interlocks with `docs/superpowers/plans/2026-08-20-port-llm-providers.md`, which ports
otto's seven providers, the `base_url` trust boundary, and env-driven provider selection. Run them
in this order:

1. **This plan, Tasks 1–4** — toolchain, `protocol`, and `crates/engine-core`.
2. **All of the port-llm-providers plan** — it depends on `engine-core` for the `Provider` trait
   and now says so in its Task 1.
3. **This plan, Tasks 5 onward** — tools, gate, session, turn machine, registry, TUI.

This plan originally contained its own tasks porting `ScriptedProvider` and `AnthropicProvider`.
They are removed: the other plan ports all seven providers plus the `base_url` trust boundary
(cross-host redirect rejection and loopback checks), which is strictly better than the two-provider
slice this plan had. From Task 5 onward, assume `light-factory-providers` exists and exposes
`ScriptedProvider` for offline tests.

### Provider interface note

otto's `Provider` trait is **prompt-and-parse**, not native tool calling: `CompleteRequest { prompt: String }` in, `CompleteResponse { text, usage }` out, with structured data extracted from the model's text via `extract_json`. This plan keeps that interface unchanged. The engine renders prompts (including history) and parses JSON out of completions. Consequences, accepted deliberately:

- All providers work identically, including local/Ollama models with no tool-calling API.
- The model's plan and tool calls are JSON in a fenced block, not native `tool_use` blocks.
- Native tool calling is a follow-on, and would be an additive change to `CompleteRequest`.

---

## File Structure

| File | Responsibility |
|---|---|
| `rust-toolchain.toml` | Toolchain pin, bumped to 1.97.0 |
| `crates/protocol/src/sensitive.rs` | The sensitive-path floor. Pure string logic, no deps. |
| `crates/protocol/src/session.rs` | `SessionId`, `Command`, `Event`, `EventKind`, `Plan`, `Scope`, `GateReason` |
| `crates/engine-core/src/traits.rs` | `Provider`, `Workspace`, `WorkspaceRead` |
| `crates/engine-core/src/types.rs` | `CompleteRequest`, `CompleteResponse`, `Usage`, `Edit` |
| `crates/engine-core/src/tool.rs` | `Tool`, `Decision`, `PermissionGate`, `PauseController` |
| `crates/providers/*` | Built by the port-llm-providers plan, not this one |
| `crates/tools/src/workspace.rs` | `LocalWorkspace` — the only `Workspace` impl |
| `crates/tools/src/fs.rs` | `fs.read`, `fs.list`, `fs.write` tools |
| `crates/tools/src/bash.rs` | `bash` tool — program + args, no shell |
| `crates/engine/src/gate.rs` | `PlanGate` — the deterministic guardrail |
| `crates/engine/src/session.rs` | `Session` actor: state, event emission, `seq` |
| `crates/engine/src/turn.rs` | Turn state machine: plan → approve → execute → complete |
| `crates/engine/src/prompt.rs` | Prompt rendering and JSON extraction from completions |
| `crates/engine/src/lib.rs` | `Engine`: session registry, `Command` routing |
| `crates/tui/src/engine_view.rs` | TUI rendering of engine events and the approval gate |

---

### Task 1: Bump the toolchain to 1.97.0

The otto port requires 1.97.0. Doing this first means every later task compiles under the final toolchain.

**Files:**
- Modify: `rust-toolchain.toml`
- Modify: `.github/workflows/deploy.yml`

**Interfaces:**
- Consumes: nothing.
- Produces: a workspace that builds on 1.97.0.

- [ ] **Step 1: Bump the pin**

```toml
[toolchain]
channel = "1.97.0"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [ ] **Step 2: Resolve the `dtolnay/rust-toolchain` SHA**

Per repo convention, actions are pinned to full 40-char commit SHAs. `dtolnay/rust-toolchain` is pinned to a `stable` branch-tip commit and must be bumped explicitly on toolchain changes.

Run: `git ls-remote https://github.com/dtolnay/rust-toolchain.git refs/heads/stable`

Take the 40-char SHA from column 1 and replace the ref in `.github/workflows/deploy.yml` (the `uses: dtolnay/rust-toolchain@...` line). Leave every other `uses:` untouched.

- [ ] **Step 3: Verify the workspace still builds and lints clean**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace`
Expected: PASS. A new toolchain can introduce new lints; fix any that appear rather than allowing them.

- [ ] **Step 4: Commit**

```bash
git add rust-toolchain.toml .github/workflows/deploy.yml
git commit -m "chore: bump toolchain to 1.97.0 for the engine port"
```

---

### Task 2: Port the sensitive-path floor into `protocol`

**Files:**
- Create: `crates/protocol/src/sensitive.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `light_factory_protocol::sensitive::{SENSITIVE_MARKERS, is_sensitive}`, re-exported at the crate root as `light_factory_protocol::{SENSITIVE_MARKERS, is_sensitive}`. `is_sensitive(&str) -> bool`.

- [ ] **Step 1: Write the failing test**

Create `crates/protocol/src/sensitive.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_secrets_case_insensitively() {
        assert!(is_sensitive(".env"));
        assert!(is_sensitive("config/.ENV.local"));
        assert!(is_sensitive(".ssh/id_rsa"));
        assert!(is_sensitive("ID_RSA"));
        assert!(is_sensitive("config/production.env"));
        assert!(!is_sensitive("src/main.rs"));
        assert!(!is_sensitive("crates/engine/src/gate.rs"));
    }
}
```

Add `pub mod sensitive;` and `pub use sensitive::{SENSITIVE_MARKERS, is_sensitive};` to `crates/protocol/src/lib.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p light-factory-protocol sensitive`
Expected: FAIL — `cannot find function 'is_sensitive' in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/protocol/src/sensitive.rs` (this is otto's `crates/protocol/src/sensitive.rs` verbatim apart from the doc comment, which is rewritten because otto's names enforcers that do not exist here):

```rust
//! The canonical sensitive-path floor: substrings that mark a path as holding secrets. It
//! lives in `protocol` — the dependency-free leaf crate — so every enforcer shares one list
//! without pulling in engine logic. Keeping one list here makes drift between enforcers
//! impossible.

/// Lowercase substrings that mark a path as sensitive. Matching is case-insensitive (see
/// [`is_sensitive`]). Symlink-to-secret escapes are out of scope for this string floor.
pub const SENSITIVE_MARKERS: &[&str] = &[
    ".env", ".ssh/", ".ssh", ".git/", ".git", "id_rsa", ".aws/", ".aws",
];

/// True if `s` names a sensitive path under the floor. Case-insensitive (ASCII) substring
/// match, so `.ENV` / `.AWS/...` cannot slip past on case-insensitive filesystems.
pub fn is_sensitive(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    SENSITIVE_MARKERS.iter().any(|m| lower.contains(m))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p light-factory-protocol sensitive`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol/src/sensitive.rs crates/protocol/src/lib.rs
git commit -m "feat(protocol): add the sensitive-path floor"
```

---

### Task 3: Add the session vocabulary to `protocol`

**Files:**
- Create: `crates/protocol/src/session.rs`
- Modify: `crates/protocol/src/lib.rs`
- Modify: `crates/protocol/Cargo.toml`

**Interfaces:**
- Consumes: nothing.
- Produces: `SessionId`, `Command`, `Event`, `EventKind`, `Plan`, `PlanStep`, `Scope`, `CommandPattern`, `ArgPattern`, `GateReason`. Every type is `Serialize + Deserialize + Debug + Clone`. `SessionId` is a newtype over `Uuid` with `SessionId::new()`.

- [ ] **Step 1: Add the uuid dependency**

`crates/protocol/Cargo.toml` — add to `[dependencies]`:

```toml
uuid = { workspace = true, features = ["serde"] }
```

Then in the root `Cargo.toml`, change the workspace uuid entry to include serde:

```toml
uuid = { version = "1", features = ["v4", "serde"] }
```

- [ ] **Step 2: Write the failing test**

Create `crates/protocol/src/session.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_round_trips_through_json() {
        let session = SessionId::new();
        let cmd = Command::ApprovePlan {
            session,
            plan_id: uuid::Uuid::nil(),
            approved: true,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{back:?}"), format!("{cmd:?}"));
    }

    #[test]
    fn event_carries_seq_and_session() {
        let session = SessionId::new();
        let ev = Event {
            seq: 7,
            session,
            kind: EventKind::TurnComplete { ok: true },
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(back.seq, 7);
        assert_eq!(back.session, session);
    }

    #[test]
    fn scope_defaults_to_empty_and_denies_everything() {
        let scope = Scope::default();
        assert!(scope.write_paths.is_empty());
        assert!(scope.commands.is_empty());
    }
}
```

Add `pub mod session;` to `crates/protocol/src/lib.rs`.

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p light-factory-protocol session`
Expected: FAIL — `cannot find type 'SessionId' in this scope`.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/protocol/src/session.rs`:

```rust
//! The engine protocol: commands a client sends, events the engine emits, and the plan
//! vocabulary the gate enforces. Serde only, no I/O.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies a single agent session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// One argument position in a [`CommandPattern`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArgPattern {
    /// Matches exactly this argument.
    Exact(String),
    /// Matches any single argument in this position.
    Any,
}

/// A command the plan authorizes: a program plus a positional argument pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandPattern {
    pub program: String,
    pub args: Vec<ArgPattern>,
}

/// What an approved plan authorizes. An empty scope authorizes nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// Globs, relative to the workspace root, that the plan may write to.
    pub write_paths: Vec<String>,
    /// Commands the plan may run.
    pub commands: Vec<CommandPattern>,
}

/// One unit of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: Uuid,
    pub description: String,
}

/// A structured plan. Approving it authorizes everything inside [`Plan::scope`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub summary: String,
    pub steps: Vec<PlanStep>,
    pub scope: Scope,
}

/// Why a gate is asking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GateReason {
    /// The action falls outside the approved plan's scope.
    OutsideScope { what: String },
    /// The path is on the sensitive floor. Plan approval never unlocks this.
    SensitiveFloor { path: PathBuf },
}

/// Commands sent from a client to the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    CreateSession { workspace: PathBuf },
    SendPrompt { session: SessionId, text: String },
    ApprovePlan { session: SessionId, plan_id: Uuid, approved: bool },
    ApproveAction { session: SessionId, request_id: Uuid, approved: bool },
    Pause { session: SessionId },
    Resume { session: SessionId },
    Abort { session: SessionId },
}

/// The body of an event emitted by the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    PlanProposed { plan_id: Uuid, plan: Plan },
    PlanDecided { plan_id: Uuid, approved: bool },
    StepStarted { step_id: Uuid, description: String },
    StepFinished { step_id: Uuid, ok: bool },
    FileEdit { path: PathBuf, bytes_written: u64 },
    CommandRun { command: String, exit_code: i32 },
    ApprovalRequest { request_id: Uuid, reason: GateReason, detail: String },
    Log { message: String },
    TokenUsage { input_tokens: u64, output_tokens: u64 },
    TurnComplete { ok: bool },
    Error { code: String, message: String },
}

/// A sequenced, session-scoped event. `seq` is monotonic per session so a reattaching
/// client can replay from its last seen value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub session: SessionId,
    pub kind: EventKind,
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p light-factory-protocol`
Expected: PASS (3 new tests plus existing).

- [ ] **Step 6: Commit**

```bash
git add crates/protocol Cargo.toml
git commit -m "feat(protocol): add the engine Command/Event vocabulary"
```

---

### Task 4: Create the `engine-core` crate

Ports otto's trait seams. This crate has no I/O and no web framework — it is the seam every later crate implements.

**Files:**
- Create: `crates/engine-core/Cargo.toml`
- Create: `crates/engine-core/src/lib.rs`
- Create: `crates/engine-core/src/types.rs`
- Create: `crates/engine-core/src/traits.rs`
- Create: `crates/engine-core/src/tool.rs`
- Modify: `Cargo.toml` (workspace deps: add `async-trait`)

**Interfaces:**
- Consumes: `light_factory_protocol::is_sensitive`.
- Produces:
  - `types`: `CompleteRequest { prompt: String }`, `CompleteResponse { text: String, usage: Option<Usage> }`, `Usage { input_tokens: u32, output_tokens: u32 }`, `Edit { path: PathBuf, new_contents: String }`
  - `traits`: `Provider { fn id(&self) -> &str; async fn complete(&self, req: CompleteRequest) -> anyhow::Result<CompleteResponse> }`, `WorkspaceRead { async fn read(&self, path: &Path) -> anyhow::Result<Vec<u8>>; async fn list(&self, glob: &str) -> anyhow::Result<Vec<PathBuf>> }`, `Workspace: WorkspaceRead { async fn apply_edit(&self, edit: &Edit) -> anyhow::Result<u64> }`
  - `tool`: `Tool { fn name(&self) -> &str; async fn call(&self, args: Value) -> anyhow::Result<Value> }`, `Decision { Allow, Ask(GateReason), Deny }`, `PermissionGate { fn evaluate(&self, tool: &str, args: &Value) -> Decision }`, `PauseController { fn should_pause(&self) -> bool; async fn wait_for_resume(&self) }`, `NeverPause`

**Deviation from the spec, deliberate:** the spec has the session own a `ToolRegistry` that runs the gate and an `Approver` before dispatch. This plan omits both. The turn state machine (Task 10) must emit events and drive the approval round-trip through its own command receiver, which needs `&mut Session` — so routing dispatch through a registry would mean an `Approver` that calls back into the session it is owned by. The gate's *behavior* is unchanged, including fail-closed on a closed channel (`await_action_decision` returns `false`). `ToolRegistry`/`Approver` return when a second client type actually needs them. This keeps engine-core smaller, which is the project's stated discipline.

- [ ] **Step 1: Create the manifest**

`crates/engine-core/Cargo.toml`:

```toml
[package]
name = "light-factory-engine-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
light-factory-protocol = { path = "../protocol" }
async-trait = { workspace = true }
anyhow = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt"] }
```

Add to the root `Cargo.toml` `[workspace.dependencies]`:

```toml
async-trait = "0.1"
```

- [ ] **Step 2: Write the failing test**

`crates/engine-core/src/tool.rs` — test module only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        async fn call(&self, args: Value) -> anyhow::Result<Value> {
            Ok(args)
        }
    }

    struct FixedGate(Decision);

    impl PermissionGate for FixedGate {
        fn evaluate(&self, _tool: &str, _args: &Value) -> Decision {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn a_tool_dispatches_by_name() {
        let tool = EchoTool;
        assert_eq!(tool.name(), "echo");
        let out = tool.call(serde_json::json!({"x": 1})).await.unwrap();
        assert_eq!(out, serde_json::json!({"x": 1}));
    }

    #[test]
    fn a_gate_returns_its_verdict() {
        let gate = FixedGate(Decision::Deny);
        assert_eq!(gate.evaluate("anything", &serde_json::json!({})), Decision::Deny);
    }

    #[tokio::test]
    async fn never_pause_does_not_park() {
        let pause = NeverPause;
        assert!(!pause.should_pause());
        pause.wait_for_resume().await;
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p light-factory-engine-core`
Expected: FAIL — the crate does not compile; `Tool`, `Decision`, `PermissionGate`, and `NeverPause` are undefined.

- [ ] **Step 4: Write `types.rs`**

```rust
//! Plain data passed across the trait seams. No behavior.

use std::path::PathBuf;

/// A request to an LLM provider. Prompt-and-parse: the engine renders the whole prompt,
/// including any history, and parses structure out of the completion text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteRequest {
    pub prompt: String,
}

/// Token usage reported by a provider. `None` for providers that do not report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A provider's completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteResponse {
    pub text: String,
    pub usage: Option<Usage>,
}

/// A single full-file edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub path: PathBuf,
    pub new_contents: String,
}
```

- [ ] **Step 5: Write `traits.rs`**

```rust
//! The trait seams the engine drives.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::types::{CompleteRequest, CompleteResponse, Edit};

/// An LLM provider.
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    async fn complete(&self, req: CompleteRequest) -> anyhow::Result<CompleteResponse>;
}

/// Read access to the workspace. This is the agent-facing view: agents may read, never mutate.
#[async_trait]
pub trait WorkspaceRead: Send + Sync {
    async fn read(&self, path: &Path) -> anyhow::Result<Vec<u8>>;
    async fn list(&self, glob: &str) -> anyhow::Result<Vec<PathBuf>>;
}

/// The writable workspace. Only the orchestrator and the gated `fs.write` tool hold this.
#[async_trait]
pub trait Workspace: WorkspaceRead {
    /// Apply a full-file edit, returning the number of bytes written.
    async fn apply_edit(&self, edit: &Edit) -> anyhow::Result<u64>;
}
```

- [ ] **Step 6: Write `tool.rs`**

Prepend above the test module written in Step 2:

```rust
//! The tool seam. Every call runs a deterministic `PermissionGate` before dispatch.

use async_trait::async_trait;
use light_factory_protocol::session::GateReason;
use serde_json::Value;

/// A callable tool: a stable name and a JSON-in / JSON-out call.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    async fn call(&self, args: Value) -> anyhow::Result<Value>;
}

/// The verdict a permission gate returns for a proposed tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask(GateReason),
    Deny,
}

/// Deterministic, non-LLM evaluation of a proposed tool call.
pub trait PermissionGate: Send + Sync {
    fn evaluate(&self, tool: &str, args: &Value) -> Decision;
}

/// Cooperative pause, consulted at phase boundaries and between tool calls.
#[async_trait]
pub trait PauseController: Send + Sync {
    fn should_pause(&self) -> bool;
    async fn wait_for_resume(&self);
}

/// Default: never pauses.
pub struct NeverPause;

#[async_trait]
impl PauseController for NeverPause {
    fn should_pause(&self) -> bool {
        false
    }
    async fn wait_for_resume(&self) {}
}

```

`crates/engine-core/src/lib.rs`:

```rust
//! Engine trait seams: providers, tools, workspaces, and the permission gate. No I/O.

pub mod tool;
pub mod traits;
pub mod types;

pub use tool::{Decision, NeverPause, PauseController, PermissionGate, Tool};
pub use traits::{Provider, Workspace, WorkspaceRead};
pub use types::{CompleteRequest, CompleteResponse, Edit, Usage};
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p light-factory-engine-core`
Expected: PASS (3 tests).

- [ ] **Step 8: Commit**

```bash
git add crates/engine-core Cargo.toml
git commit -m "feat(engine-core): port the provider, tool, and workspace seams"
```

---

### Task 5: Create the `tools` crate with `LocalWorkspace` and the fs tools

**Files:**
- Create: `crates/tools/Cargo.toml`
- Create: `crates/tools/src/lib.rs`
- Create: `crates/tools/src/workspace.rs`
- Create: `crates/tools/src/fs.rs`

**Interfaces:**
- Consumes: `Workspace`, `WorkspaceRead`, `Edit`, `Tool`.
- Produces: `LocalWorkspace::new(root: PathBuf) -> anyhow::Result<Self>`, `LocalWorkspace::resolve(&self, path: &Path) -> anyhow::Result<PathBuf>` (canonicalizing, rejecting escapes), and tools `FsReadTool`, `FsListTool`, `FsWriteTool`, each `Tool` with names `fs.read`, `fs.list`, `fs.write`.
- `fs.read` args: `{"path": String}` → `{"contents": String}`. `fs.list` args: `{"glob": String}` → `{"paths": [String]}`. `fs.write` args: `{"path": String, "contents": String}` → `{"bytes_written": u64}`.

- [ ] **Step 1: Create the manifest**

`crates/tools/Cargo.toml`:

```toml
[package]
name = "light-factory-tools"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
light-factory-engine-core = { path = "../engine-core" }
light-factory-protocol = { path = "../protocol" }
async-trait = { workspace = true }
anyhow = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
glob = "0.3"

[dev-dependencies]
tempfile = "3"
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

Add `glob = "0.3"` and `tempfile = "3"` to the root `[workspace.dependencies]` if you prefer central management; this plan uses direct versions to keep the crate self-contained.

- [ ] **Step 2: Write the failing test**

`crates/tools/tests/workspace.rs`:

```rust
use std::path::Path;

use light_factory_tools::LocalWorkspace;

#[test]
fn resolve_rejects_paths_escaping_the_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let ws = LocalWorkspace::new(dir.path().to_path_buf()).unwrap();

    assert!(ws.resolve(Path::new("src")).is_ok());
    assert!(ws.resolve(Path::new("src/../src")).is_ok());

    assert!(ws.resolve(Path::new("../outside")).is_err());
    assert!(ws.resolve(Path::new("src/../../../etc/passwd")).is_err());
    assert!(ws.resolve(Path::new("/etc/passwd")).is_err());
}

#[tokio::test]
async fn write_then_read_round_trips() {
    use light_factory_engine_core::traits::{Workspace, WorkspaceRead};
    use light_factory_engine_core::types::Edit;

    let dir = tempfile::tempdir().unwrap();
    let ws = LocalWorkspace::new(dir.path().to_path_buf()).unwrap();

    let written = ws
        .apply_edit(&Edit {
            path: "notes.txt".into(),
            new_contents: "hello".into(),
        })
        .await
        .unwrap();
    assert_eq!(written, 5);

    let back = ws.read(Path::new("notes.txt")).await.unwrap();
    assert_eq!(String::from_utf8(back).unwrap(), "hello");
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p light-factory-tools`
Expected: FAIL — `LocalWorkspace` undefined.

- [ ] **Step 4: Write `workspace.rs`**

```rust
//! `LocalWorkspace`: the only `Workspace` implementation. Edits a real directory in place.

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use light_factory_engine_core::traits::{Workspace, WorkspaceRead};
use light_factory_engine_core::types::Edit;

/// A workspace rooted at a real directory. Every path is resolved against the root and
/// rejected if it escapes, so a `..` segment cannot reach outside the repository.
pub struct LocalWorkspace {
    root: PathBuf,
}

impl LocalWorkspace {
    pub fn new(root: PathBuf) -> anyhow::Result<Self> {
        let root = root
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("workspace root {}: {e}", root.display()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve `path` (relative to the root) into an absolute path inside the root.
    ///
    /// Rejects absolute inputs and any path whose lexical resolution escapes the root. The
    /// check is lexical rather than `canonicalize`-based so it also applies to files that do
    /// not exist yet — which is the case for every new file a plan creates.
    pub fn resolve(&self, path: &Path) -> anyhow::Result<PathBuf> {
        if path.is_absolute() {
            anyhow::bail!("absolute paths are not permitted: {}", path.display());
        }

        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Normal(c) => out.push(c),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        anyhow::bail!("path escapes the workspace: {}", path.display());
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    anyhow::bail!("path escapes the workspace: {}", path.display());
                }
            }
        }

        Ok(self.root.join(out))
    }
}

#[async_trait]
impl WorkspaceRead for LocalWorkspace {
    async fn read(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        let full = self.resolve(path)?;
        Ok(tokio::fs::read(full).await?)
    }

    async fn list(&self, pattern: &str) -> anyhow::Result<Vec<PathBuf>> {
        let root = self.root.clone();
        let pattern = pattern.to_string();
        let paths = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<PathBuf>> {
            let full = format!("{}/{}", root.display(), pattern);
            let mut out = Vec::new();
            for entry in glob::glob(&full)? {
                let entry = entry?;
                if let Ok(rel) = entry.strip_prefix(&root) {
                    out.push(rel.to_path_buf());
                }
            }
            Ok(out)
        })
        .await??;
        Ok(paths)
    }
}

#[async_trait]
impl Workspace for LocalWorkspace {
    async fn apply_edit(&self, edit: &Edit) -> anyhow::Result<u64> {
        let full = self.resolve(&edit.path)?;
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&full, edit.new_contents.as_bytes()).await?;
        Ok(edit.new_contents.len() as u64)
    }
}
```

- [ ] **Step 5: Write `fs.rs`**

```rust
//! The filesystem tools. Each is a thin JSON adapter over the workspace; all gating happens
//! in the registry before dispatch.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use light_factory_engine_core::traits::{Workspace, WorkspaceRead};
use light_factory_engine_core::tool::Tool;
use light_factory_engine_core::types::Edit;
use serde_json::{Value, json};

pub struct FsReadTool {
    pub workspace: Arc<dyn WorkspaceRead>,
}

#[async_trait]
impl Tool for FsReadTool {
    fn name(&self) -> &str {
        "fs.read"
    }

    async fn call(&self, args: Value) -> anyhow::Result<Value> {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("fs.read requires a string `path`"))?;
        let bytes = self.workspace.read(Path::new(path)).await?;
        Ok(json!({ "contents": String::from_utf8_lossy(&bytes) }))
    }
}

pub struct FsListTool {
    pub workspace: Arc<dyn WorkspaceRead>,
}

#[async_trait]
impl Tool for FsListTool {
    fn name(&self) -> &str {
        "fs.list"
    }

    async fn call(&self, args: Value) -> anyhow::Result<Value> {
        let pattern = args
            .get("glob")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("fs.list requires a string `glob`"))?;
        let paths = self.workspace.list(pattern).await?;
        let paths: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        Ok(json!({ "paths": paths }))
    }
}

pub struct FsWriteTool {
    pub workspace: Arc<dyn Workspace>,
}

#[async_trait]
impl Tool for FsWriteTool {
    fn name(&self) -> &str {
        "fs.write"
    }

    async fn call(&self, args: Value) -> anyhow::Result<Value> {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("fs.write requires a string `path`"))?;
        let contents = args
            .get("contents")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("fs.write requires string `contents`"))?;

        let bytes_written = self
            .workspace
            .apply_edit(&Edit {
                path: path.into(),
                new_contents: contents.to_string(),
            })
            .await?;

        Ok(json!({ "bytes_written": bytes_written }))
    }
}
```

`crates/tools/src/lib.rs`:

```rust
//! Tools the engine exposes to the agent, plus the local workspace they operate on.

pub mod bash;
pub mod fs;
pub mod workspace;

pub use bash::BashTool;
pub use fs::{FsListTool, FsReadTool, FsWriteTool};
pub use workspace::LocalWorkspace;
```

`bash` is written in Task 6. To keep this task's deliverable compiling on its own, omit the two bash lines from `lib.rs` for now — write it as:

```rust
//! Tools the engine exposes to the agent, plus the local workspace they operate on.

pub mod fs;
pub mod workspace;

pub use fs::{FsListTool, FsReadTool, FsWriteTool};
pub use workspace::LocalWorkspace;
```

Task 6 adds `pub mod bash;` and `pub use bash::BashTool;` back.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p light-factory-tools`
Expected: PASS (2 tests).

- [ ] **Step 7: Commit**

```bash
git add crates/tools
git commit -m "feat(tools): add LocalWorkspace and the fs tools"
```

---

### Task 6: Add the `bash` tool with no shell interpretation

This is a security-critical task. The tool must never invoke a shell.

**Files:**
- Create: `crates/tools/src/bash.rs`
- Modify: `crates/tools/src/lib.rs`

**Interfaces:**
- Consumes: `Tool`, `LocalWorkspace`.
- Produces: `BashTool { workspace_root: PathBuf }`, tool name `bash`. Args: `{"program": String, "args": [String]}` → `{"exit_code": i32, "stdout": String, "stderr": String}`.

- [ ] **Step 1: Write the failing test**

`crates/tools/tests/bash.rs`:

```rust
use light_factory_engine_core::tool::Tool;
use light_factory_tools::BashTool;
use serde_json::json;

#[tokio::test]
async fn runs_a_program_with_args() {
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool { workspace_root: dir.path().to_path_buf() };

    let out = tool
        .call(json!({ "program": "echo", "args": ["hello", "world"] }))
        .await
        .unwrap();

    assert_eq!(out["exit_code"], 0);
    assert_eq!(out["stdout"].as_str().unwrap().trim(), "hello world");
}

#[tokio::test]
async fn does_not_interpret_shell_metacharacters() {
    let dir = tempfile::tempdir().unwrap();
    let canary = dir.path().join("canary.txt");
    std::fs::write(&canary, "intact").unwrap();

    let tool = BashTool { workspace_root: dir.path().to_path_buf() };

    // If a shell were involved, `;` would chain a second command and delete the canary.
    let out = tool
        .call(json!({ "program": "echo", "args": ["hi; rm -f canary.txt"] }))
        .await
        .unwrap();

    assert_eq!(out["stdout"].as_str().unwrap().trim(), "hi; rm -f canary.txt");
    assert_eq!(std::fs::read_to_string(&canary).unwrap(), "intact");
}

#[tokio::test]
async fn rejects_a_program_containing_a_path_separator() {
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool { workspace_root: dir.path().to_path_buf() };

    let err = tool
        .call(json!({ "program": "/bin/sh", "args": ["-c", "echo pwned"] }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("must be a bare program name"));
}

#[tokio::test]
async fn non_zero_exit_is_reported_not_raised() {
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool { workspace_root: dir.path().to_path_buf() };

    let out = tool
        .call(json!({ "program": "false", "args": [] }))
        .await
        .unwrap();
    assert_ne!(out["exit_code"], 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p light-factory-tools --test bash`
Expected: FAIL — `BashTool` undefined.

- [ ] **Step 3: Write the implementation**

`crates/tools/src/bash.rs`:

```rust
//! The command tool. Takes a program and an argument vector and runs it directly — there is
//! no shell, so no pipes, redirection, chaining, globbing, or substitution.
//!
//! This is deliberate. A permission gate cannot meaningfully evaluate a shell string:
//! `cargo test; rm -rf ~` matches any pattern that permits `cargo test`. Keeping the argument
//! vector structured is what makes the gate enforceable.

use std::path::PathBuf;

use async_trait::async_trait;
use light_factory_engine_core::tool::Tool;
use serde_json::{Value, json};

pub struct BashTool {
    pub workspace_root: PathBuf,
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    async fn call(&self, args: Value) -> anyhow::Result<Value> {
        let program = args
            .get("program")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("bash requires a string `program`"))?;

        if program.contains('/') || program.contains('\\') {
            anyhow::bail!("`program` must be a bare program name, not a path: {program}");
        }

        let argv: Vec<String> = args
            .get("args")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("bash requires an array `args`"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow::anyhow!("every element of `args` must be a string"))
            })
            .collect::<anyhow::Result<_>>()?;

        let output = tokio::process::Command::new(program)
            .args(&argv)
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        Ok(json!({
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }))
    }
}
```

Ensure `tokio` in `crates/tools/Cargo.toml` includes the `process` feature (the workspace `tokio` uses `features = ["full"]`, which already includes it).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p light-factory-tools`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/tools
git commit -m "feat(tools): add the bash tool with no shell interpretation"
```

---

### Task 7: Build `PlanGate` — the deterministic guardrail

The security core of the whole design. Test it hard.

**Files:**
- Create: `crates/engine/Cargo.toml`
- Create: `crates/engine/src/lib.rs`
- Create: `crates/engine/src/gate.rs`

**Interfaces:**
- Consumes: `PermissionGate`, `Decision`, `light_factory_protocol::session::{Scope, CommandPattern, ArgPattern, GateReason}`, `is_sensitive`.
- Produces: `PlanGate::new(scope: Option<Scope>) -> Self` and `PlanGate::with_scope(&mut self, scope: Scope)`. Implements `PermissionGate`. With `None` scope, every write and command is `Ask(OutsideScope)`.

- [ ] **Step 1: Create the manifest**

`crates/engine/Cargo.toml`:

```toml
[package]
name = "light-factory-engine"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
light-factory-protocol = { path = "../protocol" }
light-factory-engine-core = { path = "../engine-core" }
light-factory-providers = { path = "../providers" }
light-factory-tools = { path = "../tools" }
async-trait = { workspace = true }
anyhow = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
uuid = { workspace = true }
tracing = { workspace = true }
glob = "0.3"

[dev-dependencies]
tempfile = "3"
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "time"] }
```

- [ ] **Step 2: Write the failing test**

`crates/engine/tests/gate.rs`:

```rust
use light_factory_engine::gate::PlanGate;
use light_factory_engine_core::tool::{Decision, PermissionGate};
use light_factory_protocol::session::{ArgPattern, CommandPattern, GateReason, Scope};
use serde_json::json;

fn scope() -> Scope {
    Scope {
        write_paths: vec!["src/**".into(), "README.md".into()],
        commands: vec![CommandPattern {
            program: "cargo".into(),
            args: vec![ArgPattern::Exact("test".into()), ArgPattern::Any],
        }],
    }
}

#[test]
fn reads_are_allowed_anywhere_in_the_workspace() {
    let gate = PlanGate::new(Some(scope()));
    assert_eq!(gate.evaluate("fs.read", &json!({"path": "docs/whatever.md"})), Decision::Allow);
    assert_eq!(gate.evaluate("fs.list", &json!({"glob": "**/*.rs"})), Decision::Allow);
}

#[test]
fn reads_of_sensitive_paths_still_ask() {
    let gate = PlanGate::new(Some(scope()));
    assert!(matches!(
        gate.evaluate("fs.read", &json!({"path": ".env"})),
        Decision::Ask(GateReason::SensitiveFloor { .. })
    ));
    assert!(matches!(
        gate.evaluate("fs.read", &json!({"path": "home/.ssh/id_rsa"})),
        Decision::Ask(GateReason::SensitiveFloor { .. })
    ));
}

#[test]
fn writes_inside_scope_are_allowed() {
    let gate = PlanGate::new(Some(scope()));
    assert_eq!(gate.evaluate("fs.write", &json!({"path": "src/main.rs"})), Decision::Allow);
    assert_eq!(gate.evaluate("fs.write", &json!({"path": "README.md"})), Decision::Allow);
}

#[test]
fn writes_outside_scope_ask() {
    let gate = PlanGate::new(Some(scope()));
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": "Cargo.toml"})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
}

#[test]
fn traversal_escapes_cannot_satisfy_a_scope_glob() {
    let gate = PlanGate::new(Some(scope()));
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": "src/../../../etc/passwd"})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": "/etc/passwd"})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
}

#[test]
fn sensitive_writes_ask_even_when_a_glob_would_match() {
    let gate = PlanGate::new(Some(Scope {
        write_paths: vec!["**".into()],
        commands: vec![],
    }));
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": ".env"})),
        Decision::Ask(GateReason::SensitiveFloor { .. })
    ));
}

#[test]
fn commands_match_program_and_arg_patterns() {
    let gate = PlanGate::new(Some(scope()));
    assert_eq!(
        gate.evaluate("bash", &json!({"program": "cargo", "args": ["test", "--workspace"]})),
        Decision::Allow
    );
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "cargo", "args": ["publish", "--x"]})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "rm", "args": ["-rf", "/"]})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
}

#[test]
fn arity_must_match_the_pattern() {
    let gate = PlanGate::new(Some(scope()));
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "cargo", "args": ["test"]})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "cargo", "args": ["test", "a", "b"]})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
}

#[test]
fn no_approved_plan_means_nothing_is_authorized() {
    let gate = PlanGate::new(None);
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": "src/main.rs"})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "cargo", "args": ["test", "x"]})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert_eq!(gate.evaluate("fs.read", &json!({"path": "src/main.rs"})), Decision::Allow);
}

#[test]
fn unknown_tools_are_denied() {
    let gate = PlanGate::new(Some(scope()));
    assert_eq!(gate.evaluate("net.fetch", &json!({})), Decision::Deny);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p light-factory-engine --test gate`
Expected: FAIL — `PlanGate` undefined.

- [ ] **Step 4: Write the implementation**

`crates/engine/src/gate.rs`:

```rust
//! `PlanGate`: the deterministic, non-LLM guardrail. Reads are free inside the workspace;
//! writes and commands must fall inside the approved plan's scope; the sensitive-path floor
//! always asks and is never unlocked by plan approval.

use std::path::{Component, Path, PathBuf};

use light_factory_engine_core::tool::{Decision, PermissionGate};
use light_factory_protocol::is_sensitive;
use light_factory_protocol::session::{ArgPattern, CommandPattern, GateReason, Scope};
use serde_json::Value;

pub struct PlanGate {
    scope: Option<Scope>,
}

impl PlanGate {
    pub fn new(scope: Option<Scope>) -> Self {
        Self { scope }
    }

    pub fn with_scope(&mut self, scope: Scope) {
        self.scope = Some(scope);
    }

    /// Lexically normalize a relative path. Returns `None` if it is absolute or escapes.
    fn normalize(path: &str) -> Option<PathBuf> {
        let path = Path::new(path);
        if path.is_absolute() {
            return None;
        }
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Normal(c) => out.push(c),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        return None;
                    }
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Some(out)
    }

    fn outside(what: impl Into<String>) -> Decision {
        Decision::Ask(GateReason::OutsideScope { what: what.into() })
    }

    fn floor(path: PathBuf) -> Decision {
        Decision::Ask(GateReason::SensitiveFloor { path })
    }

    fn command_matches(pattern: &CommandPattern, program: &str, args: &[String]) -> bool {
        if pattern.program != program || pattern.args.len() != args.len() {
            return false;
        }
        pattern
            .args
            .iter()
            .zip(args)
            .all(|(p, a)| match p {
                ArgPattern::Any => true,
                ArgPattern::Exact(want) => want == a,
            })
    }
}

impl PermissionGate for PlanGate {
    fn evaluate(&self, tool: &str, args: &Value) -> Decision {
        match tool {
            "fs.read" | "fs.list" => {
                let raw = args
                    .get("path")
                    .or_else(|| args.get("glob"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();

                if is_sensitive(raw) {
                    return Self::floor(PathBuf::from(raw));
                }
                match Self::normalize(raw) {
                    Some(_) => Decision::Allow,
                    None => Self::outside(raw),
                }
            }

            "fs.write" => {
                let raw = args.get("path").and_then(Value::as_str).unwrap_or_default();

                if is_sensitive(raw) {
                    return Self::floor(PathBuf::from(raw));
                }
                let Some(normalized) = Self::normalize(raw) else {
                    return Self::outside(raw);
                };
                let Some(scope) = &self.scope else {
                    return Self::outside(raw);
                };

                let as_str = normalized.to_string_lossy();
                let permitted = scope.write_paths.iter().any(|pattern| {
                    glob::Pattern::new(pattern)
                        .map(|p| p.matches(&as_str))
                        .unwrap_or(false)
                });

                if permitted { Decision::Allow } else { Self::outside(raw) }
            }

            "bash" => {
                let program = args.get("program").and_then(Value::as_str).unwrap_or_default();
                let argv: Vec<String> = args
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|v| v.as_str().unwrap_or_default().to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                let Some(scope) = &self.scope else {
                    return Self::outside(program);
                };

                let permitted = scope
                    .commands
                    .iter()
                    .any(|p| Self::command_matches(p, program, &argv));

                if permitted {
                    Decision::Allow
                } else {
                    Self::outside(format!("{program} {}", argv.join(" ")))
                }
            }

            _ => Decision::Deny,
        }
    }
}
```

`crates/engine/src/lib.rs` (grows in later tasks):

```rust
//! The light-factory engine: session lifetime, the turn state machine, and the plan gate.

pub mod gate;

pub use gate::PlanGate;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p light-factory-engine --test gate`
Expected: PASS (10 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): add the deterministic plan gate"
```

---

### Task 8: Prompt rendering and plan extraction

**Files:**
- Create: `crates/engine/src/prompt.rs`
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: `Plan`.
- Produces: `extract_json<T: DeserializeOwned>(text: &str) -> anyhow::Result<T>`, `render_plan_prompt(goal: &str) -> String`, `render_execute_prompt(goal: &str, plan: &Plan, transcript: &[String]) -> String`.

- [ ] **Step 1: Write the failing test**

`crates/engine/tests/prompt.rs`:

```rust
use light_factory_engine::prompt::{extract_json, render_plan_prompt};
use light_factory_protocol::session::Plan;

#[test]
fn extracts_json_from_a_fenced_block() {
    let text = r#"Sure, here is the plan:

```json
{"id":"00000000-0000-0000-0000-000000000000","summary":"do a thing","steps":[],"scope":{"write_paths":["src/**"],"commands":[]}}
```

Let me know."#;

    let plan: Plan = extract_json(text).unwrap();
    assert_eq!(plan.summary, "do a thing");
    assert_eq!(plan.scope.write_paths, vec!["src/**".to_string()]);
}

#[test]
fn extracts_json_without_a_fence() {
    let text = r#"{"id":"00000000-0000-0000-0000-000000000000","summary":"bare","steps":[],"scope":{"write_paths":[],"commands":[]}}"#;
    let plan: Plan = extract_json(text).unwrap();
    assert_eq!(plan.summary, "bare");
}

#[test]
fn no_json_is_an_error() {
    let err = extract_json::<Plan>("I refuse to answer.").unwrap_err();
    assert!(err.to_string().contains("no JSON object"));
}

#[test]
fn the_plan_prompt_states_the_goal_and_demands_json() {
    let prompt = render_plan_prompt("add a health endpoint");
    assert!(prompt.contains("add a health endpoint"));
    assert!(prompt.contains("write_paths"));
    assert!(prompt.contains("program"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p light-factory-engine --test prompt`
Expected: FAIL — `light_factory_engine::prompt` does not exist.

- [ ] **Step 3: Write the implementation**

`crates/engine/src/prompt.rs`:

```rust
//! Prompt rendering and structured extraction. The provider interface is prompt-and-parse,
//! so the engine renders the whole prompt and pulls JSON back out of the completion.

use light_factory_protocol::session::Plan;
use serde::de::DeserializeOwned;

/// Parse `T` out of `text`, tolerating Markdown code fences and surrounding prose by slicing
/// from the first `{` to the last `}`.
pub fn extract_json<T: DeserializeOwned>(text: &str) -> anyhow::Result<T> {
    let start = text
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("no JSON object found in completion"))?;
    let end = text
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("no JSON object found in completion"))?;
    if end < start {
        anyhow::bail!("no JSON object found in completion");
    }
    Ok(serde_json::from_str(&text[start..=end])?)
}

/// The planning prompt. The model must answer with a `Plan` as JSON.
pub fn render_plan_prompt(goal: &str) -> String {
    format!(
        r#"You are the planner for an agentic coding session.

Goal: {goal}

Produce a plan as a single JSON object and nothing else. Shape:

{{
  "id": "<uuid v4>",
  "summary": "<one sentence>",
  "steps": [{{ "id": "<uuid v4>", "description": "<what this step does>" }}],
  "scope": {{
    "write_paths": ["<glob relative to the repo root>"],
    "commands": [{{ "program": "<bare program name>", "args": [{{"Exact": "<arg>"}}, "Any"] }}]
  }}
}}

The scope is a contract. Declare every path you will write to and every command you will run.
Anything outside it stops and asks the human. Commands run with no shell: no pipes,
redirection, chaining, or globbing. Reads need no declaration.
"#
    )
}

/// The execution prompt: the approved plan plus the transcript so far. The model answers with
/// a single tool call as JSON, or `{{"done": true}}` when the plan is complete.
pub fn render_execute_prompt(goal: &str, plan: &Plan, transcript: &[String]) -> String {
    let steps = plan
        .steps
        .iter()
        .map(|s| format!("- {}", s.description))
        .collect::<Vec<_>>()
        .join("\n");

    let history = if transcript.is_empty() {
        String::new()
    } else {
        format!("\nSo far:\n{}\n", transcript.join("\n"))
    };

    format!(
        r#"You are executing an approved plan.

Goal: {goal}
Plan: {summary}
{steps}
{history}
Answer with a single JSON object and nothing else, either a tool call:

{{ "tool": "fs.read",  "args": {{ "path": "<path>" }} }}
{{ "tool": "fs.list",  "args": {{ "glob": "<glob>" }} }}
{{ "tool": "fs.write", "args": {{ "path": "<path>", "contents": "<full file contents>" }} }}
{{ "tool": "bash",     "args": {{ "program": "<bare name>", "args": ["<arg>"] }} }}

or, when the plan is complete:

{{ "done": true }}
"#,
        summary = plan.summary
    )
}
```

Add `pub mod prompt;` to `crates/engine/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p light-factory-engine --test prompt`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): add prompt rendering and JSON extraction"
```

---

### Task 9: The `Session` actor and event sequencing

**Files:**
- Create: `crates/engine/src/session.rs`
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: `SessionId`, `Event`, `EventKind`, `LocalWorkspace`, `Provider`.
- Produces: `SessionHandle { pub id: SessionId, commands: mpsc::UnboundedSender<Command>, events: broadcast::Sender<Event> }` with `SessionHandle::send(&self, cmd: Command)` and `SessionHandle::subscribe(&self) -> broadcast::Receiver<Event>`. `Session::spawn(id, workspace, provider) -> SessionHandle`. Internally `Session::emit(&mut self, kind: EventKind)` increments `seq` starting at 1.

- [ ] **Step 1: Write the failing test**

`crates/engine/tests/session.rs`:

```rust
use std::sync::Arc;

use light_factory_engine::session::Session;
use light_factory_engine_core::traits::Provider;
use light_factory_protocol::session::{Command, EventKind, SessionId};
use light_factory_providers::ScriptedProvider;
use light_factory_tools::LocalWorkspace;

fn provider() -> Arc<dyn Provider> {
    Arc::new(ScriptedProvider::new("{\"done\": true}"))
}

#[tokio::test]
async fn events_are_sequenced_from_one_and_scoped_to_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();

    let handle = Session::spawn(id, ws, provider());
    let mut events = handle.subscribe();

    handle.send(Command::Abort { session: id });

    let first = events.recv().await.unwrap();
    assert_eq!(first.seq, 1);
    assert_eq!(first.session, id);
    assert!(matches!(first.kind, EventKind::TurnComplete { ok: false }));
}

#[tokio::test]
async fn two_subscribers_both_observe_the_stream() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();

    let handle = Session::spawn(id, ws, provider());
    let mut a = handle.subscribe();
    let mut b = handle.subscribe();

    handle.send(Command::Abort { session: id });

    assert_eq!(a.recv().await.unwrap().seq, 1);
    assert_eq!(b.recv().await.unwrap().seq, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p light-factory-engine --test session`
Expected: FAIL — `light_factory_engine::session` does not exist.

- [ ] **Step 3: Write the implementation**

`crates/engine/src/session.rs`:

```rust
//! The session actor: owns a workspace, the approved plan, and the event sequence.

use std::sync::Arc;

use light_factory_engine_core::traits::Provider;
use light_factory_protocol::session::{Command, Event, EventKind, Plan, SessionId};
use light_factory_tools::LocalWorkspace;
use tokio::sync::{broadcast, mpsc};

const EVENT_CHANNEL_CAPACITY: usize = 256;

/// A client's handle to a running session.
#[derive(Clone)]
pub struct SessionHandle {
    pub id: SessionId,
    commands: mpsc::UnboundedSender<Command>,
    events: broadcast::Sender<Event>,
}

impl SessionHandle {
    /// Send a command. A closed session drops the command silently.
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Observe the event stream. Multiple subscribers are supported so a human client and an
    /// agent peer can watch the same session.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }
}

pub struct Session {
    pub(crate) id: SessionId,
    pub(crate) workspace: Arc<LocalWorkspace>,
    pub(crate) provider: Arc<dyn Provider>,
    pub(crate) approved: Option<Plan>,
    pub(crate) paused: bool,
    pub(crate) seq: u64,
    pub(crate) events: broadcast::Sender<Event>,
}

impl Session {
    /// Spawn a session on its own task and return a handle to it.
    pub fn spawn(
        id: SessionId,
        workspace: Arc<LocalWorkspace>,
        provider: Arc<dyn Provider>,
    ) -> SessionHandle {
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (events_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let session = Session {
            id,
            workspace,
            provider,
            approved: None,
            paused: false,
            seq: 0,
            events: events_tx.clone(),
        };

        tokio::spawn(session.run(commands_rx));

        SessionHandle {
            id,
            commands: commands_tx,
            events: events_tx,
        }
    }

    /// Emit an event, assigning the next sequence number. `seq` starts at 1.
    pub(crate) fn emit(&mut self, kind: EventKind) {
        self.seq += 1;
        let _ = self.events.send(Event {
            seq: self.seq,
            session: self.id,
            kind,
        });
    }

    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        while let Some(command) = commands.recv().await {
            match command {
                Command::SendPrompt { text, .. } => self.run_turn(&text).await,
                Command::Abort { .. } => self.emit(EventKind::TurnComplete { ok: false }),
                Command::CreateSession { .. }
                | Command::ApprovePlan { .. }
                | Command::ApproveAction { .. }
                | Command::Pause { .. }
                | Command::Resume { .. } => {
                    // Handled inside `run_turn` while a turn is in flight; ignored otherwise.
                }
            }
        }
    }
}
```

Add `pub mod session;` to `crates/engine/src/lib.rs`.

Note: `run_turn` is written in Task 10. To keep this task independently testable, add a temporary stub in `session.rs`:

```rust
impl Session {
    async fn run_turn(&mut self, _goal: &str) {
        self.emit(EventKind::TurnComplete { ok: false });
    }
}
```

Task 10 replaces this stub with the real turn state machine.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p light-factory-engine --test session`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): add the session actor and event sequencing"
```

---

### Task 10: The turn state machine

Replaces the Task 9 stub with plan → approve → execute → complete.

**Files:**
- Create: `crates/engine/src/turn.rs`
- Modify: `crates/engine/src/session.rs` (remove the stub, route commands into the turn)
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: everything from Tasks 7–9.
- Produces: `Session::run_turn(&mut self, goal: &str, commands: &mut mpsc::UnboundedReceiver<Command>)`. Constant `MAX_CONSECUTIVE_DENIALS: usize = 3`. `Session::paused: bool` field (add it to the struct in `session.rs`, initialized `false`).
- Turn transcript entries are `String`s of the form `"<tool> -> <json result>"`.

- [ ] **Step 1: Write the failing test**

`crates/engine/tests/turn.rs`:

```rust
use std::sync::Arc;

use light_factory_engine::session::Session;
use light_factory_engine_core::traits::Provider;
use light_factory_protocol::session::{Command, EventKind, SessionId};
use light_factory_providers::ScriptedProvider;
use light_factory_tools::LocalWorkspace;
use tokio::sync::broadcast::Receiver;

use light_factory_protocol::session::Event;

const PLAN_JSON: &str = r#"```json
{"id":"11111111-1111-1111-1111-111111111111","summary":"write a note",
 "steps":[{"id":"22222222-2222-2222-2222-222222222222","description":"write notes.txt"}],
 "scope":{"write_paths":["notes.txt"],"commands":[]}}
```"#;

const WRITE_JSON: &str = r#"{"tool":"fs.write","args":{"path":"notes.txt","contents":"hello"}}"#;
const OUT_OF_SCOPE_JSON: &str = r#"{"tool":"fs.write","args":{"path":"Cargo.toml","contents":"x"}}"#;
const DONE_JSON: &str = r#"{"done": true}"#;

fn provider(execute_response: &str) -> Arc<dyn Provider> {
    Arc::new(
        ScriptedProvider::new(DONE_JSON)
            .on("You are the planner", PLAN_JSON)
            .on("You are executing", execute_response),
    )
}

async fn next_matching(
    events: &mut Receiver<Event>,
    pred: impl Fn(&EventKind) -> bool,
) -> EventKind {
    loop {
        let ev = events.recv().await.expect("event stream stayed open");
        if pred(&ev.kind) {
            return ev.kind;
        }
    }
}

#[tokio::test]
async fn a_turn_proposes_a_plan_and_waits_for_approval() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(DONE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt { session: id, text: "write a note".into() });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan, .. } = kind else { unreachable!() };
    assert_eq!(plan.summary, "write a note");
    assert_eq!(plan.scope.write_paths, vec!["notes.txt".to_string()]);
}

#[tokio::test]
async fn rejecting_the_plan_ends_the_turn_without_executing() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(WRITE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt { session: id, text: "write a note".into() });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else { unreachable!() };

    handle.send(Command::ApprovePlan { session: id, plan_id, approved: false });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::TurnComplete { .. })).await;
    assert!(matches!(kind, EventKind::TurnComplete { ok: false }));
    assert!(!dir.path().join("notes.txt").exists());
}

#[tokio::test]
async fn approving_the_plan_executes_inside_scope() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(WRITE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt { session: id, text: "write a note".into() });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else { unreachable!() };

    handle.send(Command::ApprovePlan { session: id, plan_id, approved: true });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::FileEdit { .. })).await;
    let EventKind::FileEdit { path, bytes_written } = kind else { unreachable!() };
    assert_eq!(path.to_string_lossy(), "notes.txt");
    assert_eq!(bytes_written, 5);
    assert_eq!(std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(), "hello");
}

#[tokio::test]
async fn pause_parks_the_turn_until_resume() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(WRITE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt { session: id, text: "write a note".into() });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else { unreachable!() };

    handle.send(Command::Pause { session: id });
    handle.send(Command::ApprovePlan { session: id, plan_id, approved: true });

    // Paused before the first execute call: the file must not appear.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(!dir.path().join("notes.txt").exists());

    handle.send(Command::Resume { session: id });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::FileEdit { .. })).await;
    assert!(matches!(kind, EventKind::FileEdit { .. }));
    assert_eq!(std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(), "hello");
}

#[tokio::test]
async fn an_out_of_scope_write_asks_instead_of_executing() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(OUT_OF_SCOPE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt { session: id, text: "write a note".into() });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else { unreachable!() };
    handle.send(Command::ApprovePlan { session: id, plan_id, approved: true });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::ApprovalRequest { .. })).await;
    let EventKind::ApprovalRequest { request_id, .. } = kind else { unreachable!() };

    handle.send(Command::ApproveAction { session: id, request_id, approved: false });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::TurnComplete { .. })).await;
    assert!(matches!(kind, EventKind::TurnComplete { ok: false }));
    assert!(!dir.path().join("Cargo.toml").exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p light-factory-engine --test turn`
Expected: FAIL — the stub `run_turn` emits `TurnComplete` immediately, so `PlanProposed` never arrives and the first test hangs or errors.

- [ ] **Step 3: Write the implementation**

`crates/engine/src/turn.rs`:

```rust
//! The turn state machine: plan, await approval, execute under the gate, complete.

use light_factory_engine_core::tool::{Decision, PermissionGate, Tool};
use light_factory_engine_core::types::CompleteRequest;
use light_factory_protocol::session::{Command, EventKind, GateReason, Plan};
use light_factory_tools::{BashTool, FsListTool, FsReadTool, FsWriteTool};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::gate::PlanGate;
use crate::prompt::{extract_json, render_execute_prompt, render_plan_prompt};
use crate::session::Session;

/// Consecutive gate denials tolerated before the turn aborts.
pub const MAX_CONSECUTIVE_DENIALS: usize = 3;

#[derive(Deserialize)]
struct ToolCall {
    #[serde(default)]
    done: bool,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    args: Option<Value>,
}

impl Session {
    /// Run one turn to completion.
    pub(crate) async fn run_turn(
        &mut self,
        goal: &str,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) {
        let plan = match self.propose_plan(goal).await {
            Some(plan) => plan,
            None => {
                self.emit(EventKind::TurnComplete { ok: false });
                return;
            }
        };

        let plan_id = plan.id;
        self.emit(EventKind::PlanProposed { plan_id, plan: plan.clone() });

        let approved = self.await_plan_decision(plan_id, commands).await;
        self.emit(EventKind::PlanDecided { plan_id, approved });
        if !approved {
            self.emit(EventKind::TurnComplete { ok: false });
            return;
        }

        self.approved = Some(plan.clone());
        let ok = self.execute(goal, &plan, commands).await;
        self.emit(EventKind::TurnComplete { ok });
    }

    async fn propose_plan(&mut self, goal: &str) -> Option<Plan> {
        let request = CompleteRequest { prompt: render_plan_prompt(goal) };
        let response = match self.provider.complete(request).await {
            Ok(r) => r,
            Err(e) => {
                self.emit(EventKind::Error {
                    code: "provider_error".into(),
                    message: e.to_string(),
                });
                return None;
            }
        };

        if let Some(usage) = response.usage {
            self.emit(EventKind::TokenUsage {
                input_tokens: usage.input_tokens as u64,
                output_tokens: usage.output_tokens as u64,
            });
        }

        match extract_json::<Plan>(&response.text) {
            Ok(plan) => Some(plan),
            Err(e) => {
                self.emit(EventKind::Error {
                    code: "invalid_plan".into(),
                    message: e.to_string(),
                });
                None
            }
        }
    }

    async fn await_plan_decision(
        &mut self,
        plan_id: Uuid,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) -> bool {
        while let Some(command) = commands.recv().await {
            match command {
                Command::ApprovePlan { plan_id: id, approved, .. } if id == plan_id => {
                    return approved;
                }
                Command::Pause { .. } => self.paused = true,
                Command::Resume { .. } => self.paused = false,
                Command::Abort { .. } => return false,
                _ => {}
            }
        }
        // The command channel closed with no answer: fail closed.
        false
    }

    async fn execute(
        &mut self,
        goal: &str,
        plan: &Plan,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) -> bool {
        let gate = PlanGate::new(Some(plan.scope.clone()));
        let mut transcript: Vec<String> = Vec::new();
        let mut denials = 0usize;

        loop {
            if !self.wait_if_paused(commands).await {
                return false;
            }

            let prompt = render_execute_prompt(goal, plan, &transcript);
            let response = match self.provider.complete(CompleteRequest { prompt }).await {
                Ok(r) => r,
                Err(e) => {
                    self.emit(EventKind::Error {
                        code: "provider_error".into(),
                        message: e.to_string(),
                    });
                    return false;
                }
            };

            if let Some(usage) = response.usage {
                self.emit(EventKind::TokenUsage {
                    input_tokens: usage.input_tokens as u64,
                    output_tokens: usage.output_tokens as u64,
                });
            }

            let call: ToolCall = match extract_json(&response.text) {
                Ok(c) => c,
                Err(e) => {
                    self.emit(EventKind::Error {
                        code: "invalid_tool_call".into(),
                        message: e.to_string(),
                    });
                    return false;
                }
            };

            if call.done {
                return true;
            }

            let (Some(name), Some(args)) = (call.tool, call.args) else {
                self.emit(EventKind::Error {
                    code: "invalid_tool_call".into(),
                    message: "expected `tool` and `args`, or `done`".into(),
                });
                return false;
            };

            match gate.evaluate(&name, &args) {
                Decision::Allow => {}
                Decision::Deny => {
                    denials += 1;
                    transcript.push(format!("{name} -> denied: unknown tool"));
                    if denials >= MAX_CONSECUTIVE_DENIALS {
                        return false;
                    }
                    continue;
                }
                Decision::Ask(reason) => {
                    let request_id = Uuid::new_v4();
                    self.emit(EventKind::ApprovalRequest {
                        request_id,
                        reason: reason.clone(),
                        detail: format!("{name} {args}"),
                    });

                    if !self.await_action_decision(request_id, commands).await {
                        denials += 1;
                        transcript.push(format!("{name} -> denied by the human: {}", describe(&reason)));
                        if denials >= MAX_CONSECUTIVE_DENIALS {
                            return false;
                        }
                        continue;
                    }
                }
            }

            denials = 0;
            match self.dispatch(&name, args).await {
                Ok(result) => transcript.push(format!("{name} -> {result}")),
                Err(e) => transcript.push(format!("{name} -> error: {e}")),
            }
        }
    }

    /// Park while paused. Returns `false` if the turn was aborted or the channel closed —
    /// both fail closed, ending the turn rather than resuming unsupervised.
    async fn wait_if_paused(&mut self, commands: &mut mpsc::UnboundedReceiver<Command>) -> bool {
        while self.paused {
            match commands.recv().await {
                Some(Command::Resume { .. }) => self.paused = false,
                Some(Command::Pause { .. }) => {}
                Some(Command::Abort { .. }) | None => return false,
                Some(_) => {}
            }
        }
        true
    }

    async fn await_action_decision(
        &mut self,
        request_id: Uuid,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) -> bool {
        while let Some(command) = commands.recv().await {
            match command {
                Command::ApproveAction { request_id: id, approved, .. } if id == request_id => {
                    return approved;
                }
                Command::Pause { .. } => self.paused = true,
                Command::Resume { .. } => self.paused = false,
                Command::Abort { .. } => return false,
                _ => {}
            }
        }
        false
    }

    async fn dispatch(&mut self, name: &str, args: Value) -> anyhow::Result<Value> {
        let tool: Box<dyn Tool> = match name {
            "fs.read" => Box::new(FsReadTool { workspace: self.workspace.clone() }),
            "fs.list" => Box::new(FsListTool { workspace: self.workspace.clone() }),
            "fs.write" => Box::new(FsWriteTool { workspace: self.workspace.clone() }),
            "bash" => Box::new(BashTool {
                workspace_root: self.workspace.root().to_path_buf(),
            }),
            other => anyhow::bail!("unknown tool: {other}"),
        };

        let result = tool.call(args.clone()).await?;

        match name {
            "fs.write" => {
                let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
                let bytes_written = result
                    .get("bytes_written")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                self.emit(EventKind::FileEdit { path: path.into(), bytes_written });
            }
            "bash" => {
                let program = args.get("program").and_then(Value::as_str).unwrap_or_default();
                let exit_code = result.get("exit_code").and_then(Value::as_i64).unwrap_or(-1) as i32;
                self.emit(EventKind::CommandRun { command: program.to_string(), exit_code });
            }
            _ => {}
        }

        Ok(result)
    }
}

fn describe(reason: &GateReason) -> String {
    match reason {
        GateReason::OutsideScope { what } => format!("outside the approved scope ({what})"),
        GateReason::SensitiveFloor { path } => {
            format!("sensitive path ({})", path.display())
        }
    }
}
```

Remove the stub `run_turn` from `session.rs` and change the command loop so `SendPrompt` passes the receiver through:

```rust
    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        while let Some(command) = commands.recv().await {
            match command {
                Command::SendPrompt { text, .. } => {
                    self.run_turn(&text, &mut commands).await;
                }
                Command::Abort { .. } => self.emit(EventKind::TurnComplete { ok: false }),
                _ => {}
            }
        }
    }
```

Add `pub mod turn;` to `crates/engine/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p light-factory-engine`
Expected: PASS (all gate, prompt, session, and turn tests — 5 turn tests).

- [ ] **Step 5: Run the whole suite and lints**

Run: `cargo fmt && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): add the turn state machine with plan approval and gating"
```

---

### Task 11: The `Engine` session registry

The spec has `Engine` hold `HashMap<SessionId, SessionHandle>` so multiple sessions work from day one and a later socket transport touches only the routing layer. Without it the TUI would hold a bare `SessionHandle` and `Command::CreateSession` would have nowhere to land.

**Files:**
- Modify: `crates/engine/src/lib.rs`
- Create: `crates/engine/tests/engine.rs`

**Interfaces:**
- Consumes: `Session::spawn`, `SessionHandle`, `Command`, `SessionId`, `LocalWorkspace`, `Provider`.
- Produces: `Engine::new(provider: Arc<dyn Provider>) -> Self`, `Engine::create_session(&mut self, workspace: PathBuf) -> anyhow::Result<SessionId>`, `Engine::handle(&self, id: SessionId) -> Option<&SessionHandle>`, `Engine::dispatch(&mut self, command: Command) -> anyhow::Result<()>` routing every session-scoped command to its session, and `Engine::session_ids(&self) -> Vec<SessionId>`.

- [ ] **Step 1: Write the failing test**

`crates/engine/tests/engine.rs`:

```rust
use std::sync::Arc;

use light_factory_engine::Engine;
use light_factory_engine_core::traits::Provider;
use light_factory_protocol::session::{Command, EventKind, SessionId};
use light_factory_providers::ScriptedProvider;

fn provider() -> Arc<dyn Provider> {
    Arc::new(ScriptedProvider::new(r#"{"done": true}"#))
}

#[tokio::test]
async fn creates_and_tracks_multiple_sessions() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(provider());

    let first = engine.create_session(a.path().to_path_buf()).unwrap();
    let second = engine.create_session(b.path().to_path_buf()).unwrap();

    assert_ne!(first, second);
    assert_eq!(engine.session_ids().len(), 2);
    assert!(engine.handle(first).is_some());
    assert!(engine.handle(second).is_some());
}

#[tokio::test]
async fn dispatch_routes_a_command_to_its_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(provider());
    let id = engine.create_session(dir.path().to_path_buf()).unwrap();

    let mut events = engine.handle(id).unwrap().subscribe();
    engine.dispatch(Command::Abort { session: id }).unwrap();

    let ev = events.recv().await.unwrap();
    assert_eq!(ev.session, id);
    assert!(matches!(ev.kind, EventKind::TurnComplete { ok: false }));
}

#[tokio::test]
async fn dispatch_to_an_unknown_session_is_an_error() {
    let mut engine = Engine::new(provider());
    let err = engine
        .dispatch(Command::Abort { session: SessionId::new() })
        .unwrap_err();
    assert!(err.to_string().contains("unknown session"));
}

#[tokio::test]
async fn create_session_rejects_a_missing_workspace() {
    let mut engine = Engine::new(provider());
    assert!(engine.create_session("/nonexistent/path/xyz".into()).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p light-factory-engine --test engine`
Expected: FAIL — `Engine` is undefined.

- [ ] **Step 3: Write the implementation**

Replace `crates/engine/src/lib.rs` with:

```rust
//! The light-factory engine: session lifetime, the turn state machine, and the plan gate.

pub mod gate;
pub mod prompt;
pub mod session;
pub mod turn;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use light_factory_engine_core::traits::Provider;
use light_factory_protocol::session::{Command, SessionId};
use light_factory_tools::LocalWorkspace;

pub use gate::PlanGate;
pub use session::{Session, SessionHandle};

/// Owns every running session and routes commands to them. In this slice the TUI holds an
/// `Engine` directly; a later daemon puts a socket in front of the same routing.
pub struct Engine {
    provider: Arc<dyn Provider>,
    sessions: HashMap<SessionId, SessionHandle>,
}

impl Engine {
    pub fn new(provider: Arc<dyn Provider>) -> Self {
        Self {
            provider,
            sessions: HashMap::new(),
        }
    }

    /// Open a session rooted at `workspace`. Fails if the directory cannot be resolved.
    pub fn create_session(&mut self, workspace: PathBuf) -> anyhow::Result<SessionId> {
        let workspace = Arc::new(LocalWorkspace::new(workspace)?);
        let id = SessionId::new();
        let handle = Session::spawn(id, workspace, self.provider.clone());
        self.sessions.insert(id, handle);
        Ok(id)
    }

    pub fn handle(&self, id: SessionId) -> Option<&SessionHandle> {
        self.sessions.get(&id)
    }

    pub fn session_ids(&self) -> Vec<SessionId> {
        self.sessions.keys().copied().collect()
    }

    /// Route a command to its session. `CreateSession` is handled by
    /// [`Engine::create_session`] instead, since it must return the new id.
    pub fn dispatch(&mut self, command: Command) -> anyhow::Result<()> {
        let session = match &command {
            Command::CreateSession { .. } => {
                anyhow::bail!("use Engine::create_session for CreateSession")
            }
            Command::SendPrompt { session, .. }
            | Command::ApprovePlan { session, .. }
            | Command::ApproveAction { session, .. }
            | Command::Pause { session }
            | Command::Resume { session }
            | Command::Abort { session } => *session,
        };

        let handle = self
            .sessions
            .get(&session)
            .ok_or_else(|| anyhow::anyhow!("unknown session: {}", session.0))?;

        handle.send(command);
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p light-factory-engine`
Expected: PASS (all gate, prompt, session, turn, and engine tests).

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat(engine): add the session registry"
```

---

### Task 12: Wire the engine into the TUI

**Files:**
- Create: `crates/tui/src/engine_view.rs`
- Modify: `crates/tui/src/app.rs`
- Modify: `crates/tui/Cargo.toml`

**Interfaces:**
- Consumes: `Engine::new`, `Engine::create_session`, `Engine::handle`, `Engine::dispatch`, `Command`, `Event`, `EventKind`, and `crate::provider::build()` (added by the port-llm-providers plan's Task 4).
- Produces: a new `Mode::Engine` in the TUI, rendering the event log and the pending gate, with `a` to approve and `d` to deny.
- `describe_event(locale: Locale, kind: &EventKind) -> String` and `pending_prompt(locale: Locale, kind: &EventKind) -> Option<String>`.

**Every user-facing string goes through the existing i18n layer** (`crates/tui/src/i18n.rs`, added by the localize-tui work). Raw English literals in the engine view would break a convention deliberately established across both clients. Note `i18n.rs` has a test asserting `keys(ES) == keys(EN)`, so each new key must be added to **both** catalogs or the suite fails.

- [ ] **Step 1: Add the dependencies**

`crates/tui/Cargo.toml` — add to `[dependencies]`:

```toml
light-factory-engine = { path = "../engine" }
light-factory-engine-core = { path = "../engine-core" }
light-factory-tools = { path = "../tools" }
uuid = { workspace = true }
```

`light-factory-providers` is already a TUI dependency at this point — the port-llm-providers plan added it.

- [ ] **Step 2: Write the failing test**

`crates/tui/tests/engine_view.rs`:

```rust
use light_factory_protocol::session::{EventKind, GateReason, Plan, Scope};
use light_factory_tui::engine_view::{describe_event, pending_prompt};
use light_factory_tui::i18n::Locale;
use uuid::Uuid;

fn plan_event() -> EventKind {
    EventKind::PlanProposed {
        plan_id: Uuid::nil(),
        plan: Plan {
            id: Uuid::nil(),
            summary: "do the thing".into(),
            steps: vec![],
            scope: Scope::default(),
        },
    }
}

#[test]
fn describes_a_file_edit_with_interpolated_values() {
    let line = describe_event(Locale::En, &EventKind::FileEdit {
        path: "src/main.rs".into(),
        bytes_written: 42,
    });
    assert!(line.contains("src/main.rs"));
    assert!(line.contains("42"));
}

#[test]
fn a_proposed_plan_prompts_for_approval() {
    let prompt = pending_prompt(Locale::En, &plan_event()).unwrap();
    assert!(prompt.contains("do the thing"));
}

#[test]
fn an_approval_request_names_the_offending_path() {
    let kind = EventKind::ApprovalRequest {
        request_id: Uuid::nil(),
        reason: GateReason::SensitiveFloor { path: ".env".into() },
        detail: "fs.write".into(),
    };
    let prompt = pending_prompt(Locale::En, &kind).unwrap();
    assert!(prompt.contains(".env"));
}

#[test]
fn ordinary_events_do_not_prompt() {
    assert!(pending_prompt(Locale::En, &EventKind::Log { message: "hi".into() }).is_none());
}

#[test]
fn spanish_differs_from_english_and_still_interpolates() {
    let en = pending_prompt(Locale::En, &plan_event()).unwrap();
    let es = pending_prompt(Locale::Es, &plan_event()).unwrap();
    assert_ne!(en, es, "the engine prompts must be translated, not passed through");
    assert!(es.contains("do the thing"), "the plan summary is data, not a translated string");
}
```

`crates/tui` currently has no library target. Add `crates/tui/src/lib.rs`:

```rust
//! Library surface of the TUI, exposed so its pure rendering helpers are testable.

pub mod engine_view;
pub mod i18n;
```

`i18n` is already a module of the binary; adding it to the library target lets the engine-view tests select a locale. Ensure `main.rs` continues to reference it through the same path.

and add to `crates/tui/Cargo.toml`:

```toml
[lib]
name = "light_factory_tui"
path = "src/lib.rs"
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p light-factory-tui --test engine_view`
Expected: FAIL — `light_factory_tui::engine_view` does not exist.

- [ ] **Step 4: Write the implementation**

`crates/tui/src/engine_view.rs`:

```rust
//! Pure rendering helpers for engine events. Kept free of ratatui types so they are
//! testable without a terminal.

use light_factory_protocol::session::{EventKind, GateReason};

use crate::i18n::{self, Locale};

/// One log line for an engine event, translated for `locale`.
pub fn describe_event(locale: Locale, kind: &EventKind) -> String {
    match kind {
        EventKind::PlanProposed { plan, .. } => {
            i18n::t_with(locale, "engine.plan_proposed", &[("summary", &plan.summary)])
        }
        EventKind::PlanDecided { approved, .. } => i18n::t(
            locale,
            if *approved { "engine.plan_approved" } else { "engine.plan_rejected" },
        )
        .to_string(),
        EventKind::StepStarted { description, .. } => {
            i18n::t_with(locale, "engine.step_started", &[("description", description)])
        }
        EventKind::StepFinished { ok, .. } => i18n::t(
            locale,
            if *ok { "engine.step_done" } else { "engine.step_failed" },
        )
        .to_string(),
        EventKind::FileEdit { path, bytes_written } => i18n::t_with(
            locale,
            "engine.file_edit",
            &[
                ("path", &path.display().to_string()),
                ("bytes", &bytes_written.to_string()),
            ],
        ),
        EventKind::CommandRun { command, exit_code } => i18n::t_with(
            locale,
            "engine.command_run",
            &[("command", command), ("code", &exit_code.to_string())],
        ),
        EventKind::ApprovalRequest { detail, .. } => {
            i18n::t_with(locale, "engine.approval_needed", &[("detail", detail)])
        }
        EventKind::Log { message } => message.clone(),
        EventKind::TokenUsage { input_tokens, output_tokens } => i18n::t_with(
            locale,
            "engine.token_usage",
            &[
                ("input", &input_tokens.to_string()),
                ("output", &output_tokens.to_string()),
            ],
        ),
        EventKind::TurnComplete { ok } => i18n::t(
            locale,
            if *ok { "engine.turn_complete" } else { "engine.turn_ended" },
        )
        .to_string(),
        EventKind::Error { code, message } => i18n::error_message(locale, code)
            .map(str::to_string)
            .unwrap_or_else(|| message.clone()),
    }
}

/// The prompt to show when an event needs a human answer, or `None` when it does not.
pub fn pending_prompt(locale: Locale, kind: &EventKind) -> Option<String> {
    let keys = i18n::t(locale, "engine.approve_keys");
    match kind {
        EventKind::PlanProposed { plan, .. } => {
            let body = i18n::t_with(
                locale,
                "engine.plan_prompt",
                &[
                    ("summary", &plan.summary),
                    ("steps", &plan.steps.len().to_string()),
                    ("paths", &plan.scope.write_paths.len().to_string()),
                    ("commands", &plan.scope.commands.len().to_string()),
                ],
            );
            Some(format!("{body}\n{keys}"))
        }
        EventKind::ApprovalRequest { reason, detail, .. } => {
            let why = match reason {
                GateReason::OutsideScope { what } => {
                    i18n::t_with(locale, "engine.reason_outside_scope", &[("what", what)])
                }
                GateReason::SensitiveFloor { path } => i18n::t_with(
                    locale,
                    "engine.reason_sensitive",
                    &[("path", &path.display().to_string())],
                ),
            };
            Some(format!("{detail}\n{why}\n{keys}"))
        }
        _ => None,
    }
}
```

- [ ] **Step 5: Add the engine keys to both catalogs**

In `crates/tui/src/i18n.rs`, append to `EN`:

```rust
    ("engine.plan_proposed", "plan proposed: {summary}"),
    ("engine.plan_approved", "plan approved"),
    ("engine.plan_rejected", "plan rejected"),
    ("engine.step_started", "step: {description}"),
    ("engine.step_done", "step done"),
    ("engine.step_failed", "step failed"),
    ("engine.file_edit", "wrote {path} ({bytes} bytes)"),
    ("engine.command_run", "ran {command} (exit {code})"),
    ("engine.approval_needed", "approval needed: {detail}"),
    ("engine.token_usage", "tokens: {input} in / {output} out"),
    ("engine.turn_complete", "turn complete"),
    ("engine.turn_ended", "turn ended"),
    (
        "engine.plan_prompt",
        "Plan: {summary}\n{steps} step(s), {paths} write path(s), {commands} command(s)",
    ),
    ("engine.reason_outside_scope", "outside the approved scope: {what}"),
    ("engine.reason_sensitive", "sensitive path: {path}"),
    ("engine.approve_keys", "[a] approve  [d] deny"),
```

and the matching entries to `ES`:

```rust
    ("engine.plan_proposed", "plan propuesto: {summary}"),
    ("engine.plan_approved", "plan aprobado"),
    ("engine.plan_rejected", "plan rechazado"),
    ("engine.step_started", "paso: {description}"),
    ("engine.step_done", "paso completado"),
    ("engine.step_failed", "paso fallido"),
    ("engine.file_edit", "escrito {path} ({bytes} bytes)"),
    ("engine.command_run", "ejecutado {command} (salida {code})"),
    ("engine.approval_needed", "se requiere aprobación: {detail}"),
    ("engine.token_usage", "tokens: {input} entrada / {output} salida"),
    ("engine.turn_complete", "turno completado"),
    ("engine.turn_ended", "turno finalizado"),
    (
        "engine.plan_prompt",
        "Plan: {summary}\n{steps} paso(s), {paths} ruta(s) de escritura, {commands} comando(s)",
    ),
    ("engine.reason_outside_scope", "fuera del alcance aprobado: {what}"),
    ("engine.reason_sensitive", "ruta sensible: {path}"),
    ("engine.approve_keys", "[a] aprobar  [d] denegar"),
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p light-factory-tui`
Expected: PASS (5 engine-view tests, plus the existing `ES must define exactly the EN key set` assertion, which now covers the new keys).

- [ ] **Step 7: Add the engine mode to the app**

In `crates/tui/src/app.rs`:

1. Add `Engine` to the `Mode` enum.
2. Add fields to `App`: `engine: Option<light_factory_engine::Engine>`, `session: Option<SessionId>`, `engine_log: Vec<String>`, `pending: Option<(EventKind, String)>`.
3. On entering `Mode::Engine`, build the session:

```rust
// Reuse the selection built by the port-llm-providers plan. It never fails: with no
// key configured it degrades to the offline LocalProvider rather than erroring.
let provider = crate::provider::build();

let mut engine = Engine::new(provider);
let session = engine.create_session(std::env::current_dir()?)?;

let mut events = engine.handle(session).expect("just created").subscribe();
let tx = self.events.clone();
tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        let _ = tx.send(UiEvent::Engine(event));
    }
});

self.engine = Some(engine);
self.session = Some(session);
```

4. Add `UiEvent::Engine(Event)` to the existing `UiEvent` enum and handle it by pushing `describe_event(self.config.lang, &event.kind)` onto `engine_log` and setting `pending` from `pending_prompt(self.config.lang, &event.kind)`. `self.config.lang` is the `Locale` the existing `t`/`t_with` helpers already use.
5. Bind `a` and `d` in `Mode::Engine` to build the matching command from the id carried in `pending` and route it with `engine.dispatch(...)`, then clear `pending`:

```rust
let (Some(engine), Some(session)) = (self.engine.as_mut(), self.session) else {
    return Ok(());
};
let command = match self.pending.as_ref().map(|(kind, _)| kind) {
    Some(EventKind::PlanProposed { plan_id, .. }) => {
        Some(Command::ApprovePlan { session, plan_id: *plan_id, approved })
    }
    Some(EventKind::ApprovalRequest { request_id, .. }) => {
        Some(Command::ApproveAction { session, request_id: *request_id, approved })
    }
    _ => None,
};
if let Some(command) = command {
    engine.dispatch(command)?;
    self.pending = None;
}
```
6. Render `engine_log` in the main pane and `pending` in a bordered footer block.

- [ ] **Step 8: Verify the binary builds and runs**

Run: `cargo run -p light-factory-tui`
Expected: the TUI starts. Sign in, enter engine mode, and confirm the pane renders.

With no API key configured, `build_provider_from_env` selects the offline `LocalProvider`, so
engine mode must still open and the turn must still reach `TurnComplete` — it will simply fail to
produce a parseable plan and surface `EventKind::Error { code: "invalid_plan", .. }`. Confirm that
path renders as an error line rather than hanging or panicking. Then set `ANTHROPIC_API_KEY` and
confirm a real plan is proposed.

Run with `LIGHT_LANG=es` (or the setting the localize-tui work established) and confirm the engine
pane renders Spanish while the plan summary itself stays as the model wrote it.

- [ ] **Step 9: Commit**

```bash
git add crates/tui
git commit -m "feat(tui): drive the engine through Command/Event"
```

---

### Task 13: Update the documentation

**Files:**
- Modify: `ARCHITECTURE.md`
- Modify: `docs/superpowers/specs/2026-08-20-engine-core-design.md`
- Modify: `docs/superpowers/plans/2026-08-20-engine-core.md`

**Interfaces:**
- Consumes: the completed implementation.
- Produces: documentation matching the code.

- [ ] **Step 1: Update the "Current state" table in `ARCHITECTURE.md`**

Change the `engine core` row from **Designed, not built** to `Built — thin vertical slice (plan → approve → execute)`. Change the `Command/Event protocol` row to `Built in crates/protocol/src/session.rs; wire.rs Ping/Pong still serves the fly server`. Add rows for the new crates in the **Crate layout** section, moving them out of the `planned` block.

- [ ] **Step 2: Mark the spec implemented**

Change the spec's `**Status:**` line to `IMPLEMENTED (2026-08-20)`.

- [ ] **Step 3: Mark every task complete in this plan**

Tick each `- [ ]` to `- [x]`.

- [ ] **Step 4: Run the full verification**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ARCHITECTURE.md docs/superpowers
git commit -m "docs: record the engine core slice as shipped"
```
