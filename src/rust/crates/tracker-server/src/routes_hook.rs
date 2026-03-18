//! Hook ingest routes for Claude Code hooks.
//!
//! Handles three endpoints:
//! - POST /api/hook/message — chat messages (user prompts and assistant responses)
//! - POST /api/hook/tool — tool usage events
//! - POST /api/hook/session — session lifecycle (start/end)

use std::sync::Arc;

use axum::{extract::State, response::Json};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::config;
use crate::AppState;

// ============================================================================
// Shared types
// ============================================================================

/// Common fields sent by all hook payloads
#[derive(Deserialize)]
struct HookContext {
    /// Claude session ID (unique per conversation)
    session_id: String,
    /// Working directory where Claude Code was launched
    cwd: String,
}

/// Standard hook response
#[derive(Serialize)]
pub(crate) struct HookResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl HookResponse {
    fn ok() -> Self {
        Self { success: true, skipped: None, error: None }
    }

    fn skipped() -> Self {
        Self { success: true, skipped: Some(true), error: None }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self { success: false, skipped: None, error: Some(msg.into()) }
    }
}

/// Resolved context after validation
struct ResolvedContext {
    claude_session_id: String,
    git_dir: String,
    session_name: String,
    window_id: String,
}

// ============================================================================
// Hook event types (broadcast to WebSocket clients)
// ============================================================================

/// Events broadcast from hook handlers to WebSocket clients
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub(crate) enum HookEvent {
    #[serde(rename = "chat_message")]
    ChatMessage {
        claude_session_id: String,
        git_dir: String,
        role: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
        timestamp: String,
    },
    #[serde(rename = "tool_event")]
    ToolEvent {
        claude_session_id: String,
        git_dir: String,
        tool_name: String,
        tool_use_id: String,
        timestamp: String,
    },
    #[serde(rename = "hook_session_update")]
    HookSessionUpdate {
        claude_session_id: String,
        git_dir: String,
        event: String,
    },
}

// ============================================================================
// Validation & resolution
// ============================================================================

/// Resolve cwd to git_dir (cached), check workspace is registered.
/// Returns None (with skipped response) if workspace is not tracked.
fn validate_and_resolve(
    state: &Arc<AppState>,
    ctx: &HookContext,
) -> Result<ResolvedContext, HookResponse> {
    if ctx.session_id.is_empty() {
        return Err(HookResponse::err("missing session_id"));
    }
    if ctx.cwd.is_empty() {
        return Err(HookResponse::err("missing cwd"));
    }

    // Resolve cwd → git_dir (cached)
    let git_dir = resolve_git_dir(state, &ctx.cwd);
    let git_dir = match git_dir {
        Some(d) => d,
        None => {
            debug!("hook: no git dir for cwd={}, skipping", ctx.cwd);
            return Err(HookResponse::skipped());
        }
    };

    // Check if this git_dir belongs to a registered workspace
    if !is_workspace_registered(state, &git_dir) {
        debug!("hook: unregistered workspace git_dir={}, skipping", git_dir);
        return Err(HookResponse::skipped());
    }

    // Resolve tmux context from tasks
    let (session_name, window_id) = resolve_tmux_context(state, &git_dir);

    Ok(ResolvedContext {
        claude_session_id: ctx.session_id.clone(),
        git_dir,
        session_name,
        window_id,
    })
}

/// Resolve cwd to git root directory, with caching.
fn resolve_git_dir(state: &Arc<AppState>, cwd: &str) -> Option<String> {
    // Check cache first
    {
        let cache = state.git_dir_cache.lock().unwrap();
        if let Some(cached) = cache.get(cwd) {
            return cached.clone();
        }
    }

    // Run git rev-parse to find the root
    let result = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        });

    // Cache the result (even None, to avoid repeated git calls)
    {
        let mut cache = state.git_dir_cache.lock().unwrap();
        cache.insert(cwd.to_string(), result.clone());
    }

    result
}

/// Check if a git_dir is registered as a workspace (config or projects table).
fn is_workspace_registered(state: &Arc<AppState>, git_dir: &str) -> bool {
    // Check config workspaces
    if let Ok(cfg) = config::AgentConfig::load() {
        for ws in cfg.workspaces.values() {
            let base = ws.base_path.to_string_lossy();
            if base == git_dir || git_dir.starts_with(base.as_ref()) {
                return true;
            }
        }
    }

    // Check projects table in DB
    let server = state.state.lock().unwrap();
    let exists: bool = server.db.conn.query_row(
        "SELECT COUNT(*) > 0 FROM projects WHERE git_dir = ?1",
        rusqlite::params![git_dir],
        |row| row.get(0),
    ).unwrap_or(false);
    exists
}

