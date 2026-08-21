use light_factory_engine_core::tool::Tool;
use light_factory_tools::BashTool;
use serde_json::json;

#[tokio::test]
async fn runs_a_program_with_args() {
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool::new(dir.path().to_path_buf());

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

    let tool = BashTool::new(dir.path().to_path_buf());

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
    let tool = BashTool::new(dir.path().to_path_buf());

    let err = tool
        .call(json!({ "program": "/bin/sh", "args": ["-c", "echo pwned"] }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("must be a bare program name"));
}

#[tokio::test]
async fn non_zero_exit_is_reported_not_raised() {
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool::new(dir.path().to_path_buf());

    let out = tool
        .call(json!({ "program": "false", "args": [] }))
        .await
        .unwrap();
    assert_ne!(out["exit_code"], 0);
}

#[tokio::test]
async fn a_command_reading_stdin_gets_eof_instead_of_blocking() {
    // With stdin redirected to null, `cat` reaches EOF immediately instead of parking the turn
    // waiting for terminal input.
    let dir = tempfile::tempdir().unwrap();
    let tool = BashTool::new(dir.path().to_path_buf());

    let out = tool
        .call(json!({ "program": "cat", "args": [] }))
        .await
        .unwrap();

    assert_eq!(out["exit_code"], 0);
}

#[tokio::test]
async fn a_command_longer_than_the_timeout_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let tool =
        BashTool::new(dir.path().to_path_buf()).with_timeout(std::time::Duration::from_millis(50));

    let out = tool
        .call(json!({ "program": "sleep", "args": ["60"] }))
        .await
        .unwrap();

    assert_ne!(out["exit_code"], 0);
    assert!(out["stderr"].as_str().unwrap().contains("timed out"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_timed_out_command_kills_its_whole_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let tool =
        BashTool::new(dir.path().to_path_buf()).with_timeout(std::time::Duration::from_millis(50));

    // `sh` forks `sleep` (a grandchild). Killing only the direct child would leave `sleep`
    // running long enough to write the marker; killing the group must stop it.
    let out = tool
        .call(json!({ "program": "sh", "args": ["-c", "sleep 1; touch marker"] }))
        .await
        .unwrap();

    assert_ne!(out["exit_code"], 0);
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    assert!(!marker.exists(), "grandchild survived the timeout kill");
}
