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
