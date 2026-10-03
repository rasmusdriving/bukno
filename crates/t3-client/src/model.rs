//! Plain Rust types for the parts of T3's contracts this client reads.
//!
//! Only fields Bukno uses are declared; serde ignores the rest, so new
//! optional fields on a newer server do not break decoding. Union types are
//! decoded by hand from their tag so an unknown member becomes a visible
//! `Unknown` value instead of a decode failure. Source of truth:
//! `packages/contracts/src/{environment,orchestrationV2,orchestrationProject,server}.ts`
//! at the pinned revision.

use serde::Deserialize;
use serde_json::Value;

/// `GET /.well-known/t3/environment`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub environment_id: String,
    pub label: String,
    pub server_version: String,
    pub orchestration_protocol_version: u32,
}

/// The routing part of a model selection. Older records use `provider`
/// instead of `instanceId`; T3 reads both the same way.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    pub model: String,
}

impl ModelSelection {
    pub fn instance(&self) -> &str {
        self.instance_id.as_deref().or(self.provider.as_deref()).unwrap_or("")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectShell {
    pub id: String,
    pub title: String,
    pub workspace_root: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LatestMessage {
    pub role: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadShell {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub provider_instance_id: String,
    pub model_selection: ModelSelection,
    /// `idle` or a run status.
    pub status: String,
    #[serde(default)]
    pub activity_run_status: Option<String>,
    #[serde(default)]
    pub pending_runtime_request: Option<Value>,
    #[serde(default)]
    pub latest_visible_message: Option<LatestMessage>,
    #[serde(default)]
    pub latest_user_message_at: Option<String>,
    #[serde(default)]
    pub pinned_at: Option<String>,
    #[serde(default)]
    pub settled_at: Option<String>,
    #[serde(default)]
    pub settled_override: Option<String>,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub deleted_at: Option<String>,
    #[serde(default)]
    pub lineage: Option<Value>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub visible_item_count: u64,
}

impl ThreadShell {
    /// The chat this one was delegated from, if any.
    pub fn parent_thread_id(&self) -> Option<&str> {
        self.lineage.as_ref()?.get("parentThreadId")?.as_str()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    pub snapshot_sequence: u64,
    pub projects: Vec<ProjectShell>,
    #[serde(default)]
    pub threads: Vec<ThreadShell>,
    #[serde(default)]
    pub archived_threads: Vec<ThreadShell>,
}

/// One item of `orchestration.subscribeShell`.
#[derive(Clone, Debug, PartialEq)]
pub enum ShellItem {
    Synchronized,
    /// The authoritative list: replaces everything.
    Snapshot(ShellSnapshot),
    /// Project metadata only (repository identity refreshes, and the first
    /// frame of a resume). Its threads are empty and it must not move the
    /// cursor, or the replayed events after it would be dropped as old.
    ProjectRefresh(ShellSnapshot),
    ProjectUpdated {
        sequence: u64,
        project: ProjectShell,
    },
    ProjectRemoved {
        sequence: u64,
        project_id: String,
    },
    ThreadUpdated {
        sequence: u64,
        archived: bool,
        thread: Box<ThreadShell>,
    },
    ThreadRemoved {
        sequence: u64,
        archived: bool,
        thread_id: String,
    },
    Unknown {
        kind: String,
    },
}

fn field<T: for<'de> Deserialize<'de>>(value: &Value, name: &str) -> Result<T, String> {
    let raw = value.get(name).ok_or_else(|| format!("missing {name}"))?;
    T::deserialize(raw).map_err(|e| format!("{name}: {e}"))
}

impl ShellItem {
    pub fn decode(value: &Value) -> Result<Self, String> {
        let kind = value.get("kind").and_then(Value::as_str).ok_or("shell item without kind")?;
        let archived = || value.get("location").and_then(Value::as_str) == Some("archive");
        Ok(match kind {
            "synchronized" => Self::Synchronized,
            "snapshot" => {
                let snapshot: ShellSnapshot = field(value, "snapshot")?;
                if value.get("resolvedRepositoryIdentityRoots").is_some() {
                    Self::ProjectRefresh(snapshot)
                } else {
                    Self::Snapshot(snapshot)
                }
            }
            "project.updated" => {
                Self::ProjectUpdated { sequence: field(value, "sequence")?, project: field(value, "project")? }
            }
            "project.removed" => {
                Self::ProjectRemoved { sequence: field(value, "sequence")?, project_id: field(value, "projectId")? }
            }
            "thread.updated" => Self::ThreadUpdated {
                sequence: field(value, "sequence")?,
                archived: archived(),
                thread: Box::new(field(value, "thread")?),
            },
            "thread.removed" => Self::ThreadRemoved {
                sequence: field(value, "sequence")?,
                archived: archived(),
                thread_id: field(value, "threadId")?,
            },
            other => Self::Unknown { kind: other.to_owned() },
        })
    }
}

// ----- Threads ---------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub status: String,
}

/// What kind of row a turn item is, with the fields Bukno shows.
#[derive(Clone, Debug, PartialEq)]
pub enum TurnKind {
    UserMessage {
        text: String,
        queued: bool,
    },
    AssistantMessage {
        text: String,
        streaming: bool,
    },
    Reasoning {
        text: String,
        streaming: bool,
    },
    ProposedPlan {
        markdown: String,
    },
    TodoList {
        steps: Vec<(String, String)>,
    },
    CommandExecution {
        input: String,
        exit_code: Option<i64>,
    },
    FileChange {
        file_name: String,
        additions: Option<u64>,
        deletions: Option<u64>,
    },
    FileSearch {
        pattern: Option<String>,
    },
    WebSearch {
        patterns: Vec<String>,
    },
    ApprovalRequest {
        request_kind: String,
        prompt: Option<String>,
    },
    UserInputRequest {
        questions: usize,
    },
    Subagent {
        prompt: String,
        result: Option<String>,
    },
    Notice {
        text: String,
    },
    Error {
        text: String,
    },
    /// A known type Bukno does not show in detail, such as a checkpoint.
    Quiet,
    /// A type this build does not know. Shown as "Unsupported item".
    Unknown,
}

/// One timeline row. `type_name` is kept for every row so unknown and quiet
/// types can still be named in the transcript and in logs.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnItem {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub ordinal: u64,
    pub status: String,
    pub title: Option<String>,
    pub type_name: String,
    pub kind: TurnKind,
}

/// The turn item types in the pinned contract.
pub const KNOWN_TURN_ITEM_TYPES: &[&str] = &[
    "notification",
    "user_message",
    "assistant_message",
    "reasoning",
    "proposed_plan",
    "todo_list",
    "user_input_request",
    "file_change",
    "command_execution",
    "file_search",
    "web_search",
    "approval_request",
    "checkpoint",
    "run_interrupt_request",
    "run_interrupt_result",
    "system_notice",
    "error",
    "compaction",
    "handoff",
    "fork",
    "thread_created",
    "subagent",
    "dynamic_tool",
];

fn text(value: &Value, name: &str) -> String {
    value.get(name).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn opt_text(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_owned)
}

fn flag(value: &Value, name: &str) -> bool {
    value.get(name).and_then(Value::as_bool).unwrap_or(false)
}

impl TurnItem {
    pub fn decode(value: &Value) -> Result<Self, String> {
        let type_name = value.get("type").and_then(Value::as_str).ok_or("turn item without type")?.to_owned();
        let id: String = field(value, "id")?;
        let thread_id: String = field(value, "threadId")?;
        let ordinal: u64 = field(value, "ordinal")?;
        let kind = match type_name.as_str() {
            "user_message" => TurnKind::UserMessage {
                text: text(value, "text"),
                queued: value.get("inputIntent").and_then(Value::as_str) == Some("queued_turn"),
            },
            "assistant_message" => {
                TurnKind::AssistantMessage { text: text(value, "text"), streaming: flag(value, "streaming") }
            }
            "reasoning" => TurnKind::Reasoning { text: text(value, "text"), streaming: flag(value, "streaming") },
            "proposed_plan" => TurnKind::ProposedPlan { markdown: text(value, "markdown") },
            "todo_list" => TurnKind::TodoList {
                steps: value
                    .get("steps")
                    .and_then(Value::as_array)
                    .map(|steps| steps.iter().map(|s| (text(s, "text"), text(s, "status"))).collect())
                    .unwrap_or_default(),
            },
            "command_execution" => TurnKind::CommandExecution {
                input: text(value, "input"),
                exit_code: value.get("exitCode").and_then(Value::as_i64),
            },
            "file_change" => TurnKind::FileChange {
                file_name: text(value, "fileName"),
                additions: value.get("additions").and_then(Value::as_u64),
                deletions: value.get("deletions").and_then(Value::as_u64),
            },
            "file_search" => TurnKind::FileSearch { pattern: opt_text(value, "pattern") },
            "web_search" => TurnKind::WebSearch {
                patterns: value
                    .get("patterns")
                    .and_then(Value::as_array)
                    .map(|p| p.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_default(),
            },
            "approval_request" => TurnKind::ApprovalRequest {
                request_kind: text(value, "requestKind"),
                prompt: opt_text(value, "prompt"),
            },
            "user_input_request" => TurnKind::UserInputRequest {
                questions: value.get("questions").and_then(Value::as_array).map_or(0, Vec::len),
            },
            "subagent" => TurnKind::Subagent { prompt: text(value, "prompt"), result: opt_text(value, "result") },
            "system_notice" | "run_interrupt_request" | "run_interrupt_result" => {
                TurnKind::Notice { text: text(value, "message") }
            }
            "error" => TurnKind::Error {
                text: value
                    .get("failure")
                    .and_then(|f| f.get("message").or_else(|| f.get("detail")))
                    .and_then(Value::as_str)
                    .unwrap_or("The provider reported an error.")
                    .to_owned(),
            },
            other if KNOWN_TURN_ITEM_TYPES.contains(&other) => TurnKind::Quiet,
            _ => TurnKind::Unknown,
        };
        Ok(Self {
            id,
            thread_id,
            run_id: opt_text(value, "runId"),
            ordinal,
            status: text(value, "status"),
            title: opt_text(value, "title"),
            type_name,
            kind,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedItem {
    pub visibility: String,
    pub source_thread_id: String,
    pub source_item_id: String,
    pub item: TurnItem,
}

impl ProjectedItem {
    pub fn decode(value: &Value) -> Result<Self, String> {
        Ok(Self {
            visibility: field(value, "visibility")?,
            source_thread_id: field(value, "sourceThreadId")?,
            source_item_id: field(value, "sourceItemId")?,
            item: TurnItem::decode(value.get("item").ok_or("projected item without item")?)?,
        })
    }

    pub fn is_local(&self) -> bool {
        self.visibility == "local"
    }

    /// Identity used to merge history pages, as in T3's `projectedItemKey`.
    pub fn key(&self) -> (String, String) {
        (self.source_thread_id.clone(), self.source_item_id.clone())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadSnapshot {
    pub snapshot_sequence: u64,
    pub title: String,
    pub runs: Vec<Run>,
    pub items: Vec<ProjectedItem>,
    pub history_cursor: Option<String>,
    pub has_more_history: bool,
    pub latest_local_turn_ordinal: Option<u64>,
}

/// The domain event types in the pinned contract.
pub const KNOWN_EVENT_TYPES: &[&str] = &[
    "thread.created",
    "thread.archived",
    "thread.unarchived",
    "thread.deleted",
    "thread.settled",
    "thread.unsettled",
    "thread.snoozed",
    "thread.unsnoozed",
    "thread.pinned",
    "thread.auto-settle-set",
    "thread.unpinned",
    "thread.pin-reordered",
    "thread.active-reordered",
    "thread.visited",
    "thread.marked-unread",
    "thread.metadata-updated",
    "thread.pull-request-synced",
    "thread.runtime-mode-updated",
    "thread.interaction-mode-updated",
    "thread.model-selection-updated",
    "thread.provider-switched",
    "run.created",
    "run.updated",
    "run.background-work-cancelled",
    "run-attempt.created",
    "run-attempt.updated",
    "node.updated",
    "subagent.updated",
    "provider-session.attached",
    "provider-session.updated",
    "provider-session.detached",
    "provider-thread.updated",
    "provider-turn.updated",
    "runtime-request.updated",
    "message.updated",
    "turn-item.updated",
    "plan.updated",
    "checkpoint-scope.created",
    "checkpoint.captured",
    "checkpoint.rollback-requested",
    "context-handoff.updated",
    "context-transfer.created",
    "context-transfer.updated",
];

#[derive(Clone, Debug, PartialEq)]
pub enum ThreadEvent {
    TurnItem(TurnItem),
    Run(Run),
    /// Thread metadata changed; the title is in the payload.
    Thread {
        title: Option<String>,
        deleted: bool,
    },
    /// A known event that does not change what Bukno shows.
    Other {
        event_type: String,
    },
}

/// One item of `orchestration.subscribeThread`.
#[derive(Clone, Debug, PartialEq)]
pub enum ThreadItem {
    Synchronized,
    Snapshot(Box<ThreadSnapshot>),
    Event {
        sequence: u64,
        thread_id: String,
        event: Box<ThreadEvent>,
    },
    /// An event type this build does not know. The cursor still moves past it.
    UnknownEvent {
        sequence: u64,
        event_type: String,
    },
    Unknown {
        kind: String,
    },
}

pub fn decode_snapshot(value: &Value, projection: &Value) -> Result<ThreadSnapshot, String> {
    let items = projection
        .get("visibleTurnItems")
        .and_then(Value::as_array)
        .ok_or("projection without visibleTurnItems")?
        .iter()
        .map(ProjectedItem::decode)
        .collect::<Result<Vec<_>, _>>()?;
    let runs = match projection.get("runs") {
        Some(runs) => Vec::<Run>::deserialize(runs).map_err(|e| format!("runs: {e}"))?,
        None => Vec::new(),
    };
    Ok(ThreadSnapshot {
        snapshot_sequence: field(value, "snapshotSequence")?,
        title: projection.get("thread").map(|t| text(t, "title")).unwrap_or_default(),
        runs,
        items,
        history_cursor: opt_text(value, "historyCursor"),
        has_more_history: flag(value, "hasMoreHistory"),
        latest_local_turn_ordinal: value.get("latestLocalTurnOrdinal").and_then(Value::as_u64),
    })
}

impl ThreadItem {
    pub fn decode(value: &Value) -> Result<Self, String> {
        let kind = value.get("kind").and_then(Value::as_str).ok_or("thread item without kind")?;
        Ok(match kind {
            "synchronized" => Self::Synchronized,
            "snapshot" => {
                let projection = value.get("projection").ok_or("snapshot without projection")?;
                Self::Snapshot(Box::new(decode_snapshot(value, projection)?))
            }
            "event" => {
                let sequence: u64 = field(value, "sequence")?;
                let event = value.get("event").ok_or("event item without event")?;
                let event_type = event.get("type").and_then(Value::as_str).ok_or("event without type")?;
                if !KNOWN_EVENT_TYPES.contains(&event_type) {
                    return Ok(Self::UnknownEvent { sequence, event_type: event_type.to_owned() });
                }
                let thread_id: String = field(event, "threadId")?;
                let payload = event.get("payload").ok_or("event without payload")?;
                let event = match event_type {
                    "turn-item.updated" => ThreadEvent::TurnItem(TurnItem::decode(payload)?),
                    "run.created" | "run.updated" => {
                        ThreadEvent::Run(Run::deserialize(payload).map_err(|e| format!("run: {e}"))?)
                    }
                    t if t.starts_with("thread.") => {
                        ThreadEvent::Thread { title: opt_text(payload, "title"), deleted: t == "thread.deleted" }
                    }
                    other => ThreadEvent::Other { event_type: other.to_owned() },
                };
                Self::Event { sequence, thread_id, event: Box::new(event) }
            }
            other => Self::Unknown { kind: other.to_owned() },
        })
    }
}

/// `GET /api/orchestration/threads/:id/history`. Rows are chronological.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryPage {
    pub items: Vec<ProjectedItem>,
    pub next_cursor: Option<String>,
    pub has_more_history: bool,
}

impl HistoryPage {
    pub fn decode(value: &Value) -> Result<Self, String> {
        Ok(Self {
            items: value
                .get("items")
                .and_then(Value::as_array)
                .ok_or("history page without items")?
                .iter()
                .map(ProjectedItem::decode)
                .collect::<Result<_, _>>()?,
            next_cursor: opt_text(value, "nextCursor"),
            has_more_history: flag(value, "hasMoreHistory"),
        })
    }
}

// ----- Server config ---------------------------------------------------------

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub is_default: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub instance_id: String,
    pub driver: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub enabled: bool,
    pub installed: bool,
    #[serde(default)]
    pub version: Option<String>,
    /// `ready`, `warning`, `error` or `disabled` in the pinned contract.
    #[serde(default)]
    pub status: Value,
    #[serde(default)]
    pub models: Vec<ProviderModel>,
}

impl Provider {
    pub fn name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.instance_id)
    }
}

/// The parts of `server.getConfig` Bukno reads: which environment answered
/// and which providers and models it offers.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerConfig {
    pub environment: Descriptor,
    pub providers: Vec<Provider>,
}

impl ServerConfig {
    pub fn decode(value: &Value) -> Result<Self, String> {
        let providers = match value.get("providers") {
            // Decode each provider on its own so one odd entry does not hide the rest.
            Some(Value::Array(list)) => list.iter().filter_map(|p| Provider::deserialize(p).ok()).collect(),
            _ => Vec::new(),
        };
        Ok(Self { environment: field(value, "environment")?, providers })
    }
}
