//! tracker-server: Agent Tracker HTTP/WebSocket server
//!
//! This is the main entry point for the tracker server.
//! It manages state directly via HTTP/WebSocket and persists to SQLite.

mod agent;
mod browser;
mod config;
mod db;
mod env_file;
mod layout;
mod port;
mod stream;
mod transcript;
mod workspace;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path as AxumPath, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use tracker_core::{commands, Envelope, Goal, HistoryRecord, Note, NoteScope, Task, TaskStatus};

use crate::db::Database;

// ============================================================================
// State Management
// ============================================================================

/// Server state (in-memory + persisted to SQLite)
struct ServerState {
    tasks: HashMap<String, Task>,
    archived_tasks: HashMap<String, Task>,
    notes: HashMap<String, Note>,
    archived_notes: HashMap<String, Note>,
    goals: HashMap<String, Goal>,
    history: Vec<HistoryRecord>,
    db: Database,
    message: String,
}

impl ServerState {
    fn new(db: Database) -> Self {
        Self {
            tasks: HashMap::new(),
            archived_tasks: HashMap::new(),
            notes: HashMap::new(),
            archived_notes: HashMap::new(),
            goals: HashMap::new(),
            history: Vec::new(),
            db,
            message: "Tracker ready".to_string(),
        }
    }

    /// Load all data from database
    fn load_from_db(&mut self) -> Result<()> {
        // Load tasks (separate active vs archived)
        let tasks = self.db.load_tasks()?;
        for task in tasks {
            let key = task.key();
            if task.archived {
                self.archived_tasks.insert(key, task);
            } else {
                self.tasks.insert(key, task);
            }
        }
        info!("Loaded {} tasks from database", self.tasks.len());

        // Load notes (separate active vs archived)
        let notes = self.db.load_notes()?;
        for note in notes {
            if note.archived {
                self.archived_notes.insert(note.id.clone(), note);
            } else {
                self.notes.insert(note.id.clone(), note);
            }
        }
        info!("Loaded {} notes from database", self.notes.len());

        // Load goals
        let goals = self.db.load_goals()?;
        for goal in goals {
            self.goals.insert(goal.id.clone(), goal);
        }
        info!("Loaded {} goals from database", self.goals.len());

        // Load history (recent 100 completed tasks)
        self.history = self.db.get_history(100)?;
        info!("Loaded {} history records from database", self.history.len());

        Ok(())
    }
}

/// Shared application state
struct AppState {
    state: Mutex<ServerState>,
    broadcast_tx: broadcast::Sender<RealtimeMessage>,
    /// Last known tmux windows (for change detection)
    last_tmux_windows: Mutex<Vec<agent::TmuxWindowInfo>>,
    /// Stream manager for real-time pane output capture
    stream_manager: stream::StreamManager,
}

impl AppState {
    fn new(db: Database) -> Result<Self> {
        let (broadcast_tx, _) = broadcast::channel(16);
        let mut state = ServerState::new(db);
        state.load_from_db()?;
        Ok(Self {
            state: Mutex::new(state),
            broadcast_tx,
            last_tmux_windows: Mutex::new(Vec::new()),
            stream_manager: stream::StreamManager::new(),
        })
    }

    /// Get current state as Envelope for WebSocket broadcast
    fn get_state_response(&self) -> Envelope {
        let state = self.state.lock().unwrap();
        Envelope {
            kind: "state".to_string(),
            tasks: state.tasks.values().cloned().collect(),
            archived_tasks: state.archived_tasks.values().cloned().collect(),
            notes: state.notes.values().cloned().collect(),
            archived: state.archived_notes.values().cloned().collect(),
            goals: state.goals.values().cloned().collect(),
            history: state.history.clone(),
            message: state.message.clone(),
            ..Default::default()
        }
    }

    /// Get current state with tmux names filled in
    fn get_state_response_with_tmux_names(&self) -> Envelope {
        let (session_map, window_map) = agent::TmuxAgent::get_tmux_name_mappings_sync();

        let state = self.state.lock().unwrap();

        // Enrich tasks with tmux names
        let tasks: Vec<Task> = state
            .tasks
            .values()
            .map(|t| {
                let mut task = t.clone();
                if task.session.is_empty() {
                    if let Some(name) = session_map.get(&task.session_id) {
                        task.session = name.clone();
                    }
                }
                if task.window.is_empty() {
                    if let Some(name) = window_map.get(&task.window_id) {
                        task.window = name.clone();
                    }
                }
                task
            })
            .collect();

        let archived_tasks: Vec<Task> = state
            .archived_tasks
            .values()
            .map(|t| {
                let mut task = t.clone();
                if task.session.is_empty() {
                    if let Some(name) = session_map.get(&task.session_id) {
                        task.session = name.clone();
                    }
                }
                if task.window.is_empty() {
                    if let Some(name) = window_map.get(&task.window_id) {
                        task.window = name.clone();
                    }
                }
                task
            })
            .collect();

        // Enrich history with tmux names
        let history: Vec<HistoryRecord> = state
            .history
            .iter()
            .map(|h| {
                let mut record = h.clone();
                if record.session.is_empty() {
                    if let Some(name) = session_map.get(&record.session_id) {
                        record.session = name.clone();
                    }
                }
                if record.window.is_empty() {
                    if let Some(name) = window_map.get(&record.window_id) {
                        record.window = name.clone();
                    }
                }
                record
            })
            .collect();

        Envelope {
            kind: "state".to_string(),
            tasks,
            archived_tasks,
            notes: state.notes.values().cloned().collect(),
            archived: state.archived_notes.values().cloned().collect(),
            goals: state.goals.values().cloned().collect(),
            history,
            message: state.message.clone(),
            ..Default::default()
        }
    }

    /// Broadcast state to all WebSocket subscribers (with tmux names)
    fn broadcast_state(&self) {
        let state = self.get_state_response_with_tmux_names();
        let tmux_windows = agent::TmuxAgent::list_all_windows_sync();

        // Write cache file for tmux status line
        Self::write_cache_file(&state);

        // Refresh tmux status line
        let _ = std::process::Command::new(agent::TMUX_BIN)
            .args(["refresh-client", "-S"])
            .status();

        let msg = RealtimeMessage { state, tmux_windows };
        if let Err(e) = self.broadcast_tx.send(msg) {
            warn!("No WebSocket subscribers: {}", e);
        }
    }

    /// Write state to cache file (atomic write)
    fn write_cache_file(envelope: &Envelope) {
        use std::io::Write;
        let cache_path = "/tmp/tmux-tracker-cache.json";
        let tmp_path = format!("/tmp/tmux-tracker-cache.{}.tmp", std::process::id());

        if let Ok(json) = serde_json::to_string(envelope) {
            if let Ok(mut file) = std::fs::File::create(&tmp_path) {
                let _ = file.write_all(json.as_bytes());
                let _ = file.sync_all();
                drop(file);
                let _ = std::fs::rename(&tmp_path, cache_path);
            }
        }
    }

    /// Subscribe to state updates
    fn subscribe(&self) -> broadcast::Receiver<RealtimeMessage> {
        self.broadcast_tx.subscribe()
    }

    /// Get current realtime message (state + tmux windows)
    fn get_realtime_message(&self) -> RealtimeMessage {
        let state = self.get_state_response_with_tmux_names();
        let tmux_windows = agent::TmuxAgent::list_all_windows_sync();
        RealtimeMessage { state, tmux_windows }
    }

    /// Broadcast if tmux windows changed
    fn broadcast_if_tmux_changed(&self, new_windows: Vec<agent::TmuxWindowInfo>) {
        let mut last = self.last_tmux_windows.lock().unwrap();

        // Simple change detection: compare serialized JSON
        let old_json = serde_json::to_string(&*last).unwrap_or_default();
        let new_json = serde_json::to_string(&new_windows).unwrap_or_default();

        if old_json != new_json {
            let old_windows = std::mem::replace(&mut *last, new_windows.clone());
            drop(last); // Release lock before broadcast

            // Detect disappeared windows (in old but not in new) and save to closed_windows
            self.detect_closed_windows(&old_windows, &new_windows);

            // Detect reappeared windows (in new but not in old) and clean up closed_windows
            self.detect_reopened_windows(&old_windows, &new_windows);

            let state = self.get_state_response_with_tmux_names();
            let msg = RealtimeMessage { state, tmux_windows: new_windows };
            let _ = self.broadcast_tx.send(msg);
        }
    }

