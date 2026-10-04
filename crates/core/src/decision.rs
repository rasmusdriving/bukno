//! Approvals and questions an engine is waiting on (specification section 11).

use crate::ids::{DecisionId, RunId, TaskId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionKind {
    /// Run a shell command.
    Command { command: String, cwd: Option<String>, reason: Option<String> },
    /// Apply file changes. `files` lists the paths the engine reported.
    FileChange { files: Vec<String>, reason: Option<String> },
    /// One or more questions with optional choices.
    Question { questions: Vec<Question> },
    /// Any other permission, such as reading a file: `what` names it.
    Access { what: String, detail: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    pub header: String,
    pub text: String,
    pub options: Vec<String>,
    /// Free text is accepted besides the options.
    pub other: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionAnswer {
    /// Allow this exact request only.
    Allow,
    Decline,
    /// Answers to a question, keyed by question ID.
    Answers(Vec<(String, Vec<String>)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionState {
    Pending,
    /// Answered; waiting for the engine to confirm it received the answer.
    Sending,
    Resolved,
    /// The run ended or its connection was lost before an answer arrived.
    Expired,
}

/// A decision as the interface shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionView {
    pub id: DecisionId,
    pub task: TaskId,
    pub run: RunId,
    /// The connection the request arrived on. An answer must carry it back.
    pub generation: u64,
    pub kind: DecisionKind,
    pub state: DecisionState,
}
