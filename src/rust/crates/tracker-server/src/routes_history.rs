//! History, session, and Claude message route handlers.
//!
//! Extracted from main.rs — these handlers are fully standalone
//! (no `State<Arc<AppState>>`), using `get_db_path()` to open their own
//! DB connections.

use axum::{
    extract::{Path as AxumPath, Query},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::{agent, db, transcript};

// ============================================================================
// Structs — History
// ============================================================================

/// History query params
#[derive(Deserialize)]
pub(crate) struct HistoryQueryParams {
    #[serde(default)]
    pub limit: Option<i32>,
    #[serde(default)]
    pub offset: Option<i32>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    /// Time range: today, yesterday, 7days, 30days, all
    #[serde(default)]
    pub range: Option<String>,
    /// Custom start date (ISO 8601)
    #[serde(default)]
    pub start_date: Option<String>,
    /// Custom end date (ISO 8601)
    #[serde(default)]
    pub end_date: Option<String>,
    /// Page number (1-indexed)
    #[serde(default)]
    pub page: Option<i32>,
    /// Items per page
    #[serde(default)]
    pub per_page: Option<i32>,
    /// Filter by project git_dir
    #[serde(default)]
    pub project: Option<String>,
    /// Group by: "window" groups entries by session:window
    #[serde(default)]
    pub group_by: Option<String>,
}

/// History group
#[derive(Serialize)]
pub(crate) struct HistoryGroup {
    pub label: String,
    pub records: Vec<HistoryEntry>,
}

/// History entry
#[derive(Serialize)]
pub(crate) struct HistoryEntry {
    pub id: i64,
    pub session: String,
    pub window: String,
    pub summary: String,
    pub completion_note: String,
    pub duration_seconds: f64,
    pub started_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ended_at: String,
    pub message_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// History response (grouped)
#[derive(Serialize)]
pub(crate) struct HistoryResponse {
    pub groups: Vec<HistoryGroup>,
    pub total: i32,
}

/// History stats response
#[derive(Serialize)]
pub(crate) struct HistoryStatsResponse {
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
pub(crate) struct HistoryDetailResponse {
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    timeline: Vec<transcript::TimelineEntry>,
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
pub(crate) struct ResumeResponse {
    success: bool,
    command: String,
    message: String,
}

/// Session query parameters
#[derive(Deserialize)]
pub(crate) struct SessionQueryParams {
    #[serde(default)]
    page: Option<i32>,
    #[serde(default)]
    per_page: Option<i32>,
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    search: Option<String>,
}

/// Response for reparse operation
#[derive(Serialize)]
pub(crate) struct ReparseResponse {
    success: bool,
    message: String,
    messages_count: usize,
    tools_count: usize,
    commits_count: usize,
}

/// Session detail query parameters
#[derive(Deserialize)]
pub(crate) struct SessionDetailParams {
    /// File path of the session JSONL
    file_path: String,
}

// ============================================================================
// Structs — Claude Messages API
// ============================================================================

/// Tool call info (from assistant's tool_use blocks)
#[derive(Serialize, Clone, Debug)]
pub(crate) struct ToolCallInfo {
    pub tool_use_id: String,
    pub tool_name: String,
    pub args_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_full: Option<String>,
}

/// Tool result info (from user's tool_result blocks)
#[derive(Serialize, Clone, Debug)]
pub(crate) struct ToolResultInfo {
    pub tool_use_id: String,
    pub content: String, // truncated display
    pub is_error: bool,
}

/// Claude message from session
#[derive(Serialize, Clone, Debug)]
pub(crate) struct ClaudeMessage {
    pub role: String, // "user" or "assistant"
    pub timestamp: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction: Option<ToolInteraction>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResultInfo>,
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct ToolInteraction {
    pub tool_name: String,
    pub questions: Vec<InteractiveQuestion>,
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct InteractiveQuestion {
    pub question: String,
    pub header: String,
    pub options: Vec<InteractiveOption>,
    pub multi_select: bool,
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct InteractiveOption {
    pub label: String,
    pub description: String,
}

/// Response for Claude messages API
#[derive(Serialize)]
pub(crate) struct ClaudeMessagesResponse {
    pub success: bool,
    pub messages: Vec<ClaudeMessage>,
    pub session_file: String,
}

/// Query params for Claude messages
#[derive(Deserialize)]
pub(crate) struct ClaudeMessagesParams {
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
    /// Tmux pane ID (e.g. "%42") - used for lsof-based JSONL file discovery
    pane: Option<String>,
}

// ============================================================================
// Helpers
// ============================================================================

fn get_db_path() -> std::path::PathBuf {
    db::default_db_path()
}

/// Strip inline <thinking>...</thinking> tags from text
fn strip_thinking_tags(text: &str) -> String {
    // Use a simple approach: find and remove <thinking>...</thinking> blocks
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find("<thinking>") {
        result.push_str(&remaining[..start]);
        if let Some(end) = remaining[start..].find("</thinking>") {
            remaining = &remaining[start + end + "</thinking>".len()..];
        } else {
            // No closing tag, skip the rest
            remaining = "";
            break;
        }
    }
    result.push_str(remaining);
    result
}

/// Truncate a string to max chars (UTF-8 safe)
fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Generate a concise args summary for a tool call
fn tool_args_summary(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Bash" => {
            let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or("");
            truncate_str(cmd, 200)
        }
        "Read" => {
            input.get("file_path").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        "Write" => {
            input.get("file_path").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        "Edit" => {
            input.get("file_path").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        "Grep" => {
            let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
            let path = input.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            format!("{} in {}", pattern, path)
        }
        "Glob" => {
            input.get("pattern").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        "Task" => {
            let desc = input.get("description").and_then(|d| d.as_str()).unwrap_or("");
            truncate_str(desc, 100)
        }
        "WebSearch" | "WebFetch" => {
            let q = input.get("query").or_else(|| input.get("url")).and_then(|v| v.as_str()).unwrap_or("");
            truncate_str(q, 150)
        }
        _ => {
            let json_str = serde_json::to_string(input).unwrap_or_default();
            truncate_str(&json_str, 200)
        }
    }
}

/// Validate that a file path is safe (no path traversal, only allowed directories)
fn validate_file_path(file_path: &str) -> Result<(), String> {
    // Reject null bytes
    if file_path.contains('\0') {
        return Err("Path contains null bytes".to_string());
    }
    // Reject relative paths
    if !file_path.starts_with('/') {
        return Err("Path must be absolute".to_string());
    }
    // Reject .. components
    if file_path.contains("..") {
        return Err("Path traversal not allowed".to_string());
    }

    // Canonicalize to resolve symlinks
    let canonical = std::fs::canonicalize(file_path)
        .map_err(|_| "Path does not exist or cannot be resolved".to_string())?;
    let canonical_str = canonical.to_string_lossy();

    // Only allow paths under known safe directories
    let home = dirs::home_dir().unwrap_or_default();
    let claude_dir = home.join(".claude");
    let tracker_dir = home.join(".config").join("agent-tracker");

    let allowed = canonical_str.starts_with(&claude_dir.to_string_lossy().to_string())
        || canonical_str.starts_with(&tracker_dir.to_string_lossy().to_string())
        || canonical_str.contains("/.aitracker/");

    if !allowed {
        return Err("Path not in allowed directory".to_string());
    }

    // Only allow .jsonl files
    if !canonical_str.ends_with(".jsonl") {
        return Err("Only .jsonl files are allowed".to_string());
    }

    Ok(())
}

// ============================================================================
// Handlers
// ============================================================================

/// Get grouped history
pub(crate) async fn get_history(Query(params): Query<HistoryQueryParams>) -> Json<HistoryResponse> {
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
                file_path: None,
                project: None,
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

/// Get sessions from session_index (scanned from Claude JSONL files)
pub(crate) async fn get_sessions(Query(params): Query<SessionQueryParams>) -> Json<HistoryResponse> {
    let db_path = get_db_path();
    let db = match db::Database::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            error!("Failed to open database: {}", e);
            return Json(HistoryResponse { groups: vec![], total: 0 });
        }
    };

    let page = params.page.unwrap_or(1).max(1) as i64;
    let per_page = params.per_page.unwrap_or(50) as i64;

    // Time range filter
    let today = chrono::Local::now().date_naive();
    let time_after = params.range.as_deref().and_then(|r| {
        let date = match r {
            "today" => Some(today),
            "yesterday" => Some(today - chrono::Duration::days(1)),
            "7days" => Some(today - chrono::Duration::days(7)),
            "30days" => Some(today - chrono::Duration::days(30)),
            _ => None,
        };
        date.map(|d| format!("{}T00:00:00Z", d.format("%Y-%m-%d")))
    });

    let (entries, total) = match db.load_sessions(
        page,
        per_page,
        time_after.as_deref(),
        params.search.as_deref(),
    ) {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to load sessions: {}", e);
            return Json(HistoryResponse { groups: vec![], total: 0 });
        }
    };

    // Convert to HistoryEntry and group by date
    let history_entries: Vec<HistoryEntry> = entries.iter().map(|e| {
        // Convert project name to readable form: -Volumes-program-... -> Volumes/program/...
        let project_display = e.project
            .strip_prefix('-')
            .unwrap_or(&e.project)
            .replace('-', "/");

        HistoryEntry {
            id: 0, // Not used for session-based entries
            session: project_display,
            window: e.file_path
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".jsonl")
                .to_string(),
            summary: e.summary.clone(),
            completion_note: String::new(),
            duration_seconds: e.duration_seconds,
            started_at: e.started_at.clone(),
            ended_at: e.ended_at.clone(),
            message_count: e.message_count,
            file_path: Some(e.file_path.clone()),
            project: None,
        }
    }).collect();

    // Group by date
    let mut today_entries = vec![];
    let mut yesterday_entries = vec![];
    let mut this_week_entries = vec![];
    let mut older_entries = vec![];
    let yesterday = today - chrono::Duration::days(1);

    for entry in history_entries {
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

    let mut groups = vec![];
    if !today_entries.is_empty() {
        groups.push(HistoryGroup { label: "Today".to_string(), records: today_entries });
    }
    if !yesterday_entries.is_empty() {
        groups.push(HistoryGroup { label: "Yesterday".to_string(), records: yesterday_entries });
    }
    if !this_week_entries.is_empty() {
        groups.push(HistoryGroup { label: "This Week".to_string(), records: this_week_entries });
    }
    if !older_entries.is_empty() {
        groups.push(HistoryGroup { label: "Older".to_string(), records: older_entries });
    }

    Json(HistoryResponse { groups, total: total as i32 })
}

/// Get history statistics
pub(crate) async fn get_history_stats() -> Json<HistoryStatsResponse> {
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
pub(crate) async fn get_history_detail(AxumPath(id): AxumPath<i64>) -> Json<HistoryDetailResponse> {
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
                timeline: vec![],
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
                timeline: vec![],
            });
        }
    };

    // Load conversation messages
    let mut messages: Vec<ConversationMessageResponse> = vec![];
    if let Ok(mut stmt) = conn.prepare(
        "SELECT role, content, COALESCE(created_at, '') FROM conversation_messages WHERE history_id = ? AND TRIM(content) != '' ORDER BY id ASC"
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
        timeline: vec![],  // Legacy history doesn't have timeline
    })
}

/// Get session detail by parsing the JSONL file on demand
pub(crate) async fn get_session_detail(Query(params): Query<SessionDetailParams>) -> impl IntoResponse {
    // Validate file path to prevent path traversal
    if let Err(msg) = validate_file_path(&params.file_path) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid file path", "message": msg})),
        ).into_response();
    }

    let empty_response = || Json(HistoryDetailResponse {
        id: 0,
        session: String::new(),
        window: String::new(),
        summary: String::new(),
        completion_note: String::new(),
        started_at: String::new(),
        ended_at: String::new(),
        transcript_path: params.file_path.clone(),
        resume_command: String::new(),
        messages: vec![],
        tool_usage: vec![],
        commits: vec![],
        stats: None,
        timeline: vec![],
    });

    let path = std::path::Path::new(&params.file_path);
    if !path.exists() {
        return empty_response().into_response();
    }

    // Parse transcript using existing infrastructure
    let result = match transcript::parse_transcript_full(path) {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to parse session file {}: {}", params.file_path, e);
            return empty_response().into_response();
        }
    };

    // Convert messages (filter empty content)
    let messages: Vec<ConversationMessageResponse> = result.messages
        .iter()
        .filter(|m| !m.content.trim().is_empty())
        .map(|m| ConversationMessageResponse {
            role: m.role.clone(),
            content: m.content.clone(),
            created_at: m.created_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        })
        .collect();

    // Convert tool usage
    let mut tools_used: Vec<String> = vec![];
    let tool_usage: Vec<ToolUsageResponse> = result.tool_usages
        .iter()
        .map(|t| {
            if !tools_used.contains(&t.tool_name) {
                tools_used.push(t.tool_name.clone());
            }
            ToolUsageResponse {
                id: t.id,
                tool_name: t.tool_name.clone(),
                tool_args: t.tool_args.clone(),
                result_summary: t.result_summary.clone(),
                success: t.success,
                timestamp: t.timestamp.map(|ts| ts.to_rfc3339()).unwrap_or_default(),
            }
        })
        .collect();

    // Convert commits
    let commits: Vec<CommitResponse> = result.commits
        .iter()
        .map(|c| CommitResponse {
            id: c.id,
            commit_hash: c.commit_hash.clone(),
            commit_message: c.commit_message.clone(),
            files_changed: c.files_changed,
            timestamp: c.timestamp.map(|ts| ts.to_rfc3339()).unwrap_or_default(),
        })
        .collect();

    // Get session metadata from index
    let db_path = get_db_path();
    let (summary, started_at, ended_at, duration_seconds, project) = if let Ok(db) = db::Database::open(&db_path) {
        let (entries, _) = db.load_sessions(1, 1, None, None).unwrap_or_default();
        // Query specific entry
        let conn = rusqlite::Connection::open(&db_path).ok();
        if let Some(conn) = conn {
            conn.query_row(
                "SELECT summary, COALESCE(started_at, ''), COALESCE(ended_at, ''), duration_seconds, project FROM session_index WHERE file_path = ?",
                [&params.file_path],
                |row| Ok((
                    row.get::<_, String>(0).unwrap_or_default(),
                    row.get::<_, String>(1).unwrap_or_default(),
                    row.get::<_, String>(2).unwrap_or_default(),
                    row.get::<_, f64>(3).unwrap_or(0.0),
                    row.get::<_, String>(4).unwrap_or_default(),
                )),
            ).unwrap_or_default()
        } else {
            Default::default()
        }
    } else {
        Default::default()
    };

    let resume_command = format!("claude --resume {}", params.file_path);

    let project_display = project
        .strip_prefix('-')
        .unwrap_or(&project)
        .replace('-', "/");

    let stats = Some(HistoryDetailStats {
        message_count: messages.len() as i32,
        tool_count: tool_usage.len() as i32,
        commit_count: commits.len() as i32,
        duration_seconds,
        tools_used,
    });

    let timeline = result.timeline;

    Json(HistoryDetailResponse {
        id: 0,
        session: project_display,
        window: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        summary,
        completion_note: String::new(),
        started_at,
        ended_at,
        transcript_path: params.file_path,
        resume_command,
        messages,
        tool_usage,
        commits,
        stats,
        timeline,
    }).into_response()
}

