use std::sync::Arc;

use light_factory_engine::session::Session;
use light_factory_engine_core::traits::Provider;
use light_factory_protocol::session::{Command, EventKind, SessionId};
use light_factory_providers::ScriptedProvider;
use light_factory_tools::LocalWorkspace;
use tokio::sync::broadcast::Receiver;

use light_factory_protocol::session::Event;

const PLAN_JSON: &str = r#"```json
{"id":"11111111-1111-1111-1111-111111111111","summary":"write a note",
 "steps":[{"id":"22222222-2222-2222-2222-222222222222","description":"write notes.txt"}],
 "scope":{"write_paths":["notes.txt"],"commands":[]}}
```"#;

const WRITE_JSON: &str = r#"{"tool":"fs.write","args":{"path":"notes.txt","contents":"hello"}}"#;
const OUT_OF_SCOPE_JSON: &str =
    r#"{"tool":"fs.write","args":{"path":"Cargo.toml","contents":"x"}}"#;
const READ_JSON: &str = r#"{"tool":"fs.read","args":{"path":"notes.txt"}}"#;
const DONE_JSON: &str = r#"{"done": true}"#;

fn provider(execute_response: &str) -> Arc<dyn Provider> {
    Arc::new(
        ScriptedProvider::new(DONE_JSON)
            .on("You are the planner", PLAN_JSON)
            .on("You are executing", execute_response),
    )
}

async fn next_matching(
    events: &mut Receiver<Event>,
    pred: impl Fn(&EventKind) -> bool,
) -> EventKind {
    loop {
        let ev = events.recv().await.expect("event stream stayed open");
        if pred(&ev.kind) {
            return ev.kind;
        }
    }
}

#[tokio::test]
async fn a_turn_proposes_a_plan_and_waits_for_approval() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(DONE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt {
        session: id,
        text: "write a note".into(),
    });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan, .. } = kind else {
        unreachable!()
    };
    assert_eq!(plan.summary, "write a note");
    assert_eq!(plan.scope.write_paths, vec!["notes.txt".to_string()]);
}

#[tokio::test]
async fn rejecting_the_plan_ends_the_turn_without_executing() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(WRITE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt {
        session: id,
        text: "write a note".into(),
    });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else {
        unreachable!()
    };

    handle.send(Command::ApprovePlan {
        session: id,
        plan_id,
        approved: false,
    });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::TurnComplete { .. })).await;
    assert!(matches!(kind, EventKind::TurnComplete { ok: false }));
    assert!(!dir.path().join("notes.txt").exists());
}

#[tokio::test]
async fn approving_the_plan_executes_inside_scope() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(WRITE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt {
        session: id,
        text: "write a note".into(),
    });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else {
        unreachable!()
    };

    handle.send(Command::ApprovePlan {
        session: id,
        plan_id,
        approved: true,
    });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::FileEdit { .. })).await;
    let EventKind::FileEdit {
        path,
        bytes_written,
    } = kind
    else {
        unreachable!()
    };
    assert_eq!(path.to_string_lossy(), "notes.txt");
    assert_eq!(bytes_written, 5);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "hello"
    );
}

#[tokio::test]
async fn pause_parks_the_turn_until_resume() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(WRITE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt {
        session: id,
        text: "write a note".into(),
    });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else {
        unreachable!()
    };

    handle.send(Command::Pause { session: id });
    handle.send(Command::ApprovePlan {
        session: id,
        plan_id,
        approved: true,
    });

    // Paused before the first execute call: the file must not appear.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(!dir.path().join("notes.txt").exists());

    handle.send(Command::Resume { session: id });

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::FileEdit { .. })).await;
    assert!(matches!(kind, EventKind::FileEdit { .. }));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "hello"
    );
}

#[tokio::test]
async fn an_out_of_scope_write_asks_instead_of_executing() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(OUT_OF_SCOPE_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt {
        session: id,
        text: "write a note".into(),
    });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else {
        unreachable!()
    };
    handle.send(Command::ApprovePlan {
        session: id,
        plan_id,
        approved: true,
    });

    // The out-of-scope write must ask, never execute. The turn feeds each denial back to the
    // model; with a static provider it re-proposes the same out-of-scope write, so the turn
    // aborts only after the consecutive-denial cap is reached — and still never writes.
    for _ in 0..light_factory_engine::turn::MAX_CONSECUTIVE_DENIALS {
        let kind = next_matching(&mut events, |k| {
            matches!(k, EventKind::ApprovalRequest { .. })
        })
        .await;
        let EventKind::ApprovalRequest { request_id, .. } = kind else {
            unreachable!()
        };
        handle.send(Command::ApproveAction {
            session: id,
            request_id,
            approved: false,
        });
    }

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::TurnComplete { .. })).await;
    assert!(matches!(kind, EventKind::TurnComplete { ok: false }));
    assert!(!dir.path().join("Cargo.toml").exists());
}

#[tokio::test]
async fn the_step_budget_stops_a_turn_that_never_finishes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
    let ws = Arc::new(LocalWorkspace::new(dir.path().to_path_buf()).unwrap());
    let id = SessionId::new();
    let handle = Session::spawn(id, ws, provider(READ_JSON));
    let mut events = handle.subscribe();

    handle.send(Command::SendPrompt {
        session: id,
        text: "read forever".into(),
    });
    let kind = next_matching(&mut events, |k| matches!(k, EventKind::PlanProposed { .. })).await;
    let EventKind::PlanProposed { plan_id, .. } = kind else {
        unreachable!()
    };
    handle.send(Command::ApprovePlan {
        session: id,
        plan_id,
        approved: true,
    });

    // The in-scope read never returns `done`, so the turn runs until the step budget trips.
    let kind = next_matching(
        &mut events,
        |k| matches!(k, EventKind::Error { code, .. } if code == "step_budget_exceeded"),
    )
    .await;
    assert!(matches!(kind, EventKind::Error { .. }));

    let kind = next_matching(&mut events, |k| matches!(k, EventKind::TurnComplete { .. })).await;
    assert!(matches!(kind, EventKind::TurnComplete { ok: false }));
}
