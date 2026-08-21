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
