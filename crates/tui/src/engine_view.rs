//! Pure rendering helpers for engine events. Kept free of ratatui types so they are
//! testable without a terminal.

use light_factory_protocol::session::{EventKind, GateReason};

use crate::i18n::{self, Locale};

/// One log line for an engine event, translated for `locale`.
pub fn describe_event(locale: Locale, kind: &EventKind) -> String {
    match kind {
        EventKind::PlanProposed { plan, .. } => i18n::t_with(
            locale,
            "engine.plan_proposed",
            &[("summary", &plan.summary)],
        ),
        EventKind::PlanDecided { approved, .. } => i18n::t(
            locale,
            if *approved {
                "engine.plan_approved"
            } else {
                "engine.plan_rejected"
            },
        )
        .to_string(),
        EventKind::FileEdit {
            path,
            bytes_written,
        } => i18n::t_with(
            locale,
            "engine.file_edit",
            &[
                ("path", &path.display().to_string()),
                ("bytes", &bytes_written.to_string()),
            ],
        ),
        EventKind::CommandRun { command, exit_code } => i18n::t_with(
            locale,
            "engine.command_run",
            &[("command", command), ("code", &exit_code.to_string())],
        ),
        EventKind::ApprovalRequest { detail, .. } => {
            i18n::t_with(locale, "engine.approval_needed", &[("detail", detail)])
        }
        EventKind::Log { message } => message.clone(),
        EventKind::TokenUsage {
            input_tokens,
            output_tokens,
        } => i18n::t_with(
            locale,
            "engine.token_usage",
            &[
                ("input", &input_tokens.to_string()),
                ("output", &output_tokens.to_string()),
            ],
        ),
        EventKind::TurnComplete { ok } => i18n::t(
            locale,
            if *ok {
                "engine.turn_complete"
            } else {
                "engine.turn_ended"
            },
        )
        .to_string(),
        EventKind::Error { code, message } => i18n::error_message(locale, code)
            .map(str::to_string)
            .unwrap_or_else(|| message.clone()),
    }
}

/// The prompt to show when an event needs a human answer, or `None` when it does not.
pub fn pending_prompt(locale: Locale, kind: &EventKind) -> Option<String> {
    let keys = i18n::t(locale, "engine.approve_keys");
    match kind {
        EventKind::PlanProposed { plan, .. } => {
            let body = i18n::t_with(
                locale,
                "engine.plan_prompt",
                &[
                    ("summary", &plan.summary),
                    ("steps", &plan.steps.len().to_string()),
                    ("paths", &plan.scope.write_paths.len().to_string()),
                    ("commands", &plan.scope.commands.len().to_string()),
                ],
            );
            Some(format!("{body}\n{keys}"))
        }
        EventKind::ApprovalRequest { reason, detail, .. } => {
            let why = match reason {
                GateReason::OutsideScope { what } => {
                    i18n::t_with(locale, "engine.reason_outside_scope", &[("what", what)])
                }
                GateReason::SensitiveFloor { path } => i18n::t_with(
                    locale,
                    "engine.reason_sensitive",
                    &[("path", &path.display().to_string())],
                ),
            };
            Some(format!("{detail}\n{why}\n{keys}"))
        }
        _ => None,
    }
}
