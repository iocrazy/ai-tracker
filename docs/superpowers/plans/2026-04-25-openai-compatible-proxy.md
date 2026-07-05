# OpenAI-Compatible API Proxy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose Claude Code tmux sessions via an OpenAI-compatible `/v1/chat/completions` endpoint, enabling external clients (Chatbox, LobeChat, custom apps) to interact with live sessions.

**Architecture:** New `routes_openai.rs` module handles requests. Sends user message to tmux pane via existing `TmuxAgent`. Listens for response via `hook_broadcast_tx` (event-driven, zero polling). Per-pane `Mutex` prevents concurrent requests. Supports both streaming SSE and non-streaming responses.

**Tech Stack:** axum, tokio broadcast channels, SSE via `axum::response::Sse`, serde for OpenAI format

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/rust/crates/tracker-server/src/routes_openai.rs` | Create | OpenAI proxy endpoint, request/response types, completion logic |
| `src/rust/crates/tracker-server/src/main.rs` | Modify | Register route, add `pane_locks` to AppState |

---

### Task 1: Request/Response Types

**Files:**
- Create: `src/rust/crates/tracker-server/src/routes_openai.rs`

- [ ] **Step 1: Create routes_openai.rs with OpenAI-compatible types**

```rust
//! OpenAI-compatible API proxy: routes external chat requests to Claude Code tmux sessions.

use std::collections::HashMap;
use std::sync::Arc;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures::stream::Stream;
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
    /// Ignored — always routes to Claude Code
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

