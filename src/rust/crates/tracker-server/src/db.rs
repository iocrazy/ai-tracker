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

use crate::project_db;

/// Database wrapper with SQLite change tracking
pub struct Database {
    pub(crate) conn: Connection,
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

        // Projects table (registry of known git projects for per-project storage)
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS projects (
                git_dir TEXT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                last_session TEXT DEFAULT '',
                last_window TEXT DEFAULT '',
                last_active_at TEXT,
                notes_count INTEGER DEFAULT 0,
                goals_count INTEGER DEFAULT 0,
                history_count INTEGER DEFAULT 0
            )",
            [],
        )?;

        // Project environment variables table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS project_env_vars (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                session_name TEXT NOT NULL,
                key          TEXT NOT NULL,
                value        TEXT NOT NULL DEFAULT '',
                is_secret    INTEGER NOT NULL DEFAULT 0,
                sort_order   INTEGER NOT NULL DEFAULT 0,
                created_at   TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(session_name, key)
            )",
            [],
        )?;

        // Project services table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS project_services (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                session_name TEXT NOT NULL,
                service_name TEXT NOT NULL,
                base_value   INTEGER NOT NULL,
                value_type   TEXT NOT NULL DEFAULT 'port',
                env_key      TEXT NOT NULL,
                sort_order   INTEGER NOT NULL DEFAULT 0,
                UNIQUE(session_name, service_name)
            )",
            [],
        )?;

        // Worktree slots table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS worktree_slots (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                session_name  TEXT NOT NULL,
                slot          INTEGER NOT NULL,
                branch        TEXT NOT NULL,
                worktree_path TEXT,
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(session_name, slot),
                UNIQUE(session_name, branch)
            )",
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

        // === Schema migrations ===
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT DEFAULT (datetime('now'))
            )", [],
        )?;

        let current_version: i32 = self.conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |row| row.get(0))
            .unwrap_or(0);

        let migrations: &[(i32, &str)] = &[
            (1, "CREATE TABLE IF NOT EXISTS global_env_vars (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT NOT NULL UNIQUE,
                value TEXT NOT NULL DEFAULT '',
                is_secret INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"),
            (2, "CREATE TABLE IF NOT EXISTS worktree_env_vars (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_name TEXT NOT NULL,
                slot INTEGER NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL DEFAULT '',
                is_secret INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(session_name, slot, key)
            )"),
            (3, "ALTER TABLE projects ADD COLUMN description TEXT DEFAULT ''"),
            (4, "ALTER TABLE projects ADD COLUMN status TEXT DEFAULT 'active'"),
            (5, "ALTER TABLE projects ADD COLUMN tags TEXT DEFAULT ''"),
            (6, "ALTER TABLE projects ADD COLUMN created_at TEXT DEFAULT ''"),
            (7, "CREATE TABLE IF NOT EXISTS notifications (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type TEXT NOT NULL,
                session_name TEXT,
                message TEXT NOT NULL,
                read INTEGER DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now'))
            )"),
            (8, "CREATE TABLE IF NOT EXISTS alert_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                condition_type TEXT NOT NULL,
                threshold_seconds INTEGER,
                enabled INTEGER DEFAULT 1,
                channels TEXT DEFAULT 'web',
                created_at TEXT DEFAULT (datetime('now'))
            )"),
            (9, "ALTER TABLE projects ADD COLUMN tech_stack TEXT DEFAULT ''"),
            (10, "CREATE TABLE IF NOT EXISTS project_todos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                git_dir TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT DEFAULT '',
                status TEXT NOT NULL DEFAULT 'todo',
                priority INTEGER DEFAULT 0,
                sort_order INTEGER DEFAULT 0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )"),
        ];

        for (version, sql) in migrations {
            if *version > current_version {
                self.conn.execute(sql, [])?;
                self.conn.execute(
                    "INSERT INTO schema_version (version) VALUES (?1)",
                    params![version],
                )?;
                info!("Applied migration v{}", version);
            }
        }

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
        project_db::archive_to_history_on(&self.conn, task)
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
        project_db::save_conversation_messages_on(&self.conn, history_id, messages)
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
        project_db::save_tool_usage_on(&self.conn, history_id, tool_usages)
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
        project_db::save_commits_on(&self.conn, history_id, commits)
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

    /// Save a closed window for later resume (deduplicates by session_name + window_name)
    pub fn save_closed_window(
        &self,
        session_id: &str,
        session_name: &str,
        window_name: &str,
        working_dir: &str,
        git_branch: &str,
        pane_count: i32,
    ) -> Result<()> {
        // Delete any existing record with the same session_name + window_name to prevent duplicates
        self.conn.execute(
            "DELETE FROM closed_windows WHERE session_name = ?1 AND window_name = ?2",
            params![session_name, window_name],
        )?;
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

    /// Load closed windows for a session (excludes windows that are currently open, deduplicates by window_name)
    pub fn load_closed_windows(&self, session_name: &str, open_window_names: &[String]) -> Result<Vec<ClosedWindow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, session_name, window_name, working_dir, git_branch, pane_count, closed_at
             FROM closed_windows
             WHERE session_name = ?1 AND id IN (
                 SELECT MAX(id) FROM closed_windows WHERE session_name = ?1 GROUP BY window_name
             )
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

    // =========================================================================
    // Project registry operations
    // =========================================================================

    /// Register a project (insert or ignore if already exists)
    pub fn register_project(&self, git_dir: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO projects (git_dir, name, last_active_at)
             VALUES (?1, ?2, ?3)",
            params![git_dir, name, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Update project activity timestamps (counts are synced separately by update_project_counts)
    pub fn update_project_activity(&self, git_dir: &str, session: &str, window: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET
                last_session = ?2,
                last_window = ?3,
                last_active_at = ?4
             WHERE git_dir = ?1",
            params![git_dir, session, window, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Update project counts from project DB stats
    pub fn update_project_counts(&self, git_dir: &str, history_count: i32, notes_count: i32, goals_count: i32) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET
                history_count = ?2,
                notes_count = ?3,
                goals_count = ?4,
                last_active_at = ?5
             WHERE git_dir = ?1",
            params![git_dir, history_count, notes_count, goals_count, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// List all registered projects
    pub fn list_projects(&self) -> Result<Vec<ProjectInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT git_dir, name, last_session, last_window, last_active_at,
                    notes_count, goals_count, history_count,
                    COALESCE(description, ''), COALESCE(status, 'active'),
                    COALESCE(tags, ''), COALESCE(created_at, ''),
                    COALESCE(tech_stack, ''),
                    COALESCE((SELECT COUNT(*) FROM project_todos WHERE project_todos.git_dir = projects.git_dir AND status != 'done'), 0)
             FROM projects
             ORDER BY last_active_at DESC",
        )?;

        let projects = stmt
            .query_map([], |row| {
                Ok(ProjectInfo {
                    git_dir: row.get(0)?,
                    name: row.get(1)?,
                    last_session: row.get(2)?,
                    last_window: row.get(3)?,
                    last_active_at: row.get(4)?,
                    notes_count: row.get(5)?,
                    goals_count: row.get(6)?,
                    history_count: row.get(7)?,
                    description: row.get(8)?,
                    status: row.get(9)?,
                    tags: row.get(10)?,
                    created_at: row.get(11)?,
                    tech_stack: row.get(12)?,
                    todos_count: row.get(13)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// Get a single project by git_dir
    pub fn get_project(&self, git_dir: &str) -> Result<Option<ProjectInfo>> {
        let result = self.conn.query_row(
            "SELECT git_dir, name, last_session, last_window, last_active_at,
                    notes_count, goals_count, history_count,
                    COALESCE(description, ''), COALESCE(status, 'active'),
                    COALESCE(tags, ''), COALESCE(created_at, ''),
                    COALESCE(tech_stack, ''),
                    COALESCE((SELECT COUNT(*) FROM project_todos WHERE project_todos.git_dir = projects.git_dir AND status != 'done'), 0)
             FROM projects WHERE git_dir = ?1",
            params![git_dir],
            |row| {
                Ok(ProjectInfo {
                    git_dir: row.get(0)?,
                    name: row.get(1)?,
                    last_session: row.get(2)?,
                    last_window: row.get(3)?,
                    last_active_at: row.get(4)?,
                    notes_count: row.get(5)?,
                    goals_count: row.get(6)?,
                    history_count: row.get(7)?,
                    description: row.get(8)?,
                    status: row.get(9)?,
                    tags: row.get(10)?,
                    created_at: row.get(11)?,
                    tech_stack: row.get(12)?,
                    todos_count: row.get(13)?,
                })
            },
        );

        match result {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update project metadata (description, status, tags)
    pub fn update_project(&self, git_dir: &str, description: Option<&str>, status: Option<&str>, tags: Option<&str>, tech_stack: Option<&str>) -> Result<()> {
        let mut sets = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];
        if let Some(v) = description { sets.push(format!("description = ?{}", params_vec.len() + 1)); params_vec.push(Box::new(v.to_string())); }
        if let Some(v) = status { sets.push(format!("status = ?{}", params_vec.len() + 1)); params_vec.push(Box::new(v.to_string())); }
        if let Some(v) = tags { sets.push(format!("tags = ?{}", params_vec.len() + 1)); params_vec.push(Box::new(v.to_string())); }
        if let Some(v) = tech_stack { sets.push(format!("tech_stack = ?{}", params_vec.len() + 1)); params_vec.push(Box::new(v.to_string())); }
        if sets.is_empty() { return Ok(()); }
        params_vec.push(Box::new(git_dir.to_string()));
        let sql = format!("UPDATE projects SET {} WHERE git_dir = ?{}", sets.join(", "), params_vec.len());
        self.conn.execute(&sql, rusqlite::params_from_iter(params_vec.iter().map(|p| p.as_ref())))?;
        Ok(())
    }

    // =========================================================================
    // Project env vars operations
    // =========================================================================

    /// List all env vars for a project session
    pub fn list_project_env_vars(&self, session_name: &str) -> Result<Vec<ProjectEnvVar>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_name, key, value, is_secret, sort_order, created_at, updated_at
             FROM project_env_vars
             WHERE session_name = ?1
             ORDER BY sort_order ASC, id ASC",
        )?;

        let rows = stmt
            .query_map(params![session_name], |row| {
                Ok(ProjectEnvVar {
                    id: row.get(0)?,
                    session_name: row.get(1)?,
                    key: row.get(2)?,
                    value: row.get(3)?,
                    is_secret: row.get(4)?,
                    sort_order: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Create a new env var for a project session
    pub fn create_project_env_var(
        &self,
        session_name: &str,
        key: &str,
        value: &str,
        is_secret: bool,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO project_env_vars (session_name, key, value, is_secret)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_name, key, value, is_secret as i32],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Update an env var (returns session_name for sync)
    pub fn update_project_env_var(
        &self,
        id: i64,
        key: Option<&str>,
        value: Option<&str>,
        is_secret: Option<bool>,
        sort_order: Option<i32>,
    ) -> Result<String> {
        let mut set_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(k) = key {
            set_clauses.push("key = ?");
            param_values.push(Box::new(k.to_string()));
        }
        if let Some(v) = value {
            set_clauses.push("value = ?");
            param_values.push(Box::new(v.to_string()));
        }
        if let Some(s) = is_secret {
            set_clauses.push("is_secret = ?");
            param_values.push(Box::new(s as i32));
        }
        if let Some(o) = sort_order {
            set_clauses.push("sort_order = ?");
            param_values.push(Box::new(o));
        }

        if set_clauses.is_empty() {
            anyhow::bail!("No fields to update");
        }

        set_clauses.push("updated_at = datetime('now')");

        // Re-number placeholders
        let numbered: Vec<String> = set_clauses
            .iter()
            .enumerate()
            .map(|(i, clause)| {
                if clause.contains('?') {
                    clause.replacen('?', &format!("?{}", i + 1), 1)
                } else {
                    clause.to_string()
                }
            })
            .collect();

        let id_param_idx = param_values.len() + 1;
        let sql = format!(
            "UPDATE project_env_vars SET {} WHERE id = ?{}",
            numbered.join(", "),
            id_param_idx
        );
        param_values.push(Box::new(id));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, params_refs.as_slice())?;

        // Return session_name for sync
        let session_name: String = self.conn.query_row(
            "SELECT session_name FROM project_env_vars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(session_name)
    }

    /// Delete an env var (returns session_name for sync)
    pub fn delete_project_env_var(&self, id: i64) -> Result<String> {
        let session_name: String = self.conn.query_row(
            "SELECT session_name FROM project_env_vars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "DELETE FROM project_env_vars WHERE id = ?1",
            params![id],
        )?;
        Ok(session_name)
    }

    // =========================================================================
    // Project todos operations
    // =========================================================================

    pub fn list_project_todos(&self, git_dir: &str) -> Result<Vec<ProjectTodo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, git_dir, title, description, status, priority, sort_order, created_at, updated_at
             FROM project_todos
             WHERE git_dir = ?1
             ORDER BY
               CASE status WHEN 'todo' THEN 0 WHEN 'in_progress' THEN 1 WHEN 'done' THEN 2 ELSE 3 END,
               sort_order ASC, id ASC",
        )?;

        let rows = stmt
            .query_map(params![git_dir], |row| {
                Ok(ProjectTodo {
                    id: row.get(0)?,
                    git_dir: row.get(1)?,
                    title: row.get(2)?,
                    description: row.get(3)?,
                    status: row.get(4)?,
                    priority: row.get(5)?,
                    sort_order: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn create_project_todo(
        &self,
        git_dir: &str,
        title: &str,
        description: &str,
        status: &str,
        priority: i32,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO project_todos (git_dir, title, description, status, priority)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![git_dir, title, description, status, priority],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_project_todo(
        &self,
        id: i64,
        title: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<i32>,
        sort_order: Option<i32>,
    ) -> Result<()> {
        let mut set_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(v) = title {
            set_clauses.push("title = ?");
            param_values.push(Box::new(v.to_string()));
        }
        if let Some(v) = description {
            set_clauses.push("description = ?");
            param_values.push(Box::new(v.to_string()));
        }
        if let Some(v) = status {
            set_clauses.push("status = ?");
            param_values.push(Box::new(v.to_string()));
        }
        if let Some(v) = priority {
            set_clauses.push("priority = ?");
            param_values.push(Box::new(v));
        }
        if let Some(v) = sort_order {
            set_clauses.push("sort_order = ?");
            param_values.push(Box::new(v));
        }

        if set_clauses.is_empty() {
            anyhow::bail!("No fields to update");
        }

        set_clauses.push("updated_at = datetime('now')");

        let numbered: Vec<String> = set_clauses
            .iter()
            .enumerate()
            .map(|(i, clause)| {
                if clause.contains('?') {
                    clause.replacen('?', &format!("?{}", i + 1), 1)
                } else {
                    clause.to_string()
                }
            })
            .collect();

        let id_param_idx = param_values.len() + 1;
        let sql = format!(
            "UPDATE project_todos SET {} WHERE id = ?{}",
            numbered.join(", "),
            id_param_idx
        );
        param_values.push(Box::new(id));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, params_refs.as_slice())?;
        Ok(())
    }

    pub fn delete_project_todo(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM project_todos WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn count_project_todos(&self, git_dir: &str) -> Result<(i32, i32)> {
        let (total, done) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END), 0)
             FROM project_todos WHERE git_dir = ?1",
            params![git_dir],
            |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?)),
        )?;
        Ok((total, done))
    }

    // =========================================================================
    // Project services operations
    // =========================================================================

    /// List all services for a project session
    pub fn list_project_services(&self, session_name: &str) -> Result<Vec<ProjectService>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_name, service_name, base_value, value_type, env_key, sort_order
             FROM project_services
             WHERE session_name = ?1
             ORDER BY sort_order ASC, id ASC",
        )?;

        let rows = stmt
            .query_map(params![session_name], |row| {
                Ok(ProjectService {
                    id: row.get(0)?,
                    session_name: row.get(1)?,
                    service_name: row.get(2)?,
                    base_value: row.get(3)?,
                    value_type: row.get(4)?,
                    env_key: row.get(5)?,
                    sort_order: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Create a new service for a project session
    pub fn create_project_service(
        &self,
        session_name: &str,
        service_name: &str,
        base_value: i32,
        value_type: &str,
        env_key: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO project_services (session_name, service_name, base_value, value_type, env_key)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_name, service_name, base_value, value_type, env_key],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Update a service (returns session_name for sync)
    pub fn update_project_service(
        &self,
        id: i64,
        service_name: Option<&str>,
        base_value: Option<i32>,
        value_type: Option<&str>,
        env_key: Option<&str>,
        sort_order: Option<i32>,
    ) -> Result<String> {
        let mut set_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(n) = service_name {
            set_clauses.push("service_name = ?");
            param_values.push(Box::new(n.to_string()));
        }
        if let Some(b) = base_value {
            set_clauses.push("base_value = ?");
            param_values.push(Box::new(b));
        }
        if let Some(t) = value_type {
            set_clauses.push("value_type = ?");
            param_values.push(Box::new(t.to_string()));
        }
        if let Some(k) = env_key {
            set_clauses.push("env_key = ?");
            param_values.push(Box::new(k.to_string()));
        }
        if let Some(o) = sort_order {
            set_clauses.push("sort_order = ?");
            param_values.push(Box::new(o));
        }

        if set_clauses.is_empty() {
            anyhow::bail!("No fields to update");
        }

        let numbered: Vec<String> = set_clauses
            .iter()
            .enumerate()
            .map(|(i, clause)| clause.replacen('?', &format!("?{}", i + 1), 1))
            .collect();

        let id_param_idx = param_values.len() + 1;
        let sql = format!(
            "UPDATE project_services SET {} WHERE id = ?{}",
            numbered.join(", "),
            id_param_idx
        );
        param_values.push(Box::new(id));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, params_refs.as_slice())?;

        let session_name: String = self.conn.query_row(
            "SELECT session_name FROM project_services WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(session_name)
    }

    /// Delete a service (returns session_name for sync)
    pub fn delete_project_service(&self, id: i64) -> Result<String> {
        let session_name: String = self.conn.query_row(
            "SELECT session_name FROM project_services WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "DELETE FROM project_services WHERE id = ?1",
            params![id],
        )?;
        Ok(session_name)
    }

    // =========================================================================
    // Worktree slots operations
    // =========================================================================

    /// List all worktree slots for a project session
    pub fn list_worktree_slots(&self, session_name: &str) -> Result<Vec<WorktreeSlot>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_name, slot, branch, worktree_path, created_at
             FROM worktree_slots
             WHERE session_name = ?1
             ORDER BY slot ASC",
        )?;

        let rows = stmt
            .query_map(params![session_name], |row| {
                Ok(WorktreeSlot {
                    id: row.get(0)?,
                    session_name: row.get(1)?,
                    slot: row.get(2)?,
                    branch: row.get(3)?,
                    worktree_path: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Find the first unused slot number (1..=15) for a session
    pub fn next_available_slot(&self, session_name: &str) -> Result<i32> {
        let mut stmt = self.conn.prepare(
            "SELECT slot FROM worktree_slots WHERE session_name = ?1 ORDER BY slot ASC",
        )?;

        let used: Vec<i32> = stmt
            .query_map(params![session_name], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        for slot in 1..=15 {
            if !used.contains(&slot) {
                return Ok(slot);
            }
        }

        anyhow::bail!("No available worktree slots (all 15 in use)")
    }

    /// Allocate a worktree slot
    pub fn allocate_worktree_slot(
        &self,
        session_name: &str,
        slot: i32,
        branch: &str,
        worktree_path: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO worktree_slots (session_name, slot, branch, worktree_path)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_name, slot, branch, worktree_path],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Free a worktree slot by branch name
    pub fn free_worktree_slot_by_branch(&self, session_name: &str, branch: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM worktree_slots WHERE session_name = ?1 AND branch = ?2",
            params![session_name, branch],
        )?;
        Ok(())
    }

    /// Free a worktree slot by ID
    pub fn free_worktree_slot_by_id(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM worktree_slots WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    // =========================================================================
    // Global env vars operations
    // =========================================================================

    pub fn list_global_env_vars(&self) -> Result<Vec<GlobalEnvVar>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, key, value, is_secret, sort_order, created_at, updated_at
             FROM global_env_vars
             ORDER BY sort_order ASC, id ASC",
        )?;

        let rows = stmt
            .query_map([], |row| {
                Ok(GlobalEnvVar {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    value: row.get(2)?,
                    is_secret: row.get(3)?,
                    sort_order: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn create_global_env_var(&self, key: &str, value: &str, is_secret: bool) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO global_env_vars (key, value, is_secret)
             VALUES (?1, ?2, ?3)",
            params![key, value, is_secret as i32],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_global_env_var(
        &self,
        id: i64,
        key: Option<&str>,
        value: Option<&str>,
        is_secret: Option<bool>,
        sort_order: Option<i32>,
    ) -> Result<()> {
        let mut set_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(k) = key {
            set_clauses.push("key = ?");
            param_values.push(Box::new(k.to_string()));
        }
        if let Some(v) = value {
            set_clauses.push("value = ?");
            param_values.push(Box::new(v.to_string()));
        }
        if let Some(s) = is_secret {
            set_clauses.push("is_secret = ?");
            param_values.push(Box::new(s as i32));
        }
        if let Some(o) = sort_order {
            set_clauses.push("sort_order = ?");
            param_values.push(Box::new(o));
        }

        if set_clauses.is_empty() {
            anyhow::bail!("No fields to update");
        }

        set_clauses.push("updated_at = datetime('now')");

        let numbered: Vec<String> = set_clauses
            .iter()
            .enumerate()
            .map(|(i, clause)| {
                if clause.contains('?') {
                    clause.replacen('?', &format!("?{}", i + 1), 1)
                } else {
                    clause.to_string()
                }
            })
            .collect();

        let id_param_idx = param_values.len() + 1;
        let sql = format!(
            "UPDATE global_env_vars SET {} WHERE id = ?{}",
            numbered.join(", "),
            id_param_idx
        );
        param_values.push(Box::new(id));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, params_refs.as_slice())?;
        Ok(())
    }

    pub fn delete_global_env_var(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM global_env_vars WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    // =========================================================================
    // Worktree env vars operations
    // =========================================================================

    pub fn list_worktree_env_vars(&self, session_name: &str, slot: i32) -> Result<Vec<WorktreeEnvVar>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_name, slot, key, value, is_secret, sort_order, created_at, updated_at
             FROM worktree_env_vars
             WHERE session_name = ?1 AND slot = ?2
             ORDER BY sort_order ASC, id ASC",
        )?;

        let rows = stmt
            .query_map(params![session_name, slot], |row| {
                Ok(WorktreeEnvVar {
                    id: row.get(0)?,
                    session_name: row.get(1)?,
                    slot: row.get(2)?,
                    key: row.get(3)?,
                    value: row.get(4)?,
                    is_secret: row.get(5)?,
                    sort_order: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn create_worktree_env_var(
        &self,
        session_name: &str,
        slot: i32,
        key: &str,
        value: &str,
        is_secret: bool,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO worktree_env_vars (session_name, slot, key, value, is_secret)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_name, slot, key, value, is_secret as i32],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_worktree_env_var(
        &self,
        id: i64,
        key: Option<&str>,
        value: Option<&str>,
        is_secret: Option<bool>,
        sort_order: Option<i32>,
    ) -> Result<()> {
        let mut set_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(k) = key {
            set_clauses.push("key = ?");
            param_values.push(Box::new(k.to_string()));
        }
        if let Some(v) = value {
            set_clauses.push("value = ?");
            param_values.push(Box::new(v.to_string()));
        }
        if let Some(s) = is_secret {
            set_clauses.push("is_secret = ?");
            param_values.push(Box::new(s as i32));
        }
        if let Some(o) = sort_order {
            set_clauses.push("sort_order = ?");
            param_values.push(Box::new(o));
        }

        if set_clauses.is_empty() {
            anyhow::bail!("No fields to update");
        }

        set_clauses.push("updated_at = datetime('now')");

        let numbered: Vec<String> = set_clauses
            .iter()
            .enumerate()
            .map(|(i, clause)| {
                if clause.contains('?') {
                    clause.replacen('?', &format!("?{}", i + 1), 1)
                } else {
                    clause.to_string()
                }
            })
            .collect();

        let id_param_idx = param_values.len() + 1;
        let sql = format!(
            "UPDATE worktree_env_vars SET {} WHERE id = ?{}",
            numbered.join(", "),
            id_param_idx
        );
        param_values.push(Box::new(id));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, params_refs.as_slice())?;
        Ok(())
    }

    pub fn delete_worktree_env_var(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM worktree_env_vars WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Get effective (merged) env vars: global → project → worktree
    pub fn get_effective_env_vars(&self, session_name: &str, slot: i32) -> Result<Vec<EffectiveEnvVar>> {
        use std::collections::HashMap;

        let mut merged: HashMap<String, EffectiveEnvVar> = HashMap::new();

        // Layer 1: Global
        for var in self.list_global_env_vars()? {
            merged.insert(var.key.clone(), EffectiveEnvVar {
                key: var.key,
                value: var.value,
                is_secret: var.is_secret,
                source: "global".to_string(),
            });
        }

        // Layer 2: Project
        for var in self.list_project_env_vars(session_name)? {
            merged.insert(var.key.clone(), EffectiveEnvVar {
                key: var.key,
                value: var.value,
                is_secret: var.is_secret,
                source: "project".to_string(),
            });
        }

        // Layer 3: Worktree
        for var in self.list_worktree_env_vars(session_name, slot)? {
            merged.insert(var.key.clone(), EffectiveEnvVar {
                key: var.key,
                value: var.value,
                is_secret: var.is_secret,
                source: "worktree".to_string(),
            });
        }

        let mut result: Vec<EffectiveEnvVar> = merged.into_values().collect();
        result.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(result)
    }

    // =========================================================================
    // Delete project
    // =========================================================================

    pub fn delete_project(&self, git_dir: &str) -> Result<()> {
        // Get session name from projects table for session-keyed tables
        let session_name: Option<String> = self.conn
            .query_row(
                "SELECT last_session FROM projects WHERE git_dir = ?1",
                params![git_dir],
                |row| row.get(0),
            )
            .ok();

        let tx = self.conn.unchecked_transaction()?;

        // 1) Delete git_dir-keyed tables
        tx.execute("DELETE FROM project_todos WHERE git_dir = ?1", params![git_dir])?;
        tx.execute("DELETE FROM session_index WHERE project = ?1", params![git_dir])?;

        // 2) Delete session_name-keyed tables (if we have a session name)
        if let Some(ref sn) = session_name {
            if !sn.is_empty() {
                // Delete history cascade: conversation_messages, tool_usage, commits via history_id
                tx.execute(
                    "DELETE FROM conversation_messages WHERE history_id IN (SELECT id FROM history WHERE session_id = ?1)",
                    params![sn],
                )?;
                tx.execute(
                    "DELETE FROM tool_usage WHERE history_id IN (SELECT id FROM history WHERE session_id = ?1)",
                    params![sn],
                )?;
                tx.execute(
                    "DELETE FROM commits WHERE history_id IN (SELECT id FROM history WHERE session_id = ?1)",
                    params![sn],
                )?;
                tx.execute("DELETE FROM history WHERE session_id = ?1", params![sn])?;
                tx.execute("DELETE FROM tasks WHERE session_id = ?1", params![sn])?;
                tx.execute("DELETE FROM notes WHERE session_id = ?1", params![sn])?;
                tx.execute("DELETE FROM goals WHERE session_id = ?1", params![sn])?;
                tx.execute("DELETE FROM closed_windows WHERE session_id = ?1", params![sn])?;
                tx.execute("DELETE FROM project_env_vars WHERE session_name = ?1", params![sn])?;
                tx.execute("DELETE FROM project_services WHERE session_name = ?1", params![sn])?;
                tx.execute("DELETE FROM worktree_slots WHERE session_name = ?1", params![sn])?;
                tx.execute("DELETE FROM worktree_env_vars WHERE session_name = ?1", params![sn])?;
            }
        }

        // 3) Delete the project itself
        tx.execute("DELETE FROM projects WHERE git_dir = ?1", params![git_dir])?;

        tx.commit()?;
        Ok(())
    }

    // =========================================================================
    // Notification operations
    // =========================================================================

    pub fn list_notifications(&self, unread_only: bool, limit: i32) -> Result<Vec<Notification>> {
        let sql = if unread_only {
            "SELECT id, type, session_name, message, read, created_at FROM notifications WHERE read = 0 ORDER BY created_at DESC LIMIT ?1"
        } else {
            "SELECT id, type, session_name, message, read, created_at FROM notifications ORDER BY created_at DESC LIMIT ?1"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(Notification {
                id: row.get(0)?,
                notification_type: row.get(1)?,
                session_name: row.get(2)?,
                message: row.get(3)?,
                read: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn create_notification(&self, notification_type: &str, session_name: Option<&str>, message: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO notifications (type, session_name, message) VALUES (?1, ?2, ?3)",
            params![notification_type, session_name, message],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn mark_notification_read(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE notifications SET read = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn mark_all_notifications_read(&self) -> Result<()> {
        self.conn.execute("UPDATE notifications SET read = 1 WHERE read = 0", [])?;
        Ok(())
    }

    pub fn unread_notification_count(&self) -> Result<i32> {
        let count: i32 = self.conn.query_row(
            "SELECT COUNT(*) FROM notifications WHERE read = 0", [], |row| row.get(0)
        )?;
        Ok(count)
    }

    // =========================================================================
    // Alert rule operations
    // =========================================================================

    pub fn list_alert_rules(&self) -> Result<Vec<AlertRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, condition_type, threshold_seconds, enabled, channels, created_at FROM alert_rules ORDER BY id"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AlertRule {
                id: row.get(0)?,
                name: row.get(1)?,
                condition_type: row.get(2)?,
                threshold_seconds: row.get(3)?,
                enabled: row.get(4)?,
                channels: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn create_alert_rule(&self, name: &str, condition_type: &str, threshold_seconds: Option<i32>, channels: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO alert_rules (name, condition_type, threshold_seconds, channels) VALUES (?1, ?2, ?3, ?4)",
            params![name, condition_type, threshold_seconds, channels],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_alert_rule(&self, id: i64, enabled: Option<bool>, threshold_seconds: Option<i32>, channels: Option<&str>) -> Result<()> {
        if let Some(e) = enabled {
            self.conn.execute("UPDATE alert_rules SET enabled = ?1 WHERE id = ?2", params![e as i32, id])?;
        }
        if let Some(t) = threshold_seconds {
            self.conn.execute("UPDATE alert_rules SET threshold_seconds = ?1 WHERE id = ?2", params![t, id])?;
        }
        if let Some(c) = channels {
            self.conn.execute("UPDATE alert_rules SET channels = ?1 WHERE id = ?2", params![c, id])?;
        }
        Ok(())
    }

    pub fn delete_alert_rule(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM alert_rules WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Check alert conditions against current tasks and create notifications
    pub fn check_alerts(&self) -> Result<Vec<Notification>> {
        let rules = self.list_alert_rules()?;
        let mut new_notifications = Vec::new();

        for rule in &rules {
            if rule.enabled == 0 { continue; }

            match rule.condition_type.as_str() {
                "task_stuck" => {
                    let threshold = rule.threshold_seconds.unwrap_or(1800); // default 30min
                    let mut stmt = self.conn.prepare(
                        "SELECT session, window, summary FROM tasks WHERE status = 'in_progress'
                         AND started_at IS NOT NULL
                         AND CAST((julianday('now') - julianday(started_at)) * 86400 AS INTEGER) > ?1"
                    )?;
                    let stuck: Vec<(String, String, String)> = stmt.query_map(params![threshold], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
                    })?.filter_map(|r| r.ok()).collect();

                    for (session, window, summary) in stuck {
                        let msg = format!("Task stuck >{}m in {}/{}: {}", threshold / 60, session, window, summary);
                        // Avoid duplicate notifications (check last hour)
                        let exists: bool = self.conn.query_row(
                            "SELECT COUNT(*) > 0 FROM notifications WHERE type = 'task_stuck' AND message = ?1 AND created_at > datetime('now', '-1 hour')",
                            params![msg], |row| row.get(0)
                        ).unwrap_or(false);
                        if !exists {
                            let id = self.create_notification("task_stuck", Some(&session), &msg)?;
                            new_notifications.push(Notification {
                                id, notification_type: "task_stuck".into(), session_name: Some(session),
                                message: msg, read: 0, created_at: String::new(),
                            });
                        }
                    }
                },
                "session_idle" => {
                    // Sessions with no activity for threshold seconds
                    let threshold = rule.threshold_seconds.unwrap_or(3600);
                    let mut stmt = self.conn.prepare(
                        "SELECT DISTINCT session FROM tasks WHERE status = 'in_progress'
                         AND started_at IS NOT NULL
                         AND CAST((julianday('now') - julianday(started_at)) * 86400 AS INTEGER) > ?1"
                    )?;
                    let idle: Vec<String> = stmt.query_map(params![threshold], |row| {
                        row.get::<_, String>(0)
                    })?.filter_map(|r| r.ok()).collect();

                    for session in idle {
                        let msg = format!("Session {} idle for >{}m", session, threshold / 60);
                        let exists: bool = self.conn.query_row(
                            "SELECT COUNT(*) > 0 FROM notifications WHERE type = 'session_idle' AND message = ?1 AND created_at > datetime('now', '-1 hour')",
                            params![msg], |row| row.get(0)
                        ).unwrap_or(false);
                        if !exists {
                            self.create_notification("session_idle", Some(&session), &msg)?;
                        }
                    }
                },
                _ => {} // Unknown condition types are ignored
            }
        }

        Ok(new_notifications)
    }

    // =========================================================================
    // Backup operations
    // =========================================================================

    pub fn backup_to(&self, path: &str) -> Result<()> {
        self.conn.execute("VACUUM INTO ?1", params![path])?;
        Ok(())
    }
}

/// Project registry info
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectInfo {
    pub git_dir: String,
    pub name: String,
    pub last_session: String,
    pub last_window: String,
    pub last_active_at: Option<String>,
    pub notes_count: i32,
    pub goals_count: i32,
    pub history_count: i32,
    pub description: String,
    pub status: String,
    pub tags: String,
    pub created_at: String,
    pub tech_stack: String,
    pub todos_count: i32,
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

/// Project environment variable
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectEnvVar {
    pub id: i64,
    pub session_name: String,
    pub key: String,
    pub value: String,
    pub is_secret: i32,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// Global environment variable
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GlobalEnvVar {
    pub id: i64,
    pub key: String,
    pub value: String,
    pub is_secret: i32,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// Worktree-scoped environment variable
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorktreeEnvVar {
    pub id: i64,
    pub session_name: String,
    pub slot: i32,
    pub key: String,
    pub value: String,
    pub is_secret: i32,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// Project todo item
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectTodo {
    pub id: i64,
    pub git_dir: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: i32,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// Merged env var with source annotation
#[derive(Debug, Clone, serde::Serialize)]
pub struct EffectiveEnvVar {
    pub key: String,
    pub value: String,
    pub is_secret: i32,
    pub source: String,
}

/// Project service (port/resource mapping)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectService {
    pub id: i64,
    pub session_name: String,
    pub service_name: String,
    pub base_value: i32,
    pub value_type: String,
    pub env_key: String,
    pub sort_order: i32,
}

/// Worktree slot allocation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorktreeSlot {
    pub id: i64,
    pub session_name: String,
    pub slot: i32,
    pub branch: String,
    pub worktree_path: Option<String>,
    pub created_at: String,
}

/// Notification record
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Notification {
    pub id: i64,
    #[serde(rename = "type")]
    pub notification_type: String,
    pub session_name: Option<String>,
    pub message: String,
    pub read: i32,
    pub created_at: String,
}

/// Alert rule
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlertRule {
    pub id: i64,
    pub name: String,
    pub condition_type: String,
    pub threshold_seconds: Option<i32>,
    pub enabled: i32,
    pub channels: String,
    pub created_at: String,
}

/// Get default database path.
/// Checks TRACKER_DATA_DIR env var first (Tauri mode), falls back to ~/.config/agent-tracker/.
pub fn default_db_path() -> std::path::PathBuf {
    if let Ok(data_dir) = std::env::var("TRACKER_DATA_DIR") {
        if !data_dir.is_empty() {
            return std::path::PathBuf::from(data_dir).join("data").join("tracker.db");
        }
    }
    crate::paths::TrackerPaths::legacy_config_dir()
        .join("data")
        .join("tracker.db")
}