/// Find tmux session/window for a git_dir by checking active tasks.
fn resolve_tmux_context(state: &Arc<AppState>, git_dir: &str) -> (String, String) {
    // Collect task candidates while holding the lock, then release before calling resolve_git_dir_for_window
    let candidates: Vec<(String, String)> = {
        let server = state.state.lock().unwrap();
        server.tasks.values()
            .filter(|t| !t.session_id.is_empty())
            .map(|t| (t.session_id.clone(), t.window_id.clone()))
            .collect()
    };

    for (session_id, window_id) in candidates {
        if let Some(task_git_dir) = state.resolve_git_dir_for_window(&session_id, &window_id) {
            if task_git_dir == git_dir {
                return (session_id, window_id);
            }
        }
    }

    // Fallback: use git_dir basename as session name
    let basename = std::path::Path::new(git_dir)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "claude".to_string());
    (basename, "hook".to_string())
}

// ============================================================================
// POST /api/hook/message
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct HookMessageRequest {
    session_id: String,
    cwd: String,
    /// Hook event type: "UserPromptSubmit", "Stop", "SubagentStop"
    #[serde(alias = "type", alias = "hook_event_name")]
    event_type: String,
    /// User prompt (for UserPromptSubmit)
    #[serde(default)]
    prompt: Option<String>,
    /// Last assistant message (for Stop/SubagentStop)
    #[serde(default)]
    last_assistant_message: Option<String>,
    /// Whether this is a subagent (for SubagentStop)
    #[serde(default)]
    is_subagent: Option<bool>,
}

pub(crate) async fn hook_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HookMessageRequest>,
) -> Json<HookResponse> {
    let ctx = HookContext { session_id: req.session_id, cwd: req.cwd };
    let resolved = match validate_and_resolve(&state, &ctx) {
        Ok(r) => r,
        Err(resp) => return Json(resp),
    };

    let (role, content) = match req.event_type.as_str() {
        "UserPromptSubmit" => {
            let prompt = req.prompt.unwrap_or_default();
            if prompt.is_empty() {
                return Json(HookResponse::ok());
            }
            ("user".to_string(), prompt)
        }
        "Stop" | "SubagentStop" => {
            let msg = req.last_assistant_message.unwrap_or_default();
            if msg.is_empty() {
                return Json(HookResponse::ok());
            }
            ("assistant".to_string(), msg)
        }
        other => {
            debug!("hook/message: unknown event type '{}', skipping", other);
            return Json(HookResponse::ok());
        }
    };

    let agent_type = if req.is_subagent.unwrap_or(false) {
        Some("subagent".to_string())
    } else {
        None
    };

    // Persist to DB
    let db_result = {
        let server = state.state.lock().unwrap();
        let history_id = server.db.find_or_create_hook_history(
            &resolved.claude_session_id,
            &resolved.session_name,
            &resolved.window_id,
            &resolved.git_dir,
        );
        match history_id {
            Ok(id) => server.db.insert_hook_message(id, &resolved.claude_session_id, &role, &content),
            Err(e) => Err(e),
        }
    };

    if let Err(e) = db_result {
        warn!("hook/message: DB error: {}", e);
        return Json(HookResponse::err(format!("DB error: {}", e)));
    }

    // Broadcast to WebSocket
    let now = chrono::Utc::now().to_rfc3339();
    let event = HookEvent::ChatMessage {
        claude_session_id: resolved.claude_session_id,
        git_dir: resolved.git_dir,
        role,
        content,
        agent_type,
        timestamp: now,
    };
    let _ = state.hook_broadcast_tx.send(event);

    Json(HookResponse::ok())
}

// ============================================================================
// POST /api/hook/tool
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct HookToolRequest {
    session_id: String,
    cwd: String,
    tool_name: String,
    #[serde(default)]
    tool_use_id: Option<String>,
    #[serde(default)]
    tool_input: Option<serde_json::Value>,
    #[serde(default)]
    tool_response: Option<String>,
}

