//! The turn state machine: plan, await approval, execute under the gate, complete.

use light_factory_engine_core::tool::{Decision, PermissionGate, Tool};
use light_factory_engine_core::types::CompleteRequest;
use light_factory_protocol::session::{Command, EventKind, GateReason, Plan};
use light_factory_tools::{BashTool, FsListTool, FsReadTool, FsWriteTool};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::gate::PlanGate;
use crate::prompt::{extract_json, render_execute_prompt, render_plan_prompt};
use crate::session::Session;

/// Consecutive gate denials tolerated before the turn aborts.
pub const MAX_CONSECUTIVE_DENIALS: usize = 3;

/// The maximum number of execute-loop iterations (one provider round-trip each) a single turn
/// may make before it is stopped as runaway.
pub const MAX_STEPS_PER_TURN: usize = 100;

/// The maximum number of characters kept per transcript entry before truncation, so a single
/// large `fs.read` result cannot dominate the re-rendered prompt.
const MAX_TRANSCRIPT_ENTRY_CHARS: usize = 4096;

/// Truncate a transcript entry to [`MAX_TRANSCRIPT_ENTRY_CHARS`] characters, tagging the cut.
fn transcript_entry(line: String) -> String {
    if line.len() <= MAX_TRANSCRIPT_ENTRY_CHARS {
        return line;
    }
    let mut truncated: String = line.chars().take(MAX_TRANSCRIPT_ENTRY_CHARS).collect();
    truncated.push_str("… [truncated]");
    truncated
}

#[derive(Deserialize)]
struct ToolCall {
    #[serde(default)]
    done: bool,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    args: Option<Value>,
}

impl Session {
    /// Run one turn to completion.
    pub(crate) async fn run_turn(
        &mut self,
        goal: &str,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) {
        let plan = match self.propose_plan(goal).await {
            Some(plan) => plan,
            None => {
                self.emit(EventKind::TurnComplete { ok: false });
                return;
            }
        };

        let plan_id = plan.id;
        self.emit(EventKind::PlanProposed {
            plan_id,
            plan: plan.clone(),
        });

        let approved = self.await_plan_decision(plan_id, commands).await;
        self.emit(EventKind::PlanDecided { plan_id, approved });
        if !approved {
            self.emit(EventKind::TurnComplete { ok: false });
            return;
        }

        self.approved = Some(plan.clone());
        let ok = self.execute(goal, &plan, commands).await;
        self.emit(EventKind::TurnComplete { ok });
    }

    async fn propose_plan(&mut self, goal: &str) -> Option<Plan> {
        let request = CompleteRequest {
            prompt: render_plan_prompt(goal),
        };
        let response = match self.provider.complete(request).await {
            Ok(r) => r,
            Err(e) => {
                self.emit(EventKind::Error {
                    code: "provider_error".into(),
                    message: e.to_string(),
                });
                return None;
            }
        };

        if let Some(usage) = response.usage {
            self.emit(EventKind::TokenUsage {
                input_tokens: usage.input_tokens as u64,
                output_tokens: usage.output_tokens as u64,
            });
        }

        match extract_json::<Plan>(&response.text) {
            Ok(plan) => Some(plan),
            Err(e) => {
                self.emit(EventKind::Error {
                    code: "invalid_plan".into(),
                    message: e.to_string(),
                });
                None
            }
        }
    }

    async fn await_plan_decision(
        &mut self,
        plan_id: Uuid,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) -> bool {
        while let Some(command) = commands.recv().await {
            match command {
                Command::ApprovePlan {
                    plan_id: id,
                    approved,
                    ..
                } if id == plan_id => {
                    return approved;
                }
                Command::Pause { .. } => self.paused = true,
                Command::Resume { .. } => self.paused = false,
                Command::Abort { .. } => return false,
                _ => {}
            }
        }
        // The command channel closed with no answer: fail closed.
        false
    }

