//! Text forms of core values in the database.

use bukno_core::decision::{DecisionKind, DecisionState, Question};
use bukno_core::message::{DeliveryState, Provider};
use bukno_core::run::RunState;
use bukno_core::task::{RunSettings, Writes};
use serde_json::{Value, json};

pub fn provider(p: Provider) -> &'static str {
    match p {
        Provider::Codex => "codex",
        Provider::Claude => "claude",
    }
}

pub fn parse_provider(s: &str) -> Provider {
    if s == "claude" { Provider::Claude } else { Provider::Codex }
}

pub fn run_state(s: RunState) -> &'static str {
    match s {
        RunState::Preparing => "preparing",
        RunState::Starting => "starting",
        RunState::Running => "running",
        RunState::WaitingForApproval => "waiting_for_approval",
        RunState::WaitingForInput => "waiting_for_input",
        RunState::Cancelling => "cancelling",
        RunState::Completed => "completed",
        RunState::Failed => "failed",
        RunState::Interrupted => "interrupted",
        RunState::OutcomeUnknown => "outcome_unknown",
    }
}

pub fn parse_run_state(s: &str) -> RunState {
    match s {
        "preparing" => RunState::Preparing,
        "starting" => RunState::Starting,
        "running" => RunState::Running,
        "waiting_for_approval" => RunState::WaitingForApproval,
        "waiting_for_input" => RunState::WaitingForInput,
        "cancelling" => RunState::Cancelling,
        "completed" => RunState::Completed,
        "failed" => RunState::Failed,
        "interrupted" => RunState::Interrupted,
        // Anything unrecognized is treated as unknown, never as finished.
        _ => RunState::OutcomeUnknown,
    }
}

pub fn delivery(s: DeliveryState) -> &'static str {
    match s {
        DeliveryState::Queued => "queued",
        DeliveryState::AboutToSend => "about_to_send",
        DeliveryState::Sent => "sent",
        DeliveryState::Acknowledged => "acknowledged",
        DeliveryState::Rejected => "rejected",
        DeliveryState::Unknown => "unknown",
    }
}

pub fn parse_delivery(s: &str) -> DeliveryState {
    match s {
        "queued" => DeliveryState::Queued,
        "about_to_send" => DeliveryState::AboutToSend,
        "sent" => DeliveryState::Sent,
        "acknowledged" => DeliveryState::Acknowledged,
        "rejected" => DeliveryState::Rejected,
        _ => DeliveryState::Unknown,
    }
}

pub fn decision_state(s: DecisionState) -> &'static str {
    match s {
        DecisionState::Pending => "pending",
        DecisionState::Sending => "sending",
        DecisionState::Resolved => "resolved",
        DecisionState::Expired => "expired",
    }
}

pub fn settings(s: &RunSettings) -> String {
    json!({
        "preset": s.preset,
        "writes": match s.writes {
            Writes::Never => "never",
            Writes::AfterApproval => "after_approval",
            Writes::Freely => "freely",
        },
        "model": s.model,
        "effort": s.effort,
    })
    .to_string()
}

pub fn parse_settings(text: &str) -> RunSettings {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    RunSettings {
        preset: s("preset").unwrap_or_default(),
        // Unknown means assume it can write, so the lease is kept.
        writes: match v.get("writes").and_then(Value::as_str) {
            Some("never") => Writes::Never,
            Some("after_approval") => Writes::AfterApproval,
            _ => Writes::Freely,
        },
        model: s("model"),
        effort: s("effort"),
    }
}

pub fn decision_kind(kind: &DecisionKind) -> String {
    match kind {
        DecisionKind::Command { command, cwd, reason } => {
            json!({"type": "command", "command": command, "cwd": cwd, "reason": reason})
        }
        DecisionKind::FileChange { files, reason } => json!({"type": "fileChange", "files": files, "reason": reason}),
        DecisionKind::Question { questions } => json!({
            "type": "question",
            "questions": questions.iter().map(question).collect::<Vec<_>>(),
        }),
        DecisionKind::Access { what, detail } => json!({"type": "access", "what": what, "detail": detail}),
    }
    .to_string()
}

fn question(q: &Question) -> Value {
    json!({"id": q.id, "header": q.header, "text": q.text, "options": q.options, "other": q.other, "multi": q.multi})
}

pub fn allowed(kind: &DecisionKind) -> &'static str {
    match kind {
        DecisionKind::Question { .. } => r#"["answer","cancel"]"#,
        _ => r#"["allow_once","decline"]"#,
    }
}

pub fn key(components: &[String]) -> String {
    serde_json::to_string(components).unwrap_or_default()
}

pub fn parse_key(text: &str) -> Vec<String> {
    serde_json::from_str(text).unwrap_or_default()
}
