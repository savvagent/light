//! Prompt rendering and structured extraction. The provider interface is prompt-and-parse,
//! so the engine renders the whole prompt and pulls JSON back out of the completion.

use light_factory_protocol::session::Plan;
use serde::de::DeserializeOwned;

/// Parse `T` out of `text`, tolerating Markdown code fences and surrounding prose by slicing
/// from the first `{` to the last `}`.
pub fn extract_json<T: DeserializeOwned>(text: &str) -> anyhow::Result<T> {
    let start = text
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("no JSON object found in completion"))?;
    let end = text
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("no JSON object found in completion"))?;
    if end < start {
        anyhow::bail!("no JSON object found in completion");
    }
    Ok(serde_json::from_str(&text[start..=end])?)
}

/// The planning prompt. The model must answer with a `Plan` as JSON.
pub fn render_plan_prompt(goal: &str) -> String {
    format!(
        r#"You are the planner for an agentic coding session.

Goal: {goal}

Produce a plan as a single JSON object and nothing else. Shape:

{{
  "id": "<uuid v4>",
  "summary": "<one sentence>",
  "steps": [{{ "id": "<uuid v4>", "description": "<what this step does>" }}],
  "scope": {{
    "write_paths": ["<glob relative to the repo root>"],
    "commands": [{{ "program": "<bare program name>", "args": [{{"Exact": "<arg>"}}, "Any"] }}]
  }}
}}

The scope is a contract. Declare every path you will write to and every command you will run.
Anything outside it stops and asks the human. Commands run with no shell: no pipes,
redirection, chaining, or globbing. Reads need no declaration.
"#
    )
}

/// The execution prompt: the approved plan plus the transcript so far. The model answers with
/// a single tool call as JSON, or `{{"done": true}}` when the plan is complete.
pub fn render_execute_prompt(goal: &str, plan: &Plan, transcript: &[String]) -> String {
    let steps = plan
        .steps
        .iter()
        .map(|s| format!("- {}", s.description))
        .collect::<Vec<_>>()
        .join("\n");

    let history = if transcript.is_empty() {
        String::new()
    } else {
        format!("\nSo far:\n{}\n", transcript.join("\n"))
    };

    format!(
        r#"You are executing an approved plan.

Goal: {goal}
Plan: {summary}
{steps}
{history}
Answer with a single JSON object and nothing else, either a tool call:

{{ "tool": "fs.read",  "args": {{ "path": "<path>" }} }}
{{ "tool": "fs.list",  "args": {{ "glob": "<glob>" }} }}
{{ "tool": "fs.write", "args": {{ "path": "<path>", "contents": "<full file contents>" }} }}
{{ "tool": "bash",     "args": {{ "program": "<bare name>", "args": ["<arg>"] }} }}

or, when the plan is complete:

{{ "done": true }}
"#,
        summary = plan.summary
    )
}
