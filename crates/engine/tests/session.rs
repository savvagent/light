use std::sync::Arc;

use light_factory_engine::session::Session;
use light_factory_engine_core::traits::Provider;
use light_factory_protocol::session::{Command, EventKind, SessionId};
use light_factory_providers::ScriptedProvider;
use light_factory_tools::LocalWorkspace;

fn provider() -> Arc<dyn Provider> {
    Arc::new(ScriptedProvider::new("{\"done\": true}"))
}

#[tokio::test]
async fn events_are_sequenced_from_one_and_scoped_to_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();

    let handle = Session::spawn(id, ws, provider());
    let mut events = handle.subscribe();

    handle.send(Command::Abort { session: id });

    let first = events.recv().await.unwrap();
    assert_eq!(first.seq, 1);
    assert_eq!(first.session, id);
    assert!(matches!(first.kind, EventKind::TurnComplete { ok: false }));
}

#[tokio::test]
async fn two_subscribers_both_observe_the_stream() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();

    let handle = Session::spawn(id, ws, provider());
    let mut a = handle.subscribe();
    let mut b = handle.subscribe();

    handle.send(Command::Abort { session: id });

    assert_eq!(a.recv().await.unwrap().seq, 1);
    assert_eq!(b.recv().await.unwrap().seq, 1);
}
