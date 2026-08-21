//! The command tool. Takes a program and an argument vector and runs it directly — there is
//! no shell, so no pipes, redirection, chaining, globbing, or substitution.
//!
//! This is deliberate. A permission gate cannot meaningfully evaluate a shell string:
//! `cargo test; rm -rf ~` matches any pattern that permits `cargo test`. Keeping the argument
//! vector structured is what makes the gate enforceable.

use std::path::PathBuf;

use async_trait::async_trait;
use light_factory_engine_core::tool::Tool;
use serde_json::{Value, json};

pub struct BashTool {
    pub workspace_root: PathBuf,
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

        let output = tokio::process::Command::new(program)
            .args(&argv)
            .current_dir(&self.workspace_root)
            .output()
            .await?;

        Ok(json!({
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }))
    }
}
