//! Codex permission presets (specification section 11).
//!
//! Each preset is a Codex sandbox mode plus an approval policy, the controls
//! Codex itself enforces. A preset is listed only after its behavior was
//! seen working with the installed engine; see the Pass 1 evidence.

use bukno_core::task::Writes;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// What enforces it, in the design's words.
    pub protection: &'static str,
    pub writes: Writes,
    /// `sandbox` for `thread/start` and `thread/resume`.
    pub sandbox: &'static str,
    /// `approvalPolicy` for threads and turns.
    pub approval: &'static str,
}

impl Preset {
    /// The `sandboxPolicy` object `turn/start` takes.
    pub fn sandbox_policy(&self) -> Value {
        match self.sandbox {
            "read-only" => json!({"type": "readOnly"}),
            _ => json!({"type": "workspaceWrite"}),
        }
    }
}

/// MCP tools run outside the Codex sandbox, so no preset claims read only.
const MCP_NOTE: &str = "Runs in the Codex sandbox. MCP tools you configured run outside it.";

pub const ASK_BEFORE_COMMANDS: Preset = Preset {
    id: "codex.workspace-write.untrusted",
    label: "Ask before commands",
    description: "Edits files in this folder. Asks before running commands Codex does not know are safe.",
    protection: MCP_NOTE,
    writes: Writes::Freely,
    sandbox: "workspace-write",
    approval: "untrusted",
};

pub const ASK_BEFORE_CHANGES: Preset = Preset {
    id: "codex.read-only.on-request",
    label: "Ask before changes",
    description: "Reads files. Asks before any edit or command that needs more than read access.",
    protection: MCP_NOTE,
    writes: Writes::AfterApproval,
    sandbox: "read-only",
    approval: "on-request",
};

/// Offered presets, most conservative usable first.
pub const PRESETS: &[Preset] = &[ASK_BEFORE_CHANGES, ASK_BEFORE_COMMANDS];

/// The default for a new chat.
pub const DEFAULT: Preset = ASK_BEFORE_COMMANDS;

/// Look up a preset by ID. An unknown ID falls back to the most conservative one.
pub fn find(id: &str) -> Preset {
    PRESETS.iter().copied().find(|p| p.id == id).unwrap_or(ASK_BEFORE_CHANGES)
}