/// Resume a conversation
pub(crate) async fn resume_history(AxumPath(id): AxumPath<i64>) -> Json<ResumeResponse> {
    Json(ResumeResponse {
        success: true,
        command: format!("claude --resume {}", id),
        message: "Use the command to resume this conversation".to_string(),
    })
}

/// Reparse transcript for a history entry
pub(crate) async fn reparse_history(AxumPath(id): AxumPath<i64>) -> Json<ReparseResponse> {
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
// Claude Messages API — Handlers
// ============================================================================

/// Background scanner: index Claude session JSONL files into session_index table
pub(crate) fn scan_claude_sessions(db_path: &std::path::Path) -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader};

    let claude_projects = dirs::home_dir()
        .map(|h| h.join(".claude/projects"))
        .unwrap_or_default();

    if !claude_projects.exists() {
        return Ok(());
    }

    let db = db::Database::open(db_path)?;
    let mut valid_paths: Vec<String> = Vec::new();

    let entries = std::fs::read_dir(&claude_projects)?;
    for project_entry in entries.flatten() {
        let project_dir = project_entry.path();
        if !project_dir.is_dir() {
            continue;
        }
        let project_name = project_dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Skip non-project directories (memory, etc.)
        let files = match std::fs::read_dir(&project_dir) {
            Ok(f) => f,
            Err(_) => continue,
        };

        for file_entry in files.flatten() {
            let path = file_entry.path();
            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                let path_str = path.to_string_lossy().to_string();
                valid_paths.push(path_str.clone());

                // Check if we need to re-index this file
                let meta = match path.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let file_size = meta.len() as i64;
                let file_mtime = meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default();

                if !db.session_needs_reindex(&path_str, file_size, &file_mtime) {
                    continue;
                }

                // Parse metadata from JSONL file
                let file = match std::fs::File::open(&path) {
                    Ok(f) => f,
                    Err(_) => continue,
                };

                let reader = BufReader::new(&file);
                let mut first_timestamp = String::new();
                let mut last_timestamp = String::new();
                let mut summary = String::new();
                let mut message_count: i32 = 0;

                // Read first few lines for start time and summary
                for line in reader.lines().take(50) {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    if line.trim().is_empty() { continue; }

                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&line) {
                        // Get timestamp
                        if first_timestamp.is_empty() {
                            if let Some(ts) = data.get("timestamp").and_then(|t| t.as_str()) {
                                first_timestamp = ts.to_string();
                            }
                        }

                        let msg_type = data.get("type").and_then(|t| t.as_str()).unwrap_or("");

                        // Count user/assistant messages
                        if msg_type == "user" || msg_type == "assistant" {
                            message_count += 1;
                        }

                        // Get first user message as summary
                        if summary.is_empty() && msg_type == "user" {
                            if let Some(msg) = data.get("message") {
                                let content = msg.get("content");
                                // Extract text from content (string or array)
                                let raw_text = if let Some(text) = content.and_then(|c| c.as_str()) {
                                    text.to_string()
                                } else if let Some(arr) = content.and_then(|c| c.as_array()) {
                                    arr.iter()
                                        .filter(|item| item.get("type").and_then(|t| t.as_str()) == Some("text"))
                                        .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                } else {
                                    String::new()
                                };

                                // Strip XML tags to get readable text
                                let stripped = {
                                    let mut result = String::new();
                                    let mut in_tag = false;
                                    for ch in raw_text.chars() {
                                        if ch == '<' { in_tag = true; continue; }
                                        if ch == '>' { in_tag = false; continue; }
                                        if !in_tag { result.push(ch); }
                                    }
                                    result
                                };
                                let trimmed = stripped.trim();
                                if !trimmed.is_empty() {
                                    summary = trimmed.chars().take(200).collect();
                                }
                            }
                        }
                    }
                }

                // Read remaining lines: count messages + get last timestamp
                // For large files, skip to near the end for last_timestamp
                let file2 = match std::fs::File::open(&path) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                let total_lines_reader = BufReader::new(&file2);
                let mut total_msg_count: i32 = 0;
                for line in total_lines_reader.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    if line.trim().is_empty() { continue; }
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&line) {
                        let msg_type = data.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        if msg_type == "user" || msg_type == "assistant" {
                            total_msg_count += 1;
                        }
                        if let Some(ts) = data.get("timestamp").and_then(|t| t.as_str()) {
                            last_timestamp = ts.to_string();
                        }
                    }
                }
                message_count = total_msg_count;

                // Calculate duration
                let duration = {
                    let start = chrono::DateTime::parse_from_rfc3339(&first_timestamp).ok();
                    let end = chrono::DateTime::parse_from_rfc3339(&last_timestamp).ok();
                    match (start, end) {
                        (Some(s), Some(e)) => (e - s).num_seconds() as f64,
                        _ => 0.0,
                    }
                };

                let entry = db::SessionIndexEntry {
                    file_path: path_str,
                    project: project_name.clone(),
                    summary,
                    started_at: first_timestamp,
                    ended_at: last_timestamp,
                    message_count,
                    duration_seconds: duration,
                    file_size,
                    file_mtime,
                };

                if let Err(e) = db.upsert_session_index(&entry) {
                    tracing::error!("Failed to index session {}: {}", path.display(), e);
                }
            }
        }
    }

    // Clean up stale entries
    if let Err(e) = db.cleanup_stale_sessions(&valid_paths) {
        tracing::error!("Failed to cleanup stale sessions: {}", e);
    }

    Ok(())
}