    /// Detect windows that disappeared and save them to closed_windows DB
    fn detect_closed_windows(&self, old_windows: &[agent::TmuxWindowInfo], new_windows: &[agent::TmuxWindowInfo]) {
        // Skip if old_windows is empty (first poll, nothing disappeared)
        if old_windows.is_empty() {
            return;
        }

        let new_ids: std::collections::HashSet<&str> = new_windows.iter().map(|w| w.window_id.as_str()).collect();

        for old_win in old_windows {
            if new_ids.contains(old_win.window_id.as_str()) {
                continue;
            }

            // Skip "main" windows (window index 0, typically the first window of each session)
            // These are identified by checking if the window_name is the session's default
            // A more reliable check: skip windows whose name matches their session_name
            // (the default first window usually inherits the session name)
            // Actually, we just skip if window index is 0 by checking if any other window
            // with the same session still exists - if no windows remain for this session,
            // the session was killed entirely, don't record individual windows
            let session_still_exists = new_windows.iter().any(|w| w.session_id == old_win.session_id);
            if !session_still_exists {
                // Entire session was killed, don't record individual closed windows
                continue;
            }

            let working_dir = match &old_win.working_dir {
                Some(dir) if !dir.is_empty() => dir.clone(),
                _ => continue, // No working_dir cached, can't save meaningful record
            };

            // Get git branch from the working directory (directory still exists even though tmux window is gone)
            let git_branch = std::process::Command::new("git")
                .args(["-C", &working_dir, "rev-parse", "--abbrev-ref", "HEAD"])
                .output()
                .ok()
                .and_then(|out| {
                    if out.status.success() {
                        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if branch.is_empty() { None } else { Some(branch) }
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            let pane_count = old_win.pane_count as i32;

            info!(
                "Auto-saving closed window: session={} window={} (id={}) working_dir={} branch={} panes={}",
                old_win.session_name, old_win.window_name, old_win.window_id, working_dir, git_branch, pane_count
            );

            let db = &self.state.lock().unwrap().db;
            if let Err(e) = db.save_closed_window(
                &old_win.session_id,
                &old_win.session_name,
                &old_win.window_name,
                &working_dir,
                &git_branch,
                pane_count,
            ) {
                warn!("Failed to auto-save closed window: {}", e);
            }
        }
    }

    /// Detect windows that reappeared and clean up their closed_windows records
    fn detect_reopened_windows(&self, old_windows: &[agent::TmuxWindowInfo], new_windows: &[agent::TmuxWindowInfo]) {
        let old_ids: std::collections::HashSet<&str> = old_windows.iter().map(|w| w.window_id.as_str()).collect();

        for new_win in new_windows {
            if old_ids.contains(new_win.window_id.as_str()) {
                continue;
            }

            // This is a newly appeared window - clean up any closed_windows record with same name
            let db = &self.state.lock().unwrap().db;
            if let Err(e) = db.delete_closed_window_by_name(&new_win.session_name, &new_win.window_name) {
                warn!("Failed to clean up closed window record for {}: {}", new_win.window_name, e);
            }
        }
    }
}

// ============================================================================
// Response Types
// ============================================================================

/// Health check response
#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}


/// Task list response
#[derive(Serialize)]
struct TasksResponse {
    tasks: Vec<Task>,
}

/// Notes list response
#[derive(Serialize)]
struct NotesResponse {
    notes: Vec<Note>,
}

/// Goals list response
#[derive(Serialize)]
struct GoalsResponse {
    goals: Vec<Goal>,
}

/// Command response
#[derive(Serialize)]
struct CommandResponse {
    success: bool,
    message: String,
}

// ============================================================================
// Request Types
// ============================================================================

/// Send command request
#[derive(Deserialize)]
struct SendCommandRequest {
    command: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    session: String,
    #[serde(default)]
    window_id: String,
    #[serde(default)]
    window: String,
    #[serde(default)]
    pane: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    note_id: String,
    #[serde(default)]
    goal_id: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    transcript_path: String,
}

/// Add note request
#[derive(Deserialize)]
struct AddNoteRequest {
    session_id: String,
    #[serde(default)]
    session: String,
    #[serde(default)]
    window_id: Option<String>,
    #[serde(default)]
    window: String,
    summary: String,
    scope: Option<String>,
}

/// Add goal request
#[derive(Deserialize)]
struct AddGoalRequest {
    session_id: String,
    #[serde(default)]
    session: String,
    summary: String,
}

/// Archive task request
#[derive(Deserialize)]
struct ArchiveTaskRequest {
    session_id: String,
    window_id: String,
    #[serde(default)]
    pane: String,
}

/// Restore task request
#[derive(Deserialize)]
struct RestoreTaskRequest {
    session_id: String,
    window_id: String,
    #[serde(default)]
    pane: String,
}

/// Archive note request
#[derive(Deserialize)]
struct ArchiveNoteRequest {
    note_id: String,
}

/// Restore note request
#[derive(Deserialize)]
struct RestoreNoteRequest {
    note_id: String,
}

// ============================================================================
// History Types
// ============================================================================

/// History query params
#[derive(Deserialize)]
struct HistoryQueryParams {
    #[serde(default)]
    limit: Option<i32>,
    #[serde(default)]
    offset: Option<i32>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    date: Option<String>,
    /// Time range: today, yesterday, 7days, 30days, all
    #[serde(default)]
    range: Option<String>,
    /// Custom start date (ISO 8601)
    #[serde(default)]
    start_date: Option<String>,
    /// Custom end date (ISO 8601)
    #[serde(default)]
    end_date: Option<String>,
    /// Page number (1-indexed)
    #[serde(default)]
    page: Option<i32>,
    /// Items per page
    #[serde(default)]
    per_page: Option<i32>,
}

/// History group
#[derive(Serialize)]
struct HistoryGroup {
    label: String,
    records: Vec<HistoryEntry>,
}

/// History entry
#[derive(Serialize)]
struct HistoryEntry {
    id: i64,
    session: String,
    window: String,
    summary: String,
    completion_note: String,
    duration_seconds: f64,
    started_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    ended_at: String,
    message_count: i32,
}

/// History response (grouped)
#[derive(Serialize)]
struct HistoryResponse {
    groups: Vec<HistoryGroup>,
    total: i32,
}

/// History stats response
#[derive(Serialize)]
struct HistoryStatsResponse {
    total_tasks: i32,
    total_duration_hours: f64,
    today: PeriodStats,
    this_week: PeriodStats,
    this_month: PeriodStats,
    by_session: Vec<SessionStats>,
}

#[derive(Serialize)]
struct PeriodStats {
    count: i32,
    duration_hours: f64,
}

#[derive(Serialize)]
struct SessionStats {
    session: String,
    count: i32,
}

/// Conversation message for API response
#[derive(Serialize)]
struct ConversationMessageResponse {
    role: String,
    content: String,
    created_at: String,
}

/// History detail response
#[derive(Serialize)]
struct HistoryDetailResponse {
    id: i64,
    session: String,
    window: String,
    summary: String,
    completion_note: String,
    started_at: String,
    ended_at: String,
    transcript_path: String,
    resume_command: String,
    messages: Vec<ConversationMessageResponse>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_usage: Vec<ToolUsageResponse>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    commits: Vec<CommitResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<HistoryDetailStats>,
}

/// Tool usage for API response
#[derive(Serialize)]
struct ToolUsageResponse {
    id: i64,
    tool_name: String,
    tool_args: String,
    result_summary: String,
    success: bool,
    timestamp: String,
}

/// Git commit for API response
#[derive(Serialize)]
struct CommitResponse {
    id: i64,
    commit_hash: String,
    commit_message: String,
    files_changed: i32,
    timestamp: String,
}

/// Statistics for a history entry
#[derive(Serialize)]
struct HistoryDetailStats {
    message_count: i32,
    tool_count: i32,
    commit_count: i32,
    duration_seconds: f64,
    tools_used: Vec<String>,
}

/// Resume response
#[derive(Serialize)]
struct ResumeResponse {
    success: bool,
    command: String,
    message: String,
}

// ============================================================================
// Workspace Types
// ============================================================================

/// Start workspace request
#[derive(Deserialize)]
struct StartWorkspaceRequest {
    git_dir: String,
    branch: String,
    /// Base branch to create new branch from (if branch doesn't exist)
    #[serde(default)]
    base_branch: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    layout: Option<String>,
    /// Enable fullstack mode (frontend + backend)
    #[serde(default)]
    fullstack_mode: Option<bool>,
    /// Base port for port allocation
    #[serde(default)]
    port_base: Option<u16>,
    /// Frontend port base (fullstack mode)
    #[serde(default)]
    frontend_port_base: Option<u16>,
    /// Backend port base (fullstack mode)
    #[serde(default)]
    backend_port_base: Option<u16>,
    /// Frontend start command (supports $PORT)
    #[serde(default)]
    frontend_cmd: Option<String>,
    /// Backend start command (supports $PORT)
    #[serde(default)]
    backend_cmd: Option<String>,
    /// Dev server command (single service mode, supports $PORT)
    #[serde(default)]
    dev_server_cmd: Option<String>,
    /// Auto-open browser after starting
    #[serde(default)]
    auto_open_browser: Option<bool>,
    /// Browser type (chrome, safari, arc)
    #[serde(default)]
    browser: Option<String>,
    /// Browser URL template (supports $PORT, $FRONTEND_PORT, $BACKEND_PORT)
    #[serde(default)]
    browser_url: Option<String>,
    /// Frontend directory (relative to worktree)
    #[serde(default)]
    frontend_dir: Option<String>,
    /// Backend directory (relative to worktree)
    #[serde(default)]
    backend_dir: Option<String>,
}

/// Start workspace response
#[derive(Serialize)]
struct StartWorkspaceResponse {
    success: bool,
    session_name: String,
    worktree_path: String,
    message: String,
    /// Allocated port (single service mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    /// Allocated frontend port (fullstack mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    frontend_port: Option<u16>,
    /// Allocated backend port (fullstack mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    backend_port: Option<u16>,
    /// Browser URL that was opened
    #[serde(skip_serializing_if = "Option::is_none")]
    browser_url: Option<String>,
}

/// Resume workspace request
#[derive(Deserialize)]
struct ResumeWorkspaceRequest {
    git_dir: String,
    branch: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    layout: Option<String>,
}

/// Destroy workspace request
#[derive(Deserialize)]
struct DestroyWorkspaceRequest {
    git_dir: String,
    branch: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    force: bool,
    /// Kill processes on allocated ports
    #[serde(default)]
    kill_ports: Option<bool>,
    /// Delete the git branch after removing worktree
    #[serde(default)]
    delete_branch: Option<bool>,
}

/// Workspace list response
#[derive(Serialize)]
struct WorkspaceListResponse {
    workspaces: Vec<agent::AgentSession>,
}

/// Config response
#[derive(Serialize)]
struct ConfigResponse {
    workspaces: std::collections::HashMap<String, config::WorkspaceConfig>,
    agents: std::collections::HashMap<String, config::AgentDef>,
    layouts: std::collections::HashMap<String, config::LayoutConfig>,
    defaults: config::Defaults,
}

// ============================================================================
// Port Management Types
// ============================================================================

/// Check port status response
#[derive(Serialize)]
struct PortStatusResponse {
    port: u16,
    in_use: bool,
}

/// Kill port request
#[derive(Deserialize)]
struct KillPortRequest {
    port: u16,
}

/// Kill port response
#[derive(Serialize)]
struct KillPortResponse {
    success: bool,
    port: u16,
    killed: bool,
    message: String,
}

/// Allocate port response
#[derive(Serialize)]
struct AllocatePortResponse {
    success: bool,
    port: u16,
    message: String,
}

// ============================================================================
// Browser Types
// ============================================================================

/// Open browser request
#[derive(Deserialize)]
struct OpenBrowserRequest {
    browser: String,
    url: String,
}

/// Switch browser tab request
#[derive(Deserialize)]
struct SwitchBrowserTabRequest {
    browser: String,
    port: u16,
}

// ============================================================================
// Workspace Activate Types
// ============================================================================

/// Activate workspace request (window focus hook)
#[derive(Deserialize)]
struct ActivateWorkspaceRequest {
    session: String,
    window: String,
}

/// Activate workspace response
#[derive(Serialize)]
struct ActivateWorkspaceResponse {
    success: bool,
    message: String,
    refreshed_lazygit: bool,
    switched_browser_tab: bool,
}

/// Workspace metadata response
#[derive(Serialize)]
struct WorkspaceMetadataResponse {
    session: String,
    window: String,
    port: Option<u16>,
    frontend_port: Option<u16>,
    backend_port: Option<u16>,
    dir: Option<String>,
    main_repo: Option<String>,
    browser: Option<String>,
    name: Option<String>,
    fullstack: bool,
}

// ============================================================================
// tmux Types
// ============================================================================

/// Send keys to tmux pane request
#[derive(Deserialize)]
struct TmuxSendKeysRequest {
    session: String,
    window: String,
    pane: String,
    keys: String,
    /// Suffix key to send after the text (e.g., "Enter", "C-m", "C-s")
    /// Empty string means no suffix key
    #[serde(default)]
    suffix_key: Option<String>,
    /// Legacy: if true and suffix_key is None, append "Enter"
    #[serde(default)]
    enter: bool,
}

/// Capture pane query params
#[derive(Deserialize)]
struct TmuxCaptureParams {
    session: String,
    window: String,
    pane: String,
    lines: Option<u32>,
}

/// List panes query params
#[derive(Deserialize)]
struct TmuxListPanesParams {
    session: String,
    window: String,
}

/// Capture pane response
#[derive(Serialize)]
struct TmuxCaptureResponse {
    success: bool,
    content: String,
}

/// List sessions response
#[derive(Serialize)]
struct TmuxSessionsResponse {
    sessions: Vec<agent::SessionInfo>,
}

/// List panes response
#[derive(Serialize)]
struct TmuxPanesResponse {
    panes: Vec<agent::PaneInfo>,
}

/// List all windows response
#[derive(Serialize)]
struct TmuxWindowsResponse {
    windows: Vec<agent::TmuxWindowInfo>,
}

/// WebSocket realtime message (combines state + tmux windows)
#[derive(Serialize, Clone)]
struct RealtimeMessage {
    state: Envelope,
    tmux_windows: Vec<agent::TmuxWindowInfo>,
}

// ============================================================================
// Stream Types
// ============================================================================

/// Start stream request
#[derive(Deserialize)]
struct StartStreamRequest {
    session: String,
    window: String,
    pane: String,
}

/// Start stream response
#[derive(Serialize)]
struct StartStreamResponse {
    success: bool,
    target: String,
    message: String,
}

/// Stop stream request
#[derive(Deserialize)]
struct StopStreamRequest {
    pane: String,
}

/// List streams response
#[derive(Serialize)]
struct ListStreamsResponse {
    streams: Vec<StreamEntry>,
}

#[derive(Serialize)]
struct StreamEntry {
    pane_id: String,
    target: String,
}

// ============================================================================
// Command Handling
// ============================================================================

/// Handle a command and update state
async fn handle_command(app_state: &AppState, req: SendCommandRequest) -> Result<(), String> {
    match req.command.as_str() {
        // =================================================================
        // Task commands
        // =================================================================
        commands::START_TASK => {
            let key = format!("{}|{}|{}", req.session_id, req.window_id, req.pane);
            let mut state = app_state.state.lock().unwrap();

            if let Some(task) = state.tasks.get_mut(&key) {
                // Always update session/window names if provided
                if !req.session.is_empty() {
                    task.session = req.session.clone();
                }
                if !req.window.is_empty() {
                    task.window = req.window.clone();
                }

                let needs_update = match task.status {
                    TaskStatus::AwaitingInput => {
                        task.status = TaskStatus::InProgress;
                        true
                    }
                    TaskStatus::Completed => {
                        task.status = TaskStatus::InProgress;
                        task.started_at = Some(Utc::now());
                        task.acknowledged = true;
                        if !req.summary.is_empty() {
                            task.summary = req.summary.clone();
                        }
                        true
                    }
                    TaskStatus::InProgress => {
                        if !req.summary.is_empty() {
                            task.summary = req.summary.clone();
                            true
                        } else {
                            // Still need to save if names were updated
                            !req.session.is_empty() || !req.window.is_empty()
                        }
                    }
                };

                if needs_update {
                    let task_clone = task.clone();
                    if let Err(e) = state.db.save_task(&task_clone) {
                        error!("Failed to save task to database: {}", e);
                    }
                }
            } else {
                let mut task = Task::new(
                    req.session_id.clone(),
                    req.window_id.clone(),
                    req.pane.clone(),
                    req.summary.clone(),
                );
                task.session = req.session.clone();
                task.window = req.window.clone();
                if let Err(e) = state.db.save_task(&task) {
                    error!("Failed to save task to database: {}", e);
                }
                state.tasks.insert(key, task);
            }
            drop(state);
            app_state.broadcast_state();
            // Window status icons are now handled by tmux status bar scripts
        }

        commands::FINISH_TASK => {
            let key = format!("{}|{}|{}", req.session_id, req.window_id, req.pane);
            let mut state = app_state.state.lock().unwrap();

            if let Some(task) = state.tasks.get_mut(&key) {
                let was_already_completed = task.status == TaskStatus::Completed;

                task.status = TaskStatus::Completed;
                task.completed_at = Some(Utc::now());
                task.acknowledged = false;
                if !req.summary.is_empty() {
                    task.completion_note = req.summary.clone();
                }
                if !req.transcript_path.is_empty() {
                    task.transcript_path = req.transcript_path.clone();
                }
                if let Some(started) = task.started_at {
                    task.duration_seconds = (Utc::now() - started).num_seconds() as f64;
                }

                let task_clone = task.clone();
                if let Err(e) = state.db.save_task(&task_clone) {
                    error!("Failed to save task to database: {}", e);
                }

                if !was_already_completed {
                    if let Err(e) = state.db.archive_to_history(&task_clone) {
                        error!("Failed to archive task to history: {}", e);
                    } else {
                        let history_id = state.db.last_insert_rowid();

                        if !task_clone.transcript_path.is_empty() {
                            let transcript_path = task_clone.transcript_path.clone();
                            let started_at = task_clone.started_at;
                            let completed_at = task_clone.completed_at;

                            tokio::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                                let path = std::path::Path::new(&transcript_path);
                                if !path.exists() {
                                    return;
                                }

                                match transcript::parse_transcript_full_for_task(
                                    path,
                                    started_at,
                                    completed_at,
                                ) {
                                    Ok(result) => {
                                        info!(
                                            "Parsed {} messages, {} tools, {} commits from transcript",
                                            result.messages.len(),
                                            result.tool_usages.len(),
                                            result.commits.len()
                                        );
                                        let db_path = db::default_db_path();
                                        if let Ok(db) = Database::open(&db_path) {
                                            // Save conversation messages
                                            if let Err(e) =
                                                db.save_conversation_messages(history_id, &result.messages)
                                            {
                                                error!(
                                                    "Failed to save conversation messages: {}",
                                                    e
                                                );
                                            }
                                            // Save tool usage
                                            if let Err(e) =
                                                db.save_tool_usage(history_id, &result.tool_usages)
                                            {
                                                error!(
                                                    "Failed to save tool usage: {}",
                                                    e
                                                );
                                            }
                                            // Save commits
                                            if let Err(e) =
                                                db.save_commits(history_id, &result.commits)
                                            {
                                                error!(
                                                    "Failed to save commits: {}",
                                                    e
                                                );
                                            }
                                        } else {
                                            error!("Failed to open database for saving messages");
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to parse transcript: {}", e);
                                    }
                                }
                            });
                        }
                    }
                }
            }
            drop(state);
            app_state.broadcast_state();
            // Window status icons are now handled by tmux status bar scripts
        }

        commands::PAUSE_TASK => {
            let key = format!("{}|{}|{}", req.session_id, req.window_id, req.pane);
            let mut state = app_state.state.lock().unwrap();

            if let Some(task) = state.tasks.get_mut(&key) {
                task.status = TaskStatus::AwaitingInput;
                if !req.summary.is_empty() {
                    task.summary = req.summary.clone();
                }

                let task_clone = task.clone();
                if let Err(e) = state.db.save_task(&task_clone) {
                    error!("Failed to save task to database: {}", e);
                }
            }
            drop(state);
            app_state.broadcast_state();
            // Window status icons are now handled by tmux status bar scripts
        }

        commands::DELETE_TASK => {
            let key = format!("{}|{}|{}", req.session_id, req.window_id, req.pane);
            let mut state = app_state.state.lock().unwrap();

            if let Err(e) = state.db.delete_task(&key) {
                error!("Failed to delete task from database: {}", e);
            }
            state.tasks.remove(&key);
            drop(state);
            app_state.broadcast_state();
        }

        commands::TASK_ARCHIVE => {
            let key = format!("{}|{}|{}", req.session_id, req.window_id, req.pane);
            let mut state = app_state.state.lock().unwrap();

            if let Err(e) = state.db.delete_task(&key) {
                error!("Failed to delete task from database: {}", e);
            }
            state.tasks.remove(&key);
            drop(state);
            app_state.broadcast_state();
        }

        commands::TASK_RESTORE => {
            // Task restore is a no-op for now (tasks are already in history)
            app_state.broadcast_state();
        }

        // =================================================================
        // Note commands
        // =================================================================
        commands::NOTE_ADD => {
            let scope = match req.scope.as_str() {
                "session" => NoteScope::Session,
                "all" => NoteScope::All,
                _ => NoteScope::Window,
            };

            let note = Note {
                id: Uuid::new_v4().to_string(),
                scope,
                session_id: req.session_id.clone(),
                session: req.session.clone(),
                window_id: req.window_id.clone(),
                window: req.window.clone(),
                pane: req.pane.clone(),
                summary: req.summary.clone(),
                completed: false,
                archived: false,
                created_at: Some(Utc::now()),
                archived_at: None,
            };

            let mut state = app_state.state.lock().unwrap();
            if let Err(e) = state.db.save_note(&note) {
                error!("Failed to save note to database: {}", e);
            }
            state.notes.insert(note.id.clone(), note);
            drop(state);
            app_state.broadcast_state();
        }

        commands::NOTE_EDIT => {
            let mut state = app_state.state.lock().unwrap();

            if let Some(note) = state.notes.get_mut(&req.note_id) {
                if !req.summary.is_empty() {
                    note.summary = req.summary.clone();
                }

                let note_clone = note.clone();
                if let Err(e) = state.db.save_note(&note_clone) {
                    error!("Failed to save note to database: {}", e);
                }
            }
            drop(state);
            app_state.broadcast_state();
        }

        commands::NOTE_DELETE => {
            let mut state = app_state.state.lock().unwrap();

            if let Err(e) = state.db.delete_note(&req.note_id) {
                error!("Failed to delete note from database: {}", e);
            }
            state.notes.remove(&req.note_id);
            drop(state);
            app_state.broadcast_state();
        }

        commands::NOTE_ARCHIVE => {
            let mut state = app_state.state.lock().unwrap();

            if let Some(note) = state.notes.get_mut(&req.note_id) {
                note.archived = true;
                note.archived_at = Some(Utc::now());

                let note_clone = note.clone();
                if let Err(e) = state.db.save_note(&note_clone) {
                    error!("Failed to save note to database: {}", e);
                }
            }
            state.notes.remove(&req.note_id);
            drop(state);
            app_state.broadcast_state();
        }

        commands::NOTE_RESTORE => {
            // Load the archived note from database and restore it
            let state = app_state.state.lock().unwrap();
            if let Ok(archived_notes) = state.db.load_archived_notes() {
                drop(state);

                let mut state = app_state.state.lock().unwrap();
                if let Some(mut note) = archived_notes
                    .into_iter()
                    .find(|n| n.id == req.note_id)
                {
                    note.archived = false;
                    note.archived_at = None;

                    if let Err(e) = state.db.save_note(&note) {
                        error!("Failed to save note to database: {}", e);
                    }
                    state.notes.insert(note.id.clone(), note);
                }
                drop(state);
                app_state.broadcast_state();
            }
        }

        commands::NOTE_TOGGLE_COMPLETE => {
            let mut state = app_state.state.lock().unwrap();

            if let Some(note) = state.notes.get_mut(&req.note_id) {
                note.completed = !note.completed;

                let note_clone = note.clone();
                if let Err(e) = state.db.save_note(&note_clone) {
                    error!("Failed to save note to database: {}", e);
                }
            }
            drop(state);
            app_state.broadcast_state();
        }

        // =================================================================
        // Goal commands
        // =================================================================
        commands::GOAL_ADD => {
            let goal = Goal {
                id: Uuid::new_v4().to_string(),
                session_id: req.session_id.clone(),
                session: req.session.clone(),
                summary: req.summary.clone(),
                completed: false,
                created_at: Some(Utc::now()),
                updated_at: Some(Utc::now()),
            };

            let mut state = app_state.state.lock().unwrap();
            if let Err(e) = state.db.save_goal(&goal) {
                error!("Failed to save goal to database: {}", e);
            }
            state.goals.insert(goal.id.clone(), goal);
            drop(state);
            app_state.broadcast_state();
        }

        commands::GOAL_DELETE => {
            let mut state = app_state.state.lock().unwrap();

            if let Err(e) = state.db.delete_goal(&req.goal_id) {
                error!("Failed to delete goal from database: {}", e);
            }
            state.goals.remove(&req.goal_id);
            drop(state);
            app_state.broadcast_state();
        }

        commands::GOAL_TOGGLE_COMPLETE => {
            let mut state = app_state.state.lock().unwrap();

            if let Some(goal) = state.goals.get_mut(&req.goal_id) {
                goal.completed = !goal.completed;
                goal.updated_at = Some(Utc::now());

                let goal_clone = goal.clone();
                if let Err(e) = state.db.save_goal(&goal_clone) {
                    error!("Failed to save goal to database: {}", e);
                }
            }
            drop(state);
            app_state.broadcast_state();
        }

        // =================================================================
        // Acknowledge command
        // =================================================================
        commands::ACKNOWLEDGE => {
            let mut state = app_state.state.lock().unwrap();
            let mut tasks_to_save = Vec::new();

            for task in state.tasks.values_mut() {
                if task.window_id == req.window_id && !task.acknowledged {
                    task.acknowledged = true;
                    tasks_to_save.push(task.clone());
                }
            }

            for task in &tasks_to_save {
                if let Err(e) = state.db.save_task(task) {
                    error!("Failed to save task to database: {}", e);
                }
            }

            let updated = !tasks_to_save.is_empty();
            drop(state);

            if updated {
                app_state.broadcast_state();
                // Window status icons are now handled by tmux status bar scripts
            }
        }

        // =================================================================
        // UI commands (just broadcast current state)
        // =================================================================
        commands::TOGGLE | commands::SHOW | commands::HIDE | commands::REFRESH => {
            app_state.broadcast_state();
        }

        // =================================================================
        // Search command
        // =================================================================
        commands::SEARCH => {
            // Search is handled by the client (filter locally)
            app_state.broadcast_state();
        }

        _ => {
            warn!("Unknown command: {}", req.command);
        }
    }
    Ok(())
}

// ============================================================================
// HTTP Handlers
// ============================================================================

/// Health check endpoint
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Get full state
async fn get_state(State(state): State<Arc<AppState>>) -> Json<Envelope> {
    Json(state.get_state_response_with_tmux_names())
}

/// Get all tasks
async fn get_tasks(State(state): State<Arc<AppState>>) -> Json<TasksResponse> {
    let server_state = state.state.lock().unwrap();
    Json(TasksResponse {
        tasks: server_state.tasks.values().cloned().collect(),
    })
}

/// Get all notes
async fn get_notes(State(state): State<Arc<AppState>>) -> Json<NotesResponse> {
    let server_state = state.state.lock().unwrap();
    Json(NotesResponse {
        notes: server_state.notes.values().cloned().collect(),
    })
}

/// Get all goals
async fn get_goals(State(state): State<Arc<AppState>>) -> Json<GoalsResponse> {
    let server_state = state.state.lock().unwrap();
    Json(GoalsResponse {
        goals: server_state.goals.values().cloned().collect(),
    })
}

/// Send a command
async fn send_command(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendCommandRequest>,
) -> Result<Json<CommandResponse>, StatusCode> {
    let cmd_name = req.command.clone();
    match handle_command(&state, req).await {
        Ok(_) => Ok(Json(CommandResponse {
            success: true,
            message: format!("Command '{}' executed", cmd_name),
        })),
        Err(e) => Ok(Json(CommandResponse {
            success: false,
            message: format!("Failed: {}", e),
        })),
    }
}

/// Add a note
async fn add_note(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddNoteRequest>,
) -> Json<CommandResponse> {
    let cmd_req = SendCommandRequest {
        command: commands::NOTE_ADD.to_string(),
        session_id: req.session_id,
        session: req.session,
        window_id: req.window_id.unwrap_or_default(),
        window: req.window,
        pane: String::new(),
        summary: req.summary,
        note_id: String::new(),
        goal_id: String::new(),
        scope: req.scope.unwrap_or_else(|| "window".to_string()),
        transcript_path: String::new(),
    };

    match handle_command(&state, cmd_req).await {
        Ok(_) => Json(CommandResponse {
            success: true,
            message: "Note added".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed: {}", e),
        }),
    }
}

/// Add a goal
async fn add_goal(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddGoalRequest>,
) -> Json<CommandResponse> {
    let cmd_req = SendCommandRequest {
        command: commands::GOAL_ADD.to_string(),
        session_id: req.session_id,
        session: req.session,
        window_id: String::new(),
        window: String::new(),
        pane: String::new(),
        summary: req.summary,
        note_id: String::new(),
        goal_id: String::new(),
        scope: String::new(),
        transcript_path: String::new(),
    };

    match handle_command(&state, cmd_req).await {
        Ok(_) => Json(CommandResponse {
            success: true,
            message: "Goal added".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed: {}", e),
        }),
    }
}

/// Archive a task
async fn archive_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ArchiveTaskRequest>,
) -> Json<CommandResponse> {
    let cmd_req = SendCommandRequest {
        command: commands::TASK_ARCHIVE.to_string(),
        session_id: req.session_id,
        session: String::new(),
        window_id: req.window_id,
        window: String::new(),
        pane: req.pane,
        summary: String::new(),
        note_id: String::new(),
        goal_id: String::new(),
        scope: String::new(),
        transcript_path: String::new(),
    };

    match handle_command(&state, cmd_req).await {
        Ok(_) => Json(CommandResponse {
            success: true,
            message: "Task archived".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed: {}", e),
        }),
    }
}

/// Restore a task
async fn restore_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RestoreTaskRequest>,
) -> Json<CommandResponse> {
    let cmd_req = SendCommandRequest {
        command: commands::TASK_RESTORE.to_string(),
        session_id: req.session_id,
        session: String::new(),
        window_id: req.window_id,
        window: String::new(),
        pane: req.pane,
        summary: String::new(),
        note_id: String::new(),
        goal_id: String::new(),
        scope: String::new(),
        transcript_path: String::new(),
    };

    match handle_command(&state, cmd_req).await {
        Ok(_) => Json(CommandResponse {
            success: true,
            message: "Task restored".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed: {}", e),
        }),
    }
}

