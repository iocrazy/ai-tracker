//! Hook ingest routes for Claude Code hooks.
//!
//! Handles three endpoints:
//! - POST /api/hook/message — chat messages (user prompts and assistant responses)
//! - POST /api/hook/tool — tool usage events
//! - POST /api/hook/session — session lifecycle (start/end)

use std::sync::Arc;

use axum::{extract::State, http::HeaderMap, response::Json};
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
        session_name: String,
        window_id: String,
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
        session_name: String,
        window_id: String,
        event: String,
    },
}

// ============================================================================
// Validation & resolution
// ============================================================================

/// Parse a hook request body, logging deserialization failures.
/// Claude Code hook payload fields drift across versions; a silent 422 hides
/// that for months (PostToolUse ingest was broken unnoticed since 2026-04).
fn parse_hook_body<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    body: &axum::body::Bytes,
) -> Result<T, Json<HookResponse>> {
    serde_json::from_slice::<T>(body).map_err(|e| {
        let sample: String = String::from_utf8_lossy(body).chars().take(300).collect();
        warn!(endpoint, error = %e, body_sample = %sample, "hook: body deserialization failed");
        Json(HookResponse::err(format!("invalid body: {}", e)))
    })
}

/// Extract the tmux pane ID forwarded by agent-hook.sh ($TMUX_PANE)
fn extract_tmux_pane(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-tmux-pane")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|p| p.starts_with('%'))
        .map(str::to_string)
}

/// Resolve cwd to git_dir (cached), check workspace is registered.
/// Returns None (with skipped response) if workspace is not tracked.
///
/// Attribution priority:
/// 1. Exact: pane binding from the X-Tmux-Pane header (disambiguates multiple
///    Claude instances in the same directory)
/// 2. Fallback: first task whose window resolves to the same git_dir (legacy,
///    ambiguous when several windows share a directory)
async fn validate_and_resolve(
    state: &Arc<AppState>,
    ctx: &HookContext,
    pane: Option<&str>,
    transcript_path: Option<&str>,
) -> Result<ResolvedContext, HookResponse> {
    if ctx.session_id.is_empty() {
        return Err(HookResponse::err("missing session_id"));
    }
    if ctx.cwd.is_empty() {
        return Err(HookResponse::err("missing cwd"));
    }

    // Resolve cwd → git_dir (cached, async git call)
    let git_dir = resolve_git_dir(state, &ctx.cwd).await;
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

    // Exact attribution via pane binding; git_dir task-matching as fallback
    let pane_binding = match pane {
        Some(p) => state.pane_registry.update(p, &ctx.session_id, transcript_path).await,
        None => None,
    };
    let (session_name, window_id) = match pane_binding {
        Some((b, changed)) => {
            if changed {
                let row = crate::pane_registry::PaneRegistry::to_row(&b);
                let state_for_db = state.clone();
                tokio::task::spawn_blocking(move || {
                    let server = state_for_db.state.lock().unwrap();
                    if let Err(e) = server.db.upsert_pane_binding(&row) {
                        warn!(pane = %row.pane_id, error = %e, "hook: pane binding persist failed");
                    }
                });
            }
            (b.session_name, b.window_id)
        }
        None => resolve_tmux_context(state, &git_dir),
    };

    Ok(ResolvedContext {
        claude_session_id: ctx.session_id.clone(),
        git_dir,
        session_name,
        window_id,
    })
}

/// Resolve cwd to git root directory, with caching.
/// Uses spawn_blocking for the git command to avoid blocking tokio runtime.
async fn resolve_git_dir(state: &Arc<AppState>, cwd: &str) -> Option<String> {
    // Check cache first (fast path, no blocking)
    {
        let cache = state.git_dir_cache.lock().unwrap();
        if let Some(cached) = cache.get(cwd) {
            return cached.clone();
        }
    }

    // Run git rev-parse in a blocking thread to avoid starving tokio runtime
    let cwd_owned = cwd.to_string();
    let result = tokio::task::spawn_blocking(move || {
        std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(&cwd_owned)
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
    }).await.unwrap_or(None);

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
    /// Agent type (e.g. "subagent" for SubagentStop)
    #[serde(default)]
    agent_type: Option<String>,
    /// Path to the Claude Code transcript JSONL (present in hook input)
    #[serde(default)]
    transcript_path: Option<String>,
}

