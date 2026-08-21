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

impl Session {
    async fn run_turn(&mut self, _goal: &str) {
        self.emit(EventKind::TurnComplete { ok: false });
    }
}