/// Parse a single JSONL line into a ClaudeMessage.
/// Used by both parse_claude_messages (HTTP API) and chat_watcher (WS push).
pub(crate) fn parse_single_jsonl_entry(line: &str) -> Option<ClaudeMessage> {
    let data = serde_json::from_str::<serde_json::Value>(line).ok()?;

    let msg_type = match data.get("type").and_then(|t| t.as_str()) {
        Some("user") => "user",
        Some("assistant") => "assistant",
        _ => return None,
    };

    let timestamp = data.get("timestamp")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let msg = data.get("message")?;
    let msg_content = msg.get("content");

    // Handle string content (user-typed messages)
    if let Some(text) = msg_content.and_then(|c| c.as_str()) {
        let cleaned = strip_thinking_tags(text);
        let trimmed = cleaned.trim();
        if !trimmed.is_empty()
            && !trimmed.starts_with("<bash")
            && !trimmed.starts_with("<system")
            && !trimmed.starts_with("<task-")
            && !trimmed.starts_with("<local-")
            && !trimmed.starts_with("<command-name>")
        {
            return Some(ClaudeMessage {
                role: msg_type.to_string(),
                timestamp,
                text: cleaned,
                thinking: None,
                interaction: None,
                tool_calls: vec![],
                tool_results: vec![],
            });
        }
        return None;
    }

    // Handle array content
    let arr = msg_content.and_then(|c| c.as_array())?;
    let mut text_parts: Vec<String> = Vec::new();
    let mut thinking_parts: Vec<String> = Vec::new();
    let mut interaction: Option<ToolInteraction> = None;
    let mut tool_calls: Vec<ToolCallInfo> = Vec::new();
    let mut tool_results: Vec<ToolResultInfo> = Vec::new();

    for item in arr {
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match item_type {
            "text" => {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && !trimmed.starts_with("<system")
                        && !trimmed.starts_with("<bash")
                        && !trimmed.starts_with("<task-")
                        && !trimmed.starts_with("<local-")
                        && !trimmed.starts_with("<command-name>")
                    {
                        let cleaned = strip_thinking_tags(text);
                        if !cleaned.trim().is_empty() {
                            text_parts.push(cleaned);
                        }
                    }
                }
            }
            "thinking" => {
                if let Some(text) = item.get("thinking").and_then(|t| t.as_str()) {
                    if !text.trim().is_empty() {
                        thinking_parts.push(text.to_string());
                    }
                }
            }
            "tool_use" => {
                let tool_name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let tool_use_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                let input = item.get("input").cloned().unwrap_or(serde_json::Value::Null);

                // AskUserQuestion -> interactive UI
                if tool_name == "AskUserQuestion" {
                    if let Some(questions_arr) = input.get("questions").and_then(|q| q.as_array()) {
                        let questions: Vec<InteractiveQuestion> = questions_arr.iter().filter_map(|q| {
                            let question = q.get("question").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let header = q.get("header").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let multi_select = q.get("multiSelect").and_then(|v| v.as_bool()).unwrap_or(false);
                            let options = q.get("options").and_then(|o| o.as_array())
                                .map(|opts| opts.iter().filter_map(|opt| {
                                    Some(InteractiveOption {
                                        label: opt.get("label").and_then(|v| v.as_str())?.to_string(),
                                        description: opt.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    })
                                }).collect())
                                .unwrap_or_default();
                            if question.is_empty() { return None; }
                            Some(InteractiveQuestion { question, header, options, multi_select })
                        }).collect();

                        if !questions.is_empty() {
                            interaction = Some(ToolInteraction {
                                tool_name: tool_name.to_string(),
                                questions,
                            });
                        }
                    }
                }
                else if tool_name == "ExitPlanMode" {
                    interaction = Some(ToolInteraction {
                        tool_name: "ExitPlanMode".to_string(),
                        questions: vec![InteractiveQuestion {
                            question: "Plan is ready for review. Would you like to proceed?".to_string(),
                            header: "PLAN APPROVAL".to_string(),
                            options: vec![
                                InteractiveOption { label: "1. Yes, clear context and bypass permissions".to_string(), description: String::new() },
                                InteractiveOption { label: "2. Yes, and bypass permissions".to_string(), description: String::new() },
                                InteractiveOption { label: "3. Yes, manually approve edits".to_string(), description: String::new() },
                            ],
                            multi_select: false,
                        }],
                    });
                }
                else {
                    // All other tools -> ToolCallInfo
                    let args_summary = tool_args_summary(tool_name, &input);
                    let args_full = serde_json::to_string(&input).ok();
                    tool_calls.push(ToolCallInfo {
                        tool_use_id,
                        tool_name: tool_name.to_string(),
                        args_summary,
                        args_full,
                    });
                }
            }
            "tool_result" => {
                let tool_use_id = item.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                let is_error = item.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false);
                let content_val = item.get("content");
                let content_str = match content_val {
                    Some(serde_json::Value::String(s)) => truncate_str(s, 2000),
                    Some(serde_json::Value::Array(arr)) => {
                        // Extract text from content array
                        let texts: Vec<&str> = arr.iter()
                            .filter_map(|item| {
                                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                                    item.get("text").and_then(|t| t.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        truncate_str(&texts.join("\n"), 2000)
                    }
                    Some(other) => truncate_str(&serde_json::to_string(other).unwrap_or_default(), 2000),
                    None => String::new(),
                };
                tool_results.push(ToolResultInfo {
                    tool_use_id,
                    content: content_str,
                    is_error,
                });
            }
            _ => {}
        }
    }

    let combined_text = text_parts.join("\n\n");
    let thinking = if thinking_parts.is_empty() { None } else { Some(thinking_parts.join("\n\n")) };

    // Include message if it has text, interaction, tool_calls, or tool_results
    if !combined_text.trim().is_empty() || interaction.is_some() || !tool_calls.is_empty() || !tool_results.is_empty() {
        Some(ClaudeMessage {
            role: msg_type.to_string(),
            timestamp,
            text: combined_text,
            thinking,
            interaction,
            tool_calls,
            tool_results,
        })
    } else {
        None
    }
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
        if let Some(msg) = parse_single_jsonl_entry(line) {
            // Merge tool-result-only user messages into the preceding assistant message.
            // These are automatic tool responses (not real user input) and should not
            // appear as separate USER bubbles in the chat timeline.
            if msg.role == "user"
                && msg.text.trim().is_empty()
                && msg.interaction.is_none()
                && !msg.tool_results.is_empty()
            {
                // Merge tool_results into the last assistant message
                if let Some(last) = all_messages.last_mut() {
                    if last.role == "assistant" {
                        last.tool_results.extend(msg.tool_results);
                        continue;
                    }
                }
                // No preceding assistant message -- skip this entry
                continue;
            }
            all_messages.push(msg);
        }
    }

    all_messages
}