/// Archive a note
async fn archive_note(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ArchiveNoteRequest>,
) -> Json<CommandResponse> {
    let cmd_req = SendCommandRequest {
        command: commands::NOTE_ARCHIVE.to_string(),
        session_id: String::new(),
        session: String::new(),
        window_id: String::new(),
        window: String::new(),
        pane: String::new(),
        summary: String::new(),
        note_id: req.note_id,
        goal_id: String::new(),
        scope: String::new(),
        transcript_path: String::new(),
    };

    match handle_command(&state, cmd_req).await {
        Ok(_) => Json(CommandResponse {
            success: true,
            message: "Note archived".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed: {}", e),
        }),
    }
}

/// Restore a note
async fn restore_note(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RestoreNoteRequest>,
) -> Json<CommandResponse> {
    let cmd_req = SendCommandRequest {
        command: commands::NOTE_RESTORE.to_string(),
        session_id: String::new(),
        session: String::new(),
        window_id: String::new(),
        window: String::new(),
        pane: String::new(),
        summary: String::new(),
        note_id: req.note_id,
        goal_id: String::new(),
        scope: String::new(),
        transcript_path: String::new(),
    };

    match handle_command(&state, cmd_req).await {
        Ok(_) => Json(CommandResponse {
            success: true,
            message: "Note restored".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed: {}", e),
        }),
    }
}

