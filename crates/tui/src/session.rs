//! Persist the bearer session token between runs.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A saved session: the opaque bearer token plus the identifying user fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub expires_at: i64,
    pub email: String,
    pub display_name: String,
}

impl Session {
    /// Location of the session file: `$XDG_CONFIG_HOME/light-factory/session.json`
    /// (falling back to `~/.config/light-factory/session.json`).
    fn path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME")
            .or_else(|_| std::env::var("HOME").map(|home| format!("{home}/.config")))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base)
            .join("light-factory")
            .join("session.json")
    }

    /// Load the saved session, if any and well-formed.
    pub fn load() -> Option<Self> {
        let raw = fs::read_to_string(Self::path()).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Persist this session, creating the config directory as needed.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Remove the saved session (logout).
    pub fn clear() -> anyhow::Result<()> {
        let path = Self::path();
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