pub(crate) async fn hook_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Json<HookResponse> {
    let req: HookMessageRequest = match parse_hook_body("message", &body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    // Subagent completion events fire inside the main session's pane —
    // never let them (re)bind the pane (herdr integration lesson)
    let pane = if req.event_type == "SubagentStop" {
        None
    } else {
        extract_tmux_pane(&headers)
    };
    let ctx = HookContext { session_id: req.session_id, cwd: req.cwd };
    let resolved = match validate_and_resolve(&state, &ctx, pane.as_deref(), req.transcript_path.as_deref()).await {
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

    let agent_type = req.agent_type;

    // Broadcast to WebSocket immediately (frontend sees it instantly)
    let now = chrono::Utc::now().to_rfc3339();
    let event = HookEvent::ChatMessage {
        claude_session_id: resolved.claude_session_id.clone(),
        git_dir: resolved.git_dir.clone(),
        session_name: resolved.session_name.clone(),
        window_id: resolved.window_id.clone(),
        role: role.clone(),
        content: content.clone(),
        agent_type,
        timestamp: now,
    };
    let _ = state.hook_broadcast_tx.send(event);

    info!(
        event = "hook.message",
        role = %role,
        session = %resolved.session_name,
        window = %resolved.window_id,
        pane = pane.as_deref().unwrap_or("-"),
        outcome = "ok",
        "hook message ingested"
    );

    // Persist to DB asynchronously (don't block the response)
    let state_for_db = state.clone();
    let resolved_for_db = resolved;
    let role_for_db = role;
    let content_for_db = content;
    tokio::task::spawn_blocking(move || {
        let server = state_for_db.state.lock().unwrap();
        let history_id = server.db.find_or_create_hook_history(
            &resolved_for_db.claude_session_id,
            &resolved_for_db.session_name,
            &resolved_for_db.window_id,
            &resolved_for_db.git_dir,
        );
        match history_id {
            Ok(id) => {
                if let Err(e) = server.db.insert_hook_message(id, &resolved_for_db.claude_session_id, &role_for_db, &content_for_db) {
                    warn!("hook/message: DB write error: {}", e);
                }
            }
            Err(e) => warn!("hook/message: history resolution error: {}", e),
        }
    });

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
    /// String in older Claude Code versions, structured object in newer ones
    #[serde(default)]
    tool_response: Option<serde_json::Value>,
    /// Path to the Claude Code transcript JSONL (present in hook input)
    #[serde(default)]
    transcript_path: Option<String>,
}

pub(crate) async fn hook_tool(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Json<HookResponse> {
    let req: HookToolRequest = match parse_hook_body("tool", &body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let pane = extract_tmux_pane(&headers);
    let ctx = HookContext { session_id: req.session_id, cwd: req.cwd };
    let resolved = match validate_and_resolve(&state, &ctx, pane.as_deref(), req.transcript_path.as_deref()).await {
        Ok(r) => r,
        Err(resp) => return Json(resp),
    };

    let tool_use_id = req.tool_use_id.unwrap_or_default();
    let tool_args = req.tool_input
        .map(|v| serde_json::to_string(&v).unwrap_or_default())
        .unwrap_or_default();
    let result_summary = req.tool_response.map(|v| {
        let s = match v {
            serde_json::Value::String(s) => s,
            other => serde_json::to_string(&other).unwrap_or_default(),
        };
        let truncated: String = s.chars().take(500).collect();
        if truncated.len() < s.len() { format!("{}...", truncated) } else { s }
    }).unwrap_or_default();

    // Broadcast to WebSocket immediately
    let now = chrono::Utc::now().to_rfc3339();
    let event = HookEvent::ToolEvent {
        claude_session_id: resolved.claude_session_id.clone(),
        git_dir: resolved.git_dir.clone(),
        tool_name: req.tool_name.clone(),
        tool_use_id: tool_use_id.clone(),
        timestamp: now,
    };
    let _ = state.hook_broadcast_tx.send(event);

    debug!(
        event = "hook.tool",
        tool = %req.tool_name,
        session = %resolved.session_name,
        window = %resolved.window_id,
        pane = pane.as_deref().unwrap_or("-"),
        outcome = "ok",
        "hook tool ingested"
    );

    // Persist to DB asynchronously
    let state_for_db = state.clone();
    let resolved_for_db = resolved;
    let tool_name = req.tool_name;
    tokio::task::spawn_blocking(move || {
        let server = state_for_db.state.lock().unwrap();
        let history_id = server.db.find_or_create_hook_history(
            &resolved_for_db.claude_session_id,
            &resolved_for_db.session_name,
            &resolved_for_db.window_id,
            &resolved_for_db.git_dir,
        );
        match history_id {
            Ok(id) => {
                if let Err(e) = server.db.insert_hook_tool_usage(
                    id, &resolved_for_db.claude_session_id, &tool_name,
                    &tool_args, &result_summary, &tool_use_id,
                ) {
                    warn!("hook/tool: DB write error: {}", e);
                }
            }
            Err(e) => warn!("hook/tool: history resolution error: {}", e),
        }
    });

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
    /// Path to the Claude Code transcript JSONL (present in hook input)
    #[serde(default)]
    transcript_path: Option<String>,
}

pub(crate) async fn hook_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Json<HookResponse> {
    let req: HookSessionRequest = match parse_hook_body("session", &body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let pane = extract_tmux_pane(&headers);
    let ctx = HookContext { session_id: req.session_id, cwd: req.cwd };
    let resolved = match validate_and_resolve(&state, &ctx, pane.as_deref(), req.transcript_path.as_deref()).await {
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
                session_name: resolved.session_name,
                window_id: resolved.window_id,
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
                session_name: resolved.session_name,
                window_id: resolved.window_id,
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