/// Streaming chunk (SSE delta)
#[derive(Serialize)]
pub(crate) struct ChatCompletionChunk {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Serialize)]
pub(crate) struct ChunkChoice {
    pub index: u32,
    pub delta: Delta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Per-pane lock to prevent concurrent requests
pub(crate) type PaneLocks = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;

pub(crate) fn new_pane_locks() -> PaneLocks {
    Arc::new(Mutex::new(HashMap::new()))
}
```

- [ ] **Step 2: Add module declaration in main.rs**

In `src/rust/crates/tracker-server/src/main.rs`, add near the other `mod` declarations:

```rust
mod routes_openai;
```

- [ ] **Step 3: Compile check**

Run: `cd src/rust && cargo check -p tracker-server 2>&1 | tail -5`
Expected: Warnings only, no errors

- [ ] **Step 4: Commit**

```bash
git add src/rust/crates/tracker-server/src/routes_openai.rs src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: add OpenAI-compatible proxy types (routes_openai.rs)"
```

---

### Task 2: Target Resolution + Pane Locking

**Files:**
- Modify: `src/rust/crates/tracker-server/src/routes_openai.rs`

- [ ] **Step 1: Add resolve_target helper**

Append to `routes_openai.rs`:

```rust
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
/// Priority: explicit request fields > first active session with Claude pane.
async fn resolve_target(req: &ChatCompletionRequest) -> Result<ResolvedTarget, String> {
    // If session + window specified, use them
    if let (Some(session), Some(window)) = (&req.session, &req.window) {
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

    // Auto-detect: find first session with a Claude pane
    let sessions = TmuxAgent::list_windows()
        .await
        .map_err(|e| format!("Failed to list sessions: {}", e))?;

    for sess in &sessions {
        for win in &sess.windows {
            if let Ok(status) = TmuxAgent::get_claude_status(&sess.name, &win.name).await {
                if let Some(pane) = &status.pane {
                    // Skip if Claude is busy (awaiting_resume or has active tool)
                    if status.awaiting_resume {
                        continue;
                    }
                    let lock_key = format!("{}:{}:{}", sess.name, win.name, pane);
                    return Ok(ResolvedTarget {
                        session: sess.name.clone(),
                        window: win.name.clone(),
                        pane: pane.clone(),
                        lock_key,
                    });
                }
            }
        }
    }

    Err("No active Claude Code session found".to_string())
}

/// Acquire per-pane lock (prevents concurrent requests to same pane)
async fn acquire_pane_lock(locks: &PaneLocks, key: &str) -> Arc<Mutex<()>> {
    let mut map = locks.lock().await;
    map.entry(key.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}
```

- [ ] **Step 2: Compile check**

Run: `cd src/rust && cargo check -p tracker-server 2>&1 | tail -5`
Expected: Warnings only

- [ ] **Step 3: Commit**

```bash
git add src/rust/crates/tracker-server/src/routes_openai.rs
git commit -m "feat: add target resolution and pane locking for OpenAI proxy"
```

---

### Task 3: Non-Streaming Completion Handler

**Files:**
- Modify: `src/rust/crates/tracker-server/src/routes_openai.rs`
- Modify: `src/rust/crates/tracker-server/src/main.rs`

- [ ] **Step 1: Add AppState fields for pane_locks**

In `main.rs`, add to `AppState` struct:

```rust
    pane_locks: routes_openai::PaneLocks,
```

In the AppState initialization block (where `AppState { ... }` is constructed), add:

```rust
    pane_locks: routes_openai::new_pane_locks(),
```

- [ ] **Step 2: Add the completion handler to routes_openai.rs**

Append to `routes_openai.rs`:

```rust
// ============================================================================
// POST /v1/chat/completions
// ============================================================================

pub(crate) async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> axum::response::Response {
    // Extract user message (last message with role=user)
    let user_msg = match req.messages.iter().rev().find(|m| m.role == "user") {
        Some(m) => m.content.clone(),
        None => {
            return error_response(400, "No user message found in messages array");
        }
    };

    // Resolve target pane
    let target = match resolve_target(&req).await {
        Ok(t) => t,
        Err(e) => return error_response(400, &e),
    };

    info!(
        "openai proxy: session={}, window={}, pane={}, stream={}, msg_len={}",
        target.session, target.window, target.pane, req.stream, user_msg.len()
    );

    // Acquire pane lock
    let lock = acquire_pane_lock(&state.pane_locks, &target.lock_key).await;
    let _guard = lock.lock().await;

    // Subscribe to hook broadcasts BEFORE sending (so we don't miss the response)
    let mut hook_rx = state.hook_broadcast_tx.subscribe();

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

    let request_id = format!("chatcmpl-{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string());
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if req.stream {
        return stream_response(state, hook_rx, target, request_id, created).await;
    }

    // Non-streaming: wait for Stop hook event with assistant response
    let timeout = tokio::time::Duration::from_secs(300); // 5 min
    let result = tokio::time::timeout(timeout, async {
        loop {
            match hook_rx.recv().await {
                Ok(HookEvent::ChatMessage {
                    role,
                    content,
                    session_name,
                    window_id,
                    ..
                }) => {
                    // Match by session+window, wait for assistant Stop message
                    if role == "assistant"
                        && session_name == target.session
                        && window_id == target.window
                    {
                        return content;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("openai proxy: hook_rx lagged by {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
                _ => {} // Skip non-ChatMessage events
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
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            };
            Json(response).into_response()
        }
        Err(_) => error_response(504, "Request timed out waiting for Claude response"),
    }
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
```

- [ ] **Step 3: Register route in main.rs**

In the router setup section, add alongside existing routes:

```rust
        .route("/v1/chat/completions", post(routes_openai::chat_completions))
```

- [ ] **Step 4: Compile check**

Run: `cd src/rust && cargo check -p tracker-server 2>&1 | tail -10`
Expected: Warnings only (stream_response not yet defined)

- [ ] **Step 5: Commit**

```bash
git add src/rust/crates/tracker-server/src/routes_openai.rs src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: non-streaming OpenAI chat completions endpoint"
```

---

### Task 4: Streaming SSE Response

**Files:**
- Modify: `src/rust/crates/tracker-server/src/routes_openai.rs`

- [ ] **Step 1: Add stream_response function**

Append to `routes_openai.rs`:

```rust
use axum::response::IntoResponse;
use tokio_stream::wrappers::BroadcastStream;

/// Stream assistant response as SSE (OpenAI streaming format)
async fn stream_response(
    state: Arc<AppState>,
    mut hook_rx: tokio::sync::broadcast::Receiver<HookEvent>,
    target: ResolvedTarget,
    request_id: String,
    created: u64,
) -> axum::response::Response {
    let stream = async_stream::stream! {
        // First chunk: role
        let first_chunk = ChatCompletionChunk {
            id: request_id.clone(),
            object: "chat.completion.chunk",
            created,
            model: "claude-code".to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta {
                    role: Some("assistant".to_string()),
                    content: None,
                },
                finish_reason: None,
            }],
        };
        yield Ok::<_, std::convert::Infallible>(
            Event::default().data(serde_json::to_string(&first_chunk).unwrap())
        );

        // Listen for chat_watcher incremental messages (JSONL-based, ~500ms granularity)
        let mut chat_rx = state.chat_watcher.subscribe();
        let timeout = tokio::time::Duration::from_secs(300);
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            tokio::select! {
                // Hook events: detect completion (Stop event with assistant role)
                result = hook_rx.recv() => {
                    match result {
                        Ok(HookEvent::ChatMessage { role, content, session_name, window_id, .. }) => {
                            if role == "assistant" && session_name == target.session && window_id == target.window {
                                // Final content from Stop hook — send as last chunk
                                let chunk = ChatCompletionChunk {
                                    id: request_id.clone(),
                                    object: "chat.completion.chunk",
                                    created,
                                    model: "claude-code".to_string(),
                                    choices: vec![ChunkChoice {
                                        index: 0,
                                        delta: Delta { role: None, content: Some(content) },
                                        finish_reason: Some("stop".to_string()),
                                    }],
                                };
                                yield Ok(Event::default().data(serde_json::to_string(&chunk).unwrap()));
                                yield Ok(Event::default().data("[DONE]".to_string()));
                                return;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        _ => {}
                    }
                }
                // Chat watcher: incremental JSONL messages (stream deltas)
                result = chat_rx.recv() => {
                    match result {
                        Ok(event) => {
                            // Filter for assistant messages only
                            for msg in &event.messages {
                                if msg.role == "assistant" && !msg.text.is_empty() {
                                    let chunk = ChatCompletionChunk {
                                        id: request_id.clone(),
                                        object: "chat.completion.chunk",
                                        created,
                                        model: "claude-code".to_string(),
                                        choices: vec![ChunkChoice {
                                            index: 0,
                                            delta: Delta { role: None, content: Some(msg.text.clone()) },
                                            finish_reason: None,
                                        }],
                                    };
                                    yield Ok(Event::default().data(serde_json::to_string(&chunk).unwrap()));
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                // Timeout
                _ = tokio::time::sleep(remaining) => {
                    break;
                }
            }
        }

        // Timeout or channel closed — send done
        yield Ok(Event::default().data("[DONE]".to_string()));
    };

    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}
```

- [ ] **Step 2: Add async-stream dependency**

In `src/rust/crates/tracker-server/Cargo.toml`, add:

```toml
async-stream = "0.3"
```

- [ ] **Step 3: Add required imports at top of routes_openai.rs**

Ensure imports include:

```rust
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
```

- [ ] **Step 4: Compile check**

Run: `cd src/rust && cargo check -p tracker-server 2>&1 | tail -10`
Expected: Clean compile (warnings OK)

- [ ] **Step 5: Commit**

```bash
git add src/rust/crates/tracker-server/src/routes_openai.rs src/rust/crates/tracker-server/Cargo.toml
git commit -m "feat: add SSE streaming for OpenAI chat completions"
```

---

### Task 5: Integration Test + Deploy

**Files:**
- No file changes — manual testing

- [ ] **Step 1: Build**

Run: `cd src/rust && cargo build -p tracker-server 2>&1 | tail -5`
Expected: Compiles successfully

- [ ] **Step 2: Deploy**

Run: `./scripts/deploy.sh --tauri 2>&1 | tail -10`
Expected: Health check passes

- [ ] **Step 3: Test non-streaming**

```bash
TOKEN=$(cat '/Users/heygo/Library/Application Support/com.agent-tracker.menubar/agent-config.json' | python3 -c "import sys,json; print(json.load(sys.stdin)['auth']['token'])")
curl -s -X POST http://localhost:3099/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-code",
    "messages": [{"role": "user", "content": "echo hello"}],
    "stream": false
  }' | python3 -m json.tool
```

Expected: JSON response with `choices[0].message.content` containing Claude's reply.

- [ ] **Step 4: Test streaming**

```bash
curl -s -N -X POST http://localhost:3099/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-code",
    "messages": [{"role": "user", "content": "echo hello"}],
    "stream": true
  }'
```

Expected: SSE events `data: {"id":"chatcmpl-...","choices":[{"delta":{"content":"..."}}]}` followed by `data: [DONE]`

- [ ] **Step 5: Test with session targeting**

```bash
curl -s -X POST http://localhost:3099/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-code",
    "messages": [{"role": "user", "content": "echo hello"}],
    "stream": false,
    "session": "1-mediahub",
    "window": "master"
  }' | python3 -m json.tool
```

- [ ] **Step 6: Test error case — no active session**

```bash
curl -s -X POST http://localhost:3099/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-code",
    "messages": [{"role": "user", "content": "test"}],
    "session": "nonexistent",
    "window": "nowindow"
  }' | python3 -m json.tool
```

Expected: Error response `{"error": {"message": "...", "code": 400}}`

- [ ] **Step 7: Final commit**

```bash
git add -A
git commit -m "feat: OpenAI-compatible proxy for Claude Code sessions"
```
