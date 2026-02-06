//! Core types for Agent Tracker
//!
//! Defines Task, Note, Goal, and other domain types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    InProgress,
    AwaitingInput,
    Completed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::InProgress => "in_progress",
            TaskStatus::AwaitingInput => "awaiting_input",
            TaskStatus::Completed => "completed",
        }
    }
}

/// Note scope
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NoteScope {
    #[default]
    Window,
    Session,
    All,
}

/// A task tracked by the agent tracker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub session_id: String,
    pub session: String,
    pub window_id: String,
    pub window: String,
    #[serde(default)]
    pub pane: String,
    pub status: TaskStatus,
    pub summary: String,
    #[serde(default)]
    pub completion_note: String,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub duration_seconds: f64,
    #[serde(default)]
    pub acknowledged: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub archived_at: Option<DateTime<Utc>>,
    /// Path to Claude transcript JSONL file
    #[serde(default)]
    pub transcript_path: String,
}

impl Task {
    pub fn new(session_id: String, window_id: String, pane: String, summary: String) -> Self {
        Self {
            session_id,
            session: String::new(),
            window_id,
            window: String::new(),
            pane,
            status: TaskStatus::InProgress,
            summary,
            completion_note: String::new(),
            started_at: Some(Utc::now()),
            completed_at: None,
            duration_seconds: 0.0,
            acknowledged: true,
            archived: false,
            archived_at: None,
            transcript_path: String::new(),
        }
    }

    /// Generate a unique key for this task
    pub fn key(&self) -> String {
        format!("{}|{}|{}", self.session_id, self.window_id, self.pane)
    }
}

/// A note attached to a window/session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    #[serde(default)]
    pub scope: NoteScope,
    pub session_id: String,
    pub session: String,
    pub window_id: String,
    pub window: String,
    #[serde(default)]
    pub pane: String,
    pub summary: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub archived_at: Option<DateTime<Utc>>,
}

/// A goal for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub session_id: String,
    pub session: String,
    pub summary: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// A conversation history record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: i64,
    pub project_path: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub user_prompt: String,
    #[serde(default)]
    pub assistant_reply: String,
    #[serde(default)]
    pub transcript_path: String,
}

/// A task history record (archived completed task)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: i64,
    pub session_id: String,
    #[serde(default)]
    pub session: String,
    pub window_id: String,
    #[serde(default)]
    pub window: String,
    #[serde(default)]
    pub pane: String,
    pub summary: String,
    #[serde(default)]
    pub completion_note: String,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub duration_seconds: f64,
    /// Path to the Claude transcript JSONL file for full conversation
    #[serde(default)]
    pub transcript_path: String,
    /// Conversation messages (loaded separately)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<ConversationMessage>,
}

/// A single message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub id: i64,
    pub history_id: i64,
    pub role: String,      // "user" or "assistant"
    pub content: String,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

/// A tool usage record from Claude transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsage {
    pub id: i64,
    pub history_id: i64,
    pub tool_name: String,
    #[serde(default)]
    pub tool_args: String,
    #[serde(default)]
    pub result_summary: String,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
}

/// A git commit record from Claude transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommit {
    pub id: i64,
    pub history_id: i64,
    pub commit_hash: String,
    pub commit_message: String,
    #[serde(default)]
    pub files_changed: i32,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
}

/// History detail with all related data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryDetail {
    pub history: HistoryRecord,
    pub messages: Vec<ConversationMessage>,
    pub tool_usage: Vec<ToolUsage>,
    pub commits: Vec<GitCommit>,
    pub summary: HistorySummary,
}

/// Summary statistics for a history record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySummary {
    pub message_count: i32,
    pub tool_count: i32,
    pub commit_count: i32,
    pub duration_seconds: f64,
    pub tools_used: Vec<String>,
}

/// Tmux target for commands
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TmuxTarget {
    pub session_name: String,
    pub session_id: String,
    pub window_name: String,
    pub window_id: String,
    pub pane_id: String,
}

impl TmuxTarget {
    pub fn is_valid(&self) -> bool {
        !self.session_id.is_empty() && !self.window_id.is_empty()
    }
}