// ============================================================================
// History Handlers
// ============================================================================

/// Get database path
fn get_db_path() -> std::path::PathBuf {
    db::default_db_path()
}

/// Get grouped history
async fn get_history(Query(params): Query<HistoryQueryParams>) -> Json<HistoryResponse> {
    let db_path = get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to open history database: {}", e);
            return Json(HistoryResponse {
                groups: vec![],
                total: 0,
            });
        }
    };

    // Handle pagination: prefer page/per_page over legacy limit/offset
    let (limit, offset) = if let Some(page) = params.page {
        let per_page = params.per_page.unwrap_or(50);
        let offset = (page - 1).max(0) * per_page;
        (per_page, offset)
    } else {
        (params.limit.unwrap_or(100), params.offset.unwrap_or(0))
    };

    let mut sql = String::from(
        "SELECT id, COALESCE(NULLIF(session, ''), session_id) as session,
                COALESCE(NULLIF(window, ''), window_id) as window,
                summary, completion_note,
                duration_seconds, started_at, completed_at, transcript_path
         FROM history WHERE 1=1",
    );

    // Handle time range filter
    let today = chrono::Local::now().date_naive();
    if let Some(ref range) = params.range {
        let date_filter = match range.as_str() {
            "today" => {
                let start = today.format("%Y-%m-%d").to_string();
                format!(" AND DATE(completed_at) = '{}'", start)
            }
            "yesterday" => {
                let start = (today - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
                format!(" AND DATE(completed_at) = '{}'", start)
            }
            "7days" => {
                let start = (today - chrono::Duration::days(7)).format("%Y-%m-%d").to_string();
                format!(" AND DATE(completed_at) >= '{}'", start)
            }
            "30days" => {
                let start = (today - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
                format!(" AND DATE(completed_at) >= '{}'", start)
            }
            "all" | _ => String::new(),
        };
        sql.push_str(&date_filter);
    }

    // Handle custom date range
    if let Some(ref start_date) = params.start_date {
        sql.push_str(&format!(
            " AND completed_at >= '{}'",
            start_date.replace('\'', "''")
        ));
    }
    if let Some(ref end_date) = params.end_date {
        sql.push_str(&format!(
            " AND completed_at <= '{}'",
            end_date.replace('\'', "''")
        ));
    }

    if let Some(ref search) = params.search {
        sql.push_str(&format!(
            " AND (summary LIKE '%{}%' OR completion_note LIKE '%{}%')",
            search.replace('\'', "''"),
            search.replace('\'', "''")
        ));
    }

    if let Some(ref session) = params.session {
        sql.push_str(&format!(
            " AND session_id = '{}'",
            session.replace('\'', "''")
        ));
    }

    sql.push_str(" ORDER BY started_at DESC");
    sql.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to prepare history query: {}", e);
            return Json(HistoryResponse {
                groups: vec![],
                total: 0,
            });
        }
    };

    let entries: Vec<HistoryEntry> = stmt
        .query_map([], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                session: row.get::<_, String>(1).unwrap_or_default(),
                window: row.get::<_, String>(2).unwrap_or_default(),
                summary: row.get::<_, String>(3).unwrap_or_default(),
                completion_note: row.get::<_, String>(4).unwrap_or_default(),
                duration_seconds: row.get::<_, f64>(5).unwrap_or(0.0),
                started_at: row.get::<_, String>(6).unwrap_or_default(),
                ended_at: row.get::<_, String>(7).unwrap_or_default(),
                message_count: 0,
            })
        })
        .ok()
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    // Group by date
    let mut groups: Vec<HistoryGroup> = vec![];
    let today = chrono::Local::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);

    let mut today_entries = vec![];
    let mut yesterday_entries = vec![];
    let mut this_week_entries = vec![];
    let mut older_entries = vec![];

    for entry in entries {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&entry.started_at) {
            let date = dt.date_naive();
            if date == today {
                today_entries.push(entry);
            } else if date == yesterday {
                yesterday_entries.push(entry);
            } else if (today - date).num_days() < 7 {
                this_week_entries.push(entry);
            } else {
                older_entries.push(entry);
            }
        } else {
            older_entries.push(entry);
        }
    }

    if !today_entries.is_empty() {
        groups.push(HistoryGroup {
            label: "Today".to_string(),
            records: today_entries,
        });
    }
    if !yesterday_entries.is_empty() {
        groups.push(HistoryGroup {
            label: "Yesterday".to_string(),
            records: yesterday_entries,
        });
    }
    if !this_week_entries.is_empty() {
        groups.push(HistoryGroup {
            label: "This Week".to_string(),
            records: this_week_entries,
        });
    }
    if !older_entries.is_empty() {
        groups.push(HistoryGroup {
            label: "Older".to_string(),
            records: older_entries,
        });
    }

    let total: i32 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
        .unwrap_or(0);

    Json(HistoryResponse { groups, total })
}

/// Get history statistics
async fn get_history_stats() -> Json<HistoryStatsResponse> {
    let db_path = get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to open history database: {}", e);
            return Json(HistoryStatsResponse {
                total_tasks: 0,
                total_duration_hours: 0.0,
                today: PeriodStats {
                    count: 0,
                    duration_hours: 0.0,
                },
                this_week: PeriodStats {
                    count: 0,
                    duration_hours: 0.0,
                },
                this_month: PeriodStats {
                    count: 0,
                    duration_hours: 0.0,
                },
                by_session: vec![],
            });
        }
    };

    let total_tasks: i32 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
        .unwrap_or(0);

    let total_duration: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_seconds), 0) FROM history",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let today_stats = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(duration_seconds), 0) FROM history WHERE DATE(started_at) = ?",
            [&today],
            |row| Ok((row.get::<_, i32>(0)?, row.get::<_, f64>(1)?)),
        )
        .unwrap_or((0, 0.0));

    let week_ago = (chrono::Local::now() - chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let week_stats = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(duration_seconds), 0) FROM history WHERE DATE(started_at) >= ?",
            [&week_ago],
            |row| Ok((row.get::<_, i32>(0)?, row.get::<_, f64>(1)?)),
        )
        .unwrap_or((0, 0.0));

    let month_ago = (chrono::Local::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let month_stats = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(duration_seconds), 0) FROM history WHERE DATE(started_at) >= ?",
            [&month_ago],
            |row| Ok((row.get::<_, i32>(0)?, row.get::<_, f64>(1)?)),
        )
        .unwrap_or((0, 0.0));

    let mut stmt = conn
        .prepare(
            "SELECT session_id, COUNT(*) FROM history GROUP BY session_id ORDER BY COUNT(*) DESC LIMIT 10",
        )
        .ok();

    let by_session: Vec<SessionStats> = stmt
        .as_mut()
        .map(|s| {
            s.query_map([], |row| {
                Ok(SessionStats {
                    session: row.get::<_, String>(0).unwrap_or_default(),
                    count: row.get(1)?,
                })
            })
            .ok()
            .map(|iter| iter.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
        })
        .unwrap_or_default();

    Json(HistoryStatsResponse {
        total_tasks,
        total_duration_hours: total_duration / 3600.0,
        today: PeriodStats {
            count: today_stats.0,
            duration_hours: today_stats.1 / 3600.0,
        },
        this_week: PeriodStats {
            count: week_stats.0,
            duration_hours: week_stats.1 / 3600.0,
        },
        this_month: PeriodStats {
            count: month_stats.0,
            duration_hours: month_stats.1 / 3600.0,
        },
        by_session,
    })
}

/// Get single history entry with conversation messages, tool usage, and commits
async fn get_history_detail(AxumPath(id): AxumPath<i64>) -> Json<HistoryDetailResponse> {
    let db_path = get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to open history database: {}", e);
            return Json(HistoryDetailResponse {
                id,
                session: String::new(),
                window: String::new(),
                summary: String::new(),
                completion_note: String::new(),
                started_at: String::new(),
                ended_at: String::new(),
                transcript_path: String::new(),
                resume_command: String::new(),
                messages: vec![],
                tool_usage: vec![],
                commits: vec![],
                stats: None,
            });
        }
    };

    // Query history record with session and window names
    let result = conn.query_row(
        "SELECT COALESCE(NULLIF(session, ''), session_id), COALESCE(NULLIF(window, ''), window_id), summary, completion_note, started_at, completed_at, COALESCE(transcript_path, ''), duration_seconds FROM history WHERE id = ?",
        [id],
        |row| {
            Ok((
                row.get::<_, String>(0).unwrap_or_default(),
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, String>(3).unwrap_or_default(),
                row.get::<_, String>(4).unwrap_or_default(),
                row.get::<_, String>(5).unwrap_or_default(),
                row.get::<_, String>(6).unwrap_or_default(),
                row.get::<_, f64>(7).unwrap_or_default(),
            ))
        },
    );

    let (session, window, summary, completion_note, started_at, ended_at, transcript_path, duration_seconds) = match result {
        Ok(data) => data,
        Err(_) => {
            return Json(HistoryDetailResponse {
                id,
                session: String::new(),
                window: String::new(),
                summary: String::new(),
                completion_note: String::new(),
                started_at: String::new(),
                ended_at: String::new(),
                transcript_path: String::new(),
                resume_command: String::new(),
                messages: vec![],
                tool_usage: vec![],
                commits: vec![],
                stats: None,
            });
        }
    };

    // Load conversation messages
    let mut messages: Vec<ConversationMessageResponse> = vec![];
    if let Ok(mut stmt) = conn.prepare(
        "SELECT role, content, COALESCE(created_at, '') FROM conversation_messages WHERE history_id = ? ORDER BY id ASC"
    ) {
        if let Ok(rows) = stmt.query_map([id], |row| {
            Ok(ConversationMessageResponse {
                role: row.get(0).unwrap_or_default(),
                content: row.get(1).unwrap_or_default(),
                created_at: row.get(2).unwrap_or_default(),
            })
        }) {
            for msg in rows.flatten() {
                messages.push(msg);
            }
        }
    }

    // Load tool usage
    let mut tool_usage: Vec<ToolUsageResponse> = vec![];
    let mut tools_used: Vec<String> = vec![];
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, tool_name, COALESCE(tool_args, ''), COALESCE(result_summary, ''), success, COALESCE(timestamp, '') FROM tool_usage WHERE history_id = ? ORDER BY id ASC"
    ) {
        if let Ok(rows) = stmt.query_map([id], |row| {
            Ok(ToolUsageResponse {
                id: row.get(0).unwrap_or_default(),
                tool_name: row.get(1).unwrap_or_default(),
                tool_args: row.get(2).unwrap_or_default(),
                result_summary: row.get(3).unwrap_or_default(),
                success: row.get::<_, i32>(4).unwrap_or(1) != 0,
                timestamp: row.get(5).unwrap_or_default(),
            })
        }) {
            for usage in rows.flatten() {
                if !tools_used.contains(&usage.tool_name) {
                    tools_used.push(usage.tool_name.clone());
                }
                tool_usage.push(usage);
            }
        }
    }

    // Load commits
    let mut commits: Vec<CommitResponse> = vec![];
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, commit_hash, commit_message, files_changed, COALESCE(timestamp, '') FROM commits WHERE history_id = ? ORDER BY id ASC"
    ) {
        if let Ok(rows) = stmt.query_map([id], |row| {
            Ok(CommitResponse {
                id: row.get(0).unwrap_or_default(),
                commit_hash: row.get(1).unwrap_or_default(),
                commit_message: row.get(2).unwrap_or_default(),
                files_changed: row.get(3).unwrap_or_default(),
                timestamp: row.get(4).unwrap_or_default(),
            })
        }) {
            for commit in rows.flatten() {
                commits.push(commit);
            }
        }
    }

    let resume_command = if !transcript_path.is_empty() {
        format!("claude --resume {}", transcript_path)
    } else {
        String::new()
    };

    let stats = Some(HistoryDetailStats {
        message_count: messages.len() as i32,
        tool_count: tool_usage.len() as i32,
        commit_count: commits.len() as i32,
        duration_seconds,
        tools_used,
    });

    Json(HistoryDetailResponse {
        id,
        session,
        window,
        summary,
        completion_note,
        started_at,
        ended_at,
        transcript_path,
        resume_command,
        messages,
        tool_usage,
        commits,
        stats,
    })
}

