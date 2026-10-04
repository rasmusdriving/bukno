//! The commands Bukno can send to T3, as typed values.
//!
//! Shapes follow `OrchestrationV2Command` and `OrchestrationV2ThreadLaunchInput`
//! in `packages/contracts/src/orchestrationV2.ts`, and how T3's own client
//! builds them in `packages/client-runtime/src/operations/commands.ts`, at the
//! pinned revision. This server resolves the command context itself
//! (`serverResolvedCommandContext`), so a send says what the user meant and
//! T3 picks the run.
//!
//! Every user action gets one command ID when it is created. T3 keeps a
//! receipt per ID and answers a repeated ID with the first result, so sending
//! the same [`CommandRequest`] again after an unconfirmed reply cannot run it
//! twice.

use serde_json::{Map, Value, json};

use crate::rpc::Method;

/// How a message is delivered while the chat may be working.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Start a turn, or let T3 choose when one is running (its default).
    Auto,
    /// Add the message to the running turn.
    Steer,
    /// Send it after the running turn finishes.
    Queue,
}

/// A model to run a new chat with: the provider instance and model slug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelChoice {
    pub instance_id: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outgoing {
    Send {
        thread_id: String,
        message_id: String,
        text: String,
        delivery: Delivery,
    },
    /// A new chat in a project, with its first message.
    Launch {
        thread_id: String,
        message_id: String,
        project_id: String,
        title: String,
        model: ModelChoice,
        runtime_mode: String,
        text: String,
    },
    Interrupt {
        thread_id: String,
        run_id: String,
    },
    /// `decision` is `accept` or `decline`.
    Approve {
        thread_id: String,
        request_id: String,
        decision: &'static str,
    },
    /// Answers by question ID: a string, or a list for multi-select.
    Answer {
        thread_id: String,
        request_id: String,
        answers: Map<String, Value>,
    },
    /// Close a question that is answered by message, without answering.
    Dismiss {
        thread_id: String,
        request_id: String,
    },
    PromoteQueued {
        thread_id: String,
        queued_run_id: String,
        target_run_id: String,
    },
    CancelQueued {
        thread_id: String,
        run_id: String,
    },
    ResumeQueue {
        thread_id: String,
    },
    SetRuntimeMode {
        thread_id: String,
        runtime_mode: String,
    },
}

/// One user action and the command ID that stays with it through retries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRequest {
    pub command_id: String,
    pub outgoing: Outgoing,
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The runtime modes T3 offers, with Bukno's words for them.
pub const RUNTIME_MODES: [(&str, &str, &str); 4] = [
    ("approval-required", "Ask before commands", "The agent asks before it runs commands or changes files."),
    ("auto-accept-edits", "Accept edits", "File changes go ahead; commands still ask first."),
    ("auto", "Auto", "The provider decides what needs a question."),
    ("full-access", "Full access", "Commands and changes run without asking."),
];

pub fn runtime_mode_label(mode: &str) -> &'static str {
    RUNTIME_MODES.iter().find(|(m, ..)| *m == mode).map_or("Ask before commands", |(_, label, _)| label)
}

impl CommandRequest {
    pub fn new(outgoing: Outgoing) -> Self {
        Self { command_id: new_id(), outgoing }
    }

    pub fn method(&self) -> Method {
        match self.outgoing {
            Outgoing::Launch { .. } => Method::LaunchThread,
            _ => Method::DispatchCommand,
        }
    }

    pub fn thread_id(&self) -> &str {
        match &self.outgoing {
            Outgoing::Send { thread_id, .. }
            | Outgoing::Launch { thread_id, .. }
            | Outgoing::Interrupt { thread_id, .. }
            | Outgoing::Approve { thread_id, .. }
            | Outgoing::Answer { thread_id, .. }
            | Outgoing::Dismiss { thread_id, .. }
            | Outgoing::PromoteQueued { thread_id, .. }
            | Outgoing::CancelQueued { thread_id, .. }
            | Outgoing::ResumeQueue { thread_id }
            | Outgoing::SetRuntimeMode { thread_id, .. } => thread_id,
        }
    }

