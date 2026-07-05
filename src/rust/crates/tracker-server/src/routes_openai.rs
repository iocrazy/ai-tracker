//! OpenAI-compatible API proxy: routes external chat requests to Claude Code tmux sessions.
//!
//! POST /v1/chat/completions — accepts OpenAI ChatCompletion format, sends user message
//! to a Claude Code tmux pane, and returns the response (streaming SSE or non-streaming JSON).

use std::collections::HashMap;
use std::sync::Arc;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::agent::TmuxAgent;
use crate::routes_hook::HookEvent;
use crate::AppState;

// ============================================================================
// OpenAI-compatible request/response types
// ============================================================================

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionRequest {
    #[serde(default)]
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    /// Target tmux session (e.g., "1-mediahub"). Falls back to first active session.
    #[serde(default)]
    pub session: Option<String>,
    /// Target tmux window name (e.g., "master"). Falls back to active window.
    #[serde(default)]
    pub window: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub(crate) struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub(crate) struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Serialize)]
pub(crate) struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Serialize)]
pub(crate) struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Serialize)]
struct ChatCompletionChunk {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    choices: Vec<ChunkChoice>,
}

#[derive(Serialize)]
struct ChunkChoice {
    index: u32,
    delta: Delta,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

#[derive(Serialize)]
struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

/// Per-pane lock to prevent concurrent requests to same pane
pub(crate) type PaneLocks = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;

pub(crate) fn new_pane_locks() -> PaneLocks {
    Arc::new(Mutex::new(HashMap::new()))
}

// ============================================================================
// Target resolution
// ============================================================================

struct ResolvedTarget {
    session: String,
    window: String,
    pane: String,
    lock_key: String,
}

/// Resolve which tmux session/window/pane to send the message to.
/// If a channel was resolved from the API key, use its session/window.
async fn resolve_target(req: &ChatCompletionRequest, channel: Option<&crate::db::Channel>) -> Result<ResolvedTarget, String> {
    // Channel key takes priority
    let (explicit_session, explicit_window) = if let Some(ch) = channel {
        (Some(ch.session_name.clone()), Some(ch.window_name.clone()))
    } else {
        (req.session.clone(), req.window.clone())
    };

    // If session + window specified, use them
    if let (Some(session), Some(window)) = (&explicit_session, &explicit_window) {
        let status = TmuxAgent::get_claude_status(session, window)
            .await
            .map_err(|e| format!("Failed to get status: {}", e))?;
        let pane = status.pane.unwrap_or_else(|| "0".to_string());
        let lock_key = format!("{}:{}:{}", session, window, pane);
        return Ok(ResolvedTarget {
            session: session.clone(),
            window: window.clone(),
            pane,
            lock_key,
        });
    }

    // Auto-detect: scan all windows, find first with a Claude pane that's idle
    let windows = TmuxAgent::list_all_windows()
        .await
        .map_err(|e| format!("Failed to list windows: {}", e))?;

    for win in &windows {
        if let Ok(status) = TmuxAgent::get_claude_status(&win.session_name, &win.window_name).await {
            if let Some(ref pane) = status.pane {
                if status.awaiting_resume {
                    continue;
                }
                let lock_key = format!("{}:{}:{}", win.session_name, win.window_name, pane);
                return Ok(ResolvedTarget {
                    session: win.session_name.clone(),
                    window: win.window_name.clone(),
                    pane: pane.clone(),
                    lock_key,
                });
            }
        }
    }

    Err("No active Claude Code session found".to_string())
}

async fn acquire_pane_lock(locks: &PaneLocks, key: &str) -> Arc<Mutex<()>> {
    let mut map = locks.lock().await;
    map.entry(key.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

// ============================================================================
// POST /v1/chat/completions
// ============================================================================

pub(crate) async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ChatCompletionRequest>,
) -> axum::response::Response {
    // Extract user message (last message with role=user)
    let user_msg = match req.messages.iter().rev().find(|m| m.role == "user") {
        Some(m) => m.content.clone(),
        None => return error_response(400, "No user message found in messages array"),
    };

    let is_stream = req.stream;

    // Look up channel by Bearer token (if it's a channel key, not the main auth token)
    let channel = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|token| {
            let server = state.state.lock().unwrap();
            server.db.find_channel_by_key(token)
        });

    // Resolve target pane
    let target = match resolve_target(&req, channel.as_ref()).await {
        Ok(t) => t,
        Err(e) => return error_response(400, &e),
    };

    info!(
        "openai proxy: session={}, window={}, pane={}, stream={}, msg_len={}",
        target.session, target.window, target.pane, is_stream, user_msg.len()
    );

    // Acquire pane lock
    let lock = acquire_pane_lock(&state.pane_locks, &target.lock_key).await;
    let _guard = lock.lock().await;

    // Subscribe to hook broadcasts BEFORE sending (so we don't miss the response)
    let hook_rx = state.hook_broadcast_tx.subscribe();

    // Send message to Claude pane
    if let Err(e) = TmuxAgent::send_keys_with_suffix(
        &target.session,
        &target.window,
        &target.pane,
        &user_msg,
        Some("Enter"),
    ).await {
        return error_response(500, &format!("Failed to send message: {}", e));
    }

    let request_id = format!("chatcmpl-{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]);
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if is_stream {
        return stream_response(state, hook_rx, target, request_id, created).await;
    }

    // Non-streaming: wait for Stop hook event with assistant response
    non_stream_response(hook_rx, target, request_id, created).await
}

async fn non_stream_response(
    mut hook_rx: tokio::sync::broadcast::Receiver<HookEvent>,
    target: ResolvedTarget,
    request_id: String,
    created: u64,
) -> axum::response::Response {
    let timeout = tokio::time::Duration::from_secs(300);
    let result = tokio::time::timeout(timeout, async {
        loop {
            match hook_rx.recv().await {
                Ok(HookEvent::ChatMessage {
                    role,
                    content,
                    session_name,
                    ..
                }) => {
                    if role == "assistant" && session_name == target.session {
                        return content;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("openai proxy: hook_rx lagged by {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
        String::new()
    }).await;

    match result {
        Ok(content) => {
            let response = ChatCompletionResponse {
                id: request_id,
                object: "chat.completion",
                created,
                model: "claude-code".to_string(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content,
                    },
                    finish_reason: "stop".to_string(),
                }],
                usage: Usage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
            };
            Json(response).into_response()
        }
        Err(_) => error_response(504, "Request timed out waiting for Claude response (5min)"),
    }
}

// ============================================================================
// SSE Streaming
// ============================================================================

async fn stream_response(
    state: Arc<AppState>,
    mut hook_rx: tokio::sync::broadcast::Receiver<HookEvent>,
    target: ResolvedTarget,
    request_id: String,
    created: u64,
) -> axum::response::Response {
    let stream = async_stream::stream! {
        // First chunk: role indicator
        yield Ok::<_, std::convert::Infallible>(Event::default().data(
            serde_json::to_string(&ChatCompletionChunk {
                id: request_id.clone(),
                object: "chat.completion.chunk",
                created,
                model: "claude-code".to_string(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta { role: Some("assistant".to_string()), content: None },
                    finish_reason: None,
                }],
            }).unwrap()
        ));

        // Dual-listen: chat_watcher for incremental JSONL, hook for completion
        let mut chat_rx = state.chat_watcher.subscribe();
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { break; }

            tokio::select! {
                // Hook: detect Stop event (assistant done)
                result = hook_rx.recv() => {
                    match result {
                        Ok(HookEvent::ChatMessage { role, content, session_name, .. }) => {
                            if role == "assistant" && session_name == target.session {
                                // Final chunk with content + finish_reason
                                yield Ok(Event::default().data(
                                    serde_json::to_string(&ChatCompletionChunk {
                                        id: request_id.clone(),
                                        object: "chat.completion.chunk",
                                        created,
                                        model: "claude-code".to_string(),
                                        choices: vec![ChunkChoice {
                                            index: 0,
                                            delta: Delta { role: None, content: Some(content) },
                                            finish_reason: Some("stop".to_string()),
                                        }],
                                    }).unwrap()
                                ));
                                yield Ok(Event::default().data("[DONE]".to_string()));
                                return;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        _ => {}
                    }
                }
                // Chat watcher: incremental JSONL messages
                result = chat_rx.recv() => {
                    match result {
                        Ok(event) => {
                            for msg in &event.messages {
                                if msg.role == "assistant" && !msg.text.is_empty() {
                                    yield Ok(Event::default().data(
                                        serde_json::to_string(&ChatCompletionChunk {
                                            id: request_id.clone(),
                                            object: "chat.completion.chunk",
                                            created,
                                            model: "claude-code".to_string(),
                                            choices: vec![ChunkChoice {
                                                index: 0,
                                                delta: Delta { role: None, content: Some(msg.text.clone()) },
                                                finish_reason: None,
                                            }],
                                        }).unwrap()
                                    ));
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = tokio::time::sleep(remaining) => break,
            }
        }

        yield Ok(Event::default().data("[DONE]".to_string()));
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

// ============================================================================
// Channel management API
// ============================================================================

#[derive(Debug, Deserialize)]
pub(crate) struct CreateChannelRequest {
    pub name: String,
    pub session_name: String,
    pub window_name: String,
}

pub(crate) async fn list_channels(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let channels = {
        let server = state.state.lock().unwrap();
        server.db.list_channels().unwrap_or_default()
    };
    Json(serde_json::json!({ "channels": channels }))
}

pub(crate) async fn create_channel(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateChannelRequest>,
) -> axum::response::Response {
    let result = {
        let server = state.state.lock().unwrap();
        server.db.create_channel(&req.name, &req.session_name, &req.window_name)
    };
    match result {
        Ok(channel) => Json(serde_json::json!({ "success": true, "channel": channel })).into_response(),
        Err(e) => error_response(500, &format!("Failed to create channel: {}", e)),
    }
}

pub(crate) async fn delete_channel(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Json<serde_json::Value> {
    let deleted = {
        let server = state.state.lock().unwrap();
        server.db.delete_channel(id).unwrap_or(false)
    };
    Json(serde_json::json!({ "success": deleted }))
}

fn error_response(status: u16, message: &str) -> axum::response::Response {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": "api_error",
            "code": status,
        }
    });
    (
        axum::http::StatusCode::from_u16(status).unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        Json(body),
    ).into_response()
}