/// Resume a conversation
async fn resume_history(AxumPath(id): AxumPath<i64>) -> Json<ResumeResponse> {
    Json(ResumeResponse {
        success: true,
        command: format!("claude --resume {}", id),
        message: "Use the command to resume this conversation".to_string(),
    })
}

/// Response for reparse operation
#[derive(Serialize)]
struct ReparseResponse {
    success: bool,
    message: String,
    messages_count: usize,
    tools_count: usize,
    commits_count: usize,
}

/// Reparse transcript for a history entry
async fn reparse_history(AxumPath(id): AxumPath<i64>) -> Json<ReparseResponse> {
    let db_path = get_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return Json(ReparseResponse {
                success: false,
                message: format!("Failed to open database: {}", e),
                messages_count: 0,
                tools_count: 0,
                commits_count: 0,
            });
        }
    };

    // Get transcript path and time range
    let result = conn.query_row(
        "SELECT COALESCE(transcript_path, ''), started_at, completed_at FROM history WHERE id = ?",
        [id],
        |row| {
            Ok((
                row.get::<_, String>(0).unwrap_or_default(),
                row.get::<_, Option<String>>(1).unwrap_or(None),
                row.get::<_, Option<String>>(2).unwrap_or(None),
            ))
        },
    );

    let (transcript_path, started_at_str, completed_at_str) = match result {
        Ok(data) => data,
        Err(e) => {
            return Json(ReparseResponse {
                success: false,
                message: format!("History entry not found: {}", e),
                messages_count: 0,
                tools_count: 0,
                commits_count: 0,
            });
        }
    };

    if transcript_path.is_empty() {
        return Json(ReparseResponse {
            success: false,
            message: "No transcript path for this entry".to_string(),
            messages_count: 0,
            tools_count: 0,
            commits_count: 0,
        });
    }

    let path = std::path::Path::new(&transcript_path);
    if !path.exists() {
        return Json(ReparseResponse {
            success: false,
            message: format!("Transcript file not found: {}", transcript_path),
            messages_count: 0,
            tools_count: 0,
            commits_count: 0,
        });
    }

    // Parse time range
    let started_at = started_at_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok()).map(|dt| dt.with_timezone(&chrono::Utc));
    let completed_at = completed_at_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok()).map(|dt| dt.with_timezone(&chrono::Utc));

    // Parse transcript
    let result = transcript::parse_transcript_full_for_task(path, started_at, completed_at);
    let parsed = match result {
        Ok(r) => r,
        Err(e) => {
            return Json(ReparseResponse {
                success: false,
                message: format!("Failed to parse transcript: {}", e),
                messages_count: 0,
                tools_count: 0,
                commits_count: 0,
            });
        }
    };

    // Clear existing data
    let _ = conn.execute("DELETE FROM conversation_messages WHERE history_id = ?", [id]);
    let _ = conn.execute("DELETE FROM tool_usage WHERE history_id = ?", [id]);
    let _ = conn.execute("DELETE FROM commits WHERE history_id = ?", [id]);

    // Save new data
    let mut messages_saved = 0;
    for msg in &parsed.messages {
        if conn.execute(
            "INSERT INTO conversation_messages (history_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                id,
                msg.role,
                msg.content,
                msg.created_at.map(|t| t.to_rfc3339()),
            ],
        ).is_ok() {
            messages_saved += 1;
        }
    }

    let mut tools_saved = 0;
    for tool in &parsed.tool_usages {
        if conn.execute(
            "INSERT INTO tool_usage (history_id, tool_name, tool_args, result_summary, success, timestamp) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                tool.tool_name,
                tool.tool_args,
                tool.result_summary,
                tool.success as i32,
                tool.timestamp.map(|t| t.to_rfc3339()),
            ],
        ).is_ok() {
            tools_saved += 1;
        }
    }

    let mut commits_saved = 0;
    for commit in &parsed.commits {
        if conn.execute(
            "INSERT INTO commits (history_id, commit_hash, commit_message, files_changed, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                id,
                commit.commit_hash,
                commit.commit_message,
                commit.files_changed,
                commit.timestamp.map(|t| t.to_rfc3339()),
            ],
        ).is_ok() {
            commits_saved += 1;
        }
    }

    info!(
        "Reparsed history {}: {} messages, {} tools, {} commits",
        id, messages_saved, tools_saved, commits_saved
    );

    Json(ReparseResponse {
        success: true,
        message: format!("Successfully reparsed transcript"),
        messages_count: messages_saved,
        tools_count: tools_saved,
        commits_count: commits_saved,
    })
}

// ============================================================================
// Claude Messages API
// ============================================================================

/// Claude message from session
#[derive(Serialize, Clone)]
struct ClaudeMessage {
    role: String,  // "user" or "assistant"
    timestamp: String,
    text: String,
}

/// Response for Claude messages API
#[derive(Serialize)]
struct ClaudeMessagesResponse {
    success: bool,
    messages: Vec<ClaudeMessage>,
    session_file: String,
}

/// Query params for Claude messages
#[derive(Deserialize)]
struct ClaudeMessagesParams {
    /// Number of messages to return (default: 1)
    count: Option<usize>,
    /// Alias for count (for frontend compatibility)
    limit: Option<usize>,
    /// Project path filter (optional)
    project: Option<String>,
    /// Tmux session name (optional) - used with window to get pane's working directory
    session: Option<String>,
    /// Tmux window name (optional) - used with session to get pane's working directory
    window: Option<String>,
}

/// Parse claude messages from a JSONL session file (blocking I/O)
/// Reads only the last chunk of the file for performance with large files
fn parse_claude_messages(session_file: &std::path::Path, _count: usize) -> Vec<ClaudeMessage> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = match std::fs::File::open(session_file) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);

    // Read last 5MB max - sufficient for recent messages, fast even for 100MB+ files
    let read_start = if file_len > 5_000_000 { file_len - 5_000_000 } else { 0 };
    if read_start > 0 {
        let _ = file.seek(SeekFrom::Start(read_start));
    }

    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf);
    let content = String::from_utf8_lossy(&buf);

    // If we seeked, skip the first partial line
    let lines_str = if read_start > 0 {
        content.splitn(2, '\n').nth(1).unwrap_or("")
    } else {
        &content
    };

    let mut all_messages: Vec<ClaudeMessage> = Vec::new();

    for line in lines_str.lines() {
        if line.is_empty() {
            continue;
        }

        let data = match serde_json::from_str::<serde_json::Value>(line) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let msg_type = match data.get("type").and_then(|t| t.as_str()) {
            Some("user") => "user",
            Some("assistant") => "assistant",
            _ => continue,
        };

        let timestamp = data.get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let msg = match data.get("message") {
            Some(m) => m,
            None => continue,
        };
        let msg_content = msg.get("content");

        // Handle string content (user-typed messages)
        if let Some(text) = msg_content.and_then(|c| c.as_str()) {
            if !text.is_empty()
                && !text.starts_with("<bash")
                && !text.starts_with("<system")
                && !text.starts_with("<task-")
                && !text.starts_with("<local-")
                && !text.starts_with("<command-name>")
            {
                all_messages.push(ClaudeMessage {
                    role: msg_type.to_string(),
                    timestamp,
                    text: text.to_string(),
                });
            }
        }
        // Handle array content
        else if let Some(arr) = msg_content.and_then(|c| c.as_array()) {
            let first_type = arr.first()
                .and_then(|item| item.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");

            if first_type == "tool_result" || first_type == "tool_use" || first_type == "thinking" {
                continue;
            }

            // Extract first meaningful text item
            for item in arr {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty()
                            && !trimmed.starts_with("<system")
                            && !trimmed.starts_with("<bash")
                            && !trimmed.starts_with("<task-")
                            && !trimmed.starts_with("<local-")
                            && !trimmed.starts_with("<command-name>")
                        {
                            all_messages.push(ClaudeMessage {
                                role: msg_type.to_string(),
                                timestamp,
                                text: text.to_string(),
                            });
                            break;
                        }
                    }
                }
            }
        }
    }

    all_messages
}