    async fn execute(
        &mut self,
        goal: &str,
        plan: &Plan,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) -> bool {
        let gate = PlanGate::new(Some(plan.scope.clone()));
        let mut transcript: Vec<String> = Vec::new();
        let mut denials = 0usize;
        let mut steps = 0usize;

        loop {
            if steps >= MAX_STEPS_PER_TURN {
                self.emit(EventKind::Error {
                    code: "step_budget_exceeded".into(),
                    message: format!(
                        "the turn exceeded its {MAX_STEPS_PER_TURN}-step budget and was stopped"
                    ),
                });
                return false;
            }
            steps += 1;

            if !self.wait_if_paused(commands).await {
                return false;
            }

            let prompt = render_execute_prompt(goal, plan, &transcript);
            let response = match self.provider.complete(CompleteRequest { prompt }).await {
                Ok(r) => r,
                Err(e) => {
                    self.emit(EventKind::Error {
                        code: "provider_error".into(),
                        message: e.to_string(),
                    });
                    return false;
                }
            };

            if let Some(usage) = response.usage {
                self.emit(EventKind::TokenUsage {
                    input_tokens: usage.input_tokens as u64,
                    output_tokens: usage.output_tokens as u64,
                });
            }

            let call: ToolCall = match extract_json(&response.text) {
                Ok(c) => c,
                Err(e) => {
                    self.emit(EventKind::Error {
                        code: "invalid_tool_call".into(),
                        message: e.to_string(),
                    });
                    return false;
                }
            };

            if call.done {
                return true;
            }

            let (Some(name), Some(args)) = (call.tool, call.args) else {
                self.emit(EventKind::Error {
                    code: "invalid_tool_call".into(),
                    message: "expected `tool` and `args`, or `done`".into(),
                });
                return false;
            };

            match gate.evaluate(&name, &args) {
                Decision::Allow => {}
                Decision::Deny => {
                    denials += 1;
                    transcript.push(transcript_entry(format!("{name} -> denied: unknown tool")));
                    if denials >= MAX_CONSECUTIVE_DENIALS {
                        return false;
                    }
                    continue;
                }
                Decision::Ask(reason) => {
                    let request_id = Uuid::new_v4();
                    self.emit(EventKind::ApprovalRequest {
                        request_id,
                        reason: reason.clone(),
                        detail: format!("{name} {args}"),
                    });

                    if !self.await_action_decision(request_id, commands).await {
                        denials += 1;
                        transcript.push(transcript_entry(format!(
                            "{name} -> denied by the human: {}",
                            describe(&reason)
                        )));
                        if denials >= MAX_CONSECUTIVE_DENIALS {
                            return false;
                        }
                        continue;
                    }
                }
            }

            denials = 0;
            match self.dispatch(&name, args).await {
                Ok(result) => transcript.push(transcript_entry(format!("{name} -> {result}"))),
                Err(e) => transcript.push(transcript_entry(format!("{name} -> error: {e}"))),
            }
        }
    }

    /// Park while paused. Returns `false` if the turn was aborted or the channel closed —
    /// both fail closed, ending the turn rather than resuming unsupervised.
    async fn wait_if_paused(&mut self, commands: &mut mpsc::UnboundedReceiver<Command>) -> bool {
        while self.paused {
            match commands.recv().await {
                Some(Command::Resume { .. }) => self.paused = false,
                Some(Command::Pause { .. }) => {}
                Some(Command::Abort { .. }) | None => return false,
                Some(_) => {}
            }
        }
        true
    }

    async fn await_action_decision(
        &mut self,
        request_id: Uuid,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) -> bool {
        while let Some(command) = commands.recv().await {
            match command {
                Command::ApproveAction {
                    request_id: id,
                    approved,
                    ..
                } if id == request_id => {
                    return approved;
                }
                Command::Pause { .. } => self.paused = true,
                Command::Resume { .. } => self.paused = false,
                Command::Abort { .. } => return false,
                _ => {}
            }
        }
        false
    }

    async fn dispatch(&mut self, name: &str, args: Value) -> anyhow::Result<Value> {
        let tool: Box<dyn Tool> = match name {
            "fs.read" => Box::new(FsReadTool {
                workspace: self.workspace.clone(),
            }),
            "fs.list" => Box::new(FsListTool {
                workspace: self.workspace.clone(),
            }),
            "fs.write" => Box::new(FsWriteTool {
                workspace: self.workspace.clone(),
            }),
            "bash" => Box::new(BashTool {
                workspace_root: self.workspace.root().to_path_buf(),
            }),
            other => anyhow::bail!("unknown tool: {other}"),
        };

        let result = tool.call(args.clone()).await?;

        match name {
            "fs.write" => {
                let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
                let bytes_written = result
                    .get("bytes_written")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                self.emit(EventKind::FileEdit {
                    path: path.into(),
                    bytes_written,
                });
            }
            "bash" => {
                let program = args
                    .get("program")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let exit_code = result
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .unwrap_or(-1) as i32;
                self.emit(EventKind::CommandRun {
                    command: program.to_string(),
                    exit_code,
                });
            }
            _ => {}
        }

        Ok(result)
    }
}

fn describe(reason: &GateReason) -> String {
    match reason {
        GateReason::OutsideScope { what } => format!("outside the approved scope ({what})"),
        GateReason::SensitiveFloor { path } => {
            format!("sensitive path ({})", path.display())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::transcript_entry;

    #[test]
    fn short_entries_pass_through_unchanged() {
        assert_eq!(
            transcript_entry("fs.read -> hello".into()),
            "fs.read -> hello"
        );
    }

    #[test]
    fn long_entries_are_truncated_and_tagged() {
        let long = "x".repeat(10_000);
        let out = transcript_entry(long);
        assert!(out.ends_with("[truncated]"));
        assert!(out.len() < 10_000);
        assert!(out.chars().count() <= 4096 + "… [truncated]".len());
    }
}
