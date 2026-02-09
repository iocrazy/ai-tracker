//! Database module for tracker-server
//!
//! Handles SQLite persistence for tasks, notes, goals, and history.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tracing::info;

use tracker_core::{ConversationMessage, GitCommit, Goal, HistoryRecord, Note, NoteScope, Task, TaskStatus, ToolUsage};

/// Database wrapper with SQLite change tracking
pub struct Database {
    conn: Connection,
    /// Tables that changed since last broadcast (populated by SQLite update_hook)
    pub changed_tables: Arc<Mutex<HashSet<String>>>,
}

impl Database {
    /// Open or create database at the given path
    pub fn open(path: &Path) -> Result<Self> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Set up SQLite update hook for change tracking
        let changed = Arc::new(Mutex::new(HashSet::new()));
        let changed_clone = changed.clone();
        conn.update_hook(Some(move |_action: rusqlite::hooks::Action, _db: &str, table: &str, _rowid: i64| {
            if matches!(table, "tasks" | "notes" | "goals") {
                if let Ok(mut set) = changed_clone.lock() {
                    set.insert(table.to_string());
                }
            }
        }));

        let db = Self { conn, changed_tables: changed };
        db.init_schema()?;
        Ok(db)
    }

    /// Take the set of changed tables (clears it for next broadcast cycle)
    pub fn take_changes(&self) -> HashSet<String> {
        let mut set = self.changed_tables.lock().unwrap();
        std::mem::take(&mut *set)
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<()> {
        // Active tasks table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS tasks (
                key TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                session TEXT DEFAULT '',
                window_id TEXT NOT NULL,
                window TEXT DEFAULT '',
                pane TEXT DEFAULT '',
                status TEXT NOT NULL DEFAULT 'in_progress',
                summary TEXT NOT NULL,
                completion_note TEXT DEFAULT '',
                started_at TEXT,
                completed_at TEXT,
                duration_seconds REAL DEFAULT 0,
                acknowledged INTEGER DEFAULT 1
            )",
            [],
        )?;

        // History table for completed tasks
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                session TEXT DEFAULT '',
                window_id TEXT NOT NULL,
                window TEXT DEFAULT '',
                pane TEXT DEFAULT '',
                summary TEXT NOT NULL,
                completion_note TEXT DEFAULT '',
                started_at TEXT,
                completed_at TEXT,
                duration_seconds REAL DEFAULT 0,
                transcript_path TEXT DEFAULT ''
            )",
            [],
        )?;

        // Add transcript_path column if it doesn't exist (migration)
        let _ = self.conn.execute(
            "ALTER TABLE history ADD COLUMN transcript_path TEXT DEFAULT ''",
            [],
        );

        // Conversation messages table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS conversation_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                history_id INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT,
                FOREIGN KEY (history_id) REFERENCES history(id)
            )",
            [],
        )?;

        // Index for conversation messages
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_messages_history
             ON conversation_messages(history_id)",
            [],
        )?;

        // Tool usage table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS tool_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                history_id INTEGER NOT NULL,
                tool_name TEXT NOT NULL,
                tool_args TEXT DEFAULT '',
                result_summary TEXT DEFAULT '',
                success INTEGER DEFAULT 1,
                timestamp TEXT,
                FOREIGN KEY (history_id) REFERENCES history(id)
            )",
            [],
        )?;

        // Index for tool usage
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tool_usage_history
             ON tool_usage(history_id)",
            [],
        )?;

        // Git commits table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS commits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                history_id INTEGER NOT NULL,
                commit_hash TEXT NOT NULL,
                commit_message TEXT NOT NULL,
                files_changed INTEGER DEFAULT 0,
                timestamp TEXT,
                FOREIGN KEY (history_id) REFERENCES history(id)
            )",
            [],
        )?;

        // Index for commits
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_commits_history
             ON commits(history_id)",
            [],
        )?;

        // Notes table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS notes (
                id TEXT PRIMARY KEY,
                scope TEXT NOT NULL DEFAULT 'window',
                session_id TEXT NOT NULL,
                session TEXT DEFAULT '',
                window_id TEXT DEFAULT '',
                window TEXT DEFAULT '',
                pane TEXT DEFAULT '',
                summary TEXT NOT NULL,
                completed INTEGER DEFAULT 0,
                archived INTEGER DEFAULT 0,
                created_at TEXT,
                archived_at TEXT
            )",
            [],
        )?;

        // Goals table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS goals (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                session TEXT DEFAULT '',
                summary TEXT NOT NULL,
                completed INTEGER DEFAULT 0,
                created_at TEXT,
                updated_at TEXT
            )",
            [],
        )?;

        // Closed windows table (for resume without worktree)
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS closed_windows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                session_name TEXT NOT NULL,
                window_name TEXT NOT NULL,
                working_dir TEXT DEFAULT '',
                git_branch TEXT DEFAULT '',
                pane_count INTEGER DEFAULT 1,
                closed_at TEXT NOT NULL
            )",
            [],
        )?;

        // Migration: Add pane_count column if it doesn't exist
        let _ = self.conn.execute(
            "ALTER TABLE closed_windows ADD COLUMN pane_count INTEGER DEFAULT 1",
            [],
        );

        // Index for closed windows
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_closed_windows_session
             ON closed_windows(session_id)",
            [],
        )?;

        // Session index table (populated by background scanner from Claude JSONL files)
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS session_index (
                file_path TEXT PRIMARY KEY,
                project TEXT NOT NULL DEFAULT '',
                summary TEXT NOT NULL DEFAULT '',
                started_at TEXT,
                ended_at TEXT,
                message_count INTEGER DEFAULT 0,
                duration_seconds REAL DEFAULT 0,
                file_size INTEGER DEFAULT 0,
                file_mtime TEXT
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_index_started ON session_index(started_at)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_index_ended ON session_index(ended_at)",
            [],
        )?;

        // Create indices
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_history_session ON history(session_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_history_completed_at ON history(completed_at)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_notes_session ON notes(session_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_goals_session ON goals(session_id)",
            [],
        )?;

        info!("Database schema initialized");
        Ok(())
    }

    // =========================================================================
    // Task operations
    // =========================================================================

    /// Save a task (insert or update)
    pub fn save_task(&self, task: &Task) -> Result<()> {
        let key = task.key();
        self.conn.execute(
            "INSERT OR REPLACE INTO tasks
             (key, session_id, session, window_id, window, pane, status, summary,
              completion_note, started_at, completed_at, duration_seconds, acknowledged)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                key,
                task.session_id,
                task.session,
                task.window_id,
                task.window,
                task.pane,
                task.status.as_str(),
                task.summary,
                task.completion_note,
                task.started_at.map(|t| t.to_rfc3339()),
                task.completed_at.map(|t| t.to_rfc3339()),
                task.duration_seconds,
                task.acknowledged as i32,
            ],
        )?;
        Ok(())
    }

    /// Delete a task by key
    pub fn delete_task(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM tasks WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Cleanup stale tasks on startup (orphan recovery)
    /// - Delete tasks with empty keys (dirty data)
    /// - Delete completed tasks (no longer needed, Timeline uses session JSONL)
    /// - Reset in_progress tasks to awaiting_input (hook chain broken by restart)
    pub fn cleanup_stale_tasks(&self) -> Result<(usize, usize, usize)> {
        // 1. Delete dirty data (empty key fields)
        let dirty = self.conn.execute(
            "DELETE FROM tasks WHERE TRIM(session_id) = '' OR TRIM(window_id) = ''",
            [],
        )?;

        // 2. Delete completed tasks
        let completed = self.conn.execute(
            "DELETE FROM tasks WHERE status = 'completed'",
            [],
        )?;

        // 3. Reset in_progress to awaiting_input
        let reset = self.conn.execute(
            "UPDATE tasks SET status = 'awaiting_input' WHERE status = 'in_progress'",
            [],
        )?;

        Ok((dirty, completed, reset))
    }

    /// Load all active tasks
    pub fn load_tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, session, window_id, window, pane, status, summary,
                    completion_note, started_at, completed_at, duration_seconds, acknowledged
             FROM tasks",
        )?;

        let tasks = stmt
            .query_map([], |row| {
                let status_str: String = row.get(5)?;
                let status = match status_str.as_str() {
                    "awaiting_input" => TaskStatus::AwaitingInput,
                    "completed" => TaskStatus::Completed,
                    _ => TaskStatus::InProgress,
                };

                let started_at: Option<String> = row.get(8)?;
                let completed_at: Option<String> = row.get(9)?;

                Ok(Task {
                    session_id: row.get(0)?,
                    session: row.get(1)?,
                    window_id: row.get(2)?,
                    window: row.get(3)?,
                    pane: row.get(4)?,
                    status,
                    summary: row.get(6)?,
                    completion_note: row.get(7)?,
                    started_at: started_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    completed_at: completed_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    duration_seconds: row.get(10)?,
                    acknowledged: row.get::<_, i32>(11)? != 0,
                    archived: false,
                    archived_at: None,
                    transcript_path: String::new(), // Tasks table doesn't store this
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tasks)
    }

    /// Archive a completed task to history (upsert: merge with recent entry for same session/window/pane)
    pub fn archive_to_history(&self, task: &Task) -> Result<i64> {
        // Check for a recent history entry for the same session/window/pane (within 60 minutes)
        let recent_id: Option<i64> = self.conn.query_row(
            "SELECT id FROM history
             WHERE session_id = ?1 AND window_id = ?2 AND pane = ?3
               AND completed_at > datetime('now', '-60 minutes')
             ORDER BY id DESC LIMIT 1",
            params![task.session_id, task.window_id, task.pane],
            |row| row.get(0),
        ).ok();

        if let Some(id) = recent_id {
            // Update existing entry — keep the longer/more informative summary
            self.conn.execute(
                "UPDATE history SET
                    summary = CASE WHEN LENGTH(?1) > LENGTH(summary) THEN ?1 ELSE summary END,
                    completion_note = CASE WHEN ?2 != '' THEN ?2 ELSE completion_note END,
                    completed_at = ?3,
                    duration_seconds = ?4,
                    transcript_path = CASE WHEN ?5 != '' THEN ?5 ELSE transcript_path END
                 WHERE id = ?6",
                params![
                    task.summary,
                    task.completion_note,
                    task.completed_at.map(|t| t.to_rfc3339()),
                    task.duration_seconds,
                    task.transcript_path,
                    id,
                ],
            )?;
            // Clear old related data (will be re-parsed from transcript)
            let _ = self.conn.execute("DELETE FROM conversation_messages WHERE history_id = ?", [id]);
            let _ = self.conn.execute("DELETE FROM tool_usage WHERE history_id = ?", [id]);
            let _ = self.conn.execute("DELETE FROM commits WHERE history_id = ?", [id]);
            Ok(id)
        } else {
            // Insert new entry
            self.conn.execute(
                "INSERT INTO history
                 (session_id, session, window_id, window, pane, summary,
                  completion_note, started_at, completed_at, duration_seconds, transcript_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    task.session_id,
                    task.session,
                    task.window_id,
                    task.window,
                    task.pane,
                    task.summary,
                    task.completion_note,
                    task.started_at.map(|t| t.to_rfc3339()),
                    task.completed_at.map(|t| t.to_rfc3339()),
                    task.duration_seconds,
                    task.transcript_path,
                ],
            )?;
            Ok(self.conn.last_insert_rowid())
        }
    }

    /// Get recent history entries
    #[allow(dead_code)]
    pub fn get_history(&self, limit: i64) -> Result<Vec<HistoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, session, window_id, window, pane, summary,
                    completion_note, started_at, completed_at, duration_seconds,
                    COALESCE(transcript_path, '')
             FROM history
             ORDER BY completed_at DESC
             LIMIT ?1",
        )?;

        let records = stmt
            .query_map([limit], |row| {
                let started_at: Option<String> = row.get(8)?;
                let completed_at: Option<String> = row.get(9)?;

                Ok(HistoryRecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    session: row.get(2)?,
                    window_id: row.get(3)?,
                    window: row.get(4)?,
                    pane: row.get(5)?,
                    summary: row.get(6)?,
                    completion_note: row.get(7)?,
                    started_at: started_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    completed_at: completed_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    duration_seconds: row.get(10)?,
                    transcript_path: row.get(11)?,
                    messages: vec![],
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    // =========================================================================
    // Note operations
    // =========================================================================

    /// Save a note (insert or update)
    pub fn save_note(&self, note: &Note) -> Result<()> {
        let scope_str = match note.scope {
            NoteScope::Window => "window",
            NoteScope::Session => "session",
            NoteScope::All => "all",
        };

        self.conn.execute(
            "INSERT OR REPLACE INTO notes
             (id, scope, session_id, session, window_id, window, pane, summary,
              completed, archived, created_at, archived_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                note.id,
                scope_str,
                note.session_id,
                note.session,
                note.window_id,
                note.window,
                note.pane,
                note.summary,
                note.completed as i32,
                note.archived as i32,
                note.created_at.map(|t| t.to_rfc3339()),
                note.archived_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Delete a note by ID
    pub fn delete_note(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Load all active notes (not archived)
    pub fn load_notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope, session_id, session, window_id, window, pane, summary,
                    completed, archived, created_at, archived_at
             FROM notes
             WHERE archived = 0",
        )?;

        let notes = stmt
            .query_map([], |row| {
                let scope_str: String = row.get(1)?;
                let scope = match scope_str.as_str() {
                    "session" => NoteScope::Session,
                    "all" => NoteScope::All,
                    _ => NoteScope::Window,
                };

                let created_at: Option<String> = row.get(10)?;
                let archived_at: Option<String> = row.get(11)?;

                Ok(Note {
                    id: row.get(0)?,
                    scope,
                    session_id: row.get(2)?,
                    session: row.get(3)?,
                    window_id: row.get(4)?,
                    window: row.get(5)?,
                    pane: row.get(6)?,
                    summary: row.get(7)?,
                    completed: row.get::<_, i32>(8)? != 0,
                    archived: row.get::<_, i32>(9)? != 0,
                    created_at: created_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    archived_at: archived_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(notes)
    }

    /// Load archived notes
    #[allow(dead_code)]
    pub fn load_archived_notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope, session_id, session, window_id, window, pane, summary,
                    completed, archived, created_at, archived_at
             FROM notes
             WHERE archived = 1
             ORDER BY archived_at DESC",
        )?;

        let notes = stmt
            .query_map([], |row| {
                let scope_str: String = row.get(1)?;
                let scope = match scope_str.as_str() {
                    "session" => NoteScope::Session,
                    "all" => NoteScope::All,
                    _ => NoteScope::Window,
                };

                let created_at: Option<String> = row.get(10)?;
                let archived_at: Option<String> = row.get(11)?;

                Ok(Note {
                    id: row.get(0)?,
                    scope,
                    session_id: row.get(2)?,
                    session: row.get(3)?,
                    window_id: row.get(4)?,
                    window: row.get(5)?,
                    pane: row.get(6)?,
                    summary: row.get(7)?,
                    completed: row.get::<_, i32>(8)? != 0,
                    archived: row.get::<_, i32>(9)? != 0,
                    created_at: created_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    archived_at: archived_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(notes)
    }

    // =========================================================================
    // Goal operations
    // =========================================================================

    /// Save a goal (insert or update)
    pub fn save_goal(&self, goal: &Goal) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO goals
             (id, session_id, session, summary, completed, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                goal.id,
                goal.session_id,
                goal.session,
                goal.summary,
                goal.completed as i32,
                goal.created_at.map(|t| t.to_rfc3339()),
                goal.updated_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Delete a goal by ID
    pub fn delete_goal(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM goals WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Load all goals
    pub fn load_goals(&self) -> Result<Vec<Goal>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, session, summary, completed, created_at, updated_at
             FROM goals",
        )?;

        let goals = stmt
            .query_map([], |row| {
                let created_at: Option<String> = row.get(5)?;
                let updated_at: Option<String> = row.get(6)?;

                Ok(Goal {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    session: row.get(2)?,
                    summary: row.get(3)?,
                    completed: row.get::<_, i32>(4)? != 0,
                    created_at: created_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    updated_at: updated_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(goals)
    }

    /// Load task history records (most recent first, limited)
    /// Also loads conversation messages for each record
    pub fn load_history(&self, limit: i32) -> Result<Vec<HistoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, session, window_id, window, pane,
                    summary, completion_note, started_at, completed_at, duration_seconds,
                    COALESCE(transcript_path, '') as transcript_path
             FROM history
             ORDER BY id DESC
             LIMIT ?",
        )?;

        let mut records: Vec<HistoryRecord> = stmt
            .query_map([limit], |row| {
                let started_at: Option<String> = row.get(8)?;
                let completed_at: Option<String> = row.get(9)?;

                Ok(HistoryRecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    session: row.get(2)?,
                    window_id: row.get(3)?,
                    window: row.get(4)?,
                    pane: row.get(5)?,
                    summary: row.get(6)?,
                    completion_note: row.get(7)?,
                    started_at: started_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    completed_at: completed_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    duration_seconds: row.get(10)?,
                    transcript_path: row.get(11)?,
                    messages: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Load conversation messages for each record
        for record in &mut records {
            if let Ok(messages) = self.load_conversation_messages(record.id) {
                record.messages = messages;
            }
        }

        Ok(records)
    }

    /// Save conversation messages for a history record
    pub fn save_conversation_messages(&self, history_id: i64, messages: &[ConversationMessage]) -> Result<()> {
        for msg in messages {
            self.conn.execute(
                "INSERT INTO conversation_messages (history_id, role, content, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    history_id,
                    msg.role,
                    msg.content,
                    msg.created_at.map(|t| t.to_rfc3339()),
                ],
            )?;
        }
        Ok(())
    }

    /// Load conversation messages for a history record
    pub fn load_conversation_messages(&self, history_id: i64) -> Result<Vec<ConversationMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, history_id, role, content, created_at
             FROM conversation_messages
             WHERE history_id = ?
             ORDER BY id ASC",
        )?;

        let messages = stmt
            .query_map([history_id], |row| {
                let created_at: Option<String> = row.get(4)?;
                Ok(ConversationMessage {
                    id: row.get(0)?,
                    history_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    created_at: created_at
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Get the last inserted row ID
    pub fn last_insert_rowid(&self) -> i64 {
        self.conn.last_insert_rowid()
    }

    // =========================================================================
    // Tool Usage operations
    // =========================================================================

    /// Save tool usage records for a history entry
    pub fn save_tool_usage(&self, history_id: i64, tool_usages: &[ToolUsage]) -> Result<()> {
        for usage in tool_usages {
            self.conn.execute(
                "INSERT INTO tool_usage (history_id, tool_name, tool_args, result_summary, success, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    history_id,
                    usage.tool_name,
                    usage.tool_args,
                    usage.result_summary,
                    usage.success as i32,
                    usage.timestamp.map(|t| t.to_rfc3339()),
                ],
            )?;
        }
        Ok(())
    }

    /// Load tool usage records for a history entry
    pub fn load_tool_usage(&self, history_id: i64) -> Result<Vec<ToolUsage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, history_id, tool_name, tool_args, result_summary, success, timestamp
             FROM tool_usage
             WHERE history_id = ?
             ORDER BY id ASC",
        )?;

        let records = stmt
            .query_map([history_id], |row| {
                let timestamp: Option<String> = row.get(6)?;
                Ok(ToolUsage {
                    id: row.get(0)?,
                    history_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    tool_args: row.get(3)?,
                    result_summary: row.get(4)?,
                    success: row.get::<_, i32>(5)? != 0,
                    timestamp: timestamp
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    // =========================================================================
    // Git Commits operations
    // =========================================================================

    /// Save git commit records for a history entry
    pub fn save_commits(&self, history_id: i64, commits: &[GitCommit]) -> Result<()> {
        for commit in commits {
            self.conn.execute(
                "INSERT INTO commits (history_id, commit_hash, commit_message, files_changed, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    history_id,
                    commit.commit_hash,
                    commit.commit_message,
                    commit.files_changed,
                    commit.timestamp.map(|t| t.to_rfc3339()),
                ],
            )?;
        }
        Ok(())
    }

    /// Load git commit records for a history entry
    pub fn load_commits(&self, history_id: i64) -> Result<Vec<GitCommit>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, history_id, commit_hash, commit_message, files_changed, timestamp
             FROM commits
             WHERE history_id = ?
             ORDER BY id ASC",
        )?;

        let records = stmt
            .query_map([history_id], |row| {
                let timestamp: Option<String> = row.get(5)?;
                Ok(GitCommit {
                    id: row.get(0)?,
                    history_id: row.get(1)?,
                    commit_hash: row.get(2)?,
                    commit_message: row.get(3)?,
                    files_changed: row.get(4)?,
                    timestamp: timestamp
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    // =========================================================================
    // Enhanced History operations
    // =========================================================================

    /// Load history with pagination and date filtering
    pub fn load_history_paginated(
        &self,
        page: i32,
        per_page: i32,
        start_date: Option<&str>,
        end_date: Option<&str>,
        search: Option<&str>,
    ) -> Result<(Vec<HistoryRecord>, i64)> {
        let offset = (page - 1) * per_page;

        // Build WHERE clause
        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = start_date {
            conditions.push("completed_at >= ?");
            params_vec.push(Box::new(start.to_string()));
        }
        if let Some(end) = end_date {
            conditions.push("completed_at <= ?");
            params_vec.push(Box::new(end.to_string()));
        }
        if let Some(q) = search {
            conditions.push("(summary LIKE ? OR completion_note LIKE ?)");
            let pattern = format!("%{}%", q);
            params_vec.push(Box::new(pattern.clone()));
            params_vec.push(Box::new(pattern));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Get total count
        let count_sql = format!("SELECT COUNT(*) FROM history {}", where_clause);
        let total: i64 = {
            let mut stmt = self.conn.prepare(&count_sql)?;
            let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            stmt.query_row(params_refs.as_slice(), |row| row.get(0))?
        };

        // Get paginated records
        let select_sql = format!(
            "SELECT id, session_id, session, window_id, window, pane,
                    summary, completion_note, started_at, completed_at, duration_seconds,
                    COALESCE(transcript_path, '') as transcript_path
             FROM history
             {}
             ORDER BY completed_at DESC
             LIMIT ? OFFSET ?",
            where_clause
        );

        params_vec.push(Box::new(per_page));
        params_vec.push(Box::new(offset));

        let mut stmt = self.conn.prepare(&select_sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        let records = stmt
            .query_map(params_refs.as_slice(), |row| {
                let started_at: Option<String> = row.get(8)?;
                let completed_at: Option<String> = row.get(9)?;

                Ok(HistoryRecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    session: row.get(2)?,
                    window_id: row.get(3)?,
                    window: row.get(4)?,
                    pane: row.get(5)?,
                    summary: row.get(6)?,
                    completion_note: row.get(7)?,
                    started_at: started_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    completed_at: completed_at
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    duration_seconds: row.get(10)?,
                    transcript_path: row.get(11)?,
                    messages: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok((records, total))
    }

    /// Get history record by ID with all related data
    pub fn get_history_detail(&self, history_id: i64) -> Result<Option<HistoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, session, window_id, window, pane,
                    summary, completion_note, started_at, completed_at, duration_seconds,
                    COALESCE(transcript_path, '') as transcript_path
             FROM history
             WHERE id = ?",
        )?;

        let record = stmt.query_row([history_id], |row| {
            let started_at: Option<String> = row.get(8)?;
            let completed_at: Option<String> = row.get(9)?;

            Ok(HistoryRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                session: row.get(2)?,
                window_id: row.get(3)?,
                window: row.get(4)?,
                pane: row.get(5)?,
                summary: row.get(6)?,
                completion_note: row.get(7)?,
                started_at: started_at
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                completed_at: completed_at
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                duration_seconds: row.get(10)?,
                transcript_path: row.get(11)?,
                messages: Vec::new(),
            })
        });

        match record {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // =========================================================================
    // Closed windows operations (for resume without worktree)
    // =========================================================================

    /// Save a closed window for later resume
    pub fn save_closed_window(
        &self,
        session_id: &str,
        session_name: &str,
        window_name: &str,
        working_dir: &str,
        git_branch: &str,
        pane_count: i32,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO closed_windows
             (session_id, session_name, window_name, working_dir, git_branch, pane_count, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session_id,
                session_name,
                window_name,
                working_dir,
                git_branch,
                pane_count,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Load closed windows for a session (excludes windows that are currently open)
    pub fn load_closed_windows(&self, session_name: &str, open_window_names: &[String]) -> Result<Vec<ClosedWindow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, session_name, window_name, working_dir, git_branch, pane_count, closed_at
             FROM closed_windows
             WHERE session_name = ?1
             ORDER BY closed_at DESC
             LIMIT 50",
        )?;

        let windows = stmt
            .query_map([session_name], |row| {
                let closed_at: String = row.get(7)?;
                Ok(ClosedWindow {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    session_name: row.get(2)?,
                    window_name: row.get(3)?,
                    working_dir: row.get(4)?,
                    git_branch: row.get(5)?,
                    pane_count: row.get(6)?,
                    closed_at: DateTime::parse_from_rfc3339(&closed_at)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })?
            .filter_map(|r| r.ok())
            // Exclude windows that are currently open
            .filter(|w| !open_window_names.contains(&w.window_name))
            .collect();

        Ok(windows)
    }

    /// Delete a closed window record (after it's been resumed)
    pub fn delete_closed_window(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM closed_windows WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Delete closed window by name (when window is opened)
    pub fn delete_closed_window_by_name(&self, session_name: &str, window_name: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM closed_windows WHERE session_name = ?1 AND window_name = ?2",
            params![session_name, window_name],
        )?;
        Ok(())
    }

    // =========================================================================
    // Session index operations (populated by background JSONL scanner)
    // =========================================================================

    /// Upsert a session index entry
    pub fn upsert_session_index(&self, entry: &SessionIndexEntry) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO session_index
             (file_path, project, summary, started_at, ended_at, message_count,
              duration_seconds, file_size, file_mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.file_path,
                entry.project,
                entry.summary,
                entry.started_at,
                entry.ended_at,
                entry.message_count,
                entry.duration_seconds,
                entry.file_size,
                entry.file_mtime,
            ],
        )?;
        Ok(())
    }

    /// Check if a session file needs re-indexing (by comparing mtime + size)
    pub fn session_needs_reindex(&self, file_path: &str, file_size: i64, file_mtime: &str) -> bool {
        let result: Option<(i64, String)> = self.conn.query_row(
            "SELECT file_size, COALESCE(file_mtime, '') FROM session_index WHERE file_path = ?1",
            [file_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok();

        match result {
            Some((size, mtime)) => size != file_size || mtime != file_mtime,
            None => true, // Not indexed yet
        }
    }

    /// Load session index entries with pagination and filtering
    pub fn load_sessions(
        &self,
        page: i64,
        page_size: i64,
        time_after: Option<&str>,
        search: Option<&str>,
    ) -> Result<(Vec<SessionIndexEntry>, i64)> {
        let mut where_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(after) = time_after {
            where_clauses.push(format!("started_at >= ?{}", param_values.len() + 1));
            param_values.push(Box::new(after.to_string()));
        }

        if let Some(q) = search {
            where_clauses.push(format!("(summary LIKE ?{} OR project LIKE ?{})", param_values.len() + 1, param_values.len() + 2));
            let pattern = format!("%{}%", q);
            param_values.push(Box::new(pattern.clone()));
            param_values.push(Box::new(pattern));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        // Count total
        let count_sql = format!("SELECT COUNT(*) FROM session_index {}", where_sql);
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let total: i64 = self.conn.query_row(&count_sql, params_ref.as_slice(), |row| row.get(0))?;

        // Query with pagination
        let offset = (page - 1) * page_size;
        let query_sql = format!(
            "SELECT file_path, project, summary, COALESCE(started_at, ''), COALESCE(ended_at, ''),
                    message_count, duration_seconds, file_size, COALESCE(file_mtime, '')
             FROM session_index {}
             ORDER BY ended_at DESC
             LIMIT ?{} OFFSET ?{}",
            where_sql,
            param_values.len() + 1,
            param_values.len() + 2,
        );
        let mut all_params = param_values;
        all_params.push(Box::new(page_size));
        all_params.push(Box::new(offset));
        let all_params_ref: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&query_sql)?;
        let entries = stmt.query_map(all_params_ref.as_slice(), |row| {
            Ok(SessionIndexEntry {
                file_path: row.get(0)?,
                project: row.get(1)?,
                summary: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                message_count: row.get(5)?,
                duration_seconds: row.get(6)?,
                file_size: row.get(7)?,
                file_mtime: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok((entries, total))
    }

    /// Remove session index entries whose files no longer exist
    pub fn cleanup_stale_sessions(&self, valid_paths: &[String]) -> Result<usize> {
        if valid_paths.is_empty() {
            return Ok(0);
        }
        // Get all indexed paths
        let mut stmt = self.conn.prepare("SELECT file_path FROM session_index")?;
        let all_paths: Vec<String> = stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut removed = 0;
        for path in &all_paths {
            if !valid_paths.contains(path) {
                self.conn.execute("DELETE FROM session_index WHERE file_path = ?1", [path])?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Closed window record
#[derive(Debug, Clone)]
pub struct ClosedWindow {
    pub id: i64,
    pub session_id: String,
    pub session_name: String,
    pub window_name: String,
    pub working_dir: String,
    pub git_branch: String,
    pub pane_count: i32,
    pub closed_at: Option<DateTime<Utc>>,
}

/// Session index entry (metadata from Claude JSONL session files)
#[derive(Debug, Clone)]
pub struct SessionIndexEntry {
    pub file_path: String,
    pub project: String,
    pub summary: String,
    pub started_at: String,
    pub ended_at: String,
    pub message_count: i32,
    pub duration_seconds: f64,
    pub file_size: i64,
    pub file_mtime: String,
}

/// Get default database path
pub fn default_db_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".config")
        .join("agent-tracker")
        .join("data")
        .join("tracker.db")
}