/// Get recent user messages from Claude Code session
async fn get_claude_messages(Query(params): Query<ClaudeMessagesParams>) -> Json<ClaudeMessagesResponse> {
    let count = params.count.or(params.limit).unwrap_or(1);

    // Determine project filter: either from explicit project param or from tmux pane's working directory
    let project_filter = if let (Some(session), Some(window)) = (&params.session, &params.window) {
        // Get the pane's current working directory via tmux
        let target = format!("{}:{}", session, window);
        let output = std::process::Command::new("tmux")
            .args(["display-message", "-p", "-t", &target, "#{pane_current_path}"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    // Convert path to Claude project directory format
                    // /Users/heygo/.config -> -Users-heygo--config
                    // Both '/' and '.' are replaced with '-'
                    Some(path.replace('/', "-").replace('.', "-"))
                } else {
                    params.project.clone()
                }
            }
            _ => params.project.clone()
        }
    } else {
        params.project.clone()
    };

    // Find session files
    let claude_projects = dirs::home_dir()
        .map(|h| h.join(".claude/projects"))
        .unwrap_or_default();

    if !claude_projects.exists() {
        return Json(ClaudeMessagesResponse {
            success: false,
            messages: vec![],
            session_file: String::new(),
        });
    }

    // Find all .jsonl files sorted by modification time (newest first)
    let mut candidate_files: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&claude_projects) {
        for entry in entries.flatten() {
            let project_dir = entry.path();
            if project_dir.is_dir() {
                // Apply project filter if specified
                // The filter is a path converted from tmux pane's working directory
                // (e.g., "/Users/heygo/.config" -> "-Users-heygo--config")
                // Use exact match on directory name to avoid matching subdirectories
                if let Some(ref filter) = project_filter {
                    let dir_name = project_dir.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if dir_name != *filter {
                        continue;
                    }
                }

                if let Ok(files) = std::fs::read_dir(&project_dir) {
                    for file in files.flatten() {
                        let path = file.path();
                        if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                            if let Ok(meta) = path.metadata() {
                                if let Ok(modified) = meta.modified() {
                                    candidate_files.push((path, modified));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort by modification time, newest first
    candidate_files.sort_by(|a, b| b.1.cmp(&a.1));

    debug!("Claude messages: Found {} candidate session files for filter {:?}", candidate_files.len(), project_filter);

    // Helper function to check if a file has real conversation content
    // Uses BufReader for efficient streaming without loading entire file
    fn file_has_conversation(path: &std::path::Path) -> bool {
        use std::io::{BufRead, BufReader};

        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return false,
        };
        let reader = BufReader::new(file);

        // Check first 200 lines for real user/assistant messages
        // Skip system messages and command invocations
        for line in reader.lines().take(200) {
            if let Ok(l) = line {
                // Must be user or assistant type
                if !l.contains("\"type\":\"user\"") && !l.contains("\"type\":\"assistant\"") {
                    continue;
                }
                // Skip if it's a command/system message (these are logged as type=user)
                if l.contains("<command-name>")
                    || l.contains("<local-command")
                    || l.contains("<system-reminder>")
                {
                    continue;
                }
                // Found a real conversation message
                return true;
            }
        }
        false
    }

    // Find the first file that has actual conversation content
    // If newest file has no messages, fallback to older files
    let mut selected_file: Option<std::path::PathBuf> = None;
    for (path, _) in &candidate_files {
        let has_conv = file_has_conversation(path);
        debug!("Checking file {:?}: has_conversation={}", path.file_name(), has_conv);
        if has_conv {
            selected_file = Some(path.clone());
            break;
        }
    }

    // Fallback to newest file if no file has conversation
    let session_file = selected_file.or_else(|| candidate_files.first().map(|(path, _)| path.clone()));

    let session_file = match session_file {
        Some(path) => path,
        None => {
            return Json(ClaudeMessagesResponse {
                success: false,
                messages: vec![],
                session_file: String::new(),
            });
        }
    };

    // Parse the JSONL file from the tail for performance (files can be 100MB+)
    // Use spawn_blocking to avoid blocking the tokio runtime
    let session_file_clone = session_file.clone();
    let messages = tokio::task::spawn_blocking(move || {
        parse_claude_messages(&session_file_clone, count)
    }).await.unwrap_or_default();

    // Return the last `count` messages (mixed user + assistant)
    let start = messages.len().saturating_sub(count);
    let messages = messages[start..].to_vec();

    Json(ClaudeMessagesResponse {
        success: true,
        messages,
        session_file: session_file.to_string_lossy().to_string(),
    })
}

// ============================================================================
// Workspace Handlers
// ============================================================================

/// Start a new workspace (enhanced with port management, layouts, browser)
async fn start_workspace(Json(req): Json<StartWorkspaceRequest>) -> Json<StartWorkspaceResponse> {
    use std::path::Path;

    let git_dir = Path::new(&req.git_dir);

    // Validation
    if !git_dir.exists() {
        return Json(StartWorkspaceResponse {
            success: false,
            session_name: String::new(),
            worktree_path: String::new(),
            message: format!("Directory '{}' does not exist", req.git_dir),
            port: None,
            frontend_port: None,
            backend_port: None,
            browser_url: None,
        });
    }

    if !git_dir.join(".git").exists() {
        return Json(StartWorkspaceResponse {
            success: false,
            session_name: String::new(),
            worktree_path: String::new(),
            message: format!("Directory '{}' is not a git repository", req.git_dir),
            port: None,
            frontend_port: None,
            backend_port: None,
            browser_url: None,
        });
    }

    // Load config
    let cfg = match config::AgentConfig::load() {
        Ok(c) => c,
        Err(e) => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name: String::new(),
                worktree_path: String::new(),
                message: format!("Failed to load config: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    let agent_name = req.agent.unwrap_or_else(|| cfg.defaults.agent.clone());
    let agent_def = match cfg.get_agent(&agent_name) {
        Some(a) => a,
        None => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name: String::new(),
                worktree_path: String::new(),
                message: format!("Agent '{}' not found in config", agent_name),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    let layout_name = req.layout.clone().unwrap_or_else(|| cfg.defaults.layout.clone());
    let layout = match cfg.get_layout(&layout_name) {
        Some(l) => l,
        None => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name: String::new(),
                worktree_path: String::new(),
                message: format!("Layout '{}' not found in config", layout_name),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    // Determine if using feature directory structure
    let use_feature_dir = req.fullstack_mode.unwrap_or(false)
        || req.frontend_cmd.is_some()
        || req.backend_cmd.is_some()
        || req.dev_server_cmd.is_some();

    // Create worktree
    let git = workspace::GitWorktree::new(git_dir);
    let worktree_path = if use_feature_dir {
        match git.create_feature_dir(&req.branch, req.base_branch.as_deref()).await {
            Ok(p) => p,
            Err(e) => {
                return Json(StartWorkspaceResponse {
                    success: false,
                    session_name: String::new(),
                    worktree_path: String::new(),
                    message: format!("Failed to create feature worktree: {}", e),
                    port: None,
                    frontend_port: None,
                    backend_port: None,
                    browser_url: None,
                });
            }
        }
    } else {
        match git.create(&req.branch).await {
            Ok(p) => p,
            Err(e) => {
                return Json(StartWorkspaceResponse {
                    success: false,
                    session_name: String::new(),
                    worktree_path: String::new(),
                    message: format!("Failed to create worktree: {}", e),
                    port: None,
                    frontend_port: None,
                    backend_port: None,
                    browser_url: None,
                });
            }
        }
    };

    let session_name = req.session.clone().unwrap_or_else(|| {
        git_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string())
    });

    // Create tmux window first (needed for port allocation based on window index)
    let actual_session = match agent::TmuxAgent::create_workspace(
        &session_name,
        &req.branch,
        &worktree_path,
        layout,
        agent_def,
    )
    .await
    {
        Ok(name) => name,
        Err(e) => {
            // Cleanup on failure
            if use_feature_dir {
                let _ = git.cleanup_feature_dir(&req.branch, true).await;
            } else {
                let _ = git.remove(&req.branch, true).await;
            }
            return Json(StartWorkspaceResponse {
                success: false,
                session_name: String::new(),
                worktree_path: String::new(),
                message: format!("Failed to create tmux window: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    // Port allocation
    let fullstack = req.fullstack_mode.unwrap_or(false);
    let mut allocated_port: Option<u16> = None;
    let mut allocated_frontend_port: Option<u16> = None;
    let mut allocated_backend_port: Option<u16> = None;

    // Get window index for port calculation
    if let Ok(window_index) = port::PortManager::get_window_index(&actual_session, &req.branch).await {
        if fullstack {
            let frontend_base = req.frontend_port_base.unwrap_or(3000);
            let backend_base = req.backend_port_base.unwrap_or(8000);
            let (fp, bp) = port::PortManager::allocate_fullstack_ports(frontend_base, backend_base, window_index);
            allocated_frontend_port = Some(fp);
            allocated_backend_port = Some(bp);
        } else if req.dev_server_cmd.is_some() || req.port_base.is_some() {
            let base = req.port_base.unwrap_or(9100);
            allocated_port = Some(port::PortManager::allocate_port(base, window_index));
        }
    }

    // Store metadata in tmux window options
    let window_name = req.branch.replace('/', "-");
    let target = format!("{}:{}", actual_session, window_name);
    let browser_type = req.browser.clone().unwrap_or_else(|| "chrome".to_string());

    // Set tmux window options for metadata
    if let Some(p) = allocated_port {
        let _ = agent::TmuxAgent::set_window_option(&target, "agent_port", &p.to_string()).await;
    }
    if let Some(fp) = allocated_frontend_port {
        let _ = agent::TmuxAgent::set_window_option(&target, "agent_frontend_port", &fp.to_string()).await;
    }
    if let Some(bp) = allocated_backend_port {
        let _ = agent::TmuxAgent::set_window_option(&target, "agent_backend_port", &bp.to_string()).await;
    }
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_dir", &worktree_path.to_string_lossy()).await;
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_main_repo", &req.git_dir).await;
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_browser", &browser_type).await;
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_name", &req.branch).await;
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_fullstack", &fullstack.to_string()).await;

    // Write feature.json if using feature directory
    let feature_dir = git.get_feature_dir(&req.branch);
    if use_feature_dir && feature_dir.exists() {
        let _ = layout::write_feature_json(
            &feature_dir,
            &req.branch,
            &worktree_path,
            &req.branch,
            "main",
            fullstack,
            allocated_port,
            allocated_frontend_port,
            allocated_backend_port,
            &browser_type,
        ).await;

        let _ = layout::write_agent_info(
            &feature_dir,
            &worktree_path,
            &req.branch,
            "main",
            allocated_port,
            allocated_frontend_port,
            allocated_backend_port,
        ).await;
    }

    // Build browser URL
    let browser_url = if req.auto_open_browser.unwrap_or(true) {
        let url_template = req.browser_url.clone().unwrap_or_default();
        let url = browser::BrowserAutomation::build_url(
            &url_template,
            allocated_port,
            allocated_frontend_port,
            allocated_backend_port,
        );
        if !url.is_empty() {
            // Open browser after a delay (let dev server start)
            let browser = browser_type.clone();
            let url_clone = url.clone();
            tokio::spawn(async move {
                let _ = browser::BrowserAutomation::open_url_delayed(&browser, &url_clone, 3).await;
            });
            Some(url)
        } else {
            None
        }
    } else {
        None
    };

    Json(StartWorkspaceResponse {
        success: true,
        session_name: actual_session,
        worktree_path: worktree_path.to_string_lossy().to_string(),
        message: "Workspace started successfully".to_string(),
        port: allocated_port,
        frontend_port: allocated_frontend_port,
        backend_port: allocated_backend_port,
        browser_url,
    })
}

/// Resume a workspace (list or attach)
async fn resume_workspace(Json(req): Json<ResumeWorkspaceRequest>) -> Json<StartWorkspaceResponse> {
    use std::path::Path;

    let git_dir = Path::new(&req.git_dir);

    if !git_dir.exists() {
        return Json(StartWorkspaceResponse {
            success: false,
            session_name: String::new(),
            worktree_path: String::new(),
            message: format!("Directory '{}' does not exist", req.git_dir),
            port: None,
            frontend_port: None,
            backend_port: None,
            browser_url: None,
        });
    }

    // Determine session name
    let session_name = req.session.clone().unwrap_or_else(|| {
        git_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string())
    });

    // Determine window name from branch
    let window_name = req.branch.replace('/', "-");

    // Find worktree path
    let worktree_path = match workspace::GitWorktree::find_worktree(git_dir, &req.branch).await {
        Ok(Some(path)) => path,
        Ok(None) => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name,
                worktree_path: String::new(),
                message: format!("Worktree for branch '{}' not found", req.branch),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
        Err(e) => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name,
                worktree_path: String::new(),
                message: format!("Failed to find worktree: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    // First, create the tmux window
    match agent::TmuxAgent::simple_new_window(&session_name, &window_name).await {
        Ok(_) => {}
        Err(e) => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name,
                worktree_path: worktree_path.display().to_string(),
                message: format!("Failed to create tmux window: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    }

    // Small delay to let tmux create the window
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Determine layout
    let layout_type = req.layout.as_deref().unwrap_or("default");
    let agent_cmd = req.agent.clone().unwrap_or_else(|| "claude".to_string());
    let layout = match layout_type {
        "workspace" => layout::LayoutTemplate::Workspace {
            agent_cmd,
            frontend_cmd: None,
            backend_cmd: None,
        },
        _ => layout::LayoutTemplate::Default { agent_cmd },
    };

    // Create tmux layout in the window
    match layout::LayoutRenderer::create_layout(&session_name, &window_name, layout, &worktree_path).await {
        Ok(_) => {
            Json(StartWorkspaceResponse {
                success: true,
                session_name,
                worktree_path: worktree_path.display().to_string(),
                message: format!("Resumed workspace '{}' with {} layout", window_name, layout_type),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            })
        }
        Err(e) => {
            Json(StartWorkspaceResponse {
                success: false,
                session_name,
                worktree_path: worktree_path.display().to_string(),
                message: format!("Failed to create layout: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            })
        }
    }
}

/// Destroy a workspace (enhanced with port cleanup and branch deletion)
async fn destroy_workspace(Json(req): Json<DestroyWorkspaceRequest>) -> Json<CommandResponse> {
    use std::path::Path;

    let git_dir = Path::new(&req.git_dir);

    if !git_dir.exists() {
        return Json(CommandResponse {
            success: false,
            message: format!("Directory '{}' does not exist", req.git_dir),
        });
    }

    let session_name = req.session.clone().unwrap_or_else(|| {
        git_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string())
    });

    let window_name = req.branch.replace('/', "-");
    let target = format!("{}:{}", session_name, window_name);

    // Get port metadata before killing window
    let mut ports_to_kill = Vec::new();
    if req.kill_ports.unwrap_or(false) {
        // Try to get ports from tmux window options
        if let Ok(Some(port_str)) = agent::TmuxAgent::get_window_option(&target, "agent_port").await {
            if let Ok(port) = port_str.parse::<u16>() {
                ports_to_kill.push(port);
            }
        }
        if let Ok(Some(port_str)) = agent::TmuxAgent::get_window_option(&target, "agent_frontend_port").await {
            if let Ok(port) = port_str.parse::<u16>() {
                ports_to_kill.push(port);
            }
        }
        if let Ok(Some(port_str)) = agent::TmuxAgent::get_window_option(&target, "agent_backend_port").await {
            if let Ok(port) = port_str.parse::<u16>() {
                ports_to_kill.push(port);
            }
        }
    }

    // Kill port processes
    if !ports_to_kill.is_empty() {
        if let Err(e) = port::PortManager::kill_ports(&ports_to_kill).await {
            warn!("Failed to kill port processes: {}", e);
        }
    }

    // Kill tmux window
    if agent::TmuxAgent::session_exists(&session_name).await {
        if let Err(e) = agent::TmuxAgent::kill_window(&session_name, &req.branch).await {
            warn!("Failed to kill window: {}", e);
        }
    }

    let git = workspace::GitWorktree::new(git_dir);

    // Try to remove feature directory first, then fall back to regular worktree
    let feature_dir = git.get_feature_dir(&req.branch);
    if feature_dir.exists() {
        if let Err(e) = git.cleanup_feature_dir(&req.branch, req.force).await {
            warn!("Failed to cleanup feature directory: {}", e);
            // Try regular worktree removal as fallback
            if let Err(e2) = git.remove(&req.branch, req.force).await {
                return Json(CommandResponse {
                    success: false,
                    message: format!("Failed to remove worktree: {} (feature dir: {})", e2, e),
                });
            }
        }
    } else {
        // Regular worktree removal
        if let Err(e) = git.remove(&req.branch, req.force).await {
            return Json(CommandResponse {
                success: false,
                message: format!("Failed to remove worktree: {}", e),
            });
        }
    }

    // Delete git branch if requested
    if req.delete_branch.unwrap_or(false) {
        if let Err(e) = git.delete_branch(&req.branch, req.force).await {
            warn!("Failed to delete branch '{}': {}", req.branch, e);
            // Don't fail the whole operation if branch deletion fails
        }
    }

    Json(CommandResponse {
        success: true,
        message: "Workspace destroyed successfully".to_string(),
    })
}

/// List all active workspaces
async fn list_workspaces() -> Json<WorkspaceListResponse> {
    let windows = agent::TmuxAgent::list_windows().await.unwrap_or_default();
    Json(WorkspaceListResponse { workspaces: windows })
}

#[derive(Serialize)]
struct BranchInfo {
    name: String,
    has_worktree: bool,
}

#[derive(Serialize)]
struct GitBranchesResponse {
    branches: Vec<String>,
    local: Vec<String>,
    remote: Vec<String>,
    /// Branches with worktree status
    branches_with_status: Vec<BranchInfo>,
}

#[derive(Deserialize)]
struct GitBranchesQuery {
    git_dir: Option<String>,
}

/// List git branches for a repository
async fn list_git_branches(Query(query): Query<GitBranchesQuery>) -> Json<GitBranchesResponse> {
    // Use provided git_dir or try to detect from current session
    let git_dir = query.git_dir.unwrap_or_else(|| ".".to_string());
    let git_dir = std::path::PathBuf::from(&git_dir);

    tracing::info!("list_git_branches: git_dir={:?}, exists={}", git_dir, git_dir.exists());

    let worktree = workspace::GitWorktree::new(&git_dir);

    let local = match worktree.list_branches().await {
        Ok(branches) => {
            tracing::info!("list_branches OK: {} branches", branches.len());
            branches
        }
        Err(e) => {
            tracing::error!("list_branches ERROR: {}", e);
            Vec::new()
        }
    };

    let remote = match worktree.list_remote_branches().await {
        Ok(branches) => branches,
        Err(e) => {
            tracing::warn!("list_remote_branches skip: {}", e);
            Vec::new()
        }
    };

    let all = worktree.list_all_branches().await.unwrap_or_default();

    // Get branches that have worktrees
    let worktree_branches: std::collections::HashSet<String> =
        get_worktree_branches(&git_dir).await;

    // Build branch info with worktree status
    let branches_with_status: Vec<BranchInfo> = all
        .iter()
        .map(|name| BranchInfo {
            name: name.clone(),
            has_worktree: worktree_branches.contains(name),
        })
        .collect();

    Json(GitBranchesResponse {
        branches: all,
        local,
        remote,
        branches_with_status,
    })
}

/// Get branches that have worktrees
async fn get_worktree_branches(git_dir: &std::path::Path) -> std::collections::HashSet<String> {
    use tokio::process::Command;

    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(git_dir)
        .output()
        .await;

    let mut branches = std::collections::HashSet::new();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                    branches.insert(branch.to_string());
                }
            }
        }
    }

    branches
}

/// Get config
async fn get_config() -> Json<ConfigResponse> {
    let cfg = config::AgentConfig::load().unwrap_or_default();
    Json(ConfigResponse {
        workspaces: cfg.workspaces,
        agents: cfg.agents,
        layouts: cfg.layouts,
        defaults: cfg.defaults,
    })
}

/// Activate workspace (window focus hook)
/// Called when a tmux window is activated to sync lazygit and browser
async fn activate_workspace(Json(req): Json<ActivateWorkspaceRequest>) -> Json<ActivateWorkspaceResponse> {
    let window_name = req.window.replace('/', "-");
    let target = format!("{}:{}", req.session, window_name);

    let mut refreshed_lazygit = false;
    let mut switched_browser_tab = false;

    // Get metadata from tmux window options
    let agent_port = agent::TmuxAgent::get_window_option(&target, "agent_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let frontend_port = agent::TmuxAgent::get_window_option(&target, "agent_frontend_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let browser_type = agent::TmuxAgent::get_window_option(&target, "agent_browser")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "chrome".to_string());

    // Refresh lazygit (send 'R' key to pane 1 or bottom-left)
    // Try to find lazygit pane and send refresh
    if let Ok(panes) = agent::TmuxAgent::list_panes(&req.session, &req.window).await {
        for pane in &panes {
            if pane.command == "lazygit" {
                if agent::TmuxAgent::send_keys_to_pane(&req.session, &req.window, &pane.index, "R", false).await.is_ok() {
                    refreshed_lazygit = true;
                    break;
                }
            }
        }
    }

    // Switch browser tab to matching port
    let port_to_switch = frontend_port.or(agent_port);
    if let Some(port) = port_to_switch {
        if let Ok(found) = browser::BrowserAutomation::switch_to_tab(&browser_type, port).await {
            switched_browser_tab = found;
        }
    }

    Json(ActivateWorkspaceResponse {
        success: true,
        message: "Workspace activated".to_string(),
        refreshed_lazygit,
        switched_browser_tab,
    })
}

/// Get workspace metadata from tmux window options
async fn get_workspace_metadata(Query(params): Query<ActivateWorkspaceRequest>) -> Json<WorkspaceMetadataResponse> {
    let window_name = params.window.replace('/', "-");
    let target = format!("{}:{}", params.session, window_name);

    let port = agent::TmuxAgent::get_window_option(&target, "agent_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let frontend_port = agent::TmuxAgent::get_window_option(&target, "agent_frontend_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let backend_port = agent::TmuxAgent::get_window_option(&target, "agent_backend_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let dir = agent::TmuxAgent::get_window_option(&target, "agent_dir")
        .await
        .ok()
        .flatten();

    let main_repo = agent::TmuxAgent::get_window_option(&target, "agent_main_repo")
        .await
        .ok()
        .flatten();

    let browser = agent::TmuxAgent::get_window_option(&target, "agent_browser")
        .await
        .ok()
        .flatten();

    let name = agent::TmuxAgent::get_window_option(&target, "agent_name")
        .await
        .ok()
        .flatten();

    let fullstack = agent::TmuxAgent::get_window_option(&target, "agent_fullstack")
        .await
        .ok()
        .flatten()
        .map(|s| s == "true")
        .unwrap_or(false);

    Json(WorkspaceMetadataResponse {
        session: params.session,
        window: params.window,
        port,
        frontend_port,
        backend_port,
        dir,
        main_repo,
        browser,
        name,
        fullstack,
    })
}

// ============================================================================
// Port Management Handlers
// ============================================================================

/// Check if a port is in use
async fn check_port(AxumPath(port): AxumPath<u16>) -> Json<PortStatusResponse> {
    Json(PortStatusResponse {
        port,
        in_use: port::PortManager::is_port_in_use(port),
    })
}

/// Kill process on a port
async fn kill_port(Json(req): Json<KillPortRequest>) -> Json<KillPortResponse> {
    match port::PortManager::kill_port_process(req.port).await {
        Ok(killed) => Json(KillPortResponse {
            success: true,
            port: req.port,
            killed,
            message: if killed {
                "Process killed".to_string()
            } else {
                "No process found on port".to_string()
            },
        }),
        Err(e) => Json(KillPortResponse {
            success: false,
            port: req.port,
            killed: false,
            message: format!("Failed to kill process: {}", e),
        }),
    }
}

/// Allocate an available port
async fn allocate_port(Query(params): Query<std::collections::HashMap<String, String>>) -> Json<AllocatePortResponse> {
    let base: u16 = params
        .get("base")
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    match port::PortManager::find_available_port(base, 100) {
        Some(port) => Json(AllocatePortResponse {
            success: true,
            port,
            message: format!("Port {} is available", port),
        }),
        None => Json(AllocatePortResponse {
            success: false,
            port: 0,
            message: format!("No available port found starting from {}", base),
        }),
    }
}

// ============================================================================
// Browser Handlers
// ============================================================================

/// Open browser to a URL
async fn open_browser(Json(req): Json<OpenBrowserRequest>) -> Json<CommandResponse> {
    match browser::BrowserAutomation::open_url(&req.browser, &req.url).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: format!("Opened {} in {}", req.url, req.browser),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to open browser: {}", e),
        }),
    }
}

/// Switch browser to a tab with specific port
async fn switch_browser_tab(Json(req): Json<SwitchBrowserTabRequest>) -> Json<CommandResponse> {
    match browser::BrowserAutomation::switch_to_tab(&req.browser, req.port).await {
        Ok(found) => Json(CommandResponse {
            success: true,
            message: if found {
                format!("Switched to tab with port {}", req.port)
            } else {
                format!("No tab found with port {}", req.port)
            },
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to switch tab: {}", e),
        }),
    }
}

// ============================================================================
// tmux Handlers
// ============================================================================

/// Send keys to a tmux pane
async fn tmux_send_keys(Json(req): Json<TmuxSendKeysRequest>) -> Json<CommandResponse> {
    // Determine suffix key: use suffix_key if provided, fallback to "Enter" if enter=true
    let suffix_key = match &req.suffix_key {
        Some(key) if !key.is_empty() => Some(key.as_str()),
        _ if req.enter => Some("Enter"),
        _ => None,
    };

    // Debug logging
    info!(
        "send_keys: session={}, window={}, pane={}, keys={}, suffix_key={:?}, enter={}",
        req.session, req.window, req.pane, req.keys, suffix_key, req.enter
    );

    match agent::TmuxAgent::send_keys_with_suffix(
        &req.session,
        &req.window,
        &req.pane,
        &req.keys,
        suffix_key,
    )
    .await
    {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Keys sent successfully".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to send keys: {}", e),
        }),
    }
}

/// Capture content from a tmux pane
async fn tmux_capture(Query(params): Query<TmuxCaptureParams>) -> Json<TmuxCaptureResponse> {
    match agent::TmuxAgent::capture_pane(
        &params.session,
        &params.window,
        &params.pane,
        params.lines,
    )
    .await
    {
        Ok(content) => Json(TmuxCaptureResponse {
            success: true,
            content,
        }),
        Err(e) => Json(TmuxCaptureResponse {
            success: false,
            content: format!("Failed to capture: {}", e),
        }),
    }
}

#[derive(Deserialize)]
struct ClaudeStatusParams {
    session: String,
    window: String,
}

#[derive(Serialize)]
struct ClaudeStatusResponse {
    success: bool,
    status: agent::ClaudeStatus,
}

/// Get Claude Code status from a tmux pane
async fn get_claude_status(Query(params): Query<ClaudeStatusParams>) -> Json<ClaudeStatusResponse> {
    match agent::TmuxAgent::get_claude_status(&params.session, &params.window).await {
        Ok(status) => Json(ClaudeStatusResponse {
            success: true,
            status,
        }),
        Err(_) => Json(ClaudeStatusResponse {
            success: false,
            status: agent::ClaudeStatus::default(),
        }),
    }
}

/// List all tmux sessions
async fn tmux_list_sessions() -> Json<TmuxSessionsResponse> {
    let sessions = agent::TmuxAgent::list_sessions()
        .await
        .unwrap_or_default();
    Json(TmuxSessionsResponse { sessions })
}

/// List panes in a tmux window
async fn tmux_list_panes(Query(params): Query<TmuxListPanesParams>) -> Json<TmuxPanesResponse> {
    let panes = agent::TmuxAgent::list_panes(&params.session, &params.window)
        .await
        .unwrap_or_default();
    Json(TmuxPanesResponse { panes })
}

/// List all tmux windows with full details
async fn tmux_list_all_windows() -> Json<TmuxWindowsResponse> {
    let windows = agent::TmuxAgent::list_all_windows()
        .await
        .unwrap_or_default();
    Json(TmuxWindowsResponse { windows })
}

/// Kill session request
#[derive(Deserialize)]
struct TmuxKillSessionRequest {
    session: String,
}

/// Kill a tmux session
async fn tmux_kill_session(Json(req): Json<TmuxKillSessionRequest>) -> Json<CommandResponse> {
    match agent::TmuxAgent::kill_session(&req.session).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Session killed successfully".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to kill session: {}", e),
        }),
    }
}

/// Kill window request
#[derive(Deserialize)]
struct TmuxKillWindowRequest {
    session: String,
    window: String,
}

/// Kill a tmux window (saves window info for resume before killing)
async fn tmux_kill_window(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TmuxKillWindowRequest>,
) -> Json<CommandResponse> {
    // Get window info before killing (for resume functionality)
    let window_info = get_window_info_before_close(&req.session, &req.window).await;

    // Save to closed_windows table if we got valid info
    if let Some((session_id, session_name, window_name, working_dir, git_branch, pane_count)) = window_info {
        let db = &state.state.lock().unwrap().db;
        if let Err(e) = db.save_closed_window(&session_id, &session_name, &window_name, &working_dir, &git_branch, pane_count) {
            warn!("Failed to save closed window info: {}", e);
        }
    }

    // Now kill the window
    match agent::TmuxAgent::kill_window(&req.session, &req.window).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Window killed successfully".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to kill window: {}", e),
        }),
    }
}

/// Get window info before closing (session_id, session_name, window_name, working_dir, git_branch, pane_count)
async fn get_window_info_before_close(session: &str, window: &str) -> Option<(String, String, String, String, String, i32)> {
    use tokio::process::Command;

    // Build target - handle window ID (starts with @) or window name
    let target = if window.starts_with('@') {
        format!("{}:{}", session, window)
    } else {
        format!("{}:={}", session, window)
    };

    // Get session_id, session_name, window_name, working_dir, and pane_count in one call
    let output = Command::new(agent::TMUX_BIN)
        .args([
            "display-message",
            "-t", &target,
            "-p",
            "#{session_id}|#{session_name}|#{window_name}|#{pane_current_path}|#{window_panes}",
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stdout.trim().split('|').collect();
    if parts.len() != 5 {
        return None;
    }

    let session_id = parts[0].to_string();
    let session_name = parts[1].to_string();
    let window_name = parts[2].to_string();
    let working_dir = parts[3].to_string();
    let pane_count = parts[4].parse::<i32>().unwrap_or(1);

    // Try to get git branch from working dir
    let git_branch = get_git_branch(&working_dir).await.unwrap_or_default();

    Some((session_id, session_name, window_name, working_dir, git_branch, pane_count))
}

/// Get current git branch from a directory
async fn get_git_branch(dir: &str) -> Option<String> {
    use tokio::process::Command;

    let output = Command::new("git")
        .args(["-C", dir, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// New window request
#[derive(Deserialize)]
struct TmuxNewWindowRequest {
    session: String,
    name: String,
}

/// Create a new tmux window
async fn tmux_new_window(Json(req): Json<TmuxNewWindowRequest>) -> Json<CommandResponse> {
    match agent::TmuxAgent::simple_new_window(&req.session, &req.name).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: format!("Window '{}' created", req.name),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to create window: {}", e),
        }),
    }
}

/// Closed window info for API response
#[derive(Serialize)]
struct ClosedWindowInfo {
    id: i64,
    session_name: String,
    window_name: String,
    working_dir: String,
    git_branch: String,
    pane_count: i32,
    closed_at: Option<String>,
}

/// Get closed windows for a session (for resume functionality)
async fn get_closed_windows(
    State(state): State<Arc<AppState>>,
    AxumPath(session_name): AxumPath<String>,
) -> Json<Vec<ClosedWindowInfo>> {
    // Get currently open window names for this session
    let open_windows = agent::TmuxAgent::list_all_windows()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.session_name == session_name)
        .map(|w| w.window_name)
        .collect::<Vec<_>>();

    let state = state.state.lock().unwrap();
    let closed = state.db.load_closed_windows(&session_name, &open_windows)
        .unwrap_or_default()
        .into_iter()
        .map(|w| ClosedWindowInfo {
            id: w.id,
            session_name: w.session_name,
            window_name: w.window_name,
            working_dir: w.working_dir,
            git_branch: w.git_branch,
            pane_count: w.pane_count,
            closed_at: w.closed_at.map(|t| t.to_rfc3339()),
        })
        .collect();

    Json(closed)
}

/// Delete a closed window record
#[derive(Deserialize)]
struct DeleteClosedWindowRequest {
    id: i64,
}

async fn delete_closed_window(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteClosedWindowRequest>,
) -> Json<CommandResponse> {
    let state = state.state.lock().unwrap();
    match state.db.delete_closed_window(req.id) {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Closed window record deleted".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to delete: {}", e),
        }),
    }
}

