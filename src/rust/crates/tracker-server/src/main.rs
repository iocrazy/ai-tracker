//! tracker-server: Agent Tracker HTTP/WebSocket server
//!
//! This is the main entry point for the tracker server.
//! It manages state directly via HTTP/WebSocket and persists to SQLite.

mod agent;
mod browser;
mod chat_watcher;
mod config;
mod db;
mod env_file;
mod layout;
mod paths;
mod port;
mod stream;
mod transcript;
mod workspace;

mod routes_history;
mod routes_projects;
mod routes_tmux;
mod routes_workspace;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Query, State,
    },
    http::{header, HeaderMap, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json},
    routing::{delete, get, post, put},
    Router,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};
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
        // Startup cleanup: fix stale state from previous run
        match self.db.cleanup_stale_tasks() {
            Ok((dirty, completed, reset)) => {
                if dirty > 0 || completed > 0 || reset > 0 {
                    info!(
                        "Startup cleanup: removed {} dirty + {} completed tasks, reset {} in_progress → awaiting_input",
                        dirty, completed, reset
                    );
                }
            }
            Err(e) => {
                error!("Failed to cleanup stale tasks: {}", e);
            }
        }

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
pub(crate) struct AppState {
    state: Mutex<ServerState>,
    broadcast_tx: broadcast::Sender<RealtimeMessage>,
    /// Last known tmux windows (for change detection)
    last_tmux_windows: Mutex<Vec<agent::TmuxWindowInfo>>,
    /// Stream manager for real-time pane output capture
    stream_manager: stream::StreamManager,
    /// Chat watcher for JSONL file monitoring → WS push
    chat_watcher: chat_watcher::ChatWatcher,
    /// Bearer token for API authentication
    auth_token: String,
    /// Allowed CORS origins
    allowed_origins: Vec<String>,
    /// Server start time (for uptime)
    start_time: std::time::Instant,
    /// Resolved paths for all server directories
    paths: paths::TrackerPaths,
    /// Active todo mapping: pane_id → (todo_id, git_dir)
    /// Used to link history entries to the todo that triggered them
    active_todo_map: Mutex<HashMap<String, (i64, String)>>,
}

impl AppState {
    fn new(db: Database, auth_token: String, allowed_origins: Vec<String>, paths: paths::TrackerPaths) -> Result<Self> {
        let (broadcast_tx, _) = broadcast::channel(16);
        let mut state = ServerState::new(db);
        state.load_from_db()?;
        Ok(Self {
            state: Mutex::new(state),
            broadcast_tx,
            last_tmux_windows: Mutex::new(Vec::new()),
            stream_manager: stream::StreamManager::new(),
            chat_watcher: chat_watcher::ChatWatcher::new(),
            auth_token,
            allowed_origins,
            start_time: std::time::Instant::now(),
            paths,
            active_todo_map: Mutex::new(HashMap::new()),
        })
    }

