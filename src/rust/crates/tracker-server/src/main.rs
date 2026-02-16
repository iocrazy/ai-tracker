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
mod port;
mod project_db;
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
    http::{header, Method, Request, StatusCode},
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
    /// Per-project database connection cache
    project_dbs: project_db::ProjectDbManager,
    /// Chat watcher for JSONL file monitoring → WS push
    chat_watcher: chat_watcher::ChatWatcher,
    /// Bearer token for API authentication
    auth_token: String,
    /// Allowed CORS origins
    allowed_origins: Vec<String>,
    /// Server start time (for uptime)
    start_time: std::time::Instant,
}

impl AppState {
    fn new(db: Database, auth_token: String, allowed_origins: Vec<String>) -> Result<Self> {
        let (broadcast_tx, _) = broadcast::channel(16);
        let mut state = ServerState::new(db);
        state.load_from_db()?;
        Ok(Self {
            state: Mutex::new(state),
            broadcast_tx,
            last_tmux_windows: Mutex::new(Vec::new()),
            stream_manager: stream::StreamManager::new(),
            project_dbs: project_db::ProjectDbManager::new(),
            chat_watcher: chat_watcher::ChatWatcher::new(),
            auth_token,
            allowed_origins,
            start_time: std::time::Instant::now(),
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

    /// Write to project DB if git_dir can be resolved, and register project in global DB
    fn write_to_project_db_if_possible(
        &self,
        git_dir: &str,
        session: &str,
        window: &str,
        action: impl FnOnce(&project_db::ProjectDatabase) -> anyhow::Result<()>,
    ) {
        let project_name = std::path::Path::new(git_dir)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Write to project DB first (no global lock needed)
        let counts = match self.project_dbs.get_or_open(git_dir) {
            Ok(pdb) => {
                if let Err(e) = action(&pdb) {
                    warn!("Failed to write to project DB {}: {}", git_dir, e);
                }
                Some((pdb.history_count(), pdb.notes_count(), pdb.goals_count()))
            }
            Err(e) => {
                warn!("Failed to open project DB {}: {}", git_dir, e);
                None
            }
        };

        // Single lock for all global DB updates
        let state = self.state.lock().unwrap();
        let _ = state.db.register_project(git_dir, &project_name);
        let _ = state.db.update_project_activity(git_dir, session, window);
        if let Some((h, n, g)) = counts {
            let _ = state.db.update_project_counts(git_dir, h, n, g);
        }
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
        let mut state = self.get_state_response_with_tmux_names();

        // Populate changed tables from SQLite hook tracking
        {
            let server_state = self.state.lock().unwrap();
            let changes = server_state.db.take_changes();
            if !changes.is_empty() {
                state.changed = changes.into_iter().collect();
            }
        }

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

            // Save to global DB
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

            // Also save to project DB if git_dir is available
            let git_dir = old_win.git_dir.clone()
                .or_else(|| agent::TmuxAgent::find_git_root_sync(&working_dir));
            if let Some(ref gd) = git_dir {
                self.write_to_project_db_if_possible(gd, &old_win.session_name, &old_win.window_name, |pdb| {
                    pdb.save_closed_window(
                        &old_win.session_id,
                        &old_win.session_name,
                        &old_win.window_name,
                        &working_dir,
                        &git_branch,
                        pane_count,
                    )
                });
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
                // Archive to history before deleting
                let _ = state.db.archive_to_history(&task);
                let _ = state.db.delete_task(key);
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
            if let Some(task) = state.tasks.remove(&key) {
                if let Err(e) = state.db.delete_task(&key) {
                    error!("Failed to delete finished task: {}", e);
                }
                let task_clone = task.clone();
                let session = task.session.clone();
                let window = task.window.clone();
                let session_id = task.session_id.clone();
                let window_id = task.window_id.clone();
                drop(state);

                // Try to archive to project DB
                let git_dir = app_state.resolve_git_dir_for_window(&session_id, &window_id);
                if let Some(ref gd) = git_dir {
                    app_state.write_to_project_db_if_possible(gd, &session, &window, |pdb| {
                        pdb.archive_to_history(&task_clone)?;
                        Ok(())
                    });
                }
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

            let mut state = app_state.state.lock().unwrap();
            if let Err(e) = state.db.save_note(&note) {
                error!("Failed to save note to database: {}", e);
            }
            state.notes.insert(note.id.clone(), note.clone());
            drop(state);

            // Also save to project DB
            let git_dir = app_state.resolve_git_dir_for_window(&req.session_id, &req.window_id);
            if let Some(ref gd) = git_dir {
                app_state.write_to_project_db_if_possible(gd, &req.session, &req.window, |pdb| {
                    pdb.save_note(&note)?;
                    Ok(())
                });
            }

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
            state.goals.insert(goal.id.clone(), goal.clone());
            drop(state);

            // Also save to project DB
            let git_dir = app_state.resolve_git_dir_for_window(&req.session_id, &req.window_id);
            if let Some(ref gd) = git_dir {
                app_state.write_to_project_db_if_possible(gd, &req.session, &req.window, |pdb| {
                    pdb.save_goal(&goal)?;
                    Ok(())
                });
            }

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

    // Check tmux server
    let tmux_ok = std::process::Command::new(agent::TMUX_BIN)
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .map(|o| {
            let count = String::from_utf8_lossy(&o.stdout).lines().count();
            checks.insert("tmux".into(), serde_json::json!({
                "status": if o.status.success() { "ok" } else { "error" },
                "sessions": count,
            }));
            o.status.success()
        })
        .unwrap_or_else(|_| {
            checks.insert("tmux".into(), serde_json::json!({"status": "error", "message": "tmux not found"}));
            false
        });

    // Check database
    let db_ok = {
        let server_state = state.state.lock().unwrap();
        match server_state.db.conn.query_row("PRAGMA integrity_check", [], |row| {
            row.get::<_, String>(0)
        }) {
            Ok(result) if result == "ok" => {
                let db_path = db::default_db_path();
                let size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
                let size_str = if size > 1024 * 1024 {
                    format!("{:.1}MB", size as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{:.0}KB", size as f64 / 1024.0)
                };
                checks.insert("database".into(), serde_json::json!({"status": "ok", "size": size_str}));
                true
            }
            Ok(result) => {
                checks.insert("database".into(), serde_json::json!({"status": "degraded", "message": result}));
                false
            }
            Err(e) => {
                checks.insert("database".into(), serde_json::json!({"status": "error", "message": e.to_string()}));
                false
            }
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
    Query(params): Query<LogQuery>,
) -> Json<serde_json::Value> {
    // Try known log file locations
    let log_paths = [
        "/opt/homebrew/var/log/agent-tracker-server.log",
        &format!("{}/.config/agent-tracker/logs/tracker-server.log", std::env::var("HOME").unwrap_or_default()),
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
    let home = std::env::var("HOME").unwrap_or_default();
    let backup_dir = format!("{}/.config/agent-tracker/backups", home);
    let _ = std::fs::create_dir_all(&backup_dir);

    let now = chrono::Utc::now().format("%Y-%m-%d_%H%M%S");
    let backup_path = format!("{}/tracker-{}.db", backup_dir, now);

    let server = state.state.lock().unwrap();
    match server.db.backup_to(&backup_path) {
        Ok(_) => Json(serde_json::json!({ "success": true, "path": backup_path })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

async fn list_backups() -> Json<serde_json::Value> {
    let home = std::env::var("HOME").unwrap_or_default();
    let backup_dir = format!("{}/.config/agent-tracker/backups", home);

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

    // Subscribe to chat message events
    let mut chat_rx = state.chat_watcher.subscribe();

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

    // Spawn task to forward chat message events to WebSocket
    let sender_clone3 = sender.clone();
    let chat_send_task = tokio::spawn(async move {
        while let Ok(event) = chat_rx.recv().await {
            let json = serde_json::to_string(&event).unwrap_or_default();
            let mut sender_guard = sender_clone3.lock().await;
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
    chat_send_task.abort();
    info!("WebSocket connection closed");
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
        let list_output = std::process::Command::new("tmux")
            .args(["list-panes", "-t", &format!("{}:{}", session, window), "-F", "#{pane_current_path}"])
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
            result.push((key, path));
        }
    }

    result
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tracker_server=info".parse()?),
        )
        .init();

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
    let app_state = Arc::new(AppState::new(db, auth_token, allowed_origins)?);

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

    // Start background task for chat watcher (JSONL file polling → WS push)
    // Two parts: 1) poll for file changes every 500ms, 2) sync watched sessions every 5s
    let chat_poll_state = app_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        loop {
            interval.tick().await;
            chat_poll_state.chat_watcher.poll();
        }
    });

    let chat_sync_state = app_state.clone();
    tokio::spawn(async move {
        // Initial delay to let things settle
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            // Discover active sessions and their JSONL files
            let active_files = discover_active_jsonl_files(&chat_sync_state);
            chat_sync_state.chat_watcher.sync_sessions(active_files);
        }
    });

    // Start background task for Claude session file scanning
    let scanner_db_path = db_path.clone();
    tokio::spawn(async move {
        // Initial delay to let server start up
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(e) = routes_history::scan_claude_sessions(&scanner_db_path) {
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
            let server = alert_state.state.lock().unwrap();
            match server.db.check_alerts() {
                Ok(new) if !new.is_empty() => {
                    info!("Alert check: {} new notifications", new.len());
                }
                Err(e) => {
                    debug!("Alert check error: {}", e);
                }
                _ => {}
            }
        }
    });

    // Background task: daily auto-backup
    let backup_state = app_state.clone();
    tokio::spawn(async move {
        // Check on startup, then every 6 hours
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(6 * 3600));
        loop {
            interval.tick().await;
            let home = std::env::var("HOME").unwrap_or_default();
            let backup_dir = format!("{}/.config/agent-tracker/backups", home);
            let _ = std::fs::create_dir_all(&backup_dir);

            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let today_backup = format!("{}/tracker-{}.db", backup_dir, today);

            if !std::path::Path::new(&today_backup).exists() {
                let server = backup_state.state.lock().unwrap();
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
        // Project environment & worktree isolation
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
        // Stream (real-time pane output)
        .route("/api/stream/start", post(stream_start))
        .route("/api/stream/stop", post(stream_stop))
        .route("/api/stream/list", get(stream_list))
        // Auth
        .route("/api/auth/verify", get(verify_auth))
        // Health check (no auth required — bypassed in auth_middleware)
        .route("/api/health", get(health_check))
        .route("/api/logs", get(get_logs))
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
