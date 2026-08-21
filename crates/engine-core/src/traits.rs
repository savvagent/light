//! The trait seams the engine drives.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::types::{CompleteRequest, CompleteResponse, Edit};

/// An LLM provider.
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    /// Whether this provider is the offline fallback (performs no real completion). The engine
    /// uses this to reject a turn up front rather than failing later on plan parsing.
    fn is_offline(&self) -> bool {
        false
    }
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
