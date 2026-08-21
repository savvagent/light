use light_factory_engine::gate::PlanGate;
use light_factory_engine_core::tool::{Decision, PermissionGate};
use light_factory_protocol::session::{ArgPattern, CommandPattern, GateReason, Scope};
use serde_json::json;

fn scope() -> Scope {
    Scope {
        write_paths: vec!["src/**".into(), "README.md".into()],
        commands: vec![CommandPattern {
            program: "cargo".into(),
            args: vec![ArgPattern::Exact("test".into()), ArgPattern::Any],
        }],
    }
}

#[test]
fn reads_are_allowed_anywhere_in_the_workspace() {
    let gate = PlanGate::new(Some(scope()));
    assert_eq!(
        gate.evaluate("fs.read", &json!({"path": "docs/whatever.md"})),
        Decision::Allow
    );
    assert_eq!(
        gate.evaluate("fs.list", &json!({"glob": "**/*.rs"})),
        Decision::Allow
    );
}

#[test]
fn reads_of_sensitive_paths_still_ask() {
    let gate = PlanGate::new(Some(scope()));
    assert!(matches!(
        gate.evaluate("fs.read", &json!({"path": ".env"})),
        Decision::Ask(GateReason::SensitiveFloor { .. })
    ));
    assert!(matches!(
        gate.evaluate("fs.read", &json!({"path": "home/.ssh/id_rsa"})),
        Decision::Ask(GateReason::SensitiveFloor { .. })
    ));
}

#[test]
fn writes_inside_scope_are_allowed() {
    let gate = PlanGate::new(Some(scope()));
    assert_eq!(
        gate.evaluate("fs.write", &json!({"path": "src/main.rs"})),
        Decision::Allow
    );
    assert_eq!(
        gate.evaluate("fs.write", &json!({"path": "README.md"})),
        Decision::Allow
    );
}

#[test]
fn writes_outside_scope_ask() {
    let gate = PlanGate::new(Some(scope()));
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": "Cargo.toml"})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
}

#[test]
fn traversal_escapes_cannot_satisfy_a_scope_glob() {
    let gate = PlanGate::new(Some(scope()));
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": "src/../../../etc/passwd"})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": "/etc/passwd"})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
}

#[test]
fn sensitive_writes_ask_even_when_a_glob_would_match() {
    let gate = PlanGate::new(Some(Scope {
        write_paths: vec!["**".into()],
        commands: vec![],
    }));
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": ".env"})),
        Decision::Ask(GateReason::SensitiveFloor { .. })
    ));
}

#[test]
fn commands_match_program_and_arg_patterns() {
    let gate = PlanGate::new(Some(scope()));
    assert_eq!(
        gate.evaluate(
            "bash",
            &json!({"program": "cargo", "args": ["test", "--workspace"]})
        ),
        Decision::Allow
    );
    assert!(matches!(
        gate.evaluate(
            "bash",
            &json!({"program": "cargo", "args": ["publish", "--x"]})
        ),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "rm", "args": ["-rf", "/"]})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
}

#[test]
fn arity_must_match_the_pattern() {
    let gate = PlanGate::new(Some(scope()));
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "cargo", "args": ["test"]})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert!(matches!(
        gate.evaluate(
            "bash",
            &json!({"program": "cargo", "args": ["test", "a", "b"]})
        ),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
}

#[test]
fn sensitive_command_arguments_ask_even_when_the_pattern_matches() {
    let gate = PlanGate::new(Some(Scope {
        write_paths: vec![],
        commands: vec![CommandPattern {
            program: "cat".into(),
            args: vec![ArgPattern::Any],
        }],
    }));
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "cat", "args": [".env"]})),
        Decision::Ask(GateReason::SensitiveFloor { .. })
    ));
}

#[test]
fn no_approved_plan_means_nothing_is_authorized() {
    let gate = PlanGate::new(None);
    assert!(matches!(
        gate.evaluate("fs.write", &json!({"path": "src/main.rs"})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert!(matches!(
        gate.evaluate("bash", &json!({"program": "cargo", "args": ["test", "x"]})),
        Decision::Ask(GateReason::OutsideScope { .. })
    ));
    assert_eq!(
        gate.evaluate("fs.read", &json!({"path": "src/main.rs"})),
        Decision::Allow
    );
}

#[test]
fn unknown_tools_are_denied() {
    let gate = PlanGate::new(Some(scope()));
    assert_eq!(gate.evaluate("net.fetch", &json!({})), Decision::Deny);
}
