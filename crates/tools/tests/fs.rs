use std::sync::Arc;

use light_factory_engine_core::tool::Tool;
use light_factory_engine_core::traits::WorkspaceRead;
use light_factory_tools::{FsReadTool, LocalWorkspace};
use serde_json::json;

#[tokio::test]
async fn reading_a_non_utf8_file_is_an_error_not_a_lossy_string() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("blob.bin"), [0xFFu8, 0xFE, 0x00, 0x80]).unwrap();

    let workspace: Arc<dyn WorkspaceRead> =
        Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let tool = FsReadTool { workspace };

    let err = tool.call(json!({ "path": "blob.bin" })).await.unwrap_err();
    assert!(err.to_string().contains("not valid UTF-8"));
}

#[tokio::test]
async fn reading_a_utf8_file_returns_its_contents() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "hello").unwrap();

    let workspace: Arc<dyn WorkspaceRead> =
        Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let tool = FsReadTool { workspace };

    let out = tool.call(json!({ "path": "note.txt" })).await.unwrap();
    assert_eq!(out["contents"].as_str().unwrap(), "hello");
}
