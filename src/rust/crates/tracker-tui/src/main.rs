//! tracker-tui: Agent Tracker TUI client
//!
//! This is the terminal UI for viewing and managing tasks, notes, and goals.
//! Also supports headless command mode for use in hooks.
//! Additionally provides workspace management for Git worktrees and tmux sessions.

mod agent;
mod client;
mod config;
mod workspace;

use std::io;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::{SinkExt, StreamExt};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Tabs},
    Frame, Terminal,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use client::{CommandRequest, TrackerClient};
use tracker_core::{commands, Envelope, Goal, HistoryRecord, Note, NoteScope, Task, TaskStatus};

/// Realtime message from WebSocket (matches server's RealtimeMessage)
#[derive(Debug, Clone, serde::Deserialize)]
struct RealtimeMessage {
    state: ServerState,
    #[serde(default)]
    tmux_windows: Vec<serde_json::Value>, // We don't use this in TUI for now
}

/// Server state (matches server's BackendState)
#[derive(Debug, Clone, serde::Deserialize)]
struct ServerState {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    tasks: Vec<Task>,
    #[serde(default)]
    archived_tasks: Vec<Task>,
    #[serde(default)]
    notes: Vec<Note>,
    #[serde(default)]
    archived: Vec<Note>,
    #[serde(default)]
    goals: Vec<Goal>,
    #[serde(default)]
    history: Vec<HistoryRecord>,
    #[serde(default)]
    message: String,
}

/// Agent Tracker CLI client
#[derive(Parser)]
#[command(name = "tracker-client")]
#[command(about = "Agent Tracker CLI client", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the TUI interface
    Ui {
        /// Tmux client tty
        #[arg(long)]
        client: Option<String>,
    },
    /// Send a command to the tracker server (headless)
    Command {
        /// Command name (e.g., start_task, finish_task, pause_task)
        #[arg(index = 1)]
        cmd_name: String,

        /// Tmux client tty
        #[arg(long)]
        client: Option<String>,

        /// Tmux session name
        #[arg(long, short = 's')]
        session: Option<String>,

        /// Tmux session id
        #[arg(long)]
        session_id: Option<String>,

        /// Tmux window name
        #[arg(long, short = 'w')]
        window: Option<String>,

        /// Tmux window id
        #[arg(long)]
        window_id: Option<String>,

        /// Tmux pane id
        #[arg(long, short = 'p')]
        pane: Option<String>,

        /// Task/note summary
        #[arg(long)]
        summary: Option<String>,

        /// Note scope (window, session, all)
        #[arg(long)]
        scope: Option<String>,

        /// Note ID
        #[arg(long)]
        note_id: Option<String>,

        /// Project path for history
        #[arg(long)]
        project: Option<String>,

        /// User prompt for history
        #[arg(long)]
        prompt: Option<String>,

        /// Assistant reply for history
        #[arg(long)]
        reply: Option<String>,

        /// Transcript path for history
        #[arg(long)]
        transcript: Option<String>,

        /// Claude session ID for history
        #[arg(long)]
        claude_session: Option<String>,

        /// Search term for history query
        #[arg(long)]
        search: Option<String>,

        /// Limit for history query
        #[arg(long, default_value = "50")]
        limit: i32,

        /// Offset for history query
        #[arg(long, default_value = "0")]
        offset: i32,
    },
    /// Get current state from tracker server
    State {
        /// Tmux client tty
        #[arg(long)]
        client: Option<String>,
    },

    // ============ Workspace Management Commands ============

    /// Start a new agent workspace (creates worktree + tmux session)
    Start {
        /// Workspace/project name (must be registered in config)
        #[arg(short, long)]
        workspace: String,

        /// Branch name (creates worktree)
        #[arg(short, long)]
        branch: String,

        /// Agent to use (default: from config)
        #[arg(short, long)]
        agent: Option<String>,

        /// Layout template (default: from config)
        #[arg(short, long)]
        layout: Option<String>,

        /// Attach to session after creation
        #[arg(long)]
        attach: bool,
    },

    /// Resume an existing agent workspace
    Resume {
        /// Workspace/project name
        #[arg(short, long)]
        workspace: Option<String>,

        /// Branch name
        #[arg(short, long)]
        branch: Option<String>,

        /// Attach to session
        #[arg(long)]
        attach: bool,
    },

    /// Destroy an agent workspace (kills tmux session + removes worktree)
    Destroy {
        /// Workspace/project name
        #[arg(short, long)]
        workspace: String,

        /// Branch name
        #[arg(short, long)]
        branch: String,

        /// Force removal even with uncommitted changes
        #[arg(long)]
        force: bool,
    },

    /// List all active agent workspaces
    #[command(name = "list")]
    ListWorkspaces {
        /// Filter by workspace name
        #[arg(short, long)]
        workspace: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Active tab in the UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ActiveTab {
    #[default]
    Tasks,
    Notes,
    Goals,
    History,
}

impl ActiveTab {
    fn next(&self) -> Self {
        match self {
            ActiveTab::Tasks => ActiveTab::Notes,
            ActiveTab::Notes => ActiveTab::Goals,
            ActiveTab::Goals => ActiveTab::History,
            ActiveTab::History => ActiveTab::Tasks,
        }
    }

    fn prev(&self) -> Self {
        match self {
            ActiveTab::Tasks => ActiveTab::History,
            ActiveTab::Notes => ActiveTab::Tasks,
            ActiveTab::Goals => ActiveTab::Notes,
            ActiveTab::History => ActiveTab::Goals,
        }
    }

    fn index(&self) -> usize {
        match self {
            ActiveTab::Tasks => 0,
            ActiveTab::Notes => 1,
            ActiveTab::Goals => 2,
            ActiveTab::History => 3,
        }
    }
}

/// Input mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InputMode {
    #[default]
    Normal,
    Search,
    AddNote,
    HistoryDetail,       // Viewing history record details
    HistoryDetailSearch, // Searching within history detail
}

/// Scope for adding notes
#[derive(Debug, Clone, Copy, Default)]
enum AddNoteScope {
    #[default]
    Window,
    Session,
    Global,
}

impl AddNoteScope {
    fn next(&self) -> Self {
        match self {
            AddNoteScope::Window => AddNoteScope::Session,
            AddNoteScope::Session => AddNoteScope::Global,
            AddNoteScope::Global => AddNoteScope::Window,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            AddNoteScope::Window => "W",
            AddNoteScope::Session => "S",
            AddNoteScope::Global => "G",
        }
    }

    fn to_scope_str(&self) -> &'static str {
        match self {
            AddNoteScope::Window => "window",
            AddNoteScope::Session => "session",
            AddNoteScope::Global => "all",
        }
    }
}

/// App state
struct App {
    // Data
    tasks: Vec<Task>,
    notes: Vec<Note>,
    goals: Vec<Goal>,
    history: Vec<HistoryRecord>,
    // Archived data
    archived_tasks: Vec<Task>,
    archived_notes: Vec<Note>,
    // View archived toggle
    view_archived: bool,
    // UI state
    active_tab: ActiveTab,
    selected_task: usize,
    selected_note: usize,
    selected_goal: usize,
    selected_history: usize,
    history_scroll: u16, // Scroll offset for history detail view
    message: String,
    // Filter state
    input_mode: InputMode,
    search_query: String,
    filter_session: Option<String>,
    filter_scope: AddNoteScope, // Scope filter for notes (W/S/G)
    // Current session (where TUI is running)
    current_session: Option<String>,
    current_window: Option<String>,
    current_pane: Option<String>,
    // Window mapping: window_id -> (session_index, window_index) for display
    window_map: std::collections::HashMap<String, (usize, usize)>,
    // Note input
    note_input: String,
    note_scope: AddNoteScope,
    // History detail search
    detail_search_query: String,
    detail_search_matches: Vec<u16>, // Line numbers with matches
    detail_search_index: usize,      // Current match index
    // List states for scrolling
    task_list_state: ListState,
    note_list_state: ListState,
    goal_list_state: ListState,
    history_list_state: ListState,
}

