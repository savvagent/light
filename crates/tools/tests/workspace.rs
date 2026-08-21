use std::path::Path;

use light_factory_tools::LocalWorkspace;

#[test]
fn resolve_rejects_paths_escaping_the_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let ws = LocalWorkspace::new(dir.path().to_path_buf()).unwrap();

    assert!(ws.resolve(Path::new("src")).is_ok());
    assert!(ws.resolve(Path::new("src/../src")).is_ok());

    assert!(ws.resolve(Path::new("../outside")).is_err());
    assert!(ws.resolve(Path::new("src/../../../etc/passwd")).is_err());
    assert!(ws.resolve(Path::new("/etc/passwd")).is_err());
}

#[tokio::test]
async fn write_then_read_round_trips() {
    use light_factory_engine_core::traits::{Workspace, WorkspaceRead};
    use light_factory_engine_core::types::Edit;

    let dir = tempfile::tempdir().unwrap();
    let ws = LocalWorkspace::new(dir.path().to_path_buf()).unwrap();

    let written = ws
        .apply_edit(&Edit {
            path: "notes.txt".into(),
            new_contents: "hello".into(),
        })
        .await
        .unwrap();
    assert_eq!(written, 5);

    let back = ws.read(Path::new("notes.txt")).await.unwrap();
    assert_eq!(String::from_utf8(back).unwrap(), "hello");
}
