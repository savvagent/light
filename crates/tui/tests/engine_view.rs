use light_factory_protocol::session::{EventKind, GateReason, Plan, Scope};
use light_factory_tui::engine_view::{describe_event, pending_prompt};
use light_factory_tui::i18n::Locale;
use uuid::Uuid;

fn plan_event() -> EventKind {
    EventKind::PlanProposed {
        plan_id: Uuid::nil(),
        plan: Plan {
            id: Uuid::nil(),
            summary: "do the thing".into(),
            steps: vec![],
            scope: Scope::default(),
        },
    }
}

#[test]
fn describes_a_file_edit_with_interpolated_values() {
    let line = describe_event(
        Locale::En,
        &EventKind::FileEdit {
            path: "src/main.rs".into(),
            bytes_written: 42,
        },
    );
    assert!(line.contains("src/main.rs"));
    assert!(line.contains("42"));
}

#[test]
fn a_proposed_plan_prompts_for_approval() {
    let prompt = pending_prompt(Locale::En, &plan_event()).unwrap();
    assert!(prompt.contains("do the thing"));
}

#[test]
fn an_approval_request_names_the_offending_path() {
    let kind = EventKind::ApprovalRequest {
        request_id: Uuid::nil(),
        reason: GateReason::SensitiveFloor {
            path: ".env".into(),
        },
        detail: "fs.write".into(),
    };
    let prompt = pending_prompt(Locale::En, &kind).unwrap();
    assert!(prompt.contains(".env"));
}

#[test]
fn ordinary_events_do_not_prompt() {
    assert!(
        pending_prompt(
            Locale::En,
            &EventKind::Log {
                message: "hi".into()
            }
        )
        .is_none()
    );
}

#[test]
fn spanish_differs_from_english_and_still_interpolates() {
    let en = pending_prompt(Locale::En, &plan_event()).unwrap();
    let es = pending_prompt(Locale::Es, &plan_event()).unwrap();
    assert_ne!(
        en, es,
        "the engine prompts must be translated, not passed through"
    );
    assert!(
        es.contains("do the thing"),
        "the plan summary is data, not a translated string"
    );
}
