use light_factory_engine_core::tool::Tool;
use light_factory_tools::BashTool;
use serde_json::json;

#[tokio::test]
async fn runs_a_program_with_args() {
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool {
        workspace_root: dir.path().to_path_buf(),
    };

    let out = tool
        .call(json!({ "program": "echo", "args": ["hello", "world"] }))
        .await
        .unwrap();

    assert_eq!(out["exit_code"], 0);
    assert_eq!(out["stdout"].as_str().unwrap().trim(), "hello world");
}

#[tokio::test]
async fn does_not_interpret_shell_metacharacters() {
    let dir = tempfile::tempdir().unwrap();
    let canary = dir.path().join("canary.txt");
    std::fs::write(&canary, "intact").unwrap();

    let tool = BashTool {
        workspace_root: dir.path().to_path_buf(),
    };

    // If a shell were involved, `;` would chain a second command and delete the canary.
    let out = tool
        .call(json!({ "program": "echo", "args": ["hi; rm -f canary.txt"] }))
        .await
        .unwrap();

    assert_eq!(
        out["stdout"].as_str().unwrap().trim(),
        "hi; rm -f canary.txt"
    );
    assert_eq!(std::fs::read_to_string(&canary).unwrap(), "intact");
}

#[tokio::test]
async fn rejects_a_program_containing_a_path_separator() {
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool {
        workspace_root: dir.path().to_path_buf(),
    };

    let err = tool
        .call(json!({ "program": "/bin/sh", "args": ["-c", "echo pwned"] }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("must be a bare program name"));
}

#[tokio::test]
async fn non_zero_exit_is_reported_not_raised() {
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool {
        workspace_root: dir.path().to_path_buf(),
    };

    let out = tool
        .call(json!({ "program": "false", "args": [] }))
        .await
        .unwrap();
    assert_ne!(out["exit_code"], 0);
}
