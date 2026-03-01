//! Per-project database module for .aitracker storage
//!
//! Each git project gets its own `.aitracker/tracker.db` for persistent data
//! (history, notes, goals, closed_windows). A connection cache (ProjectDbManager)
//! avoids repeatedly opening connections.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tracing::info;

use tracker_core::{ConversationMessage, GitCommit, Goal, Note, NoteScope, Task, ToolUsage};

use crate::db::ClosedWindow;

// =============================================================================
// Shared free functions (used by both global Database and ProjectDatabase)
// =============================================================================

/// Archive a completed task to history (with 60-min merge window).
/// Shared logic used by both global DB and per-project DB.
pub fn archive_to_history_on(conn: &Connection, task: &Task) -> Result<i64> {
    let recent_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM history
             WHERE session_id = ?1 AND window_id = ?2 AND pane = ?3
               AND completed_at > datetime('now', '-60 minutes')
             ORDER BY id DESC LIMIT 1",
            params![task.session_id, task.window_id, task.pane],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = recent_id {
        conn.execute(
            "UPDATE history SET
                summary = CASE WHEN LENGTH(?1) > LENGTH(summary) THEN ?1 ELSE summary END,
                completion_note = CASE WHEN ?2 != '' THEN ?2 ELSE completion_note END,
                completed_at = ?3,
                duration_seconds = ?4,
                transcript_path = CASE WHEN ?5 != '' THEN ?5 ELSE transcript_path END,
                todo_id = CASE WHEN ?7 IS NOT NULL THEN ?7 ELSE todo_id END
             WHERE id = ?6",
            params![
                task.summary,
                task.completion_note,
                task.completed_at.map(|t| t.to_rfc3339()),
                task.duration_seconds,
                task.transcript_path,
                id,
                task.todo_id,
            ],
        )?;
        let _ = conn.execute("DELETE FROM conversation_messages WHERE history_id = ?", [id]);
        let _ = conn.execute("DELETE FROM tool_usage WHERE history_id = ?", [id]);
        let _ = conn.execute("DELETE FROM commits WHERE history_id = ?", [id]);
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO history
             (session_id, session, window_id, window, pane, summary,
              completion_note, started_at, completed_at, duration_seconds, transcript_path, todo_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                task.todo_id,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

/// Save conversation messages for a history entry. Shared logic.
pub fn save_conversation_messages_on(
    conn: &Connection,
    history_id: i64,
    messages: &[ConversationMessage],
) -> Result<()> {
    for msg in messages {
        conn.execute(
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

/// Save tool usage records for a history entry. Shared logic.
pub fn save_tool_usage_on(
    conn: &Connection,
    history_id: i64,
    tool_usages: &[ToolUsage],
) -> Result<()> {
    for usage in tool_usages {
        conn.execute(
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

/// Save git commit records for a history entry. Shared logic.
pub fn save_commits_on(
    conn: &Connection,
    history_id: i64,
    commits: &[GitCommit],
) -> Result<()> {
    for commit in commits {
        conn.execute(
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

/// Per-project database stored at `<git_root>/.aitracker/tracker.db`
///
/// Connection is wrapped in Mutex for thread-safety (required since we cache in Arc).
pub struct ProjectDatabase {
    conn: Mutex<Connection>,
    pub git_dir: String,
}

impl ProjectDatabase {
    /// Open (or create) the project database at `<git_dir>/.aitracker/tracker.db`
    ///
    /// Auto-creates the `.aitracker/` directory and `.gitignore` if needed.
    pub fn open(git_dir: &str) -> Result<Self> {
        let base = Path::new(git_dir).join(".aitracker");

        // Create directory if needed
        if !base.exists() {
            std::fs::create_dir_all(&base)
                .with_context(|| format!("Failed to create .aitracker dir at {:?}", base))?;
            info!("Created .aitracker directory at {:?}", base);
        }

        // Write .gitignore with `*` to auto-ignore everything
        let gitignore_path = base.join(".gitignore");
        if !gitignore_path.exists() {
            std::fs::write(&gitignore_path, "*\n")
                .with_context(|| format!("Failed to write .gitignore at {:?}", gitignore_path))?;
        }

        let db_path = base.join("tracker.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open project DB at {:?}", db_path))?;

        // Run schema init before wrapping in Mutex
        Self::init_schema_on(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            git_dir: git_dir.to_string(),
        })
    }

    /// Initialize per-project schema
    fn init_schema_on(conn: &Connection) -> Result<()> {
        conn.execute(
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

        conn.execute(
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

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversation_messages_history
             ON conversation_messages(history_id)",
            [],
        )?;

        conn.execute(
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

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tool_usage_history ON tool_usage(history_id)",
            [],
        )?;

        conn.execute(
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

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_commits_history ON commits(history_id)",
            [],
        )?;

        conn.execute(
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

        conn.execute(
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

        conn.execute(
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

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_history_completed_at ON history(completed_at)",
            [],
        )?;

        // Migration: add todo_id column to history
        let _ = conn.execute("ALTER TABLE history ADD COLUMN todo_id INTEGER DEFAULT NULL", []);

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_history_session_window_pane
             ON history(session_id, window_id, pane)",
            [],
        )?;

        Ok(())
    }

    /// Get a lock on the database connection (for direct SQL in handlers)
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    // =========================================================================
    // History operations
    // =========================================================================

    /// Archive a completed task to history (with 60-min merge window)
    pub fn archive_to_history(&self, task: &Task) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        archive_to_history_on(&conn, task)
    }

    /// Load history with pagination and filtering (for API queries)
    pub fn load_history_paginated(
        &self,
        limit: i32,
        offset: i32,
        start_date: Option<&str>,
        end_date: Option<&str>,
        search: Option<&str>,
    ) -> Result<(Vec<HistoryEntry>, i32)> {
        let conn = self.conn.lock().unwrap();

        let mut conditions: Vec<String> = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = start_date {
            conditions.push("started_at >= ?".to_string());
            params_vec.push(Box::new(start.to_string()));
        }
        if let Some(end) = end_date {
            conditions.push("started_at <= ?".to_string());
            params_vec.push(Box::new(end.to_string()));
        }
        if let Some(q) = search {
            conditions.push("(summary LIKE ? OR completion_note LIKE ?)".to_string());
            let pattern = format!("%{}%", q);
            params_vec.push(Box::new(pattern.clone()));
            params_vec.push(Box::new(pattern));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Get total count with same filters
        let count_sql = format!("SELECT COUNT(*) FROM history {}", where_clause);
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let total: i32 = conn
            .prepare(&count_sql)?
            .query_row(params_refs.as_slice(), |row| row.get(0))
            .unwrap_or(0);

        // Query with pagination
        let select_sql = format!(
            "SELECT id, COALESCE(NULLIF(session, ''), session_id) as session,
                    COALESCE(NULLIF(window, ''), window_id) as window,
                    summary, completion_note,
                    duration_seconds, started_at, completed_at, transcript_path
             FROM history {} ORDER BY started_at DESC LIMIT ? OFFSET ?",
            where_clause
        );

        params_vec.push(Box::new(limit));
        params_vec.push(Box::new(offset));
        let all_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&select_sql)?;
        let entries: Vec<HistoryEntry> = stmt
            .query_map(all_refs.as_slice(), |row| {
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
                    file_path: None,
                    project: None,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok((entries, total))
    }

    /// Load history grouped by session:window (for grouped timeline view)
    pub fn load_history_grouped(
        &self,
        limit: i32,
        offset: i32,
        start_date: Option<&str>,
        end_date: Option<&str>,
        search: Option<&str>,
    ) -> Result<(Vec<WindowGroupEntry>, i32)> {
        let conn = self.conn.lock().unwrap();

        let mut conditions: Vec<String> = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = start_date {
            conditions.push("started_at >= ?".to_string());
            params_vec.push(Box::new(start.to_string()));
        }
        if let Some(end) = end_date {
            conditions.push("started_at <= ?".to_string());
            params_vec.push(Box::new(end.to_string()));
        }
        if let Some(q) = search {
            conditions.push("(summary LIKE ? OR completion_note LIKE ?)".to_string());
            let pattern = format!("%{}%", q);
            params_vec.push(Box::new(pattern.clone()));
            params_vec.push(Box::new(pattern));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Count distinct groups
        let count_sql = format!(
            "SELECT COUNT(*) FROM (
                SELECT 1 FROM history {}
                GROUP BY COALESCE(NULLIF(session, ''), session_id),
                         COALESCE(NULLIF(window, ''), window_id)
            )",
            where_clause
        );
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let total: i32 = conn
            .prepare(&count_sql)?
            .query_row(params_refs.as_slice(), |row| row.get(0))
            .unwrap_or(0);

        // Grouped query
        let select_sql = format!(
            "SELECT COALESCE(NULLIF(session, ''), session_id) as sess,
                    COALESCE(NULLIF(window, ''), window_id) as win,
                    GROUP_CONCAT(id, ',') as entry_ids,
                    COUNT(*) as task_count,
                    MIN(started_at) as first_started,
                    MAX(COALESCE(completed_at, started_at)) as last_ended,
                    SUM(duration_seconds) as total_duration,
                    GROUP_CONCAT(COALESCE(NULLIF(summary, ''), 'No summary'), '|||') as summaries
             FROM history {}
             GROUP BY sess, win
             ORDER BY MAX(started_at) DESC
             LIMIT ? OFFSET ?",
            where_clause
        );

        params_vec.push(Box::new(limit));
        params_vec.push(Box::new(offset));
        let all_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&select_sql)?;
        let entries: Vec<WindowGroupEntry> = stmt
            .query_map(all_refs.as_slice(), |row| {
                let session: String = row.get::<_, String>(0).unwrap_or_default();
                let window: String = row.get::<_, String>(1).unwrap_or_default();
                let ids_str: String = row.get::<_, String>(2).unwrap_or_default();
                let summaries_str: String = row.get::<_, String>(7).unwrap_or_default();

                let entry_ids: Vec<i64> = ids_str
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                let summaries: Vec<String> = summaries_str
                    .split("|||")
                    .map(|s| s.to_string())
                    .collect();

                let group_key = if session.is_empty() && window.is_empty() {
                    "Unknown".to_string()
                } else if session.is_empty() {
                    window.clone()
                } else {
                    format!("{}:{}", session, window)
                };

                Ok(WindowGroupEntry {
                    group_key,
                    session,
                    window,
                    entry_ids,
                    task_count: row.get::<_, i32>(3).unwrap_or(0),
                    total_messages: 0, // filled in below
                    total_duration: row.get::<_, f64>(6).unwrap_or(0.0),
                    first_started: row.get::<_, String>(4).unwrap_or_default(),
                    last_ended: row.get::<_, String>(5).unwrap_or_default(),
                    summaries,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Fill in message counts per group
        let entries: Vec<WindowGroupEntry> = entries
            .into_iter()
            .map(|mut g| {
                if !g.entry_ids.is_empty() {
                    let placeholders: String = g.entry_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                    let sql = format!(
                        "SELECT COUNT(*) FROM conversation_messages WHERE history_id IN ({})",
                        placeholders
                    );
                    if let Ok(mut stmt) = conn.prepare(&sql) {
                        let params: Vec<Box<dyn rusqlite::ToSql>> =
                            g.entry_ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>).collect();
                        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
                        if let Ok(count) = stmt.query_row(refs.as_slice(), |row| row.get::<_, i32>(0)) {
                            g.total_messages = count;
                        }
                    }
                }
                g
            })
            .collect();

        Ok((entries, total))
    }

    /// Get history detail by ID (raw query for the API handler pattern)
    pub fn get_history_detail_raw(
        &self,
        history_id: i64,
    ) -> Result<Option<(String, String, String, String, String, String, String, f64)>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT COALESCE(NULLIF(session, ''), session_id), COALESCE(NULLIF(window, ''), window_id), summary, completion_note, started_at, completed_at, COALESCE(transcript_path, ''), duration_seconds FROM history WHERE id = ?",
            [history_id],
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

        match result {
            Ok(data) => Ok(Some(data)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_conversation_messages(
        &self,
        history_id: i64,
        messages: &[ConversationMessage],
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        save_conversation_messages_on(&conn, history_id, messages)
    }

    pub fn save_tool_usage(&self, history_id: i64, tool_usages: &[ToolUsage]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        save_tool_usage_on(&conn, history_id, tool_usages)
    }

    pub fn save_commits(&self, history_id: i64, commits: &[GitCommit]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        save_commits_on(&conn, history_id, commits)
    }

    // =========================================================================
    // Note operations
    // =========================================================================

    pub fn save_note(&self, note: &Note) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let scope_str = match note.scope {
            NoteScope::Window => "window",
            NoteScope::Session => "session",
            NoteScope::All => "all",
        };

        conn.execute(
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

    pub fn delete_note(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        Ok(())
    }

    // =========================================================================
    // Goal operations
    // =========================================================================

    pub fn save_goal(&self, goal: &Goal) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
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

    pub fn delete_goal(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM goals WHERE id = ?1", params![id])?;
        Ok(())
    }

    // =========================================================================
    // Closed windows operations
    // =========================================================================

    pub fn save_closed_window(
        &self,
        session_id: &str,
        session_name: &str,
        window_name: &str,
        working_dir: &str,
        git_branch: &str,
        pane_count: i32,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Delete any existing record with the same session_name + window_name to prevent duplicates
        conn.execute(
            "DELETE FROM closed_windows WHERE session_name = ?1 AND window_name = ?2",
            params![session_name, window_name],
        )?;
        conn.execute(
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

    pub fn load_closed_windows(
        &self,
        session_name: &str,
        open_window_names: &[String],
    ) -> Result<Vec<ClosedWindow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
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
            .filter(|w| !open_window_names.contains(&w.window_name))
            .collect();

        Ok(windows)
    }

    pub fn delete_closed_window(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM closed_windows WHERE id = ?1", params![id])?;
        Ok(())
    }

    // =========================================================================
    // Count helpers (for project registry updates)
    // =========================================================================

    pub fn history_count(&self) -> i32 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap_or(0)
    }

    pub fn notes_count(&self) -> i32 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE archived = 0",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0)
    }

    pub fn goals_count(&self) -> i32 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM goals", [], |row| row.get(0))
            .unwrap_or(0)
    }

    /// Get the DB file path for this project
    pub fn db_path(&self) -> PathBuf {
        Path::new(&self.git_dir)
            .join(".aitracker")
            .join("tracker.db")
    }
}

/// History entry for API responses (matches main.rs HistoryEntry)
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub session: String,
    pub window: String,
    pub summary: String,
    pub completion_note: String,
    pub duration_seconds: f64,
    pub started_at: String,
    pub ended_at: String,
    pub message_count: i32,
    pub file_path: Option<String>,
    pub project: Option<String>,
}

/// Grouped history entry (multiple tasks in the same session:window)
#[derive(Debug, Clone, serde::Serialize)]
pub struct WindowGroupEntry {
    pub group_key: String,
    pub session: String,
    pub window: String,
    pub entry_ids: Vec<i64>,
    pub task_count: i32,
    pub total_messages: i32,
    pub total_duration: f64,
    pub first_started: String,
    pub last_ended: String,
    pub summaries: Vec<String>,
}

// =============================================================================
// Connection cache
// =============================================================================

/// Manages cached connections to per-project databases
pub struct ProjectDbManager {
    connections: Mutex<HashMap<String, Arc<ProjectDatabase>>>,
}

impl ProjectDbManager {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
        }
    }

    /// Get or open a project database connection (cached)
    pub fn get_or_open(&self, git_dir: &str) -> Result<Arc<ProjectDatabase>> {
        let mut cache = self.connections.lock().unwrap();

        if let Some(db) = cache.get(git_dir) {
            return Ok(db.clone());
        }

        let db = ProjectDatabase::open(git_dir)?;
        let arc = Arc::new(db);
        cache.insert(git_dir.to_string(), arc.clone());
        info!("Opened project database for {}", git_dir);
        Ok(arc)
    }

    /// List all currently cached project git_dirs
    pub fn cached_projects(&self) -> Vec<String> {
        self.connections
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }
}
