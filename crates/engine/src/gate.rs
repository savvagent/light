//! `PlanGate`: the deterministic, non-LLM guardrail. Reads are free inside the workspace;
//! writes and commands must fall inside the approved plan's scope; the sensitive-path floor
//! always asks and is never unlocked by plan approval.

use std::path::{Component, Path, PathBuf};

use light_factory_engine_core::tool::{Decision, PermissionGate};
use light_factory_protocol::is_sensitive;
use light_factory_protocol::session::{ArgPattern, CommandPattern, GateReason, Scope};
use serde_json::Value;

pub struct PlanGate {
    scope: Option<Scope>,
}

impl PlanGate {
    pub fn new(scope: Option<Scope>) -> Self {
        Self { scope }
    }

    pub fn with_scope(&mut self, scope: Scope) {
        self.scope = Some(scope);
    }

    /// Lexically normalize a relative path. Returns `None` if it is absolute or escapes.
    fn normalize(path: &str) -> Option<PathBuf> {
        let path = Path::new(path);
        if path.is_absolute() {
            return None;
        }
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Normal(c) => out.push(c),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        return None;
                    }
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Some(out)
    }

    fn outside(what: impl Into<String>) -> Decision {
        Decision::Ask(GateReason::OutsideScope { what: what.into() })
    }

    fn floor(path: PathBuf) -> Decision {
        Decision::Ask(GateReason::SensitiveFloor { path })
    }

    fn command_matches(pattern: &CommandPattern, program: &str, args: &[String]) -> bool {
        if pattern.program != program || pattern.args.len() != args.len() {
            return false;
        }
        pattern.args.iter().zip(args).all(|(p, a)| match p {
            ArgPattern::Any => true,
            ArgPattern::Exact(want) => want == a,
        })
    }
}

impl PermissionGate for PlanGate {
    fn evaluate(&self, tool: &str, args: &Value) -> Decision {
        match tool {
            "fs.read" | "fs.list" => {
                let raw = args
                    .get("path")
                    .or_else(|| args.get("glob"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();

                if is_sensitive(raw) {
                    return Self::floor(PathBuf::from(raw));
                }
                match Self::normalize(raw) {
                    Some(_) => Decision::Allow,
                    None => Self::outside(raw),
                }
            }

            "fs.write" => {
                let raw = args.get("path").and_then(Value::as_str).unwrap_or_default();

                if is_sensitive(raw) {
                    return Self::floor(PathBuf::from(raw));
                }
                let Some(normalized) = Self::normalize(raw) else {
                    return Self::outside(raw);
                };
                let Some(scope) = &self.scope else {
                    return Self::outside(raw);
                };

                let as_str = normalized.to_string_lossy();
                let permitted = scope.write_paths.iter().any(|pattern| {
                    glob::Pattern::new(pattern)
                        .map(|p| p.matches(&as_str))
                        .unwrap_or(false)
                });

                if permitted {
                    Decision::Allow
                } else {
                    Self::outside(raw)
                }
            }

            "bash" => {
                let program = args
                    .get("program")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let argv: Vec<String> = args
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|v| v.as_str().unwrap_or_default().to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                if let Some(sensitive) = argv.iter().find(|a| is_sensitive(a)) {
                    return Self::floor(PathBuf::from(sensitive));
                }

                let Some(scope) = &self.scope else {
                    return Self::outside(program);
                };

                let permitted = scope
                    .commands
                    .iter()
                    .any(|p| Self::command_matches(p, program, &argv));

                if permitted {
                    Decision::Allow
                } else {
                    Self::outside(format!("{program} {}", argv.join(" ")))
                }
            }

            _ => Decision::Deny,
        }
    }
}