pub(crate) async fn hook_tool(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HookToolRequest>,
) -> Json<HookResponse> {
    let ctx = HookContext { session_id: req.session_id, cwd: req.cwd };
    let resolved = match validate_and_resolve(&state, &ctx) {
        Ok(r) => r,
        Err(resp) => return Json(resp),
    };

    let tool_use_id = req.tool_use_id.unwrap_or_default();
    let tool_args = req.tool_input
        .map(|v| serde_json::to_string(&v).unwrap_or_default())
        .unwrap_or_default();
    let result_summary = req.tool_response.map(|s| {
        if s.len() > 500 { format!("{}...", &s[..500]) } else { s }
    }).unwrap_or_default();

    // Persist to DB
    let db_result = {
        let server = state.state.lock().unwrap();
        let history_id = server.db.find_or_create_hook_history(
            &resolved.claude_session_id,
            &resolved.session_name,
            &resolved.window_id,
            &resolved.git_dir,
        );
        match history_id {
            Ok(id) => server.db.insert_hook_tool_usage(
                id,
                &resolved.claude_session_id,
                &req.tool_name,
                &tool_args,
                &result_summary,
                &tool_use_id,
            ),
            Err(e) => Err(e),
        }
    };

    if let Err(e) = db_result {
        warn!("hook/tool: DB error: {}", e);
        return Json(HookResponse::err(format!("DB error: {}", e)));
    }

    // Broadcast to WebSocket
    let now = chrono::Utc::now().to_rfc3339();
    let event = HookEvent::ToolEvent {
        claude_session_id: resolved.claude_session_id,
        git_dir: resolved.git_dir,
        tool_name: req.tool_name,
        tool_use_id,
        timestamp: now,
    };
    let _ = state.hook_broadcast_tx.send(event);

    Json(HookResponse::ok())
}

// ============================================================================
// POST /api/hook/session
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct HookSessionRequest {
    session_id: String,
    cwd: String,
    /// "SessionStart" or "SessionEnd"
    #[serde(alias = "type", alias = "hook_event_name")]
    event_type: String,
    /// Reason for session end (e.g., "user_exit", "error")
    #[serde(default)]
    reason: Option<String>,
}

pub(crate) async fn hook_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HookSessionRequest>,
) -> Json<HookResponse> {
    let ctx = HookContext { session_id: req.session_id, cwd: req.cwd };
    let resolved = match validate_and_resolve(&state, &ctx) {
        Ok(r) => r,
        Err(resp) => return Json(resp),
    };

    match req.event_type.as_str() {
        "SessionStart" => {
            // Create history entry
            let result = {
                let server = state.state.lock().unwrap();
                server.db.find_or_create_hook_history(
                    &resolved.claude_session_id,
                    &resolved.session_name,
                    &resolved.window_id,
                    &resolved.git_dir,
                )
            };
            if let Err(e) = result {
                warn!("hook/session start: DB error: {}", e);
                return Json(HookResponse::err(format!("DB error: {}", e)));
            }
            info!("hook: session started claude_session={}", &resolved.claude_session_id);

            // Broadcast state update (new history entry)
            state.broadcast_state();

            let event = HookEvent::HookSessionUpdate {
                claude_session_id: resolved.claude_session_id,
                git_dir: resolved.git_dir,
                event: "start".to_string(),
            };
            let _ = state.hook_broadcast_tx.send(event);
        }
        "SessionEnd" => {
            let reason = req.reason.unwrap_or_else(|| "session_end".to_string());
            let result = {
                let server = state.state.lock().unwrap();
                server.db.close_hook_session(&resolved.claude_session_id, &reason)
            };
            if let Err(e) = result {
                warn!("hook/session end: DB error: {}", e);
                return Json(HookResponse::err(format!("DB error: {}", e)));
            }
            info!("hook: session ended claude_session={} reason={}", &resolved.claude_session_id, reason);

            // Broadcast state update (history entry completed)
            state.broadcast_state();

            let event = HookEvent::HookSessionUpdate {
                claude_session_id: resolved.claude_session_id,
                git_dir: resolved.git_dir,
                event: "end".to_string(),
            };
            let _ = state.hook_broadcast_tx.send(event);
        }
        other => {
            debug!("hook/session: unknown event type '{}', ignoring", other);
        }
    }

    Json(HookResponse::ok())
}