/// Get recent user messages from Claude Code session
pub(crate) async fn get_claude_messages(Query(params): Query<ClaudeMessagesParams>) -> Json<ClaudeMessagesResponse> {
    let count = params.count.or(params.limit).unwrap_or(1);

    // Determine project filter: either from explicit project param or from tmux pane's working directory
    // Two-tier: exact path filters first, parent path filters only as fallback
    let (project_filters, parent_filters): (Vec<String>, Vec<String>) = if let (Some(session), Some(window)) = (&params.session, &params.window) {
        // Try all panes in the window to find one that matches a Claude project directory
        // This handles cases where the active pane is lazygit but Claude runs in another pane
        let list_output = std::process::Command::new(agent::TMUX_BIN)
            .args(["-S", "/private/tmp/tmux-501/default", "list-panes", "-t", &format!("{}:{}", session, window), "-F", "#{pane_current_path}"])
            .output();

        let mut paths: Vec<String> = Vec::new();
        if let Ok(out) = list_output {
            if out.status.success() {
                let output_str = String::from_utf8_lossy(&out.stdout);
                for line in output_str.lines() {
                    let path = line.trim().to_string();
                    if !path.is_empty() && !paths.contains(&path) {
                        paths.push(path);
                    }
                }
            }
        }

        // Fallback: try the active pane if list-panes failed
        if paths.is_empty() {
            let target = format!("{}:{}", session, window);
            let output = std::process::Command::new(agent::TMUX_BIN)
                .args(["-S", "/private/tmp/tmux-501/default", "display-message", "-p", "-t", &target, "#{pane_current_path}"])
                .output();
            if let Ok(out) = output {
                if out.status.success() {
                    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !path.is_empty() {
                        paths.push(path);
                    }
                }
            }
        }

        // Convert each path to Claude project directory format
        // Claude Code replaces '/', '.', and '_' with '-' in project directory names
        // Use two-tier matching: exact paths first, parent paths only as fallback
        let mut exact_filters: Vec<String> = Vec::new();
        let mut parent_filters: Vec<String> = Vec::new();
        for path in &paths {
            let converted = path.replace('/', "-").replace('.', "-").replace('_', "-");
            if !exact_filters.contains(&converted) {
                exact_filters.push(converted);
            }
            // Also generate parent path filters as fallback
            let mut current = path.as_str();
            loop {
                match current.rfind('/') {
                    Some(pos) if pos > 0 => {
                        current = &current[..pos];
                        let parent = current.replace('/', "-").replace('.', "-").replace('_', "-");
                        if !exact_filters.contains(&parent) && !parent_filters.contains(&parent) {
                            parent_filters.push(parent);
                        }
                    }
                    _ => break,
                }
            }
        }
        // Return exact filters; parent_filters used below as fallback
        (exact_filters, parent_filters)
    } else if let Some(ref project) = params.project {
        (vec![project.clone()], vec![])
    } else {
        (vec![], vec![])
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

    // Collect JSONL files matching exact filters first, then parent filters as fallback
    let collect_candidates = |filters: &[String]| -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&claude_projects) {
            for entry in entries.flatten() {
                let project_dir = entry.path();
                if project_dir.is_dir() {
                    if !filters.is_empty() {
                        let dir_name = project_dir.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if !filters.iter().any(|f| dir_name == *f) {
                            continue;
                        }
                    }
                    if let Ok(dir_files) = std::fs::read_dir(&project_dir) {
                        for file in dir_files.flatten() {
                            let path = file.path();
                            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                                if let Ok(meta) = path.metadata() {
                                    if let Ok(modified) = meta.modified() {
                                        files.push((path, modified));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        files.sort_by(|a, b| b.1.cmp(&a.1));
        files
    };

    // Try exact filters first; fall back to parent filters if no files found
    let mut candidate_files = collect_candidates(&project_filters);
    if candidate_files.is_empty() && !parent_filters.is_empty() {
        debug!("No JSONL files for exact filters {:?}, trying parent filters {:?}", project_filters, parent_filters);
        candidate_files = collect_candidates(&parent_filters);
    }

    // Legacy path: if no filters at all, scan everything
    if project_filters.is_empty() && parent_filters.is_empty() {
        candidate_files = collect_candidates(&[]);
    }

    debug!("Claude messages: Found {} candidate session files for exact filters {:?}", candidate_files.len(), project_filters);

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