/// Resume closed window request
#[derive(Deserialize)]
struct ResumeClosedWindowRequest {
    session: String,
    window_name: String,
    working_dir: String,
    #[serde(default)]
    layout: Option<String>,  // "default" or "workspace"
    #[serde(default)]
    closed_window_id: Option<i64>,  // ID to delete after resume
}

/// Resume a closed window with optional layout
async fn resume_closed_window(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ResumeClosedWindowRequest>,
) -> Json<CommandResponse> {
    use std::path::Path;

    // Create new window
    if let Err(e) = agent::TmuxAgent::simple_new_window(&req.session, &req.window_name).await {
        return Json(CommandResponse {
            success: false,
            message: format!("Failed to create window: {}", e),
        });
    }

    // Apply layout if requested and working_dir exists
    let layout_type = req.layout.as_deref().unwrap_or("simple");
    if layout_type != "simple" && !req.working_dir.is_empty() {
        let working_dir = Path::new(&req.working_dir);
        if working_dir.exists() {
            // Default agent command
            let agent_cmd = "claude".to_string();

            let template = match layout_type {
                "workspace" => layout::LayoutTemplate::Workspace {
                    agent_cmd,
                    frontend_cmd: None,
                    backend_cmd: None,
                },
                _ => layout::LayoutTemplate::Default { agent_cmd },  // "default" = 3-pane
            };

            // Give tmux time to create the window
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            if let Err(e) = layout::LayoutRenderer::create_layout(
                &req.session,
                &req.window_name,
                template,
                working_dir,
            ).await {
                warn!("Failed to apply layout: {}", e);
                // Continue anyway - window was created
            }
        }
    }

    // Delete closed window record if ID provided
    if let Some(id) = req.closed_window_id {
        let db = &state.state.lock().unwrap().db;
        let _ = db.delete_closed_window(id);
    }

    Json(CommandResponse {
        success: true,
        message: format!("Window '{}' resumed with {} layout", req.window_name, layout_type),
    })
}

