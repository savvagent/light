use std::sync::Arc;

use light_factory_engine::Engine;
use light_factory_engine_core::traits::Provider;
use light_factory_protocol::session::{Command, EventKind, SessionId};
use light_factory_providers::ScriptedProvider;

fn provider() -> Arc<dyn Provider> {
    Arc::new(ScriptedProvider::new(r#"{"done": true}"#))
}

#[tokio::test]
async fn creates_and_tracks_multiple_sessions() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(provider());

    let first = engine.create_session(a.path().to_path_buf()).unwrap();
    let second = engine.create_session(b.path().to_path_buf()).unwrap();

    assert_ne!(first, second);
    assert_eq!(engine.session_ids().len(), 2);
    assert!(engine.handle(first).is_some());
    assert!(engine.handle(second).is_some());
}

#[tokio::test]
async fn dispatch_routes_a_command_to_its_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(provider());
    let id = engine.create_session(dir.path().to_path_buf()).unwrap();

    let mut events = engine.handle(id).unwrap().subscribe();
    engine.dispatch(Command::Abort { session: id }).unwrap();

    let ev = events.recv().await.unwrap();
    assert_eq!(ev.session, id);
    assert!(matches!(ev.kind, EventKind::TurnComplete { ok: false }));
}

#[tokio::test]
async fn dispatch_to_an_unknown_session_is_an_error() {
    let mut engine = Engine::new(provider());
    let err = engine
        .dispatch(Command::Abort { session: SessionId::new() })
        .unwrap_err();
    assert!(err.to_string().contains("unknown session"));
}

#[tokio::test]
async fn create_session_rejects_a_missing_workspace() {
    let mut engine = Engine::new(provider());
    assert!(engine.create_session("/nonexistent/path/xyz".into()).is_err());
}
