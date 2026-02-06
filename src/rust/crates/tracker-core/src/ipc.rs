//! IPC Protocol for Agent Tracker
//!
//! Defines the Envelope type used for communication between
//! tracker-client and tracker-server over Unix socket.

use serde::{Deserialize, Serialize};

use crate::types::{Conversation, Goal, HistoryRecord, Note, Task};

/// Message envelope for IPC communication
///
/// This matches the Go version in internal/ipc/envelope.go
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Envelope {
    /// Message type: "command" | "ui-register" | "state" | "ack"
    pub kind: String,

    /// Command name for "command" kind
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,

    /// Tmux client tty
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client: String,

    /// Tmux session name
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session: String,

    /// Tmux session id
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_id: String,

    /// Tmux window name
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub window: String,

    /// Tmux window id
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub window_id: String,

    /// Tmux pane id
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pane: String,

    /// Note scope: window | session | all
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,

    /// Note ID
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note_id: String,

    /// Goal ID
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal_id: String,

    /// UI position
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub position: String,

    /// UI visibility
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,

    /// Status message
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,

    /// Task/note summary
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,

    /// Task list (for state messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<Task>,

    /// Archived tasks (for state messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archived_tasks: Vec<Task>,

    /// Note list (for state messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,

    /// Archived notes (for state messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archived: Vec<Note>,

    /// Goal list (for state messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub goals: Vec<Goal>,

    /// Task history records (for state messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryRecord>,

    // History fields
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project_path: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prompt: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reply: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transcript_path: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub claude_session_id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub search: String,

    #[serde(default, skip_serializing_if = "is_zero")]
    pub limit: i32,

    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: i32,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversations: Vec<Conversation>,
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

impl Envelope {
    /// Create a new command envelope
    pub fn command(cmd: &str) -> Self {
        Self {
            kind: "command".to_string(),
            command: cmd.to_string(),
            ..Default::default()
        }
    }

    /// Create a new ack envelope
    pub fn ack() -> Self {
        Self {
            kind: "ack".to_string(),
            ..Default::default()
        }
    }

    /// Create a new state envelope
    pub fn state(message: &str) -> Self {
        Self {
            kind: "state".to_string(),
            message: message.to_string(),
            ..Default::default()
        }
    }

    /// Create a UI register envelope
    pub fn ui_register(client: &str) -> Self {
        Self {
            kind: "ui-register".to_string(),
            client: client.to_string(),
            ..Default::default()
        }
    }
}

/// Known command names
pub mod commands {
    pub const START_TASK: &str = "start_task";
    pub const FINISH_TASK: &str = "finish_task";
    pub const PAUSE_TASK: &str = "pause_task";
    pub const ACKNOWLEDGE: &str = "acknowledge";
    pub const DELETE_TASK: &str = "delete_task";
    pub const TASK_ARCHIVE: &str = "task_archive";
    pub const TASK_RESTORE: &str = "task_restore";

    pub const NOTE_ADD: &str = "note_add";
    pub const NOTE_EDIT: &str = "note_edit";
    pub const NOTE_DELETE: &str = "note_delete";
    pub const NOTE_ARCHIVE: &str = "note_archive";
    pub const NOTE_RESTORE: &str = "note_restore";
    pub const NOTE_TOGGLE_COMPLETE: &str = "note_toggle_complete";

    pub const GOAL_ADD: &str = "goal_add";
    pub const GOAL_DELETE: &str = "goal_delete";
    pub const GOAL_TOGGLE_COMPLETE: &str = "goal_toggle_complete";

    pub const TOGGLE: &str = "toggle";
    pub const SHOW: &str = "show";
    pub const HIDE: &str = "hide";
    pub const REFRESH: &str = "refresh";

    pub const HISTORY_START: &str = "history_start";
    pub const HISTORY_END: &str = "history_end";
    pub const HISTORY_QUERY: &str = "history_query";
    pub const HISTORY_GROUPED: &str = "history_grouped";
    pub const HISTORY_STATS: &str = "history_stats";
    pub const HISTORY_GET: &str = "history_get";

    pub const SEARCH: &str = "search";
}
