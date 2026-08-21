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