/// Select window request
#[derive(Deserialize)]
struct TmuxSelectWindowRequest {
    session: String,
    window: String,
    #[serde(default)]
    window_id: Option<String>,  // tmux window ID like @9 for precise targeting
}

/// Select (switch to) a tmux window
async fn tmux_select_window(Json(req): Json<TmuxSelectWindowRequest>) -> Json<CommandResponse> {
    // If window_id is provided, use it for precise targeting
    let window_target = req.window_id.as_deref().unwrap_or(&req.window);

    match agent::TmuxAgent::select_window_by_target(&req.session, window_target).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: format!("Switched to window '{}'", req.window),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to select window: {}", e),
        }),
    }
}

// ============================================================================
// Stream Handlers
// ============================================================================

/// Start streaming output from a pane
async fn stream_start(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StartStreamRequest>,
) -> Json<StartStreamResponse> {
    match state
        .stream_manager
        .start_stream(&req.session, &req.window, &req.pane)
        .await
    {
        Ok(target) => Json(StartStreamResponse {
            success: true,
            target,
            message: "Stream started".to_string(),
        }),
        Err(e) => Json(StartStreamResponse {
            success: false,
            target: String::new(),
            message: e,
        }),
    }
}

/// Stop streaming from a pane
async fn stream_stop(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StopStreamRequest>,
) -> Json<CommandResponse> {
    match state.stream_manager.stop_stream(&req.pane).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Stream stopped".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: e,
        }),
    }
}

/// List active streams
async fn stream_list(State(state): State<Arc<AppState>>) -> Json<ListStreamsResponse> {
    let streams = state
        .stream_manager
        .list_streams()
        .await
        .into_iter()
        .map(|(pane_id, target)| StreamEntry { pane_id, target })
        .collect();
    Json(ListStreamsResponse { streams })
}

// ============================================================================
// WebSocket Handler
// ============================================================================

/// WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Stream chunk message for WebSocket
#[derive(Serialize)]
struct StreamMessage {
    kind: String,
    chunk: stream::StreamChunk,
}

/// Handle WebSocket connection
async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(tokio::sync::Mutex::new(sender));

    // Subscribe to realtime state updates
    let mut state_rx = state.subscribe();

    // Subscribe to stream chunks
    let mut stream_rx = state.stream_manager.subscribe();

    // Send initial realtime message (state + tmux windows)
    let initial_msg = state.get_realtime_message();
    let initial_json = serde_json::to_string(&initial_msg).unwrap_or_default();

    {
        let mut sender_guard = sender.lock().await;
        if sender_guard.send(Message::Text(initial_json)).await.is_err() {
            return;
        }
    }

    // Spawn task to forward state updates to WebSocket
    let sender_clone = sender.clone();
    let state_send_task = tokio::spawn(async move {
        while let Ok(msg) = state_rx.recv().await {
            let json = serde_json::to_string(&msg).unwrap_or_default();
            let mut sender_guard = sender_clone.lock().await;
            if sender_guard.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    // Spawn task to forward stream chunks to WebSocket
    let sender_clone2 = sender.clone();
    let stream_send_task = tokio::spawn(async move {
        while let Ok(chunk) = stream_rx.recv().await {
            let msg = StreamMessage {
                kind: "stream".to_string(),
                chunk,
            };
            let json = serde_json::to_string(&msg).unwrap_or_default();
            let mut sender_guard = sender_clone2.lock().await;
            if sender_guard.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages (ping/pong, close, commands)
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(data)) => {
                let _ = data;
            }
            Ok(Message::Text(text)) => {
                // Handle command from WebSocket
                if let Ok(req) = serde_json::from_str::<SendCommandRequest>(&text) {
                    let _ = handle_command(&state, req).await;
                }
            }
            Err(e) => {
                warn!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    // Clean up
    state_send_task.abort();
    stream_send_task.abort();
    info!("WebSocket connection closed");
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tracker_server=info".parse()?),
        )
        .init();

    // Initialize database
    let db_path = db::default_db_path();
    info!("Opening database at {:?}", db_path);
    let db = Database::open(&db_path)?;

    // Create application state
    let app_state = Arc::new(AppState::new(db)?);

    // Start background task for tmux monitoring (real-time updates)
    let tmux_monitor_state = app_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let windows = agent::TmuxAgent::list_all_windows_sync();
            tmux_monitor_state.broadcast_if_tmux_changed(windows);
        }
    });

    // Build router
    let app = Router::new()
        // Health
        .route("/health", get(health))
        // State
        .route("/api/state", get(get_state))
        .route("/api/tasks", get(get_tasks))
        .route("/api/notes", get(get_notes))
        .route("/api/goals", get(get_goals))
        // Commands
        .route("/api/command", post(send_command))
        .route("/api/notes/add", post(add_note))
        .route("/api/goals/add", post(add_goal))
        // Archive/Restore
        .route("/api/task/archive", post(archive_task))
        .route("/api/task/restore", post(restore_task))
        .route("/api/note/archive", post(archive_note))
        .route("/api/note/restore", post(restore_note))
        // History
        .route("/api/history", get(get_history))
        .route("/api/history/stats", get(get_history_stats))
        .route("/api/history/:id/resume", post(resume_history))
        .route("/api/history/:id/reparse", post(reparse_history))
        .route("/api/history/:id", get(get_history_detail))
        // Claude session
        .route("/api/claude/messages", get(get_claude_messages))
        // Workspace management
        .route("/api/workspace/start", post(start_workspace))
        .route("/api/workspace/resume", post(resume_workspace))
        .route("/api/workspace/destroy", post(destroy_workspace))
        .route("/api/workspace/list", get(list_workspaces))
        .route("/api/workspace/activate", post(activate_workspace))
        .route("/api/workspace/metadata", get(get_workspace_metadata))
        .route("/api/config", get(get_config))
        // Git
        .route("/api/git/branches", get(list_git_branches))
        // Port management
        .route("/api/port/check/:port", get(check_port))
        .route("/api/port/kill", post(kill_port))
        .route("/api/port/allocate", get(allocate_port))
        // Browser automation
        .route("/api/browser/open", post(open_browser))
        .route("/api/browser/switch-tab", post(switch_browser_tab))
        // tmux interaction
        .route("/api/tmux/send-keys", post(tmux_send_keys))
        .route("/api/tmux/capture", get(tmux_capture))
        .route("/api/tmux/claude-status", get(get_claude_status))
        .route("/api/tmux/sessions", get(tmux_list_sessions))
        .route("/api/tmux/panes", get(tmux_list_panes))
        .route("/api/tmux/windows", get(tmux_list_all_windows))
        .route("/api/tmux/kill-session", post(tmux_kill_session))
        .route("/api/tmux/kill-window", post(tmux_kill_window))
        .route("/api/tmux/closed-windows/:session", get(get_closed_windows))
        .route("/api/tmux/closed-windows", delete(delete_closed_window))
        .route("/api/tmux/resume-window", post(resume_closed_window))
        .route("/api/tmux/new-window", post(tmux_new_window))
        .route("/api/tmux/select-window", post(tmux_select_window))
        // Stream (real-time pane output)
        .route("/api/stream/start", post(stream_start))
        .route("/api/stream/stop", post(stream_stop))
        .route("/api/stream/list", get(stream_list))
        // WebSocket
        .route("/ws", get(ws_handler))
        // CORS
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    // Static file serving for web frontend
    // Look for dist in multiple locations
    let web_dist_paths = vec![
        std::path::PathBuf::from("/Users/heygo/.config/agent-tracker/web/dist"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("../../web/dist")))
            .unwrap_or_default(),
        std::path::PathBuf::from("./web/dist"),
    ];

    let web_dist = web_dist_paths
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("/Users/heygo/.config/agent-tracker/web/dist"));

    info!("Serving static files from {:?}", web_dist);

    let index_file = web_dist.join("index.html");
    let serve_dir = ServeDir::new(&web_dist).not_found_service(ServeFile::new(&index_file));

    let app = app.fallback_service(serve_dir);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 3099));
    info!("Starting tracker-server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
