use light_factory_engine::prompt::{extract_json, render_plan_prompt};
use light_factory_protocol::session::Plan;

#[test]
fn extracts_json_from_a_fenced_block() {
    let text = r#"Sure, here is the plan:

```json
{"id":"00000000-0000-0000-0000-000000000000","summary":"do a thing","steps":[],"scope":{"write_paths":["src/**"],"commands":[]}}
```

Let me know."#;

    let plan: Plan = extract_json(text).unwrap();
    assert_eq!(plan.summary, "do a thing");
    assert_eq!(plan.scope.write_paths, vec!["src/**".to_string()]);
}

#[test]
fn extracts_json_without_a_fence() {
    let text = r#"{"id":"00000000-0000-0000-0000-000000000000","summary":"bare","steps":[],"scope":{"write_paths":[],"commands":[]}}"#;
    let plan: Plan = extract_json(text).unwrap();
    assert_eq!(plan.summary, "bare");
}

#[test]
fn no_json_is_an_error() {
    let err = extract_json::<Plan>("I refuse to answer.").unwrap_err();
    assert!(err.to_string().contains("no JSON object"));
}

#[test]
fn the_plan_prompt_states_the_goal_and_demands_json() {
    let prompt = render_plan_prompt("add a health endpoint");
    assert!(prompt.contains("add a health endpoint"));
    assert!(prompt.contains("write_paths"));
    assert!(prompt.contains("program"));
}
