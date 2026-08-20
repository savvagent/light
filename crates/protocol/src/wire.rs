//! WebSocket protocol types.
//!
//! This is the seam that will grow into the full `Command`/`Event` engine
//! protocol. For the sign-in milestone it only needs a small handshake:
//! the server authenticates the upgrade and announces who is connected.

use serde::{Deserialize, Serialize};

use crate::auth::UserView;

/// Messages sent by the client after the WebSocket is upgraded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Echo request used to keep the connection alive and verify round-trips.
    Ping { nonce: String },
}

/// Messages sent by the server over an authenticated WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Sent immediately after a successful upgrade: announces the session.
    Ready { user: UserView },
    /// Reply to [`ClientMessage::Ping`].
    Pong { nonce: String },
    /// A terminal or non-terminal error condition.
    Error { code: String, message: String },
}