impl Default for App {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            notes: Vec::new(),
            goals: Vec::new(),
            history: Vec::new(),
            archived_tasks: Vec::new(),
            archived_notes: Vec::new(),
            view_archived: false,
            active_tab: ActiveTab::Tasks,
            selected_task: 0,
            selected_note: 0,
            selected_goal: 0,
            selected_history: 0,
            history_scroll: 0,
            message: "Connecting...".to_string(),
            input_mode: InputMode::Normal,
            search_query: String::new(),
            filter_session: None,
            filter_scope: AddNoteScope::Window, // Default to Window scope
            current_session: None,
            current_window: None,
            current_pane: None,
            window_map: std::collections::HashMap::new(),
            note_input: String::new(),
            note_scope: AddNoteScope::default(),
            detail_search_query: String::new(),
            detail_search_matches: Vec::new(),
            detail_search_index: 0,
            task_list_state: ListState::default().with_selected(Some(0)),
            note_list_state: ListState::default().with_selected(Some(0)),
            goal_list_state: ListState::default().with_selected(Some(0)),
            history_list_state: ListState::default().with_selected(Some(0)),
        }
    }
}

impl App {
    fn new_with_context(
        session_id: Option<String>,
        window_id: Option<String>,
        pane_id: Option<String>,
    ) -> Self {
        let window_map = Self::build_window_map();
        Self {
            current_session: session_id,
            current_window: window_id,
            current_pane: pane_id,
            window_map,
            ..Default::default()
        }
    }

    /// Build mapping from window_id to (session_index, window_index) by querying tmux
    fn build_window_map() -> std::collections::HashMap<String, (usize, usize)> {
        let mut map = std::collections::HashMap::new();

        // Get tmux window list: session_id, session_index, window_id, window_index
        if let Ok(output) = std::process::Command::new("tmux")
            .args(["list-windows", "-a", "-F", "#{session_id} #{window_id} #{window_index}"])
            .output()
        {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                // Build session_id -> session_index mapping first
                let mut session_indices: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                let mut session_order = Vec::new();

                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        let session_id = parts[0];
                        if !session_indices.contains_key(session_id) {
                            session_order.push(session_id.to_string());
                            session_indices.insert(session_id.to_string(), session_order.len());
                        }
                    }
                }

                // Now build window map
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        let session_id = parts[0];
                        let window_id = parts[1];
                        let window_index: usize = parts[2].parse().unwrap_or(1);