    /// The message this command carries, if it sends one.
    pub fn message_id(&self) -> Option<&str> {
        match &self.outgoing {
            Outgoing::Send { message_id, .. } | Outgoing::Launch { message_id, .. } => Some(message_id),
            _ => None,
        }
    }

    /// What the command does, in a few words for notices.
    pub fn describe(&self) -> &'static str {
        match self.outgoing {
            Outgoing::Send { .. } => "Your message",
            Outgoing::Launch { .. } => "The new chat",
            Outgoing::Interrupt { .. } => "Stop",
            Outgoing::Approve { .. } => "Your decision",
            Outgoing::Answer { .. } | Outgoing::Dismiss { .. } => "Your answer",
            Outgoing::PromoteQueued { .. } => "Steer now",
            Outgoing::CancelQueued { .. } => "Remove",
            Outgoing::ResumeQueue { .. } => "Resume",
            Outgoing::SetRuntimeMode { .. } => "The permission change",
        }
    }

    /// The RPC payload. The same request always gives the same payload.
    pub fn payload(&self) -> Value {
        let id = &self.command_id;
        match &self.outgoing {
            Outgoing::Send { thread_id, message_id, text, delivery } => {
                let mut command = json!({
                    "type": "message.dispatch",
                    "commandId": id,
                    "createdBy": "user",
                    "creationSource": "web",
                    "threadId": thread_id,
                    "messageId": message_id,
                    "text": text,
                    "attachments": [],
                });
                match delivery {
                    Delivery::Queue => command["dispatchMode"] = json!({"type": "queue_after_active"}),
                    Delivery::Auto | Delivery::Steer => {
                        command["dispatchMode"] = json!({"type": "start_immediately"});
                        command["deliveryIntent"] = json!(if *delivery == Delivery::Steer { "steer" } else { "auto" });
                    }
                }
                command
            }
            Outgoing::Launch { thread_id, message_id, project_id, title, model, runtime_mode, text } => json!({
                "commandId": id,
                "creationSource": "web",
                "threadId": thread_id,
                "projectId": project_id,
                "title": title,
                "generateTitle": true,
                "modelSelection": {"instanceId": model.instance_id, "model": model.model},
                "runtimeMode": runtime_mode,
                "interactionMode": "default",
                "workspaceStrategy": {"type": "root"},
                "initialMessage": {"messageId": message_id, "text": text, "attachments": []},
            }),
            Outgoing::Interrupt { thread_id, run_id } => json!({
                "type": "run.interrupt",
                "commandId": id,
                "threadId": thread_id,
                "runId": run_id,
                "holdQueue": true,
            }),
            Outgoing::Approve { thread_id, request_id, decision } => json!({
                "type": "runtime-request.respond",
                "commandId": id,
                "threadId": thread_id,
                "requestId": request_id,
                "decision": decision,
            }),
            Outgoing::Answer { thread_id, request_id, answers } => json!({
                "type": "runtime-request.respond",
                "commandId": id,
                "threadId": thread_id,
                "requestId": request_id,
                "answers": answers,
            }),
            Outgoing::Dismiss { thread_id, request_id } => json!({
                "type": "thread.user-input.dismiss",
                "commandId": id,
                "threadId": thread_id,
                "requestId": request_id,
            }),
            Outgoing::PromoteQueued { thread_id, queued_run_id, target_run_id } => json!({
                "type": "queued-message.promote-to-steer",
                "commandId": id,
                "threadId": thread_id,
                "queuedRunId": queued_run_id,
                "targetRunId": target_run_id,
            }),
            Outgoing::CancelQueued { thread_id, run_id } => json!({
                "type": "queued-run.cancel",
                "commandId": id,
                "threadId": thread_id,
                "runId": run_id,
            }),
            Outgoing::ResumeQueue { thread_id } => json!({
                "type": "queue.resume",
                "commandId": id,
                "threadId": thread_id,
            }),
            Outgoing::SetRuntimeMode { thread_id, runtime_mode } => json!({
                "type": "thread.runtime-mode.set",
                "commandId": id,
                "threadId": thread_id,
                "runtimeMode": runtime_mode,
            }),
        }
    }
}

/// A chat title from the first message: its first line, shortened.
pub fn title_from(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("New chat");
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    title
}
