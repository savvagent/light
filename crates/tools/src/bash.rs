//! The command tool. Takes a program and an argument vector and runs it directly — there is
//! no shell, so no pipes, redirection, chaining, globbing, or substitution.
//!
//! This is deliberate. A permission gate cannot meaningfully evaluate a shell string:
//! `cargo test; rm -rf ~` matches any pattern that permits `cargo test`. Keeping the argument
//! vector structured is what makes the gate enforceable.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use light_factory_engine_core::tool::Tool;
use serde_json::{Value, json};

/// The default wall-clock bound on a single command.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

pub struct BashTool {
    pub workspace_root: PathBuf,
    timeout: Duration,
}

impl BashTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    /// Override the command timeout (primarily for tests).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    async fn call(&self, args: Value) -> anyhow::Result<Value> {
        let program = args
            .get("program")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("bash requires a string `program`"))?;

        if program.contains('/') || program.contains('\\') {
            anyhow::bail!("`program` must be a bare program name, not a path: {program}");
        }

        let argv: Vec<String> = args
            .get("args")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("bash requires an array `args`"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow::anyhow!("every element of `args` must be a string"))
            })
            .collect::<anyhow::Result<_>>()?;

        let mut command = tokio::process::Command::new(program);
        command
            .args(&argv)
            .current_dir(&self.workspace_root)
            .stdin(Stdio::null())
            .kill_on_drop(true);

        // Bound the child so a blocked or long-running command cannot park the turn. On expiry
        // the child is killed (via kill_on_drop when the future is dropped) and the tool
        // returns a non-zero result the model can react to.
        let output = match tokio::time::timeout(self.timeout, command.output()).await {
            Ok(output) => output?,
            Err(_) => {
                return Ok(json!({
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": format!("command timed out after {}s", self.timeout.as_secs()),
                }));
            }
        };

        Ok(json!({
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }))
    }
}