                        if let Some(&session_index) = session_indices.get(session_id) {
                            map.insert(window_id.to_string(), (session_index, window_index));
                        }
                    }
                }
            }
        }
        map
    }

    fn update_from_envelope(&mut self, env: Envelope) {
        if env.kind == "state" {
            self.tasks = env.tasks;
            self.notes = env.notes;
            self.goals = env.goals;
            self.history = env.history;
            self.archived_tasks = env.archived_tasks;
            self.archived_notes = env.archived;
            self.message = env.message;
            // Clamp selections
            self.clamp_selections();
        }
    }

    /// Update from server's RealtimeMessage format
    fn update_from_realtime(&mut self, msg: RealtimeMessage) {
        self.tasks = msg.state.tasks;
        self.notes = msg.state.notes;
        self.goals = msg.state.goals;
        self.history = msg.state.history;
        self.archived_tasks = msg.state.archived_tasks;
        self.archived_notes = msg.state.archived;
        self.message = msg.state.message;
        // Clamp selections
        self.clamp_selections();
    }

    fn clamp_selections(&mut self) {
        let filtered_tasks = self.filtered_tasks();
        if !filtered_tasks.is_empty() && self.selected_task >= filtered_tasks.len() {
            self.selected_task = filtered_tasks.len() - 1;
        }
        let filtered_notes = self.filtered_notes();
        if !filtered_notes.is_empty() && self.selected_note >= filtered_notes.len() {
            self.selected_note = filtered_notes.len() - 1;
        }
        let filtered_goals = self.filtered_goals();
        if !filtered_goals.is_empty() && self.selected_goal >= filtered_goals.len() {
            self.selected_goal = filtered_goals.len() - 1;
        }
        let filtered_history = self.filtered_history();
        if !filtered_history.is_empty() && self.selected_history >= filtered_history.len() {
            self.selected_history = filtered_history.len() - 1;
        }
    }

    fn next_item(&mut self) {
        match self.active_tab {
            ActiveTab::Tasks => {
                let filtered = self.filtered_tasks();
                if !filtered.is_empty() {
                    self.selected_task = (self.selected_task + 1) % filtered.len();
                    self.task_list_state.select(Some(self.selected_task));
                }
            }
            ActiveTab::Notes => {
                let filtered = self.filtered_notes();
                if !filtered.is_empty() {
                    self.selected_note = (self.selected_note + 1) % filtered.len();
                    self.note_list_state.select(Some(self.selected_note));
                }
            }
            ActiveTab::Goals => {
                let filtered = self.filtered_goals();
                if !filtered.is_empty() {
                    self.selected_goal = (self.selected_goal + 1) % filtered.len();
                    self.goal_list_state.select(Some(self.selected_goal));
                }
            }
            ActiveTab::History => {
                let filtered = self.filtered_history();
                if !filtered.is_empty() {
                    self.selected_history = (self.selected_history + 1) % filtered.len();
                    self.history_list_state.select(Some(self.selected_history));
                }
            }
        }
    }

    fn prev_item(&mut self) {
        match self.active_tab {
            ActiveTab::Tasks => {
                let filtered = self.filtered_tasks();
                if !filtered.is_empty() {
                    self.selected_task = self.selected_task.checked_sub(1).unwrap_or(filtered.len() - 1);
                    self.task_list_state.select(Some(self.selected_task));
                }
            }
            ActiveTab::Notes => {
                let filtered = self.filtered_notes();
                if !filtered.is_empty() {
                    self.selected_note = self.selected_note.checked_sub(1).unwrap_or(filtered.len() - 1);
                    self.note_list_state.select(Some(self.selected_note));
                }
            }
            ActiveTab::Goals => {
                let filtered = self.filtered_goals();
                if !filtered.is_empty() {
                    self.selected_goal = self.selected_goal.checked_sub(1).unwrap_or(filtered.len() - 1);
                    self.goal_list_state.select(Some(self.selected_goal));
                }
            }
            ActiveTab::History => {
                let filtered = self.filtered_history();
                if !filtered.is_empty() {
                    self.selected_history = self.selected_history.checked_sub(1).unwrap_or(filtered.len() - 1);
                    self.history_list_state.select(Some(self.selected_history));
                }
            }
        }
    }

    fn next_tab(&mut self) {
        self.active_tab = self.active_tab.next();
    }

    fn prev_tab(&mut self) {
        self.active_tab = self.active_tab.prev();
    }

    /// Get filtered tasks (active or archived based on view_archived flag)
    fn filtered_tasks(&self) -> Vec<&Task> {
        let source = if self.view_archived {
            &self.archived_tasks
        } else {
            &self.tasks
        };
        source
            .iter()
            .filter(|t| {
                // Session filter
                if let Some(ref session) = self.filter_session {
                    if &t.session_id != session {
                        return false;
                    }
                }
                // Search filter
                if !self.search_query.is_empty() {
                    if !t.summary.to_lowercase().contains(&self.search_query.to_lowercase()) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Get filtered notes (active or archived based on view_archived flag)
    fn filtered_notes(&self) -> Vec<&Note> {
        let source = if self.view_archived {
            &self.archived_notes
        } else {
            &self.notes
        };
        source
            .iter()
            .filter(|n| {
                // Scope filter (Go logic: matchesScope)
                // W filter: show Window notes (this window) + Session notes (this session) + All notes
                // S filter: show Session notes (this session) + All notes
                // G filter: show only All notes (global)
                let matches_scope = match self.filter_scope {
                    AddNoteScope::Window => {
                        match n.scope {
                            NoteScope::All => true,
                            NoteScope::Session => {
                                self.current_session.as_ref().map_or(false, |s| s == &n.session_id)
                            }
                            NoteScope::Window => {
                                self.current_window.as_ref().map_or(false, |w| w == &n.window_id)
                            }
                        }
                    }
                    AddNoteScope::Session => {
                        match n.scope {
                            NoteScope::All => true,
                            NoteScope::Session | NoteScope::Window => {
                                self.current_session.as_ref().map_or(false, |s| s == &n.session_id)
                            }
                        }
                    }
                    AddNoteScope::Global => {
                        // Only show global notes (scope=all)
                        matches!(n.scope, NoteScope::All)
                    }
                };
                if !matches_scope {
                    return false;
                }
                // Search filter
                if !self.search_query.is_empty() {
                    if !n.summary.to_lowercase().contains(&self.search_query.to_lowercase()) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Get filtered goals
    fn filtered_goals(&self) -> Vec<&Goal> {
        self.goals
            .iter()
            .filter(|g| {
                // Session filter
                if let Some(ref session) = self.filter_session {
                    if &g.session_id != session {
                        return false;
                    }
                }
                // Search filter
                if !self.search_query.is_empty() {
                    if !g.summary.to_lowercase().contains(&self.search_query.to_lowercase()) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Get filtered history
    fn filtered_history(&self) -> Vec<&HistoryRecord> {
        self.history
            .iter()
            .filter(|h| {
                // Search filter - match against summary, completion_note, or messages
                if !self.search_query.is_empty() {
                    let query = self.search_query.to_lowercase();
                    let matches_summary = h.summary.to_lowercase().contains(&query);
                    let matches_note = h.completion_note.to_lowercase().contains(&query);
                    let matches_messages = h.messages.iter().any(|m| m.content.to_lowercase().contains(&query));
                    if !matches_summary && !matches_note && !matches_messages {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Toggle session filter
    fn toggle_session_filter(&mut self) {
        if self.filter_session.is_some() {
            self.filter_session = None;
        } else {
            // Filter by the session where TUI is running (not selected item's session)
            if let Some(ref session) = self.current_session {
                self.filter_session = Some(session.clone());
            }
        }
    }

    /// Get command to delete currently selected item
    fn get_delete_command(&self) -> Option<Envelope> {
        match self.active_tab {
            ActiveTab::Tasks => {
                let task = self.tasks.get(self.selected_task)?;
                let mut env = Envelope::command(commands::DELETE_TASK);
                env.session_id = task.session_id.clone();
                env.window_id = task.window_id.clone();
                env.pane = task.pane.clone();
                Some(env)
            }
            ActiveTab::Notes => {
                let note = self.notes.get(self.selected_note)?;
                let mut env = Envelope::command(commands::NOTE_DELETE);
                env.note_id = note.id.clone();
                Some(env)
            }
            ActiveTab::Goals => {
                let goal = self.goals.get(self.selected_goal)?;
                let mut env = Envelope::command(commands::GOAL_DELETE);
                env.goal_id = goal.id.clone();
                Some(env)
            }
            ActiveTab::History => None, // History is read-only
        }
    }

    /// Get command to toggle currently selected item
    fn get_toggle_command(&self) -> Option<Envelope> {
        match self.active_tab {
            ActiveTab::Tasks => {
                // For tasks, toggle between in_progress and awaiting_input
                let task = self.tasks.get(self.selected_task)?;
                let cmd = if task.status == TaskStatus::AwaitingInput {
                    commands::START_TASK
                } else {
                    commands::PAUSE_TASK
                };
                let mut env = Envelope::command(cmd);
                env.session_id = task.session_id.clone();
                env.window_id = task.window_id.clone();
                env.pane = task.pane.clone();
                env.summary = task.summary.clone();
                Some(env)
            }
            ActiveTab::Notes => {
                let note = self.notes.get(self.selected_note)?;
                let mut env = Envelope::command(commands::NOTE_TOGGLE_COMPLETE);
                env.note_id = note.id.clone();
                Some(env)
            }
            ActiveTab::Goals => {
                let goal = self.goals.get(self.selected_goal)?;
                let mut env = Envelope::command(commands::GOAL_TOGGLE_COMPLETE);
                env.goal_id = goal.id.clone();
                Some(env)
            }
            ActiveTab::History => None, // History is read-only
        }
    }

    /// Get command to archive currently selected item (task or note)
    fn get_archive_command(&self) -> Option<Envelope> {
        match self.active_tab {
            ActiveTab::Tasks => {
                if self.view_archived {
                    return None; // Can't archive already archived
                }
                let task = self.tasks.get(self.selected_task)?;
                let mut env = Envelope::command(commands::TASK_ARCHIVE);
                env.session_id = task.session_id.clone();
                env.window_id = task.window_id.clone();
                env.pane = task.pane.clone();
                Some(env)
            }
            ActiveTab::Notes => {
                if self.view_archived {
                    return None; // Can't archive already archived
                }
                let note = self.notes.get(self.selected_note)?;
                let mut env = Envelope::command(commands::NOTE_ARCHIVE);
                env.note_id = note.id.clone();
                Some(env)
            }
            _ => None,
        }
    }

    /// Get command to restore currently selected archived item
    fn get_restore_command(&self) -> Option<Envelope> {
        if !self.view_archived {
            return None; // Can only restore in archived view
        }
        match self.active_tab {
            ActiveTab::Tasks => {
                let task = self.archived_tasks.get(self.selected_task)?;
                let mut env = Envelope::command(commands::TASK_RESTORE);
                env.session_id = task.session_id.clone();
                env.window_id = task.window_id.clone();
                env.pane = task.pane.clone();
                Some(env)
            }
            ActiveTab::Notes => {
                let note = self.archived_notes.get(self.selected_note)?;
                let mut env = Envelope::command(commands::NOTE_RESTORE);
                env.note_id = note.id.clone();
                Some(env)
            }
            _ => None,
        }
    }

    /// Update search matches for history detail view
    fn update_detail_search_matches(&mut self) {
        self.detail_search_matches.clear();
        if self.detail_search_query.is_empty() {
            return;
        }

        // Get the index in the filtered list, then clone data we need
        let query = self.detail_search_query.to_lowercase();
        let idx = self.selected_history;

        // Filter history and get the record at index, clone it to avoid borrow issues
        let record: Option<HistoryRecord> = self.history
            .iter()
            .filter(|h| {
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    let matches_summary = h.summary.to_lowercase().contains(&q);
                    let matches_note = h.completion_note.to_lowercase().contains(&q);
                    let matches_messages = h.messages.iter().any(|m| m.content.to_lowercase().contains(&q));
                    matches_summary || matches_note || matches_messages
                } else {
                    true
                }
            })
            .nth(idx)
            .cloned();

        if let Some(record) = record {
            let mut line_num: u16 = 0;
            let mut matches = Vec::new();

            if !record.messages.is_empty() {
                for msg in &record.messages {
                    line_num += 1; // Role header line
                    for l in msg.content.lines() {
                        if l.to_lowercase().contains(&query) {
                            matches.push(line_num);
                        }
                        line_num += 1;
                    }
                    line_num += 1; // Empty separator line
                }
            } else {
                line_num += 1; // "Prompt:" header
                for l in record.summary.lines() {
                    if l.to_lowercase().contains(&query) {
                        matches.push(line_num);
                    }
                    line_num += 1;
                }
                line_num += 2; // Empty line + "Reply:" header
                for l in record.completion_note.lines() {
                    if l.to_lowercase().contains(&query) {
                        matches.push(line_num);
                    }
                    line_num += 1;
                }
            }

            self.detail_search_matches = matches;
        }
        self.detail_search_index = 0;
    }

    /// Get command to add a new note
    fn get_add_note_command(&self) -> Option<Envelope> {
        let text = self.note_input.trim();
        if text.is_empty() {
            return None;
        }
        let mut env = Envelope::command(commands::NOTE_ADD);
        env.summary = text.to_string();
        env.scope = self.note_scope.to_scope_str().to_string();

        // Set session/window/pane based on scope
        match self.note_scope {
            AddNoteScope::Window => {
                if let Some(ref s) = self.current_session {
                    env.session_id = s.clone();
                }
                if let Some(ref w) = self.current_window {
                    env.window_id = w.clone();
                }
                if let Some(ref p) = self.current_pane {
                    env.pane = p.clone();
                }
            }
            AddNoteScope::Session => {
                if let Some(ref s) = self.current_session {
                    env.session_id = s.clone();
                }
            }
            AddNoteScope::Global => {
                // No session/window/pane for global notes
            }
        }
        Some(env)
    }
}

/// Run the command subcommand (headless, for hooks)
async fn run_command(
    cmd_name: String,
    _client: Option<String>,
    session: Option<String>,
    session_id: Option<String>,
    window: Option<String>,
    window_id: Option<String>,
    pane: Option<String>,
    summary: Option<String>,
    scope: Option<String>,
    note_id: Option<String>,
    _project: Option<String>,
    _prompt: Option<String>,
    _reply: Option<String>,
    transcript: Option<String>,
    _claude_session: Option<String>,
    _search: Option<String>,
    _limit: i32,
    _offset: i32,
) -> Result<()> {
    let client = TrackerClient::new();

    // Build command request
    let req = CommandRequest {
        command: cmd_name.clone(),
        session_id: session_id.unwrap_or_default(),
        session: session.unwrap_or_default(),
        window_id: window_id.unwrap_or_default(),
        window: window.unwrap_or_default(),
        pane: pane.unwrap_or_default(),
        summary: summary.unwrap_or_default(),
        note_id: note_id.unwrap_or_default(),
        goal_id: String::new(),
        scope: scope.unwrap_or_default(),
        transcript_path: transcript.unwrap_or_default(),
    };

    // Send command via HTTP
    let response = client.send_command(req).await?;

    if !response.success {
        eprintln!("Command failed: {}", response.message);
    }

    Ok(())
}

/// Run the state subcommand
async fn run_state(_client: Option<String>) -> Result<()> {
    let client = TrackerClient::new();

    // Get state via HTTP
    let state = client.get_state().await?;

    // Output state as JSON
    println!("{}", serde_json::to_string(&state)?);

    Ok(())
}

/// Render the main UI
fn ui(frame: &mut Frame, app: &mut App) {
    // History detail view takes full screen
    if app.input_mode == InputMode::HistoryDetail || app.input_mode == InputMode::HistoryDetailSearch {
        render_history_detail(frame, app, frame.area());
        return;
    }

    let footer_height = if matches!(app.input_mode, InputMode::Search | InputMode::AddNote) { 2 } else { 1 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),           // Header + Tabs
            Constraint::Length(1),           // Separator
            Constraint::Min(0),              // Content
            Constraint::Length(footer_height), // Footer (+ search input)
        ])
        .split(frame.area());

    // Header with tabs
    render_header(frame, app, chunks[0]);

    // Separator line
    let separator = Paragraph::new("─".repeat(frame.area().width as usize))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(separator, chunks[1]);

    // Content based on active tab
    match app.active_tab {
        ActiveTab::Tasks => render_tasks(frame, app, chunks[2]),
        ActiveTab::Notes => render_notes(frame, app, chunks[2]),
        ActiveTab::Goals => render_goals(frame, app, chunks[2]),
        ActiveTab::History => render_history(frame, app, chunks[2]),
    }

    // Footer
    render_footer(frame, app, chunks[3]);
}

/// Render header with tabs
fn render_header(frame: &mut Frame, app: &mut App, area: Rect) {
    let header_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(2)])
        .split(area);

    // Title with filter indicator
    let mut title_spans = vec![
        Span::styled("▌ ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "Agent Tracker",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
    ];

    // Show current session (convert $0 -> S1 format)
    if let Some(ref session) = app.current_session {
        let session_num: i32 = session.trim_start_matches('$').parse().unwrap_or(0);
        title_spans.push(Span::styled(
            format!(" [S{}]", session_num + 1),
            Style::default().fg(Color::Yellow),
        ));
    }

    // Show session filter indicator if active
    if app.filter_session.is_some() {
        title_spans.push(Span::styled(
            " ◉",
            Style::default().fg(Color::Green),
        ));
    }

    // Show search query if active
    if !app.search_query.is_empty() {
        title_spans.push(Span::styled(
            format!(" 🔍{}", app.search_query),
            Style::default().fg(Color::Magenta),
        ));
    }

    title_spans.push(Span::raw(" - "));
    title_spans.push(Span::styled(&app.message, Style::default().fg(Color::Gray)));

    let title = Paragraph::new(Line::from(title_spans));
    frame.render_widget(title, header_chunks[0]);

    // Tabs - show filtered counts
    let tab_titles = vec![
        format!(" Tasks ({}) ", app.filtered_tasks().len()),
        format!(" Notes ({}) ", app.filtered_notes().len()),
        format!(" Goals ({}) ", app.filtered_goals().len()),
        format!(" History ({}) ", app.filtered_history().len()),
    ];
    let tabs = Tabs::new(tab_titles)
        .select(app.active_tab.index())
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::raw(" │ "));
    frame.render_widget(tabs, header_chunks[1]);
}

/// Format task address like "S1W2" from session_id "$1" and window_id "@2"
/// Numbers start from 1 (user-friendly), so $0 -> S1, @0 -> W1
fn format_task_address(
    window_id: &str,
    window_map: &std::collections::HashMap<String, (usize, usize)>,
) -> String {
    if let Some(&(session_index, window_index)) = window_map.get(window_id) {
        format!("S{}W{} ", session_index, window_index)
    } else {
        // Fallback: just show window_id
        format!("{} ", window_id)
    }
}

/// Render tasks list
fn render_tasks(frame: &mut Frame, app: &mut App, area: Rect) {
    // Collect data first to avoid borrow issues
    let selected_task = app.selected_task;
    let window_map = app.window_map.clone();
    let search_query = app.search_query.clone();
    let filter_session = app.filter_session.clone();

    let items: Vec<ListItem> = app.tasks
        .iter()
        .filter(|t| {
            if let Some(ref session) = filter_session {
                if &t.session_id != session { return false; }
            }
            if !search_query.is_empty() {
                if !t.summary.to_lowercase().contains(&search_query.to_lowercase()) { return false; }
            }
            true
        })
        .enumerate()
        .map(|(i, task)| {
            let (icon, base_color) = match task.status {
                TaskStatus::InProgress => ("▶ ", Color::LightYellow),
                TaskStatus::AwaitingInput => ("● ", Color::Rgb(255, 165, 0)),
                TaskStatus::Completed => {
                    if task.acknowledged { ("✓ ", Color::LightGreen) } else { ("✔ ", Color::LightGreen) }
                }
            };

            let is_selected = i == selected_task;
            let bg = if is_selected { Color::Rgb(47, 79, 79) } else { Color::Reset };
            let style = Style::default().bg(bg);
            let address = format_task_address(&task.window_id, &window_map);

            let duration = if let Some(started) = task.started_at {
                let dur = chrono::Utc::now() - started;
                if dur.num_hours() > 0 { format!(" {}h{}m", dur.num_hours(), dur.num_minutes() % 60) }
                else if dur.num_minutes() > 0 { format!(" {}m", dur.num_minutes()) }
                else { format!(" {}s", dur.num_seconds()) }
            } else { String::new() };

            let line = Line::from(vec![
                Span::styled(icon, style.fg(base_color).add_modifier(Modifier::BOLD)),
                Span::styled(address, style.fg(Color::DarkGray)),
                Span::styled(&task.summary, style.fg(base_color)),
                Span::styled(duration, style.fg(Color::DarkGray)),
            ]);
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default());
    frame.render_stateful_widget(list, area, &mut app.task_list_state);
}

/// Render notes list
fn render_notes(frame: &mut Frame, app: &mut App, area: Rect) {
    let selected_note = app.selected_note;
    let search_query = app.search_query.clone();
    let filter_scope = app.filter_scope;
    let current_session = app.current_session.clone();
    let current_window = app.current_window.clone();

    let items: Vec<ListItem> = app.notes
        .iter()
        .filter(|n| {
            let matches_scope = match filter_scope {
                AddNoteScope::Window => match n.scope {
                    NoteScope::All => true,
                    NoteScope::Session => current_session.as_ref().map_or(false, |s| s == &n.session_id),
                    NoteScope::Window => current_window.as_ref().map_or(false, |w| w == &n.window_id),
                },
                AddNoteScope::Session => match n.scope {
                    NoteScope::All => true,
                    NoteScope::Session | NoteScope::Window => current_session.as_ref().map_or(false, |s| s == &n.session_id),
                },
                AddNoteScope::Global => matches!(n.scope, NoteScope::All),
            };
            if !matches_scope { return false; }
            if !search_query.is_empty() {
                if !n.summary.to_lowercase().contains(&search_query.to_lowercase()) { return false; }
            }
            true
        })
        .enumerate()
        .map(|(i, note)| {
            let icon = if note.completed { "✓ " } else { "○ " };
            let (scope_badge, scope_color) = match note.scope {
                NoteScope::Window => ("W", Color::LightYellow),
                NoteScope::Session => ("S", Color::Green),
                NoteScope::All => ("G", Color::Cyan),
            };

            let is_selected = i == selected_note;
            let bg = if is_selected { Color::Rgb(47, 79, 79) } else { Color::Reset };
            let style = Style::default().bg(bg);

            let line = Line::from(vec![
                Span::styled(icon, style.fg(scope_color)),
                Span::styled(format!("[{}] ", scope_badge), style.fg(scope_color).add_modifier(Modifier::BOLD)),
                Span::styled(&note.summary, style.fg(scope_color)),
            ]);
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default());
    frame.render_stateful_widget(list, area, &mut app.note_list_state);
}

/// Render goals list
fn render_goals(frame: &mut Frame, app: &mut App, area: Rect) {
    let selected_goal = app.selected_goal;
    let search_query = app.search_query.clone();
    let filter_session = app.filter_session.clone();

    let items: Vec<ListItem> = app.goals
        .iter()
        .filter(|g| {
            if let Some(ref session) = filter_session {
                if &g.session_id != session { return false; }
            }
            if !search_query.is_empty() {
                if !g.summary.to_lowercase().contains(&search_query.to_lowercase()) { return false; }
            }
            true
        })
        .enumerate()
        .map(|(i, goal)| {
            let (icon, color) = if goal.completed { ("★ ", Color::LightGreen) } else { ("☆ ", Color::Magenta) };

            let is_selected = i == selected_goal;
            let bg = if is_selected { Color::Rgb(47, 79, 79) } else { Color::Reset };
            let style = Style::default().bg(bg);

            let line = Line::from(vec![
                Span::styled(icon, style.fg(color)),
                Span::styled(&goal.summary, style.fg(color)),
            ]);
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default());
    frame.render_stateful_widget(list, area, &mut app.goal_list_state);
}

/// Render history list
fn render_history(frame: &mut Frame, app: &mut App, area: Rect) {
    let selected_history = app.selected_history;
    let window_map = app.window_map.clone();
    let search_query = app.search_query.clone();

    let items: Vec<ListItem> = app.history
        .iter()
        .filter(|h| {
            if !search_query.is_empty() {
                let q = search_query.to_lowercase();
                let matches_summary = h.summary.to_lowercase().contains(&q);
                let matches_note = h.completion_note.to_lowercase().contains(&q);
                let matches_messages = h.messages.iter().any(|m| m.content.to_lowercase().contains(&q));
                if !matches_summary && !matches_note && !matches_messages { return false; }
            }
            true
        })
        .enumerate()
        .map(|(i, record)| {
            let address = if let Some(&(session_index, window_index)) = window_map.get(&record.window_id) {
                format!("#{} S{}W{} ", record.id, session_index, window_index + 1)
            } else {
                format!("#{} {} ", record.id, record.window_id)
            };

            let duration = if record.duration_seconds > 0.0 {
                let secs = record.duration_seconds as u64;
                if secs >= 60 { format!(" {}m", secs / 60) } else { format!(" {}s", secs) }
            } else { String::new() };

            let summary: String = record.summary.chars().take(35).collect();
            let reply: String = record.completion_note.lines().next().unwrap_or("").chars().take(40).collect();

            let is_selected = i == selected_history;
            let bg = if is_selected { Color::Rgb(47, 79, 79) } else { Color::Reset };
            let style = Style::default().bg(bg);

            let line = Line::from(vec![
                Span::styled(address, style.fg(Color::DarkGray)),
                Span::styled(summary, style.fg(Color::White)),
                Span::styled(" → ", style.fg(Color::DarkGray)),
                Span::styled(reply, style.fg(Color::Cyan)),
                Span::styled(duration, style.fg(Color::DarkGray)),
            ]);
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default());
    frame.render_stateful_widget(list, area, &mut app.history_list_state);
}

/// Render history detail view with scrolling
fn render_history_detail(frame: &mut Frame, app: &mut App, area: Rect) {
    let filtered = app.filtered_history();
    if let Some(record) = filtered.get(app.selected_history) {
        let is_searching = app.input_mode == InputMode::HistoryDetailSearch;
        let footer_height = if is_searching { 2 } else { 1 };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),           // Header
                Constraint::Min(0),              // Scrollable content
                Constraint::Length(footer_height), // Footer hints (+ search input)
            ])
            .split(area);

        // Header with ID and metadata
        let duration_str = if record.duration_seconds >= 60.0 {
            format!("{}m", (record.duration_seconds / 60.0) as u64)
        } else {
            format!("{}s", record.duration_seconds as u64)
        };
        let msg_count = if !record.messages.is_empty() {
            format!("  {} messages", record.messages.len())
        } else {
            String::new()
        };
        let mut header_spans = vec![
            Span::styled(format!("History #{}", record.id), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(format!("Duration: {}", duration_str), Style::default().fg(Color::DarkGray)),
            Span::styled(msg_count, Style::default().fg(Color::Green)),
        ];
        // Show search indicator if active
        if !app.detail_search_query.is_empty() {
            header_spans.push(Span::styled(
                format!("  🔍\"{}\" ({}/{})",
                    app.detail_search_query,
                    if app.detail_search_matches.is_empty() { 0 } else { app.detail_search_index + 1 },
                    app.detail_search_matches.len()
                ),
                Style::default().fg(Color::Magenta),
            ));
        }
        let header = Paragraph::new(Line::from(header_spans));
        frame.render_widget(header, chunks[0]);

        // Build all content lines with search highlighting
        let mut lines: Vec<Line> = Vec::new();
        let search_query = app.detail_search_query.to_lowercase();
        let has_search = !search_query.is_empty();

        if !record.messages.is_empty() {
            // Display full conversation from messages
            for msg in &record.messages {
                let (icon, color) = if msg.role == "user" {
                    ("👤 User:", Color::Yellow)
                } else {
                    ("🤖 Assistant:", Color::Cyan)
                };
                lines.push(Line::from(Span::styled(icon, Style::default().fg(color).add_modifier(Modifier::BOLD))));
                for l in msg.content.lines() {
                    if has_search && l.to_lowercase().contains(&search_query) {
                        // Highlight matching line
                        lines.push(Line::from(Span::styled(l.to_string(), Style::default().bg(Color::DarkGray).fg(Color::White))));
                    } else {
                        lines.push(Line::from(Span::raw(l.to_string())));
                    }
                }
                lines.push(Line::from("")); // Empty line separator
            }
        } else {
            // Fallback: display summary and completion_note
            lines.push(Line::from(Span::styled("📝 Prompt:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            for l in record.summary.lines() {
                if has_search && l.to_lowercase().contains(&search_query) {
                    lines.push(Line::from(Span::styled(l.to_string(), Style::default().bg(Color::DarkGray).fg(Color::White))));
                } else {
                    lines.push(Line::from(Span::raw(l.to_string())));
                }
            }
            lines.push(Line::from("")); // Empty line separator

            lines.push(Line::from(Span::styled("🤖 Reply:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
            for l in record.completion_note.lines() {
                if has_search && l.to_lowercase().contains(&search_query) {
                    lines.push(Line::from(Span::styled(l.to_string(), Style::default().bg(Color::DarkGray).fg(Color::White))));
                } else {
                    lines.push(Line::from(Span::raw(l.to_string())));
                }
            }
        }

        // Scrollable content
        let content = Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((app.history_scroll, 0));
        frame.render_widget(content, chunks[1]);

        // Footer
        if is_searching {
            let footer_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1)])
                .split(chunks[2]);

            let search_input = Paragraph::new(Line::from(vec![
                Span::styled("Search: ", Style::default().fg(Color::Yellow)),
                Span::styled(&app.detail_search_query, Style::default().fg(Color::White)),
                Span::styled("█", Style::default().fg(Color::White)), // Cursor
            ]));
            frame.render_widget(search_input, footer_chunks[0]);

            let hints = Paragraph::new(Line::from(vec![
                Span::styled("[Enter]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Confirm  "),
                Span::styled("[Esc]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Cancel"),
            ]));
            frame.render_widget(hints, footer_chunks[1]);
        } else {
            let footer = Paragraph::new(Line::from(vec![
                Span::styled("[h/Esc]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Back  "),
                Span::styled("[j/k]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Scroll  "),
                Span::styled("[J/K]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Fast  "),
                Span::styled("[/]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Search  "),
                Span::styled("[n/N]", Style::default().fg(Color::DarkGray)),
                Span::raw(" Next/Prev  "),
                Span::styled("[p]", Style::default().fg(Color::DarkGray)),
                Span::raw(" PrevRec",
            )]));
            frame.render_widget(footer, chunks[2]);
        }
    }
}

/// Render footer with keybindings
fn render_footer(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.input_mode == InputMode::Search {
        // Search mode footer
        let footer_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(area);

        let search_input = Paragraph::new(Line::from(vec![
            Span::styled("Search: ", Style::default().fg(Color::Yellow)),
            Span::styled(&app.search_query, Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::White)), // Cursor
        ]));
        frame.render_widget(search_input, footer_chunks[0]);

        let hints = Paragraph::new(Line::from(vec![
            Span::styled("[Enter]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Confirm  "),
            Span::styled("[Esc]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Cancel"),
        ]));
        frame.render_widget(hints, footer_chunks[1]);
    } else if app.input_mode == InputMode::AddNote {
        // AddNote mode footer
        let footer_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(area);

        let note_input = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("Add note ({}): ", app.note_scope.label()),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(&app.note_input, Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::White)), // Cursor
        ]));
        frame.render_widget(note_input, footer_chunks[0]);

        let hints = Paragraph::new(Line::from(vec![
            Span::styled("[Tab]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Scope  "),
            Span::styled("[Enter]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Confirm  "),
            Span::styled("[Esc]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Cancel"),
        ]));
        frame.render_widget(hints, footer_chunks[1]);
    } else {
        // Normal mode footer
        let mut spans = vec![
            Span::styled("[Tab]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Switch  "),
            Span::styled("[j/k]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Nav  "),
            Span::styled("[/]", Style::default().fg(Color::DarkGray)),
            Span::raw(" Search  "),
        ];

        // Add context-specific hints
        if app.active_tab == ActiveTab::Notes {
            // Notes tab: [s] Scope W/S/G  [n] Add  [a] Archive
            spans.push(Span::styled("[s]", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(format!(" Scope:{}  ", app.filter_scope.label())));
            spans.push(Span::styled("[n]", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(" Add  "));
            spans.push(Span::styled("[a]", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(" Archive  "));
        } else if app.active_tab == ActiveTab::Tasks {
            // Tasks tab: [s] Session  [f/⏎] Focus
            spans.push(Span::styled("[s]", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(" Session  "));
            spans.push(Span::styled("[f/⏎]", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(" Focus  "));
        } else if app.active_tab == ActiveTab::History {
            // History tab: [l] View (vim-style)
            spans.push(Span::styled("[l]", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(" View  "));
        } else {
            // Goals tab: [s] Session
            spans.push(Span::styled("[s]", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(" Session  "));
        }

        spans.push(Span::styled("[q]", Style::default().fg(Color::DarkGray)));
        spans.push(Span::raw(" Quit"));

        let footer = Paragraph::new(Line::from(spans));
        frame.render_widget(footer, area);
    }
}

/// Run the TUI
async fn run_ui(_client: Option<String>) -> Result<()> {
    // Get current session/window/pane from tmux
    let tmux_info = std::process::Command::new("tmux")
        .args(["display", "-p", "#{session_id}:::#{window_id}:::#{pane_id}"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let (current_session, current_window, current_pane) = if let Some(info) = tmux_info {
        let parts: Vec<&str> = info.split(":::").collect();
        (
            parts.get(0).filter(|s| !s.is_empty()).map(|s| s.to_string()),
            parts.get(1).filter(|s| !s.is_empty()).map(|s| s.to_string()),
            parts.get(2).filter(|s| !s.is_empty()).map(|s| s.to_string()),
        )
    } else {
        (None, None, None)
    };

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new_with_context(current_session, current_window, current_pane);

    // Connect to server via WebSocket
    let client = TrackerClient::new();
    let ws_url = client.ws_url();

    let (ws_stream, _) = match connect_async(&ws_url).await {
        Ok(s) => s,
        Err(e) => {
            app.message = format!("Failed to connect to {}: {}", ws_url, e);
            // Show error and exit
            terminal.draw(|f| ui(f, &mut app))?;
            tokio::time::sleep(Duration::from_secs(2)).await;
            // Cleanup
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            return Err(e.into());
        }
    };

    let (mut ws_write, mut ws_read) = ws_stream.split();

    // Register as UI client
    let register = serde_json::to_string(&Envelope::ui_register(""))?;
    ws_write.send(Message::Text(register)).await?;

    loop {
        // Draw UI
        terminal.draw(|f| ui(f, &mut app))?;

        // Handle events with timeout
        tokio::select! {
            // Handle keyboard and mouse input
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if event::poll(Duration::from_millis(0))? {
                    let evt = event::read()?;

                    // Handle mouse scroll
                    if let Event::Mouse(mouse) = &evt {
                        use crossterm::event::MouseEventKind;
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                match app.active_tab {
                                    ActiveTab::Tasks => app.prev_item(),
                                    ActiveTab::Notes => app.prev_item(),
                                    ActiveTab::Goals => app.prev_item(),
                                    ActiveTab::History => {
                                        if app.input_mode == InputMode::HistoryDetail {
                                            app.history_scroll = app.history_scroll.saturating_sub(3);
                                        } else {
                                            app.prev_item();
                                        }
                                    }
                                }
                            }
                            MouseEventKind::ScrollDown => {
                                match app.active_tab {
                                    ActiveTab::Tasks => app.next_item(),
                                    ActiveTab::Notes => app.next_item(),
                                    ActiveTab::Goals => app.next_item(),
                                    ActiveTab::History => {
                                        if app.input_mode == InputMode::HistoryDetail {
                                            app.history_scroll = app.history_scroll.saturating_add(3);
                                        } else {
                                            app.next_item();
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    if let Event::Key(key) = evt {
                        let mut cmd_to_send: Option<Envelope> = None;

                        match app.input_mode {
                            InputMode::Search => {
                                // Search mode key handling
                                match key.code {
                                    KeyCode::Esc => {
                                        app.input_mode = InputMode::Normal;
                                        app.search_query.clear();
                                    }
                                    KeyCode::Enter => {
                                        app.input_mode = InputMode::Normal;
                                        // Keep search_query for filtering
                                    }
                                    KeyCode::Backspace => {
                                        app.search_query.pop();
                                    }
                                    KeyCode::Char(c) => {
                                        app.search_query.push(c);
                                    }
                                    _ => {}
                                }
                            }
                            InputMode::AddNote => {
                                // AddNote mode key handling
                                match key.code {
                                    KeyCode::Esc => {
                                        app.input_mode = InputMode::Normal;
                                        app.note_input.clear();
                                    }
                                    KeyCode::Tab => {
                                        // Cycle scope: W -> S -> G -> W
                                        app.note_scope = app.note_scope.next();
                                    }
                                    KeyCode::Enter => {
                                        // Submit note
                                        cmd_to_send = app.get_add_note_command();
                                        app.input_mode = InputMode::Normal;
                                        app.note_input.clear();
                                    }
                                    KeyCode::Backspace => {
                                        app.note_input.pop();
                                    }
                                    KeyCode::Char(c) => {
                                        app.note_input.push(c);
                                    }
                                    _ => {}
                                }
                            }
                            InputMode::HistoryDetail => {
                                // History detail view - vim-style navigation
                                match (key.code, key.modifiers) {
                                    // h or Esc to go back
                                    (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Esc, _) => {
                                        app.input_mode = InputMode::Normal;
                                        app.history_scroll = 0;
                                        app.detail_search_query.clear();
                                        app.detail_search_matches.clear();
                                    }
                                    // Alt+T to close panel completely
                                    (KeyCode::Char('t'), KeyModifiers::ALT) => {
                                        break;
                                    }
                                    // j/k to scroll content (1 line)
                                    (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => {
                                        app.history_scroll = app.history_scroll.saturating_add(1);
                                    }
                                    (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => {
                                        app.history_scroll = app.history_scroll.saturating_sub(1);
                                    }
                                    // Shift+J/K to scroll fast (10 lines)
                                    (KeyCode::Char('J'), KeyModifiers::SHIFT) => {
                                        app.history_scroll = app.history_scroll.saturating_add(10);
                                    }
                                    (KeyCode::Char('K'), KeyModifiers::SHIFT) => {
                                        app.history_scroll = app.history_scroll.saturating_sub(10);
                                    }
                                    // '/' to enter search mode
                                    (KeyCode::Char('/'), _) => {
                                        app.input_mode = InputMode::HistoryDetailSearch;
                                        app.detail_search_query.clear();
                                        app.detail_search_matches.clear();
                                    }
                                    // 'n' for next search match (not next record when searching)
                                    (KeyCode::Char('n'), KeyModifiers::NONE) => {
                                        if !app.detail_search_matches.is_empty() {
                                            app.detail_search_index = (app.detail_search_index + 1) % app.detail_search_matches.len();
                                            app.history_scroll = app.detail_search_matches[app.detail_search_index];
                                        } else {
                                            // No search active, go to next record
                                            let filtered = app.filtered_history();
                                            if !filtered.is_empty() {
                                                app.selected_history = (app.selected_history + 1) % filtered.len();
                                                app.history_scroll = 0;
                                            }
                                        }
                                    }
                                    // 'N' for prev search match
                                    (KeyCode::Char('N'), KeyModifiers::SHIFT) => {
                                        if !app.detail_search_matches.is_empty() {
                                            app.detail_search_index = app.detail_search_index.checked_sub(1).unwrap_or(app.detail_search_matches.len() - 1);
                                            app.history_scroll = app.detail_search_matches[app.detail_search_index];
                                        }
                                    }
                                    // 'p' for prev record
                                    (KeyCode::Char('p'), KeyModifiers::NONE) => {
                                        let filtered = app.filtered_history();
                                        if !filtered.is_empty() {
                                            app.selected_history = app.selected_history.checked_sub(1).unwrap_or(filtered.len() - 1);
                                            app.history_scroll = 0;
                                            app.detail_search_matches.clear(); // Clear search on record change
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            InputMode::HistoryDetailSearch => {
                                // Search mode within history detail
                                match key.code {
                                    KeyCode::Esc => {
                                        app.input_mode = InputMode::HistoryDetail;
                                        app.detail_search_query.clear();
                                        app.detail_search_matches.clear();
                                    }
                                    KeyCode::Enter => {
                                        app.input_mode = InputMode::HistoryDetail;
                                        // Keep search results, jump to first match
                                        if !app.detail_search_matches.is_empty() {
                                            app.detail_search_index = 0;
                                            app.history_scroll = app.detail_search_matches[0];
                                        }
                                    }
                                    KeyCode::Backspace => {
                                        app.detail_search_query.pop();
                                        app.update_detail_search_matches();
                                    }
                                    KeyCode::Char(c) => {
                                        app.detail_search_query.push(c);
                                        app.update_detail_search_matches();
                                    }
                                    _ => {}
                                }
                            }
                            InputMode::Normal => {
                                // Normal mode key handling
                                match (key.code, key.modifiers) {
                                    // Quit (q, Esc, Ctrl+c, or Alt+t to toggle)
                                    (KeyCode::Char('q'), _) => break,
                                    (KeyCode::Char('t'), KeyModifiers::ALT) => break, // Toggle close
                                    (KeyCode::Esc, _) => {
                                        // Clear filters on Esc, or quit if no filters
                                        if app.filter_session.is_some() || !app.search_query.is_empty() {
                                            app.filter_session = None;
                                            app.search_query.clear();
                                        } else {
                                            break;
                                        }
                                    }
                                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                                    // Navigation
                                    (KeyCode::Char('j'), _) | (KeyCode::Down, _) => app.next_item(),
                                    (KeyCode::Char('k'), _) | (KeyCode::Up, _) => app.prev_item(),
                                    // Tab switching
                                    (KeyCode::Tab, _) => app.next_tab(),
                                    (KeyCode::BackTab, _) => app.prev_tab(),
                                    (KeyCode::Char('1'), _) => app.active_tab = ActiveTab::Tasks,
                                    (KeyCode::Char('2'), _) => app.active_tab = ActiveTab::Notes,
                                    (KeyCode::Char('3'), _) => app.active_tab = ActiveTab::Goals,
                                    (KeyCode::Char('4'), _) => app.active_tab = ActiveTab::History,
                                    // Search mode
                                    (KeyCode::Char('/'), _) => {
                                        app.input_mode = InputMode::Search;
                                        app.search_query.clear();
                                    }
                                    // Session filter (Tasks/Goals) or Scope filter (Notes)
                                    (KeyCode::Char('s'), _) => {
                                        if app.active_tab == ActiveTab::Notes {
                                            // Cycle scope filter: W -> S -> G -> W
                                            app.filter_scope = app.filter_scope.next();
                                            app.selected_note = 0; // Reset selection
                                        } else {
                                            app.toggle_session_filter();
                                        }
                                    }
                                    // Actions - space to toggle status (not Enter)
                                    (KeyCode::Char(' '), _) => {
                                        cmd_to_send = app.get_toggle_command();
                                    }
                                    (KeyCode::Char('d'), _) => {
                                        cmd_to_send = app.get_delete_command();
                                    }
                                    // 'n' for new note in Notes tab
                                    (KeyCode::Char('n'), _) => {
                                        if app.active_tab == ActiveTab::Notes {
                                            app.input_mode = InputMode::AddNote;
                                            app.note_input.clear();
                                            app.note_scope = app.filter_scope; // Use current filter scope as default
                                        }
                                    }
                                    // 'a' for archive
                                    (KeyCode::Char('a'), KeyModifiers::NONE) => {
                                        cmd_to_send = app.get_archive_command();
                                    }
                                    // 'A' (Shift) for toggle archived view
                                    (KeyCode::Char('A'), KeyModifiers::SHIFT) => {
                                        if app.active_tab == ActiveTab::Tasks || app.active_tab == ActiveTab::Notes {
                                            app.view_archived = !app.view_archived;
                                            app.selected_task = 0;
                                            app.selected_note = 0;
                                        }
                                    }
                                    // 'r' for restore (in archived view)
                                    (KeyCode::Char('r'), KeyModifiers::NONE) => {
                                        if app.view_archived {
                                            cmd_to_send = app.get_restore_command();
                                        }
                                    }
                                    // Focus: jump to task's tmux pane
                                    (KeyCode::Char('f'), _) | (KeyCode::Enter, _) => {
                                        if app.active_tab == ActiveTab::Tasks {
                                            if let Some(task) = app.tasks.get(app.selected_task) {
                                                // Execute tmux switch commands and exit
                                                let _ = std::process::Command::new("tmux")
                                                    .args([
                                                        "switch-client", "-t", &task.session_id,
                                                    ])
                                                    .status();
                                                let _ = std::process::Command::new("tmux")
                                                    .args([
                                                        "select-window", "-t", &task.window_id,
                                                    ])
                                                    .status();
                                                let _ = std::process::Command::new("tmux")
                                                    .args([
                                                        "select-pane", "-t", &task.pane,
                                                    ])
                                                    .status();
                                                break; // Exit TUI after focusing
                                            }
                                        }
                                    }
                                    // 'l' to enter history detail view (vim-style)
                                    (KeyCode::Char('l'), _) => {
                                        if app.active_tab == ActiveTab::History {
                                            if !app.filtered_history().is_empty() {
                                                app.input_mode = InputMode::HistoryDetail;
                                                app.history_scroll = 0;
                                                app.detail_search_query.clear();
                                                app.detail_search_matches.clear();
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // Send command if any
                        if let Some(env) = cmd_to_send {
                            let json = serde_json::to_string(&env)?;
                            ws_write.send(Message::Text(json)).await?;
                        }
                    }
                }
            }
            // Handle server messages via WebSocket
            msg = ws_read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        // Try new RealtimeMessage format first
                        if let Ok(realtime) = serde_json::from_str::<RealtimeMessage>(&text) {
                            app.update_from_realtime(realtime);
                        }
                        // Fall back to legacy Envelope format
                        else if let Ok(env) = serde_json::from_str::<Envelope>(&text) {
                            app.update_from_envelope(env);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        app.message = "Server disconnected".to_string();
                        break;
                    }
                    Some(Err(e)) => {
                        app.message = format!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {} // Ignore ping/pong/binary
                }
            }
        }
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Ui { client }) => {
            run_ui(client).await
        }
        Some(Commands::Command {
            cmd_name,
            client,
            session,
            session_id,
            window,
            window_id,
            pane,
            summary,
            scope,
            note_id,
            project,
            prompt,
            reply,
            transcript,
            claude_session,
            search,
            limit,
            offset,
        }) => {
            run_command(
                cmd_name,
                client,
                session,
                session_id,
                window,
                window_id,
                pane,
                summary,
                scope,
                note_id,
                project,
                prompt,
                reply,
                transcript,
                claude_session,
                search,
                limit,
                offset,
            ).await
        }
        Some(Commands::State { client }) => {
            run_state(client).await
        }

        // Workspace management commands
        Some(Commands::Start {
            workspace,
            branch,
            agent,
            layout,
            attach,
        }) => run_start(workspace, branch, agent, layout, attach),

        Some(Commands::Resume {
            workspace,
            branch,
            attach,
        }) => run_resume(workspace, branch, attach),

        Some(Commands::Destroy {
            workspace,
            branch,
            force,
        }) => run_destroy(workspace, branch, force),

        Some(Commands::ListWorkspaces { workspace, json }) => run_list(workspace, json),

        None => {
            // Default: run UI
            run_ui(None).await
        }
    }
}

// ============ Workspace Management Functions ============

/// Start a new agent workspace
fn run_start(
    workspace_name: String,
    branch: String,
    agent_name: Option<String>,
    layout_name: Option<String>,
    attach: bool,
) -> Result<()> {
    use crate::agent::TmuxAgent;
    use crate::config::AgentConfig;
    use crate::workspace::GitWorktree;

    // Load config
    let config = AgentConfig::load()?;

    // Get workspace config
    let ws_config = config
        .get_workspace(&workspace_name)
        .ok_or_else(|| anyhow::anyhow!("Workspace '{}' not found in config", workspace_name))?;

    // Get agent
    let agent_name = agent_name.unwrap_or_else(|| config.defaults.agent.clone());
    let agent = config
        .get_agent(&agent_name)
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found in config", agent_name))?;

    // Get layout
    let layout_name = layout_name.unwrap_or_else(|| config.defaults.layout.clone());
    let layout = config
        .get_layout(&layout_name)
        .ok_or_else(|| anyhow::anyhow!("Layout '{}' not found in config", layout_name))?;

    // Create worktree
    let git = GitWorktree::from_config(ws_config);
    let worktree_path = git.create(&branch)?;
    println!("Created worktree at {:?}", worktree_path);

    // Create tmux session
    let session_name = TmuxAgent::session_name(&workspace_name, &branch);
    TmuxAgent::create_session(&session_name, &worktree_path, layout, agent)?;
    println!("Created tmux session: {}", session_name);

    // Attach if requested
    if attach {
        if TmuxAgent::is_inside_tmux() {
            TmuxAgent::switch_client(&session_name)?;
        } else {
            TmuxAgent::attach(&session_name)?;
        }
    }

    Ok(())
}

/// Resume an existing agent workspace
fn run_resume(
    workspace_name: Option<String>,
    branch: Option<String>,
    attach: bool,
) -> Result<()> {
    use crate::agent::TmuxAgent;

    // If both workspace and branch are provided, attach directly
    if let (Some(ws), Some(br)) = (&workspace_name, &branch) {
        let session_name = TmuxAgent::session_name(ws, br);
        if !TmuxAgent::session_exists(&session_name) {
            anyhow::bail!("Session '{}' does not exist", session_name);
        }

        if attach {
            if TmuxAgent::is_inside_tmux() {
                TmuxAgent::switch_client(&session_name)?;
            } else {
                TmuxAgent::attach(&session_name)?;
            }
        } else {
            println!("Session exists: {}", session_name);
        }
        return Ok(());
    }

    // List available sessions for selection
    let sessions = TmuxAgent::list_sessions()?;

    // Filter by workspace if provided
    let sessions: Vec<_> = if let Some(ws) = &workspace_name {
        sessions.into_iter().filter(|s| s.workspace == *ws).collect()
    } else {
        sessions
    };

    if sessions.is_empty() {
        println!("No active agent sessions found");
        return Ok(());
    }

    // Print sessions for selection
    println!("Active agent sessions:");
    for (i, session) in sessions.iter().enumerate() {
        let attached = if session.attached { " (attached)" } else { "" };
        println!(
            "  [{}] {}:{}{}",
            i + 1,
            session.workspace,
            session.branch,
            attached
        );
    }

    // If only one session, offer to attach
    if sessions.len() == 1 && attach {
        let session = &sessions[0];
        let session_name = TmuxAgent::session_name(&session.workspace, &session.branch);
        if TmuxAgent::is_inside_tmux() {
            TmuxAgent::switch_client(&session_name)?;
        } else {
            TmuxAgent::attach(&session_name)?;
        }
    }

    Ok(())
}

/// Destroy an agent workspace
fn run_destroy(workspace_name: String, branch: String, force: bool) -> Result<()> {
    use crate::agent::TmuxAgent;
    use crate::config::AgentConfig;
    use crate::workspace::GitWorktree;

    // Load config
    let config = AgentConfig::load()?;

    // Get workspace config
    let ws_config = config
        .get_workspace(&workspace_name)
        .ok_or_else(|| anyhow::anyhow!("Workspace '{}' not found in config", workspace_name))?;

    // Kill tmux session first
    let session_name = TmuxAgent::session_name(&workspace_name, &branch);
    if TmuxAgent::session_exists(&session_name) {
        TmuxAgent::kill_session(&session_name)?;
        println!("Killed tmux session: {}", session_name);
    }

    // Remove worktree
    let git = GitWorktree::from_config(ws_config);
    git.remove(&branch, force)?;
    println!("Removed worktree for branch: {}", branch);

    Ok(())
}

/// List all active agent workspaces
fn run_list(workspace_filter: Option<String>, json: bool) -> Result<()> {
    use crate::agent::TmuxAgent;

    let sessions = TmuxAgent::list_sessions()?;

    // Filter by workspace if provided
    let sessions: Vec<_> = if let Some(ws) = &workspace_filter {
        sessions.into_iter().filter(|s| s.workspace == *ws).collect()
    } else {
        sessions
    };

    if json {
        let json_output = serde_json::to_string_pretty(&sessions.iter().map(|s| {
            serde_json::json!({
                "workspace": s.workspace,
                "branch": s.branch,
                "session": s.session_name,
                "attached": s.attached
            })
        }).collect::<Vec<_>>())?;
        println!("{}", json_output);
    } else {
        if sessions.is_empty() {
            println!("No active agent sessions");
            return Ok(());
        }

        println!("{:<20} {:<30} {:<10}", "WORKSPACE", "BRANCH", "STATUS");
        println!("{}", "-".repeat(60));
        for session in &sessions {
            let status = if session.attached { "attached" } else { "detached" };
            println!("{:<20} {:<30} {:<10}", session.workspace, session.branch, status);
        }
    }

    Ok(())
}
