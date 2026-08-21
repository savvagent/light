//! The tool seam. Every call runs a deterministic `PermissionGate` before dispatch.

use async_trait::async_trait;
use light_factory_protocol::session::GateReason;
use serde_json::Value;

/// A callable tool: a stable name and a JSON-in / JSON-out call.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    async fn call(&self, args: Value) -> anyhow::Result<Value>;
}

/// The verdict a permission gate returns for a proposed tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask(GateReason),
    Deny,
}

/// Deterministic, non-LLM evaluation of a proposed tool call.
pub trait PermissionGate: Send + Sync {
    fn evaluate(&self, tool: &str, args: &Value) -> Decision;
}

/// Cooperative pause, consulted at phase boundaries and between tool calls.
#[async_trait]
pub trait PauseController: Send + Sync {
    fn should_pause(&self) -> bool;
    async fn wait_for_resume(&self);
}

/// Default: never pauses.
pub struct NeverPause;

#[async_trait]
impl PauseController for NeverPause {
    fn should_pause(&self) -> bool {
        false
    }
    async fn wait_for_resume(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        async fn call(&self, args: Value) -> anyhow::Result<Value> {
            Ok(args)
        }
    }

    struct FixedGate(Decision);

    impl PermissionGate for FixedGate {
        fn evaluate(&self, _tool: &str, _args: &Value) -> Decision {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn a_tool_dispatches_by_name() {
        let tool = EchoTool;
        assert_eq!(tool.name(), "echo");
        let out = tool.call(serde_json::json!({"x": 1})).await.unwrap();
        assert_eq!(out, serde_json::json!({"x": 1}));
    }

    #[test]
    fn a_gate_returns_its_verdict() {
        let gate = FixedGate(Decision::Deny);
        assert_eq!(
            gate.evaluate("anything", &serde_json::json!({})),
            Decision::Deny
        );
    }

    #[tokio::test]
    async fn never_pause_does_not_park() {
        let pause = NeverPause;
        assert!(!pause.should_pause());
        pause.wait_for_resume().await;
    }
}