    /// Resolve git_dir for a session/window from cached tmux windows
    fn resolve_git_dir_for_window(&self, session_id: &str, window_id: &str) -> Option<String> {
        let windows = self.last_tmux_windows.lock().unwrap();
        for win in windows.iter() {
            if win.session_id == session_id && win.window_id == window_id {
                // Prefer cached git_dir, fall back to working_dir -> git root discovery
                if let Some(ref gd) = win.git_dir {
                    if !gd.is_empty() {
                        return Some(gd.clone());
                    }
                }
                if let Some(ref wd) = win.working_dir {
                    if !wd.is_empty() {
                        return agent::TmuxAgent::find_git_root_sync(wd);
                    }
                }
            }
        }
        None
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

    /// Broadcast state to all WebSocket subscribers (with tmux names).
    /// Uses cached tmux windows from the polling loop to avoid blocking the async runtime.
    fn broadcast_state(&self) {
        let mut state = self.get_state_response_with_tmux_names();

        // Populate changed tables from SQLite hook tracking
        {
            let server_state = self.state.lock().unwrap();
            let changes = server_state.db.take_changes();
            if !changes.is_empty() {
                state.changed = changes.into_iter().collect();
            }
        }

        // Use cached tmux windows from the polling loop (updated every 1s)
        // instead of calling list_all_windows_sync() which blocks the async runtime.
        let tmux_windows = self.last_tmux_windows.lock().unwrap().clone();

        // Write cache file for tmux status line
        Self::write_cache_file(&state);

        // Refresh tmux status line (fire-and-forget via thread to avoid blocking)
        std::thread::spawn(|| {
            let _ = std::process::Command::new(agent::TMUX_BIN.as_str())
                .args(["-S", agent::TMUX_SOCKET.as_str(), "refresh-client", "-S"])
                .status();
        });

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

    /// Get current realtime message (state + tmux windows).
    /// Uses cached tmux windows to avoid blocking the async runtime.
    fn get_realtime_message(&self) -> RealtimeMessage {
        let state = self.get_state_response_with_tmux_names();
        let tmux_windows = self.last_tmux_windows.lock().unwrap().clone();
        RealtimeMessage { state, tmux_windows }
    }

    /// Broadcast if tmux windows changed
    fn broadcast_if_tmux_changed(&self, new_windows: Vec<agent::TmuxWindowInfo>) {
        let mut last = self.last_tmux_windows.lock().unwrap();

        // Safety: if tmux returns empty but we previously had windows,
        // treat it as a transient failure and skip the update.
        // This prevents a single tmux command failure from wiping all state.
        if new_windows.is_empty() && !last.is_empty() {
            tracing::warn!(
                "tmux returned 0 windows but {} were cached — ignoring transient failure",
                last.len()
            );
            return;
        }

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

            // Clean up stale tasks whose session/window no longer exist in tmux
            self.cleanup_stale_tasks(&new_windows);

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

            // Resolve git_dir for project tagging
            let git_dir = old_win.git_dir.clone()
                .or_else(|| agent::TmuxAgent::find_git_root_sync(&working_dir))
                .unwrap_or_default();

            // Save to global DB with git_dir tag
            let db = &self.state.lock().unwrap().db;
            if let Err(e) = db.save_closed_window(
                &old_win.session_id,
                &old_win.session_name,
                &old_win.window_name,
                &working_dir,
                &git_branch,
                pane_count,
                &git_dir,
            ) {
                warn!("Failed to auto-save closed window: {}", e);
            }

            // Register project in global DB if git_dir available
            if !git_dir.is_empty() {
                let project_name = std::path::Path::new(&git_dir)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let state = self.state.lock().unwrap();
                let _ = state.db.register_project(&git_dir, &project_name);
                let _ = state.db.update_project_activity(&git_dir, &old_win.session_name, &old_win.window_name);
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

    /// Clean up stale tasks whose tmux session or window no longer exists.
    /// This prevents ghost icons in the tmux status bar.
    fn cleanup_stale_tasks(&self, current_windows: &[agent::TmuxWindowInfo]) {
        // Build sets of live session_ids and (session_id, window_id) pairs
        let live_sessions: std::collections::HashSet<&str> = current_windows.iter()
            .map(|w| w.session_id.as_str()).collect();
        let live_windows: std::collections::HashSet<(&str, &str)> = current_windows.iter()
            .map(|w| (w.session_id.as_str(), w.window_id.as_str())).collect();

        let mut state = self.state.lock().unwrap();
        let stale_keys: Vec<String> = state.tasks.iter()
            .filter(|(_, task)| {
                // Task is stale if its session doesn't exist OR its window doesn't exist
                !live_sessions.contains(task.session_id.as_str())
                    || !live_windows.contains(&(task.session_id.as_str(), task.window_id.as_str()))
            })
            .map(|(key, _)| key.clone())
            .collect();

        for key in &stale_keys {
            if let Some(task) = state.tasks.remove(key) {
                info!(
                    "Cleaned up stale task: session={} window={} (id={}) status={}",
                    task.session, task.window, task.window_id, task.status.as_str()
                );
                // Archive to history before deleting (no git_dir for stale tasks)
                let _ = state.db.archive_to_history(&task, "");
                let _ = state.db.delete_task(key);
            }
        }
    }
}

/// Auto-fix stale awaiting_input tasks by checking if Claude is actually active.
/// When PermissionRequest hook fires, the task becomes awaiting_input. But after
/// the user grants permission, there's no hook to set it back to in_progress.
/// This function detects that Claude is actively working and promotes the task.
async fn autofix_stale_awaiting_tasks(app_state: &AppState) {
    // Collect awaiting_input tasks (session name + window ID + window name + task key)
    let awaiting: Vec<(String, String, String, String)> = {
        let state = app_state.state.lock().unwrap();
        state.tasks.iter()
            .filter(|(_, task)| matches!(task.status, TaskStatus::AwaitingInput))
            .map(|(key, task)| (
                task.session.clone(),
                task.window_id.clone(),
                task.window.clone(),
                key.clone(),
            ))
            .collect()
    };

    if awaiting.is_empty() {
        return;
    }

    let mut promoted = false;

    for (session_name, window_id, window_name, task_key) in &awaiting {
        // Skip if we don't have session name
        if session_name.is_empty() {
            continue;
        }

        // Prefer window_id (e.g., "@2") for reliable targeting — window names
        // with dots (e.g., "2.1.45") cause tmux to misparse them.
        let window_target = if !window_id.is_empty() {
            window_id.as_str()
        } else if !window_name.is_empty() {
            window_name.as_str()
        } else {
            continue;
        };

        // Check if Claude is actively working in this window
        match agent::TmuxAgent::get_claude_status(session_name, window_target).await {
            Ok(status) => {
                // If Claude has an agent_type detected, it's running.
                // If it also has a current_tool or action, it's actively working.
                // But if a permission prompt is showing, it's genuinely waiting.
                let is_active = status.agent_type.is_some()
                    && (status.current_tool.is_some() || status.action.is_some())
                    && !status.awaiting_permission;

                if is_active {
                    let mut state = app_state.state.lock().unwrap();
                    if let Some(task) = state.tasks.get_mut(task_key) {
                        if matches!(task.status, TaskStatus::AwaitingInput) {
                            info!(
                                "Auto-fix: promoting awaiting_input → in_progress for {}:{} (claude active, tool={:?})",
                                session_name, window_name, status.current_tool
                            );
                            task.status = TaskStatus::InProgress;
                            let task_clone = task.clone();
                            if let Err(e) = state.db.save_task(&task_clone) {
                                error!("Failed to save auto-fixed task: {}", e);
                            }
                            promoted = true;
                        }
                    }
                }
            }
            Err(e) => {
                debug!("Auto-fix: failed to get claude status for {}:{}: {}", session_name, window_target, e);
            }
        }
    }

    if promoted {
        app_state.broadcast_state();
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
pub(crate) struct CommandResponse {
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

/// Resolved tmux context from a pane ID
#[derive(Debug, Clone, Default)]
struct TmuxContext {
    session_id: String,
    session_name: String,
    window_id: String,
    window_name: String,
    pane_id: String,
}

/// Resolve tmux context from a pane ID (e.g. "%42")
/// Runs tmux display-message synchronously — call via spawn_blocking
fn resolve_tmux_context(pane: &str) -> Option<TmuxContext> {
    if pane.is_empty() {
        return None;
    }
    let output = std::process::Command::new(agent::TMUX_BIN.as_str())
        .args([
            "-S", agent::TMUX_SOCKET.as_str(),
            "display-message",
            "-t", pane,
            "-p",
            "#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{pane_id}",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parts: Vec<&str> = line.splitn(5, '|').collect();
    if parts.len() < 5 {
        return None;
    }

    Some(TmuxContext {
        session_id: parts[0].to_string(),
        session_name: parts[1].to_string(),
        window_id: parts[2].to_string(),
        window_name: parts[3].to_string(),
        pane_id: parts[4].to_string(),
    })
}

/// Extract last assistant message from a Claude Code JSONL transcript file.
/// Reads last ~32KB, finds last type=assistant entry, extracts text content.
/// Truncates to max_chars. Call via spawn_blocking.
fn extract_last_assistant_message(transcript_path: &str, max_chars: usize) -> String {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = std::fs::File::open(transcript_path) else {
        return String::new();
    };
    let Ok(meta) = file.metadata() else {
        return String::new();
    };

    // Read last 32KB
    let size = meta.len();
    let read_from = if size > 32768 { size - 32768 } else { 0 };
    if read_from > 0 {
        let _ = file.seek(SeekFrom::Start(read_from));
    }
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return String::new();
    }

    // Find last assistant message with text content
    let mut last_text = String::new();
    for line in buf.lines() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                // Extract text from message.content[] array
                if let Some(content) = val
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                {
                    let texts: Vec<&str> = content
                        .iter()
                        .filter(|c| c.get("type").and_then(|t| t.as_str()) == Some("text"))
                        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                        .collect();
                    if !texts.is_empty() {
                        last_text = texts.join(" ");
                    }
                }
            }
        }
    }

    // Clean up: replace newlines with spaces, truncate
    let clean = last_text.replace('\n', " ");
    if clean.len() > max_chars {
        clean.chars().take(max_chars).collect()
    } else {
        clean
    }
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

            // Remove completed task from memory and DB
            // Also archive to per-project DB if git_dir can be resolved
            if let Some(mut task) = state.tasks.remove(&key) {
                // Set completion_note from the summary (last assistant message)
                if !req.summary.is_empty() {
                    task.completion_note = req.summary.clone();
                }
                task.completed_at = Some(Utc::now());
                if let Some(started) = task.started_at {
                    task.duration_seconds = (Utc::now() - started).num_seconds() as f64;
                }
                if !req.transcript_path.is_empty() {
                    task.transcript_path = req.transcript_path.clone();
                }
                task.status = TaskStatus::Completed;

                // Link to active todo if this pane has one mapped
                if let Ok(map) = app_state.active_todo_map.lock() {
                    if let Some((tid, _)) = map.get(&task.pane) {
                        task.todo_id = Some(*tid);
                        info!("FINISH_TASK: linked to todo #{}", tid);
                    }
                }

                // Resolve git_dir before archiving
                let git_dir = app_state.resolve_git_dir_for_window(&task.session_id, &task.window_id)
                    .unwrap_or_default();

                if let Err(e) = state.db.delete_task(&key) {
                    error!("Failed to delete finished task: {}", e);
                }
                // Archive with full completion data and git_dir tag
                if let Err(e) = state.db.archive_to_history(&task, &git_dir) {
                    error!("Failed to archive task to history: {}", e);
                }

                // Register project activity in global DB
                if !git_dir.is_empty() {
                    let project_name = std::path::Path::new(&git_dir)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let _ = state.db.register_project(&git_dir, &project_name);
                    let _ = state.db.update_project_activity(&git_dir, &task.session, &task.window);
                }
                drop(state);
            } else {
                drop(state);
            }
            app_state.broadcast_state();
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

            // Resolve git_dir for project tagging
            let git_dir = app_state.resolve_git_dir_for_window(&req.session_id, &req.window_id)
                .unwrap_or_default();

            let mut state = app_state.state.lock().unwrap();
            if let Err(e) = state.db.save_note(&note, &git_dir) {
                error!("Failed to save note to database: {}", e);
            }
            state.notes.insert(note.id.clone(), note.clone());

            // Register project activity
            if !git_dir.is_empty() {
                let project_name = std::path::Path::new(&git_dir)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let _ = state.db.register_project(&git_dir, &project_name);
                let _ = state.db.update_project_activity(&git_dir, &req.session, &req.window);
            }
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
                if let Err(e) = state.db.save_note(&note_clone, "") {
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
                if let Err(e) = state.db.save_note(&note_clone, "") {
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

                    if let Err(e) = state.db.save_note(&note, "") {
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
                if let Err(e) = state.db.save_note(&note_clone, "") {
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

            // Resolve git_dir for project tagging
            let git_dir = app_state.resolve_git_dir_for_window(&req.session_id, &req.window_id)
                .unwrap_or_default();

            let mut state = app_state.state.lock().unwrap();
            if let Err(e) = state.db.save_goal(&goal, &git_dir) {
                error!("Failed to save goal to database: {}", e);
            }
            state.goals.insert(goal.id.clone(), goal.clone());

            // Register project activity
            if !git_dir.is_empty() {
                let project_name = std::path::Path::new(&git_dir)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let _ = state.db.register_project(&git_dir, &project_name);
                let _ = state.db.update_project_activity(&git_dir, &req.session, &req.window);
            }
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
                if let Err(e) = state.db.save_goal(&goal_clone, "") {
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
// Hook Handler (Claude Code hooks → server-side processing)
// ============================================================================

/// Parse conventional commit-style prefix from a prompt.
/// Returns (prefix_type, is_urgent, title) if matched.
/// Supports: feat, feature, fix, bug, chore, refactor, docs, perf, test, style, ci, build
fn parse_todo_prefix(prompt: &str) -> Option<(String, bool, String)> {
    // Match: type(!)?:  title
    let trimmed = prompt.trim();
    let re = regex::Regex::new(
        r"(?i)^(feat|feature|fix|bug|chore|refactor|docs|perf|test|style|ci|build)(!)?:\s*(.+)"
    ).ok()?;
    let caps = re.captures(trimmed)?;

    let prefix_type = caps.get(1)?.as_str().to_lowercase();
    // Normalize: "feature" → "feat", "bug" → "fix"
    let normalized = match prefix_type.as_str() {
        "feature" => "feat".to_string(),
        "bug" => "fix".to_string(),
        other => other.to_string(),
    };
    let is_urgent = caps.get(2).is_some();
    let title = caps.get(3)?.as_str().trim().to_string();
    if title.is_empty() {
        return None;
    }
    Some((normalized, is_urgent, title))
}

/// Resolve git root directory from a tmux pane's current working path.
fn resolve_git_dir_from_pane(pane_id: &str) -> Option<String> {
    if pane_id.is_empty() {
        return None;
    }
    // Get pane current path
    let output = std::process::Command::new(agent::TMUX_BIN.as_str())
        .args(["-S", agent::TMUX_SOCKET.as_str(), "display-message", "-p", "-t", pane_id, "#{pane_current_path}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let cwd = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if cwd.is_empty() {
        return None;
    }
    // Get git toplevel
    let git_output = std::process::Command::new("git")
        .args(["-C", &cwd, "rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !git_output.status.success() {
        return None;
    }
    let git_dir = String::from_utf8_lossy(&git_output.stdout).trim().to_string();
    if git_dir.is_empty() { None } else { Some(git_dir) }
}

/// Handle raw Claude Code hook events.
/// Replaces the shell pipeline: hook → jq → agent-event.sh → curl /api/command
/// Now: hook → curl /api/hook (server does tmux resolution + transcript parsing)
async fn handle_hook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<CommandResponse>, StatusCode> {
    let event = headers
        .get("x-hook-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let tmux_pane = headers
        .get("x-tmux-pane")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Parse body as JSON (best-effort, some events may have empty body)
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();

    // Resolve tmux context from pane ID
    let pane_for_tmux = tmux_pane.clone();
    let tmux_ctx = tokio::task::spawn_blocking(move || resolve_tmux_context(&pane_for_tmux))
        .await
        .unwrap_or(None)
        .unwrap_or_default();

    // Auto-capture todo from conventional commit prefixes in UserPromptSubmit
    let mut captured_todo_id: Option<i64> = None;
    if event == "UserPromptSubmit" {
        let prompt_raw = body_json.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
        if let Some((prefix, is_urgent, title)) = parse_todo_prefix(prompt_raw) {
            let pane_for_git = tmux_ctx.pane_id.clone();
            let state_ref = state.clone();
            captured_todo_id = tokio::task::spawn_blocking(move || {
                let git_dir = match resolve_git_dir_from_pane(&pane_for_git) {
                    Some(d) => d,
                    None => {
                        info!("todo: no git_dir for pane '{}'", pane_for_git);
                        return None;
                    }
                };
                let priority = if is_urgent { 2 } else if prefix == "fix" { 1 } else { 0 };
                let todo_title = format!("[{}{}] {}", prefix, if is_urgent { "!" } else { "" }, title);

                let todo_id = {
                    let server_state = state_ref.state.lock().unwrap();
                    let existing = server_state.db.list_project_todos(&git_dir)
                        .unwrap_or_default()
                        .into_iter()
                        .find(|t| t.title == todo_title && t.status != "done");
                    if let Some(t) = existing {
                        info!("todo: duplicate '{}', linking pane", todo_title);
                        Some(t.id)
                    } else {
                        match server_state.db.create_project_todo(&git_dir, &todo_title, "", "todo", priority) {
                            Ok(id) => {
                                info!("todo: created #{} '{}' p={} in {}", id, todo_title, priority, git_dir);
                                Some(id)
                            }
                            Err(e) => {
                                warn!("todo: db error: {}", e);
                                None
                            }
                        }
                    }
                };
                // Also store in active_todo_map as backup
                if let Some(tid) = todo_id {
                    let mut map = state_ref.active_todo_map.lock().unwrap();
                    map.insert(pane_for_git.clone(), (tid, git_dir));
                }
                todo_id
            }).await.unwrap_or(None);
        }
    }

    // Map event to command + extract summary
    let (command, summary, transcript_path) = match event.as_str() {
        "UserPromptSubmit" => {
            let prompt = body_json
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or("working...");
            let truncated: String = prompt.replace('\n', " ").chars().take(100).collect();
            (commands::START_TASK, truncated, String::new())
        }
        "Stop" => {
            let transcript = body_json
                .get("transcript_path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();

            // Prefer last_assistant_message from event JSON (available directly),
            // fall back to reading transcript file
            let summary = match body_json.get("last_assistant_message").and_then(|m| m.as_str()) {
                Some(msg) if !msg.is_empty() => {
                    let clean = msg.replace('\n', " ");
                    clean.chars().take(200).collect()
                }
                _ => {
                    let tp = transcript.clone();
                    tokio::task::spawn_blocking(move || {
                        extract_last_assistant_message(&tp, 200)
                    })
                    .await
                    .unwrap_or_default()
                }
            };
            (commands::FINISH_TASK, summary, transcript)
        }
        "PermissionRequest" => {
            let tool = body_json
                .get("tool")
                .or_else(|| body_json.get("tool_name"))
                .and_then(|t| t.as_str())
                .unwrap_or("权限请求");
            let summary = format!("权限: {}", tool);
            (commands::PAUSE_TASK, summary, String::new())
        }
        "Notification" => {
            (commands::FINISH_TASK, "空闲".to_string(), String::new())
        }
        _ => {
            return Ok(Json(CommandResponse {
                success: false,
                message: format!("Unknown hook event: {}", event),
            }));
        }
    };

    let pane_id = tmux_ctx.pane_id.clone();
    let task_key = format!("{}|{}|{}", tmux_ctx.session_id, tmux_ctx.window_id, tmux_ctx.pane_id);

    let cmd_req = SendCommandRequest {
        command: command.to_string(),
        session_id: tmux_ctx.session_id,
        session: tmux_ctx.session_name,
        window_id: tmux_ctx.window_id,
        window: tmux_ctx.window_name,
        pane: tmux_ctx.pane_id,
        summary,
        note_id: String::new(),
        goal_id: String::new(),
        scope: String::new(),
        transcript_path,
    };

    match handle_command(&state, cmd_req).await {
        Ok(_) => {
            // After START_TASK, persist todo_id on the task so it survives restarts
            if let Some(tid) = captured_todo_id {
                let mut server_state = state.state.lock().unwrap();
                if let Some(task) = server_state.tasks.get_mut(&task_key) {
                    task.todo_id = Some(tid);
                    let task_clone = task.clone();
                    if let Err(e) = server_state.db.save_task(&task_clone) {
                        warn!("todo: failed to persist todo_id on task: {}", e);
                    }
                    info!("todo: set todo_id={} on task '{}'", tid, task_key);
                }
            }
            Ok(Json(CommandResponse {
                success: true,
                message: format!("Hook '{}' processed", event),
            }))
        }
        Err(e) => Ok(Json(CommandResponse {
            success: false,
            message: format!("Hook '{}' failed: {}", event, e),
        })),
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
// Authentication
// ============================================================================

/// Auth middleware: validates Bearer token for /api/* and /ws paths
async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    let path = req.uri().path().to_string();

    // Skip auth for non-API paths (static files) and /health
    if !path.starts_with("/api/") && !path.starts_with("/ws") {
        return next.run(req).await;
    }
    if path == "/health" || path == "/api/health" {
        return next.run(req).await;
    }

    let token_valid = if path.starts_with("/ws") {
        // WebSocket: extract token from query string
        req.uri()
            .query()
            .and_then(|q| {
                q.split('&')
                    .find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        match (parts.next(), parts.next()) {
                            (Some("token"), Some(v)) => Some(v.to_string()),
                            _ => None,
                        }
                    })
            })
            .map(|t| t == state.auth_token)
            .unwrap_or(false)
    } else {
        // API: extract Bearer token from Authorization header
        req.headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|t| t == state.auth_token)
            .unwrap_or(false)
    };

    if token_valid {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Unauthorized", "message": "Invalid or missing auth token"})),
        )
            .into_response()
    }
}

/// Verify auth token (if middleware passes, token is valid)
async fn verify_auth() -> Json<serde_json::Value> {
    Json(serde_json::json!({"authenticated": true}))
}

// ============================================================================
// Health Check
// ============================================================================

async fn health_check(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let start = std::time::Instant::now();
    let mut checks = serde_json::Map::new();
    let mut overall = "healthy";

    // Check tmux server — offload blocking Command to spawn_blocking
    let tmux_ok = match tokio::task::spawn_blocking(|| {
        std::process::Command::new(agent::TMUX_BIN.as_str())
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()
    }).await {
        Ok(Ok(o)) => {
            let count = String::from_utf8_lossy(&o.stdout).lines().count();
            checks.insert("tmux".into(), serde_json::json!({
                "status": if o.status.success() { "ok" } else { "error" },
                "sessions": count,
            }));
            o.status.success()
        }
        _ => {
            checks.insert("tmux".into(), serde_json::json!({"status": "error", "message": "tmux check failed"}));
            false
        }
    };

    // Check database — offload blocking SQLite + mutex lock to spawn_blocking
    let state_ref = state.clone();
    let db_ok = match tokio::task::spawn_blocking(move || {
        let server_state = state_ref.state.lock().unwrap();
        server_state.db.conn.query_row("PRAGMA integrity_check", [], |row| {
            row.get::<_, String>(0)
        }).map(|result| {
            let db_path = db::default_db_path();
            let size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
            (result, size)
        })
    }).await {
        Ok(Ok((result, size))) if result == "ok" => {
            let size_str = if size > 1024 * 1024 {
                format!("{:.1}MB", size as f64 / (1024.0 * 1024.0))
            } else {
                format!("{:.0}KB", size as f64 / 1024.0)
            };
            checks.insert("database".into(), serde_json::json!({"status": "ok", "size": size_str}));
            true
        }
        Ok(Ok((result, _))) => {
            checks.insert("database".into(), serde_json::json!({"status": "degraded", "message": result}));
            false
        }
        Ok(Err(e)) => {
            checks.insert("database".into(), serde_json::json!({"status": "error", "message": e.to_string()}));
            false
        }
        Err(e) => {
            checks.insert("database".into(), serde_json::json!({"status": "error", "message": e.to_string()}));
            false
        }
    };

    // Uptime (from process start)
    let uptime_secs = state.start_time.elapsed().as_secs();
    let uptime_str = if uptime_secs >= 86400 {
        format!("{}d {}h {}m", uptime_secs / 86400, (uptime_secs % 86400) / 3600, (uptime_secs % 3600) / 60)
    } else if uptime_secs >= 3600 {
        format!("{}h {}m", uptime_secs / 3600, (uptime_secs % 3600) / 60)
    } else {
        format!("{}m {}s", uptime_secs / 60, uptime_secs % 60)
    };
    checks.insert("uptime".into(), serde_json::json!(uptime_str));

    // Response time
    let response_ms = start.elapsed().as_millis();

    if !tmux_ok || !db_ok {
        overall = if tmux_ok || db_ok { "degraded" } else { "unhealthy" };
    }

    Json(serde_json::json!({
        "status": overall,
        "checks": checks,
        "response_ms": response_ms,
    }))
}

// ============================================================================
// Diagnostics
// ============================================================================

async fn diagnostics(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let start = std::time::Instant::now();
    let mut components = Vec::<serde_json::Value>::new();
    let mut has_error = false;
    let mut has_warning = false;

    let config_dir = state.paths.data_dir.to_string_lossy().to_string();

    // 1. Server: PID, version, uptime, memory
    {
        let pid = std::process::id();
        let version = env!("CARGO_PKG_VERSION");
        let uptime_secs = state.start_time.elapsed().as_secs();
        let uptime_str = if uptime_secs >= 86400 {
            format!("{}d {}h {}m", uptime_secs / 86400, (uptime_secs % 86400) / 3600, (uptime_secs % 3600) / 60)
        } else if uptime_secs >= 3600 {
            format!("{}h {}m", uptime_secs / 3600, (uptime_secs % 3600) / 60)
        } else {
            format!("{}m {}s", uptime_secs / 60, uptime_secs % 60)
        };
        // Memory: read from /proc on Linux, use ps on macOS
        let mem_str = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|kb| {
                if kb > 1024 { format!("{:.1}MB", kb as f64 / 1024.0) }
                else { format!("{}KB", kb) }
            })
            .unwrap_or_else(|| "unknown".to_string());

        components.push(serde_json::json!({
            "name": "server",
            "status": "ok",
            "detail": format!("PID {}, v{}, uptime {}, mem {}", pid, version, uptime_str, mem_str)
        }));
    }

    // 2. Tmux: server status, session count, window count
    {
        match std::process::Command::new(agent::TMUX_BIN.as_str())
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()
        {
            Ok(o) if o.status.success() => {
                let sessions = String::from_utf8_lossy(&o.stdout)
                    .lines().filter(|l| !l.is_empty()).count();
                let windows = std::process::Command::new(agent::TMUX_BIN.as_str())
                    .args(["list-windows", "-a", "-F", "#{window_id}"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.is_empty()).count())
                    .unwrap_or(0);
                components.push(serde_json::json!({
                    "name": "tmux",
                    "status": "ok",
                    "detail": format!("{} sessions, {} windows", sessions, windows)
                }));
            }
            _ => {
                has_warning = true;
                components.push(serde_json::json!({
                    "name": "tmux",
                    "status": "warning",
                    "detail": "tmux server not running"
                }));
            }
        }
    }

    // 3. Database: integrity, file size, table count
    {
        let server_state = state.state.lock().unwrap();
        let db_path = db::default_db_path();
        let integrity = server_state.db.conn.query_row(
            "PRAGMA integrity_check", [], |row| row.get::<_, String>(0)
        );
        let table_count: usize = server_state.db.conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'", [], |row| row.get(0)
        ).unwrap_or(0);
        drop(server_state);

        let size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        let size_str = if size > 1024 * 1024 {
            format!("{:.1}MB", size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.0}KB", size as f64 / 1024.0)
        };

        match integrity {
            Ok(ref s) if s == "ok" => {
                components.push(serde_json::json!({
                    "name": "database",
                    "status": "ok",
                    "detail": format!("integrity OK, {}, {} tables", size_str, table_count)
                }));
            }
            Ok(s) => {
                has_warning = true;
                components.push(serde_json::json!({
                    "name": "database",
                    "status": "warning",
                    "detail": format!("integrity: {}, {}", s, size_str)
                }));
            }
            Err(e) => {
                has_error = true;
                components.push(serde_json::json!({
                    "name": "database",
                    "status": "error",
                    "detail": format!("integrity check failed: {}", e)
                }));
            }
        }
    }

    // 4. WebSocket: active connections via broadcast receiver count
    {
        let count = state.broadcast_tx.receiver_count();
        components.push(serde_json::json!({
            "name": "websocket",
            "status": "ok",
            "detail": format!("{} active connections", count)
        }));
    }

    // 5. Hooks: check agent-event.sh
    {
        let hook_path = state.paths.scripts_dir.join("agent-event.sh").to_string_lossy().to_string();
        let path = std::path::Path::new(&hook_path);
        if path.exists() {
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(path)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
            };
            if executable {
                components.push(serde_json::json!({
                    "name": "hooks",
                    "status": "ok",
                    "detail": "agent-event.sh present and executable"
                }));
            } else {
                has_warning = true;
                components.push(serde_json::json!({
                    "name": "hooks",
                    "status": "warning",
                    "detail": "agent-event.sh exists but not executable"
                }));
            }
        } else {
            has_warning = true;
            components.push(serde_json::json!({
                "name": "hooks",
                "status": "warning",
                "detail": "agent-event.sh not found"
            }));
        }
    }

    // 6. Disk: config directory total size
    {
        let du_output = std::process::Command::new("du")
            .args(["-sk", &config_dir])
            .output();
        let size_str = du_output.ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.split_whitespace().next().and_then(|n| n.parse::<u64>().ok()))
            .map(|kb| {
                if kb > 1024 * 1024 { format!("{:.1}GB", kb as f64 / (1024.0 * 1024.0)) }
                else if kb > 1024 { format!("{:.1}MB", kb as f64 / 1024.0) }
                else { format!("{}KB", kb) }
            })
            .unwrap_or_else(|| "unknown".to_string());

        components.push(serde_json::json!({
            "name": "disk",
            "status": "ok",
            "detail": format!("{} total", size_str)
        }));
    }

    // 7. Logs: file size, recent entry time
    {
        let log_path_str = state.paths.log_path.to_string_lossy().to_string();
        let log_path = if std::path::Path::new(&log_path_str).exists() {
            Some(log_path_str.clone())
        } else {
            let legacy = "/opt/homebrew/var/log/agent-tracker-server.log".to_string();
            if std::path::Path::new(&legacy).exists() { Some(legacy) } else { None }
        };
        match log_path {
            Some(ref p) => {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                let size_str = if size > 1024 * 1024 {
                    format!("{:.1}MB", size as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{:.0}KB", size as f64 / 1024.0)
                };
                let modified = std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|t| {
                        let dt: chrono::DateTime<chrono::Utc> = t.into();
                        dt.format("%H:%M:%S").to_string()
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                components.push(serde_json::json!({
                    "name": "logs",
                    "status": "ok",
                    "detail": format!("{}, last write {}", size_str, modified)
                }));
            }
            None => {
                components.push(serde_json::json!({
                    "name": "logs",
                    "status": "warning",
                    "detail": "log file not found"
                }));
                has_warning = true;
            }
        }
    }

    let overall = if has_error { "unhealthy" }
        else if has_warning { "degraded" }
        else { "healthy" };

    let response_ms = start.elapsed().as_millis();

    Json(serde_json::json!({
        "status": overall,
        "components": components,
        "timestamp": Utc::now().to_rfc3339(),
        "response_ms": response_ms,
    }))
}

// ============================================================================
// Admin
// ============================================================================

async fn admin_restart() -> Json<serde_json::Value> {
    info!("Admin restart requested — shutting down for launchd restart");
    // Spawn a delayed exit so we can return the response first
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        std::process::exit(0);
    });
    Json(serde_json::json!({
        "success": true,
        "message": "Restart requested — server shutting down"
    }))
}

async fn admin_clear_logs(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let log_path_str = state.paths.log_path.to_string_lossy().to_string();
    let log_paths = [
        log_path_str,
        "/opt/homebrew/var/log/agent-tracker-server.log".to_string(),
    ];

    let log_path = log_paths.iter().find(|p| std::path::Path::new(p).exists());
    match log_path {
        Some(p) => {
            let before = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            match std::fs::write(p, "") {
                Ok(_) => {
                    info!("Admin clear-logs: cleared {} bytes from {}", before, p);
                    Json(serde_json::json!({
                        "success": true,
                        "before_bytes": before,
                        "after_bytes": 0,
                        "path": p,
                    }))
                }
                Err(e) => Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to clear log: {}", e),
                })),
            }
        }
        None => Json(serde_json::json!({
            "success": false,
            "error": "Log file not found",
        })),
    }
}

// ============================================================================
// Log Viewer
// ============================================================================

#[derive(Deserialize)]
struct LogQuery {
    #[serde(default = "default_log_limit")]
    limit: usize,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    search: Option<String>,
}

fn default_log_limit() -> usize { 100 }

#[derive(Serialize)]
struct LogEntry {
    timestamp: String,
    level: String,
    module: String,
    message: String,
}

async fn get_logs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LogQuery>,
) -> Json<serde_json::Value> {
    // Try known log file locations
    let log_path_str = state.paths.log_path.to_string_lossy().to_string();
    let log_paths = [
        log_path_str,
        "/opt/homebrew/var/log/agent-tracker-server.log".to_string(),
    ];

    let log_path = log_paths.iter().find(|p| std::path::Path::new(p).exists());
    let Some(log_path) = log_path else {
        return Json(serde_json::json!({ "entries": [], "error": "Log file not found" }));
    };

    // Read last N*3 lines (since we filter, read more than needed)
    let read_limit = params.limit * 3;
    let output = std::process::Command::new("tail")
        .args(["-n", &read_limit.to_string(), log_path])
        .output();

    let Ok(output) = output else {
        return Json(serde_json::json!({ "entries": [], "error": "Failed to read log file" }));
    };

    let content = String::from_utf8_lossy(&output.stdout);
    // Strip ANSI escape codes
    let ansi_re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();

    let mut entries: Vec<LogEntry> = Vec::new();
    let level_filter: Option<String> = params.level.as_deref().map(|l: &str| l.to_uppercase());
    let search_filter: Option<String> = params.search.as_deref().map(|s: &str| s.to_lowercase());

    for line in content.lines() {
        let clean = ansi_re.replace_all(line, "").to_string();
        // Parse tracing format: "2026-02-16T01:09:38.608110Z  INFO tracker_server::agent: message"
        // Use split_whitespace to handle variable spacing
        let parts: Vec<&str> = clean.splitn(4, |c: char| c.is_whitespace()).filter(|s| !s.is_empty()).collect();
        if parts.len() < 3 { continue; }

        let timestamp = parts[0].to_string();
        let level = parts[1].to_string();
        let rest = if parts.len() >= 3 { parts[2..].join(" ") } else { String::new() };
        let (module, message) = if let Some(idx) = rest.find(": ") {
            (rest[..idx].to_string(), rest[idx+2..].to_string())
        } else {
            (String::new(), rest)
        };

        // Apply filters
        if let Some(ref lf) = level_filter {
            if &level != lf { continue; }
        }
        if let Some(ref sf) = search_filter {
            if !message.to_lowercase().contains(sf) && !module.to_lowercase().contains(sf) { continue; }
        }

        entries.push(LogEntry { timestamp, level, module, message });
    }

    // Take last N entries (most recent)
    let total = entries.len();
    let entries: Vec<LogEntry> = entries.into_iter().rev().take(params.limit).collect::<Vec<_>>().into_iter().rev().collect();

    Json(serde_json::json!({
        "entries": entries,
        "total": total,
    }))
}

// ============================================================================
// Notification Handlers
// ============================================================================

#[derive(Deserialize)]
struct NotificationQuery {
    #[serde(default)]
    unread_only: Option<bool>,
    #[serde(default = "default_notification_limit")]
    limit: i32,
}

fn default_notification_limit() -> i32 { 50 }

async fn list_notifications(
    Query(params): Query<NotificationQuery>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let server = state.state.lock().unwrap();
    let unread = params.unread_only.unwrap_or(false);
    match server.db.list_notifications(unread, params.limit) {
        Ok(notifications) => Json(serde_json::json!({ "notifications": notifications })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

async fn mark_notification_read(
    axum::extract::Path(id): axum::extract::Path<i64>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let server = state.state.lock().unwrap();
    match server.db.mark_notification_read(id) {
        Ok(_) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

async fn mark_all_notifications_read(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let server = state.state.lock().unwrap();
    match server.db.mark_all_notifications_read() {
        Ok(_) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

async fn unread_notification_count(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let server = state.state.lock().unwrap();
    match server.db.unread_notification_count() {
        Ok(count) => Json(serde_json::json!({ "count": count })),
        Err(e) => Json(serde_json::json!({ "count": 0, "error": e.to_string() })),
    }
}

// ============================================================================
// Alert Rule Handlers
// ============================================================================

#[derive(Deserialize)]
struct CreateAlertRuleRequest {
    name: String,
    condition_type: String,
    threshold_seconds: Option<i32>,
    #[serde(default = "default_channels")]
    channels: String,
}

fn default_channels() -> String { "web".to_string() }

#[derive(Deserialize)]
struct UpdateAlertRuleRequest {
    enabled: Option<bool>,
    threshold_seconds: Option<i32>,
    channels: Option<String>,
}

async fn list_alert_rules(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let server = state.state.lock().unwrap();
    match server.db.list_alert_rules() {
        Ok(rules) => Json(serde_json::json!({ "rules": rules })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

async fn create_alert_rule(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateAlertRuleRequest>,
) -> Json<serde_json::Value> {
    let server = state.state.lock().unwrap();
    match server.db.create_alert_rule(&req.name, &req.condition_type, req.threshold_seconds, &req.channels) {
        Ok(id) => Json(serde_json::json!({ "success": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

async fn update_alert_rule(
    axum::extract::Path(id): axum::extract::Path<i64>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateAlertRuleRequest>,
) -> Json<serde_json::Value> {
    let server = state.state.lock().unwrap();
    match server.db.update_alert_rule(id, req.enabled, req.threshold_seconds, req.channels.as_deref()) {
        Ok(_) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

async fn delete_alert_rule(
    axum::extract::Path(id): axum::extract::Path<i64>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let server = state.state.lock().unwrap();
    match server.db.delete_alert_rule(id) {
        Ok(_) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

// ============================================================================
// Backup Handlers
// ============================================================================

async fn create_backup(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let backup_dir = state.paths.backup_dir.to_string_lossy().to_string();
    let _ = std::fs::create_dir_all(&backup_dir);

    let now = chrono::Utc::now().format("%Y-%m-%d_%H%M%S");
    let backup_path = format!("{}/tracker-{}.db", backup_dir, now);

    let server = state.state.lock().unwrap();
    match server.db.backup_to(&backup_path) {
        Ok(_) => Json(serde_json::json!({ "success": true, "path": backup_path })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

async fn list_backups(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let backup_dir = state.paths.backup_dir.to_string_lossy().to_string();

    let entries = match std::fs::read_dir(&backup_dir) {
        Ok(e) => e,
        Err(_) => return Json(serde_json::json!({ "backups": [] })),
    };

    let mut backups: Vec<serde_json::Value> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "db"))
        .map(|e| {
            let meta = e.metadata().ok();
            serde_json::json!({
                "name": e.file_name().to_string_lossy(),
                "path": e.path().to_string_lossy(),
                "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                "created": meta.and_then(|m| m.modified().ok())
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_default(),
            })
        })
        .collect();

    backups.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
    Json(serde_json::json!({ "backups": backups }))
}

// ============================================================================
// WebSocket Handler
// ============================================================================

/// Generate a short random ID for WS connection tracking
fn rand_id() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut h = s.build_hasher();
    h.write_u64(0);
    h.finish() % 100_000
}

/// Format a chrono Duration as human-readable string
fn format_duration_human(d: chrono::Duration) -> String {
    let secs = d.num_seconds();
    if secs < 60 { return format!("{}s", secs); }
    if secs < 3600 { return format!("{}m {}s", secs / 60, secs % 60); }
    format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
}

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
    let ws_id: u64 = rand_id();
    let connected_at = chrono::Utc::now();
    info!("[WS-{}] Client connected", ws_id);

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(tokio::sync::Mutex::new(sender));

    // Subscribe to realtime state updates
    let mut state_rx = state.subscribe();

    // Subscribe to stream chunks
    let mut stream_rx = state.stream_manager.subscribe();

    // Subscribe to chat message events
    let mut chat_rx = state.chat_watcher.subscribe();

    // Send initial realtime message (state + tmux windows)
    // If cached tmux windows are empty, do a live query to ensure first client gets real data
    let initial_msg = {
        let mut msg = state.get_realtime_message();
        if msg.tmux_windows.is_empty() {
            if let Ok(windows) = tokio::task::spawn_blocking(agent::TmuxAgent::list_all_windows_sync).await {
                if !windows.is_empty() {
                    state.broadcast_if_tmux_changed(windows.clone());
                    msg.tmux_windows = windows;
                }
            }
        }
        msg
    };
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
        loop {
            match state_rx.recv().await {
                Ok(msg) => {
                    let json = serde_json::to_string(&msg).unwrap_or_default();
                    let mut sender_guard = sender_clone.lock().await;
                    if sender_guard.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket state receiver lagged, skipped {} messages", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Spawn task to forward stream chunks to WebSocket
    let sender_clone2 = sender.clone();
    let stream_send_task = tokio::spawn(async move {
        loop {
            match stream_rx.recv().await {
                Ok(chunk) => {
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
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket stream receiver lagged, skipped {} messages", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Spawn task to forward chat message events to WebSocket
    let sender_clone3 = sender.clone();
    let chat_send_task = tokio::spawn(async move {
        loop {
            match chat_rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    let mut sender_guard = sender_clone3.lock().await;
                    if sender_guard.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket chat receiver lagged, skipped {} messages", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Server-side heartbeat: send Ping every 30s, disconnect if no Pong within 90s
    // Generous timeout avoids false disconnects when Tauri app is backgrounded
    let sender_ping = sender.clone();
    let ping_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await; // skip immediate first tick
        loop {
            interval.tick().await;
            let mut sender_guard = sender_ping.lock().await;
            if sender_guard.send(Message::Ping(vec![b'h', b'b'])).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages (ping/pong, close, commands)
    let mut last_pong = tokio::time::Instant::now();
    loop {
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(90));
        tokio::select! {
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(frame))) => {
                        info!("[WS-{}] Client sent Close frame: {:?}", ws_id, frame.map(|f| f.reason));
                        break;
                    }
                    None => {
                        info!("[WS-{}] Client stream ended (connection dropped)", ws_id);
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let mut sender_guard = sender.lock().await;
                        let _ = sender_guard.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_pong = tokio::time::Instant::now();
                    }
                    Some(Ok(Message::Text(text))) => {
                        // Any message counts as activity
                        last_pong = tokio::time::Instant::now();
                        if let Ok(req) = serde_json::from_str::<SendCommandRequest>(&text) {
                            let _ = handle_command(&state, req).await;
                        }
                    }
                    Some(Err(e)) => {
                        warn!("[WS-{}] Error: {}", ws_id, e);
                        break;
                    }
                    _ => {}
                }
            }
            _ = timeout => {
                // No activity for 90s — connection is dead
                if last_pong.elapsed() > std::time::Duration::from_secs(90) {
                    warn!("[WS-{}] Heartbeat timeout ({}s since last pong), closing", ws_id, last_pong.elapsed().as_secs());
                    break;
                }
            }
        }
    }

    // Clean up
    state_send_task.abort();
    stream_send_task.abort();
    chat_send_task.abort();
    ping_task.abort();
    let duration = chrono::Utc::now().signed_duration_since(connected_at);
    info!("[WS-{}] Disconnected after {}", ws_id, format_duration_human(duration));
}

/// Discover active JSONL files for chat_watcher.
/// Looks at current tasks (in_progress/awaiting_input) and finds their JSONL session files.
fn discover_active_jsonl_files(state: &AppState) -> Vec<(String, std::path::PathBuf)> {
    let claude_projects = match dirs::home_dir().map(|h| h.join(".claude/projects")) {
        Some(p) if p.exists() => p,
        _ => return vec![],
    };

    // Get active tasks with their session/window/pane info
    let server_state = state.state.lock().unwrap();
    let active_windows: Vec<(String, String, String)> = server_state.tasks.values()
        .filter(|t| matches!(t.status, TaskStatus::InProgress | TaskStatus::AwaitingInput))
        .map(|t| (t.session.clone(), t.window.clone(), t.pane.clone()))
        .collect();
    drop(server_state);

    if active_windows.is_empty() {
        return vec![];
    }

    let mut result = Vec::new();

    for (session, window, _pane) in &active_windows {
        let key = format!("{}:{}", session, window);

        // Directory-based lookup with two-tier filtering (exact paths first, then parents)
        let list_output = std::process::Command::new(agent::TMUX_BIN.as_str())
            .args(["-S", agent::TMUX_SOCKET.as_str(), "list-panes", "-t", &format!("{}:{}", session, window), "-F", "#{pane_current_path}"])
            .output();

        let mut pane_paths: Vec<String> = Vec::new();
        if let Ok(out) = list_output {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let path = line.trim().to_string();
                    if !path.is_empty() && !pane_paths.contains(&path) {
                        pane_paths.push(path);
                    }
                }
            }
        }

        // Convert paths to Claude project directory names (two-tier: exact then parents)
        let mut exact_dirs: Vec<String> = Vec::new();
        let mut parent_dirs: Vec<String> = Vec::new();
        for path in &pane_paths {
            let converted = path.replace('/', "-").replace('.', "-").replace('_', "-");
            if !exact_dirs.contains(&converted) {
                exact_dirs.push(converted);
            }
            let mut current = path.as_str();
            loop {
                match current.rfind('/') {
                    Some(pos) if pos > 0 => {
                        current = &current[..pos];
                        let parent = current.replace('/', "-").replace('.', "-").replace('_', "-");
                        if !exact_dirs.contains(&parent) && !parent_dirs.contains(&parent) {
                            parent_dirs.push(parent);
                        }
                    }
                    _ => break,
                }
            }
        }

        // Find newest JSONL file — try exact dirs first, fall back to parent dirs
        let find_best = |dirs: &[String]| -> Option<(std::path::PathBuf, std::time::SystemTime)> {
            let mut best: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
            if let Ok(entries) = std::fs::read_dir(&claude_projects) {
                for entry in entries.flatten() {
                    let project_dir = entry.path();
                    if !project_dir.is_dir() { continue; }
                    let dir_name = project_dir.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !dirs.iter().any(|f| dir_name == *f) { continue; }
                    if let Ok(files) = std::fs::read_dir(&project_dir) {
                        for file in files.flatten() {
                            let path = file.path();
                            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                                if let Ok(meta) = path.metadata() {
                                    let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                                    if best.as_ref().map_or(true, |(_, t)| mtime > *t) {
                                        best = Some((path, mtime));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            best
        };

        let best = find_best(&exact_dirs).or_else(|| find_best(&parent_dirs));
        if let Some((path, _)) = best {
            // Use full file path as key so it matches the session_file returned by
            // /api/claude/messages — the frontend filters WebSocket chat events by this key.
            let file_key = path.to_string_lossy().to_string();
            result.push((file_key, path));
        }
    }

    result
}

#[tokio::main]
async fn main() -> Result<()> {
    // Resolve and initialize paths (before logging, so we know where to write logs)
    let paths = paths::TrackerPaths::resolve();
    if let Err(e) = paths.ensure_dirs() {
        eprintln!("Failed to create data directories: {}", e);
    }

    // Initialize logging to both stdout and file
    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("tracker_server=info".parse()?);

    let file_appender = tracing_appender::rolling::never(&paths.log_dir, "tracker-server.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(non_blocking))
        .init();
    paths.migrate_if_needed();

    // Load configuration and initialize auth token
    let mut config = config::AgentConfig::load().unwrap_or_default();
    if config.auth.token.is_empty() {
        // Generate a 64-char hex token (256-bit entropy) from two UUIDs
        let token = format!(
            "{}{}",
            Uuid::new_v4().as_simple(),
            Uuid::new_v4().as_simple()
        );
        config.auth.token = token;
        if let Err(e) = config.save() {
            error!("Failed to save config with generated auth token: {}", e);
        } else {
            info!("Generated new auth token and saved to config");
        }
    }
    info!("Auth token loaded ({}...)", &config.auth.token[..8]);

    let auth_token = config.auth.token.clone();
    let allowed_origins = config.auth.allowed_origins.clone();

    // Initialize database
    let db_path = db::default_db_path();
    info!("Opening database at {:?}", db_path);
    let db = Database::open(&db_path)?;

    // Create application state
    let app_state = Arc::new(AppState::new(db, auth_token, allowed_origins, paths)?);

    // Pre-populate tmux windows before accepting connections
    // so the first WebSocket client gets actual data instead of empty list
    {
        let init_state = app_state.clone();
        if let Ok(windows) = tokio::task::spawn_blocking(agent::TmuxAgent::list_all_windows_sync).await {
            init_state.broadcast_if_tmux_changed(windows);
            info!("Pre-populated tmux windows cache");
        }
    }

    // Start background task for tmux monitoring (real-time updates)
    // Uses spawn_blocking to avoid blocking the tokio async runtime with sync tmux commands.
    let tmux_monitor_state = app_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let windows = match tokio::time::timeout(
                tokio::time::Duration::from_secs(5),
                tokio::task::spawn_blocking(agent::TmuxAgent::list_all_windows_sync),
            ).await {
                Ok(Ok(w)) => w,
                Ok(Err(e)) => {
                    tracing::error!("tmux monitor spawn_blocking failed: {}", e);
                    continue;
                }
                Err(_) => {
                    tracing::warn!("tmux monitor: list_all_windows timed out after 5s");
                    continue;
                }
            };
            tmux_monitor_state.broadcast_if_tmux_changed(windows);
        }
    });

    // Background task: auto-fix stale awaiting_input tasks
    // When PermissionRequest hook fires, task becomes awaiting_input.
    // But there's no "PermissionApproved" hook, so after permission is granted
    // and Claude resumes, the task stays stuck as awaiting_input.
    // This loop detects active Claude processes and promotes tasks back to in_progress.
    let autofix_state = app_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            autofix_stale_awaiting_tasks(&autofix_state).await;
        }
    });

    // Start background task for chat watcher (JSONL file polling → WS push)
    // Two parts: 1) poll for file changes every 500ms, 2) sync watched sessions every 5s
    // poll() does blocking file I/O + mutex lock, so offload to spawn_blocking.
    let chat_poll_state = app_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        loop {
            interval.tick().await;
            let state_ref = chat_poll_state.clone();
            let _ = tokio::task::spawn_blocking(move || {
                state_ref.chat_watcher.poll();
            }).await;
        }
    });

    let chat_sync_state = app_state.clone();
    tokio::spawn(async move {
        // Initial delay to let things settle
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            // discover_active_jsonl_files runs blocking tmux commands + mutex locks,
            // so offload to a blocking thread to avoid starving the async runtime.
            let state_ref = chat_sync_state.clone();
            let active_files = match tokio::task::spawn_blocking(move || {
                discover_active_jsonl_files(&state_ref)
            }).await {
                Ok(files) => files,
                Err(e) => {
                    tracing::error!("discover_active_jsonl_files spawn_blocking failed: {}", e);
                    continue;
                }
            };
            chat_sync_state.chat_watcher.sync_sessions(active_files);
        }
    });

    // Start background task for Claude session file scanning
    // scan_claude_sessions does heavy file I/O (reading JSONL files), so offload to blocking thread.
    let scanner_db_path = db_path.clone();
    tokio::spawn(async move {
        // Initial delay to let server start up
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let db_path = scanner_db_path.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || {
                routes_history::scan_claude_sessions(&db_path)
            }).await.unwrap_or_else(|e| Err(anyhow::anyhow!("spawn_blocking failed: {}", e))) {
                tracing::error!("Session scanner error: {}", e);
            }
        }
    });

    // Background task: check alert rules every 60s
    let alert_state = app_state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let state_ref = alert_state.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let server = state_ref.state.lock().unwrap();
                match server.db.check_alerts() {
                    Ok(new) if !new.is_empty() => {
                        info!("Alert check: {} new notifications", new.len());
                    }
                    Err(e) => {
                        debug!("Alert check error: {}", e);
                    }
                    _ => {}
                }
            }).await;
        }
    });

    // Background task: daily auto-backup
    // Offloaded to spawn_blocking because backup_to does blocking SQLite I/O.
    let backup_state = app_state.clone();
    let backup_dir_str = app_state.paths.backup_dir.to_string_lossy().to_string();
    tokio::spawn(async move {
        // Check on startup, then every 6 hours
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(6 * 3600));
        loop {
            interval.tick().await;
            let backup_dir = backup_dir_str.clone();
            let state_ref = backup_state.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                let today_backup = format!("{}/tracker-{}.db", backup_dir, today);

                if !std::path::Path::new(&today_backup).exists() {
                    let server = state_ref.state.lock().unwrap();
                    match server.db.backup_to(&today_backup) {
                        Ok(_) => info!("Daily backup created: {}", today_backup),
                        Err(e) => error!("Backup failed: {}", e),
                    }

                    // Clean up backups older than 30 days
                    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
                        let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
                        for entry in entries.filter_map(|e| e.ok()) {
                            if let Ok(meta) = entry.metadata() {
                                if let Ok(modified) = meta.modified() {
                                    let dt: chrono::DateTime<chrono::Utc> = modified.into();
                                    if dt < cutoff {
                                        let _ = std::fs::remove_file(entry.path());
                                        info!("Cleaned old backup: {}", entry.path().display());
                                    }
                                }
                            }
                        }
                    }
                }
            }).await;
        }
    });

    // Background safety net: auto-broadcast if DB changes weren't explicitly broadcast
    let safety_state = app_state.clone();
    let changed_tables = {
        let state = safety_state.state.lock().unwrap();
        state.db.changed_tables.clone()
    };
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        loop {
            interval.tick().await;
            if !changed_tables.lock().unwrap().is_empty() {
                debug!("Safety net: broadcasting un-handled DB changes");
                safety_state.broadcast_state();
            }
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
        .route("/api/hook", post(handle_hook))
        .route("/api/notes/add", post(add_note))
        .route("/api/goals/add", post(add_goal))
        // Archive/Restore
        .route("/api/task/archive", post(archive_task))
        .route("/api/task/restore", post(restore_task))
        .route("/api/note/archive", post(archive_note))
        .route("/api/note/restore", post(restore_note))
        // Sessions (from Claude JSONL files)
        .route("/api/sessions", get(routes_history::get_sessions))
        .route("/api/sessions/detail", get(routes_history::get_session_detail))
        // Projects (per-project .aitracker storage)
        .route("/api/projects", get(routes_projects::get_projects))
        .route("/api/projects/history", get(routes_projects::get_project_history))
        .route("/api/projects/history/grouped-detail", get(routes_projects::get_grouped_detail))
        // History (legacy, from hooks)
        .route("/api/history", get(routes_history::get_history))
        .route("/api/history/stats", get(routes_history::get_history_stats))
        .route("/api/history/:id/resume", post(routes_history::resume_history))
        .route("/api/history/:id/reparse", post(routes_history::reparse_history))
        .route("/api/history/:id", get(routes_history::get_history_detail))
        // Claude session
        .route("/api/claude/messages", get(routes_history::get_claude_messages))
        // Workspace management
        .route("/api/workspace/start", post(routes_workspace::start_workspace))
        .route("/api/workspace/resume", post(routes_workspace::resume_workspace))
        .route("/api/workspace/destroy", post(routes_workspace::destroy_workspace))
        .route("/api/workspace/list", get(routes_workspace::list_workspaces))
        .route("/api/workspace/activate", post(routes_workspace::activate_workspace))
        .route("/api/workspace/metadata", get(routes_workspace::get_workspace_metadata))
        .route("/api/config", get(routes_workspace::get_config))
        // Git
        .route("/api/git/branches", get(routes_workspace::list_git_branches))
        // Port management
        .route("/api/port/check/:port", get(routes_workspace::check_port))
        .route("/api/port/kill", post(routes_workspace::kill_port))
        .route("/api/port/allocate", get(routes_workspace::allocate_port))
        // Global env vars
        .route("/api/global/env-vars", get(routes_projects::list_global_env_vars).post(routes_projects::create_global_env_var))
        .route("/api/global/env-vars/:id", put(routes_projects::update_global_env_var).delete(routes_projects::delete_global_env_var))
        // Worktree env vars + effective
        .route("/api/project/worktree-env-vars", get(routes_projects::list_worktree_env_vars).post(routes_projects::create_worktree_env_var))
        .route("/api/project/worktree-env-vars/:id", put(routes_projects::update_worktree_env_var).delete(routes_projects::delete_worktree_env_var))
        .route("/api/project/effective-env-vars", get(routes_projects::get_effective_env_vars))
        // Session creation + project delete
        .route("/api/sessions/create", post(routes_projects::create_session))
        .route("/api/projects/:git_dir", delete(routes_projects::delete_project).put(routes_projects::update_project))
        .route("/api/projects/git-info", get(routes_projects::get_git_info))
        .route("/api/projects/statistics", get(routes_projects::get_project_statistics))
        .route("/api/projects/files", get(routes_projects::get_project_files))
        // Project environment & worktree isolation
        .route("/api/projects/todos", get(routes_projects::list_project_todos).post(routes_projects::create_project_todo))
        .route("/api/projects/todos/:id", put(routes_projects::update_project_todo).delete(routes_projects::delete_project_todo))
        .route("/api/projects/todos/:id/status", put(routes_projects::update_project_todo_status))
        .route("/api/projects/todos/:id/history", get(routes_projects::get_todo_history))
        .route("/api/project/env-vars", get(routes_projects::list_project_env_vars).post(routes_projects::create_project_env_var))
        .route("/api/project/env-vars/:id", put(routes_projects::update_project_env_var).delete(routes_projects::delete_project_env_var))
        .route("/api/project/services", get(routes_projects::list_project_services).post(routes_projects::create_project_service))
        .route("/api/project/services/:id", put(routes_projects::update_project_service).delete(routes_projects::delete_project_service))
        .route("/api/project/worktree-slots", get(routes_projects::list_worktree_slots).post(routes_projects::create_worktree_slot))
        .route("/api/project/worktree-slots/:id", delete(routes_projects::delete_worktree_slot))
        // Browser automation
        .route("/api/browser/open", post(routes_tmux::open_browser))
        .route("/api/browser/switch-tab", post(routes_tmux::switch_browser_tab))
        // tmux interaction
        .route("/api/tmux/send-keys", post(routes_tmux::tmux_send_keys))
        .route("/api/tmux/capture", get(routes_tmux::tmux_capture))
        .route("/api/tmux/claude-status", get(routes_tmux::get_claude_status))
        .route("/api/tmux/sessions", get(routes_tmux::tmux_list_sessions))
        .route("/api/tmux/panes", get(routes_tmux::tmux_list_panes))
        .route("/api/tmux/windows", get(routes_tmux::tmux_list_all_windows))
        .route("/api/tmux/kill-session", post(routes_tmux::tmux_kill_session))
        .route("/api/tmux/kill-window", post(routes_tmux::tmux_kill_window))
        .route("/api/tmux/closed-windows/:session", get(routes_tmux::get_closed_windows))
        .route("/api/tmux/closed-windows", delete(routes_tmux::delete_closed_window))
        .route("/api/tmux/resume-window", post(routes_tmux::resume_closed_window))
        .route("/api/tmux/send-image", post(routes_tmux::tmux_send_image).layer(DefaultBodyLimit::max(50 * 1024 * 1024)))
        .route("/api/tmux/new-window", post(routes_tmux::tmux_new_window))
        .route("/api/tmux/select-window", post(routes_tmux::tmux_select_window))
        .route("/api/tmux/swap-window", post(routes_tmux::tmux_swap_window))
        .route("/api/tmux/rename-window", post(routes_tmux::tmux_rename_window))
        .route("/api/tmux/rename-session", post(routes_tmux::tmux_rename_session))
        .route("/api/tmux/reset-layout", post(routes_tmux::tmux_reset_layout))
        // Stream (real-time pane output)
        .route("/api/stream/start", post(stream_start))
        .route("/api/stream/stop", post(stream_stop))
        .route("/api/stream/list", get(stream_list))
        // Auth
        .route("/api/auth/verify", get(verify_auth))
        // Health check (no auth required — bypassed in auth_middleware)
        .route("/api/health", get(health_check))
        .route("/api/diagnostics", get(diagnostics))
        .route("/api/logs", get(get_logs))
        // Admin
        .route("/api/admin/restart", post(admin_restart))
        .route("/api/admin/clear-logs", post(admin_clear_logs))
        // Notifications
        .route("/api/notifications", get(list_notifications))
        .route("/api/notifications/:id/read", post(mark_notification_read))
        .route("/api/notifications/read-all", post(mark_all_notifications_read))
        .route("/api/notifications/count", get(unread_notification_count))
        // Alert rules
        .route("/api/alert-rules", get(list_alert_rules).post(create_alert_rule))
        .route("/api/alert-rules/:id", put(update_alert_rule).delete(delete_alert_rule))
        // Backup
        .route("/api/backup", post(create_backup))
        .route("/api/backup/list", get(list_backups))
        // WebSocket
        .route("/ws", get(ws_handler))
        // Auth middleware (applied before CORS so preflight OPTIONS bypass auth)
        .layer(middleware::from_fn_with_state(app_state.clone(), auth_middleware))
        // CORS — configured based on allowed_origins
        .layer({
            let origins = app_state.allowed_origins.clone();
            if origins.is_empty() {
                CorsLayer::new()
                    .allow_origin(AllowOrigin::mirror_request())
                    .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
                    .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
            } else {
                let parsed: Vec<_> = origins
                    .iter()
                    .filter_map(|o| o.parse().ok())
                    .collect();
                CorsLayer::new()
                    .allow_origin(parsed)
                    .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
                    .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
            }
        })
        .with_state(app_state.clone());

    // Static file serving for web frontend
    let web_dist = if app_state.paths.web_dist_dir.exists() {
        app_state.paths.web_dist_dir.clone()
    } else {
        // Fallback: try legacy path and ./web/dist
        let legacy = paths::TrackerPaths::legacy_config_dir()
            .join("web")
            .join("dist");
        let cwd = std::path::PathBuf::from("./web/dist");
        [legacy, cwd]
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or(app_state.paths.web_dist_dir.clone())
    };

    info!("Serving static files from {:?}", web_dist);

    let index_file = web_dist.join("index.html");
    let serve_dir = ServeDir::new(&web_dist).not_found_service(ServeFile::new(&index_file));

    // Wrap static files with cache-control headers:
    // - /assets/* (content-hashed) → immutable, long cache
    // - everything else (index.html, sw.js) → no-cache, always revalidate
    use tower::ServiceBuilder;
    use tower_http::set_header::SetResponseHeaderLayer;
    use axum::http::HeaderValue;

    let cached_assets = ServeDir::new(web_dist.join("assets"));
    let assets_service = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ))
        .service(cached_assets);

    let nocache_service = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ))
        .service(serve_dir);

    // Mount assets with long cache, everything else with no-cache
    let app = app
        .nest_service("/assets", assets_service)
        .fallback_service(nocache_service);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 3099));
    info!("Starting tracker-server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
