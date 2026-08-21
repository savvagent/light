//! The filesystem tools. Each is a thin JSON adapter over the workspace; all gating happens
//! in the registry before dispatch.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use light_factory_engine_core::tool::Tool;
use light_factory_engine_core::traits::{Workspace, WorkspaceRead};
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
