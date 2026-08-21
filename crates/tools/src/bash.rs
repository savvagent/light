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
use tokio::io::AsyncReadExt;

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

/// Drain a child's stdout/stderr handle into memory.
async fn drain(
    mut stream: impl tokio::io::AsyncRead + Unpin + Send + 'static,
) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    Ok(buf)
}

/// Kill a spawned child and, on Unix, its whole process group. `process_group(0)` makes the
/// child a group leader, so its pid is the pgid and `kill(-pgid)` reaches every descendant
/// (e.g. a `cargo test` subprocess), not just the direct child.
fn kill_child(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.start_kill();
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        command.process_group(0);

        let mut child = command.spawn()?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let out_task = tokio::spawn(drain(stdout));
        let err_task = tokio::spawn(drain(stderr));

        // Bound the child so a blocked or long-running command cannot park the turn. On expiry
        // the child and its whole process group are killed, and the tool returns a non-zero
        // result the model can react to.
        let status = match tokio::time::timeout(self.timeout, child.wait()).await {
            Ok(status) => status?,
            Err(_) => {
                kill_child(&mut child);
                let _ = child.wait().await;
                return Ok(json!({
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": format!("command timed out after {}s", self.timeout.as_secs()),
                }));
            }
        };

        let stdout = out_task.await??;
        let stderr = err_task.await??;

        Ok(json!({
            "exit_code": status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&stdout),
            "stderr": String::from_utf8_lossy(&stderr),
        }))
    }
}
