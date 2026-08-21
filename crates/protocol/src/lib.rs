//! Wire types shared by the `light-factory-server` and the `light-factory` TUI.
//!
//! This crate performs no I/O and depends on nothing but `serde`. It is the
//! single source of truth for everything that crosses the HTTP/WebSocket
//! boundary between client and server.

pub mod auth;
pub mod sensitive;
pub mod session;
pub mod wire;

pub use sensitive::{SENSITIVE_MARKERS, is_sensitive};
