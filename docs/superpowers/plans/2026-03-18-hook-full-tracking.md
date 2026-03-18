# Hook Full-Chain Tracking Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate 6 Claude Code hooks for real-time conversation capture, tool usage tracking, and session lifecycle management.

**Architecture:** New `routes_hook.rs` module handles 3 ingest endpoints. Hook script (`agent-hook.sh`) routes stdin JSON to the correct endpoint. Frontend listens for new WebSocket events (`chat_message`, `tool_event`) for real-time updates.

**Tech Stack:** Rust/axum backend, `jq` in hook script, React/TypeScript frontend, SQLite

**Spec:** `docs/superpowers/specs/2026-03-18-hook-full-tracking-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/rust/crates/tracker-server/src/db.rs` | Modify | Migrations 103-106 + hook CRUD methods |
| `src/rust/crates/tracker-server/src/routes_hook.rs` | Create | 3 hook ingest endpoints + git_dir resolver |
| `src/rust/crates/tracker-server/src/main.rs` | Modify | Wire routes_hook, add git_dir cache to AppState, staleness timer |
| `scripts/agent-hook.sh` | Create | Unified hook script for all 6 events |
| `web/src/services/state.ts` | Modify | Handle new WebSocket event types |
| `web/src/components/ChatHistoryModal.tsx` | Modify | Append real-time chat_message events |
| `web/src/App.tsx` | Modify | Status sync from session events |

---

### Task 1: Database Migrations (103-106)

**Files:**
- Modify: `src/rust/crates/tracker-server/src/db.rs:545-550` (migrations array)

- [ ] **Step 1: Add 4 migrations after the totp_config migration**

In `db.rs`, inside the `migrations` array (after migration 102), add:

```rust
            (103, "ALTER TABLE history ADD COLUMN claude_session_id TEXT DEFAULT ''"),
            (104, "CREATE INDEX IF NOT EXISTS idx_history_claude_session ON history(claude_session_id)"),
            (105, "ALTER TABLE conversation_messages ADD COLUMN claude_session_id TEXT DEFAULT ''"),
            (106, "ALTER TABLE conversation_messages ADD COLUMN source TEXT DEFAULT 'hook'"),
            (107, "ALTER TABLE tool_usage ADD COLUMN claude_session_id TEXT DEFAULT ''"),
            (108, "ALTER TABLE tool_usage ADD COLUMN tool_use_id TEXT DEFAULT ''"),
            (109, "CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_usage_use_id ON tool_usage(tool_use_id) WHERE tool_use_id != ''"),
```

- [ ] **Step 2: Add hook-specific DB methods**

After the TOTP methods in db.rs, add:

```rust
    // =========================================================================
    // Hook Ingest
    // =========================================================================

    /// Find or create a history entry for a Claude session.
    /// Returns the history_id.
    pub fn find_or_create_hook_history(
        &self,
        claude_session_id: &str,
        session_name: &str,
        window_id: &str,
        git_dir: &str,
    ) -> Result<i64> {
        // Try to find existing
        let existing: Option<i64> = self.conn.query_row(
            "SELECT id FROM history WHERE claude_session_id = ?1 ORDER BY id DESC LIMIT 1",
            params![claude_session_id],
            |row| row.get(0),
        ).ok();

        if let Some(id) = existing {
            return Ok(id);
        }

        // Create new history entry
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO history (session_id, session, window_id, window, pane, summary, started_at, claude_session_id, git_dir)
             VALUES (?1, ?2, ?3, ?4, '1', ?5, ?6, ?7, ?8)",
            params![
                session_name, session_name, window_id, window_id,
                format!("Claude session {}", &claude_session_id[..8.min(claude_session_id.len())]),
                now, claude_session_id, git_dir,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a conversation message from hook.
    pub fn insert_hook_message(
        &self,
        history_id: i64,
        claude_session_id: &str,
        role: &str,
        content: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO conversation_messages (history_id, role, content, created_at, claude_session_id, source)
             VALUES (?1, ?2, ?3, ?4, ?5, 'hook')",
            params![history_id, role, content, now, claude_session_id],
        )?;
        Ok(())
    }

    /// Insert a tool usage record from hook (with dedup via tool_use_id).
    pub fn insert_hook_tool_usage(
        &self,
        history_id: i64,
        claude_session_id: &str,
        tool_name: &str,
        tool_args: &str,
        result_summary: &str,
        tool_use_id: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO tool_usage (history_id, tool_name, tool_args, result_summary, success, timestamp, claude_session_id, tool_use_id)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7)",
            params![history_id, tool_name, tool_args, result_summary, now, claude_session_id, tool_use_id],
        )?;
        Ok(())
    }

    /// Close a Claude session's history entry.
    pub fn close_hook_session(&self, claude_session_id: &str, reason: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE history SET completed_at = ?1, completion_note = ?2
             WHERE claude_session_id = ?3 AND completed_at IS NULL",
            params![now, reason, claude_session_id],
        )?;
        Ok(())
    }

    /// Auto-close stale hook sessions (no activity for given minutes).
    pub fn close_stale_hook_sessions(&self, stale_minutes: i64) -> Result<usize> {
        let count = self.conn.execute(
            "UPDATE history SET completed_at = datetime('now'), completion_note = 'auto-closed: stale'
             WHERE claude_session_id != '' AND completed_at IS NULL
             AND started_at < datetime('now', ?1)",
            params![format!("-{} minutes", stale_minutes)],
        )?;
        Ok(count)
    }
```

- [ ] **Step 3: Verify build**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check`
Expected: Compiles (new columns in ALTER TABLE don't break existing queries)

- [ ] **Step 4: Commit**

```bash
git add src/rust/crates/tracker-server/src/db.rs
git commit -m "feat: add hook tracking DB migrations and CRUD methods"
```

---

### Task 2: Create routes_hook.rs (3 API Endpoints)

**Files:**
- Create: `src/rust/crates/tracker-server/src/routes_hook.rs`

- [ ] **Step 1: Create routes_hook.rs**

Full file content:

```rust
//! Hook ingest routes for Claude Code integration.
//!
//! Handles: /api/hook/message, /api/hook/tool, /api/hook/session

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use serde::Deserialize;
use tracing::{debug, error, info, warn};

use crate::AppState;

// ============================================================================
// Git dir resolution + workspace matching
// ============================================================================

/// Resolve cwd to git_dir, using cache. Returns None if not a git repo.
fn resolve_git_dir(
    cwd: &str,
    cache: &std::sync::Mutex<HashMap<String, Option<String>>>,
) -> Option<String> {
    // Check cache
    {
        let c = cache.lock().unwrap();
        if let Some(cached) = c.get(cwd) {
            return cached.clone();
        }
    }

    // Shell out to git
    let output = std::process::Command::new("git")
        .args(["-C", cwd, "rev-parse", "--show-toplevel"])
        .output()
        .ok();

    let git_dir = output
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    // Cache result
    {
        let mut c = cache.lock().unwrap();
        c.insert(cwd.to_string(), git_dir.clone());
    }

    git_dir
}

/// Check if git_dir matches any registered workspace.
fn is_registered_workspace(git_dir: &str, state: &AppState) -> bool {
    let config = state.state.lock().unwrap();
    // Check against registered workspaces' base_path
    for (_name, ws) in &config.config.workspaces {
        let base = ws.base_path.to_string_lossy();
        if base == git_dir || git_dir.starts_with(base.as_ref()) {
            return true;
        }
    }
    // Also check if git_dir appears in any active tmux window's git_dir
    false
}

/// Resolve cwd to tmux session:window context (best effort).
fn resolve_tmux_context(git_dir: &str, state: &AppState) -> (String, String) {
    // Try to find a tmux window associated with this git_dir
    let server = state.state.lock().unwrap();
    for task in &server.tasks {
        if let Some(ref td) = task.git_dir {
            if td == git_dir {
                return (task.session_id.clone(), task.window_id.clone());
            }
        }
    }
    // Fallback: use git_dir basename as session name
    let basename = PathBuf::from(git_dir)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    (basename, "hook".to_string())
}

// ============================================================================
// Shared request parsing
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct HookPayload {
    hook_event_name: Option<String>,
    session_id: Option<String>,
    cwd: Option<String>,
    transcript_path: Option<String>,
    // Message fields
    prompt: Option<String>,
    last_assistant_message: Option<String>,
    agent_type: Option<String>,
    // Tool fields
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
    tool_response: Option<String>,
    tool_use_id: Option<String>,
    // Session fields
    source: Option<String>,
    model: Option<String>,
    reason: Option<String>,
}

fn validate_and_resolve(
    payload: &HookPayload,
    state: &AppState,
) -> Result<(String, String), (StatusCode, Json<serde_json::Value>)> {
    let cwd = payload.cwd.as_deref().unwrap_or("");
    if cwd.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "missing cwd"})),
        ));
    }

    let git_dir = resolve_git_dir(cwd, &state.git_dir_cache)
        .ok_or_else(|| (
            StatusCode::OK,
            Json(serde_json::json!({"success": true, "skipped": true})),
        ))?;

    if !is_registered_workspace(&git_dir, state) {
        return Err((
            StatusCode::OK,
            Json(serde_json::json!({"success": true, "skipped": true})),
        ));
    }

    let claude_sid = payload.session_id.as_deref().unwrap_or("unknown").to_string();
    Ok((claude_sid, git_dir))
}

// ============================================================================
// POST /api/hook/message
// ============================================================================

pub(crate) async fn hook_message(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (claude_sid, git_dir) = match validate_and_resolve(&payload, &state) {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };

    let event = payload.hook_event_name.as_deref().unwrap_or("");
    let (role, content) = match event {
        "UserPromptSubmit" => ("user", payload.prompt.as_deref().unwrap_or("")),
        "Stop" | "SubagentStop" => ("assistant", payload.last_assistant_message.as_deref().unwrap_or("")),
        _ => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "unknown event"}))).into_response();
        }
    };

    if content.is_empty() {
        return Json(serde_json::json!({"success": true})).into_response();
    }

    let (session_name, window_id) = resolve_tmux_context(&git_dir, &state);

    let result = {
        let server = state.state.lock().unwrap();
        let history_id = server.db.find_or_create_hook_history(
            &claude_sid, &session_name, &window_id, &git_dir,
        )?;
        server.db.insert_hook_message(history_id, &claude_sid, role, content)?;
        Ok::<_, anyhow::Error>(history_id)
    };

    match result {
        Ok(_history_id) => {
            // WebSocket broadcast
            let _ = state.broadcast_tx.send(crate::RealtimeMessage::ChatMessage {
                claude_session_id: claude_sid.clone(),
                git_dir: git_dir.clone(),
                role: role.to_string(),
                content: content.to_string(),
                agent_type: payload.agent_type.clone(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
            debug!("Hook message stored: {} {} {}", event, claude_sid, role);
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(e) => {
            error!("Hook message store failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{}", e)}))).into_response()
        }
    }
}

// ============================================================================
// POST /api/hook/tool
// ============================================================================

pub(crate) async fn hook_tool(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (claude_sid, git_dir) = match validate_and_resolve(&payload, &state) {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };

    let tool_name = payload.tool_name.as_deref().unwrap_or("unknown");
    let tool_args = payload.tool_input
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .unwrap_or_default();
    let result_summary = payload.tool_response.as_deref().unwrap_or("");
    let result_preview = if result_summary.len() > 500 {
        &result_summary[..500]
    } else {
        result_summary
    };
    let tool_use_id = payload.tool_use_id.as_deref().unwrap_or("");

    let (session_name, window_id) = resolve_tmux_context(&git_dir, &state);

    let result = {
        let server = state.state.lock().unwrap();
        let history_id = server.db.find_or_create_hook_history(
            &claude_sid, &session_name, &window_id, &git_dir,
        )?;
        server.db.insert_hook_tool_usage(
            history_id, &claude_sid, tool_name, &tool_args, result_preview, tool_use_id,
        )?;
        Ok::<_, anyhow::Error>(())
    };

    match result {
        Ok(()) => {
            let _ = state.broadcast_tx.send(crate::RealtimeMessage::ToolEvent {
                claude_session_id: claude_sid.clone(),
                git_dir: git_dir.clone(),
                tool_name: tool_name.to_string(),
                tool_use_id: tool_use_id.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
            debug!("Hook tool stored: {} {}", tool_name, claude_sid);
            Json(serde_json::json!({"success": true})).into_response()
        }
        Err(e) => {
            error!("Hook tool store failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{}", e)}))).into_response()
        }
    }
}

// ============================================================================
// POST /api/hook/session
// ============================================================================

pub(crate) async fn hook_session(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<HookPayload>,
) -> impl IntoResponse {
    let (claude_sid, git_dir) = match validate_and_resolve(&payload, &state) {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };

    let event = payload.hook_event_name.as_deref().unwrap_or("");

    match event {
        "SessionStart" => {
            let (session_name, window_id) = resolve_tmux_context(&git_dir, &state);
            // Auto-close any previous open session for this workspace
            {
                let server = state.state.lock().unwrap();
                let _ = server.db.close_hook_session(&claude_sid, "superseded");
            }
            // Create new history entry
            let result = {
                let server = state.state.lock().unwrap();
                server.db.find_or_create_hook_history(
                    &claude_sid, &session_name, &window_id, &git_dir,
                )
            };
            match result {
                Ok(_) => {
                    info!("Hook session started: {} in {}", claude_sid, git_dir);
                    // Broadcast state refresh to trigger UI BUSY
                    let _ = state.broadcast_tx.send(crate::RealtimeMessage::HookSessionUpdate {
                        claude_session_id: claude_sid,
                        git_dir,
                        event: "start".to_string(),
                    });
                    Json(serde_json::json!({"success": true})).into_response()
                }
                Err(e) => {
                    error!("Hook session start failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{}", e)}))).into_response()
                }
            }
        }
        "SessionEnd" => {
            let reason = payload.reason.as_deref().unwrap_or("unknown");
            let result = {
                let server = state.state.lock().unwrap();
                server.db.close_hook_session(&claude_sid, reason)
            };
            match result {
                Ok(()) => {
                    info!("Hook session ended: {} reason={}", claude_sid, reason);
                    let _ = state.broadcast_tx.send(crate::RealtimeMessage::HookSessionUpdate {
                        claude_session_id: claude_sid,
                        git_dir,
                        event: "end".to_string(),
                    });
                    Json(serde_json::json!({"success": true})).into_response()
                }
                Err(e) => {
                    error!("Hook session end failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{}", e)}))).into_response()
                }
            }
        }
        _ => {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "unknown session event"}))).into_response()
        }
    }
}
```

**IMPORTANT NOTES for implementer:**
- The `RealtimeMessage` enum in `main.rs` needs new variants: `ChatMessage`, `ToolEvent`, `HookSessionUpdate`. See Task 3.
- The `validate_and_resolve` function uses `?` on `Result` inside the handler — the actual implementation may need to use explicit match instead of `?` since the error type differs. Adapt as needed to compile.
- The `server.tasks` field accessed in `resolve_tmux_context` — check the actual field name in `ServerState`. It might be `state.tasks` or similar.
- The `config.workspaces` path — verify via `ServerState` struct how workspaces config is accessed.

- [ ] **Step 2: Verify it compiles (will fail until Task 3 wires it)**

This file references `RealtimeMessage` variants and `AppState.git_dir_cache` that don't exist yet. Expected to fail. That's OK.

- [ ] **Step 3: Commit**

```bash
git add src/rust/crates/tracker-server/src/routes_hook.rs
git commit -m "feat: add hook ingest routes (message, tool, session)"
```

---

### Task 3: Wire into AppState, Router, and WebSocket

**Files:**
- Modify: `src/rust/crates/tracker-server/src/main.rs`

- [ ] **Step 1: Add `mod routes_hook;`**

After `mod routes_totp;` (line ~20), add:
```rust
mod routes_hook;
```

- [ ] **Step 2: Add `git_dir_cache` to AppState**

In the `AppState` struct, add after `totp_rate_limiter`:
```rust
    /// Cache: cwd path → git root dir (for hook endpoint)
    git_dir_cache: std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
```

In `AppState::new()`, add to struct literal:
```rust
            git_dir_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
```

- [ ] **Step 3: Add new RealtimeMessage variants**

Find the `RealtimeMessage` enum (search for `enum RealtimeMessage`). Add these variants:

```rust
    ChatMessage {
        claude_session_id: String,
        git_dir: String,
        role: String,
        content: String,
        agent_type: Option<String>,
        timestamp: String,
    },
    ToolEvent {
        claude_session_id: String,
        git_dir: String,
        tool_name: String,
        tool_use_id: String,
        timestamp: String,
    },
    HookSessionUpdate {
        claude_session_id: String,
        git_dir: String,
        event: String,
    },
```

Ensure the WebSocket serialization handles these (check how existing variants are serialized — likely via `#[derive(Serialize, Clone)]`).

- [ ] **Step 4: Add hook routes to middleware whitelist**

In `auth_middleware`, after the TOTP whitelist, add:
```rust
    // Hook ingest endpoints (bearer token auth, not JWT)
    if path.starts_with("/api/hook/") {
        return next.run(req).await;
    }
```

Note: The hook endpoints do their own bearer token auth internally (checking `Authorization` header against `state.auth_token`). OR you can let the existing middleware handle it since it already accepts bearer tokens. **Choose one approach** — if the existing middleware already validates bearer tokens, no whitelist change is needed.

- [ ] **Step 5: Register hook routes**

After the TOTP routes, add:
```rust
        // Hook ingest routes (Claude Code hooks)
        .route("/api/hook/message", post(routes_hook::hook_message))
        .route("/api/hook/tool", post(routes_hook::hook_tool))
        .route("/api/hook/session", post(routes_hook::hook_session))
```

- [ ] **Step 6: Add staleness timer**

Find where periodic tasks are set up (likely a `tokio::spawn` with `interval`). Add a 60-second timer to auto-close stale sessions:

```rust
    // Auto-close stale hook sessions (every 60s)
    {
        let state_clone = app_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let count = {
                    let server = state_clone.state.lock().unwrap();
                    server.db.close_stale_hook_sessions(10).unwrap_or(0)
                };
                if count > 0 {
                    tracing::info!("Auto-closed {} stale hook sessions", count);
                }
            }
        });
    }
```

- [ ] **Step 7: Build and verify**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo build --release`
MUST compile. Fix any type errors.

- [ ] **Step 8: Commit**

```bash
git add src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: wire hook routes into AppState, router, and WebSocket"
```

---

### Task 4: Create Hook Script + Install

**Files:**
- Create: `scripts/agent-hook.sh`

- [ ] **Step 1: Create agent-hook.sh**

```bash
#!/bin/bash
# agent-hook.sh — Unified Claude Code hook forwarder
# Routes hook events to tracker-server ingest endpoints.
# Reads JSON from stdin, uses jq for parsing.

INPUT=$(cat)
EVENT=$(echo "$INPUT" | jq -r '.hook_event_name // ""' 2>/dev/null)

# Load env (token, URL)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$SCRIPT_DIR/env.sh" ] && source "$SCRIPT_DIR/env.sh"

TOKEN="${TRACKER_TOKEN:-}"
URL="${TRACKER_URL:-http://127.0.0.1:3099}"

[ -z "$EVENT" ] && exit 0
[ -z "$TOKEN" ] && exit 0

case "$EVENT" in
  UserPromptSubmit|Stop|SubagentStop)  EP="/api/hook/message" ;;
  PostToolUse)                          EP="/api/hook/tool" ;;
  SessionStart|SessionEnd)              EP="/api/hook/session" ;;
  *)                                    exit 0 ;;
esac

# Fire and forget (async, max 3s timeout)
curl -sf -X POST "$URL$EP" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d "$INPUT" \
  --max-time 3 >/dev/null 2>&1 &

exit 0
```

- [ ] **Step 2: Make executable**

```bash
chmod +x scripts/agent-hook.sh
```

- [ ] **Step 3: Test the script locally**

```bash
echo '{"hook_event_name":"SessionStart","session_id":"test123","cwd":"/Volumes/program/project-code/repos/ai-tracker","source":"startup"}' | bash scripts/agent-hook.sh
```

Check server logs for the request.

- [ ] **Step 4: Copy to runtime location**

```bash
cp scripts/agent-hook.sh ~/.config/agent-tracker/scripts/agent-hook.sh
chmod +x ~/.config/agent-tracker/scripts/agent-hook.sh
```

- [ ] **Step 5: Commit**

```bash
git add scripts/agent-hook.sh
git commit -m "feat: add unified hook script for Claude Code integration"
```

---

### Task 5: Configure Claude Code settings.json

**Files:**
- Modify: `~/.claude/settings.json` (user's global Claude Code config)

- [ ] **Step 1: Add hook entries to settings.json**

Read `~/.claude/settings.json`, then add to the `"hooks"` section (create if not exists):

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "~/.config/agent-tracker/scripts/agent-hook.sh" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "~/.config/agent-tracker/scripts/agent-hook.sh" }] }],
    "SubagentStop": [{ "hooks": [{ "type": "command", "command": "~/.config/agent-tracker/scripts/agent-hook.sh" }] }],
    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "~/.config/agent-tracker/scripts/agent-hook.sh" }] }],
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "~/.config/agent-tracker/scripts/agent-hook.sh" }] }],
    "SessionEnd": [{ "hooks": [{ "type": "command", "command": "~/.config/agent-tracker/scripts/agent-hook.sh" }] }]
  }
}
```

**IMPORTANT:** Merge with existing hooks — do NOT overwrite existing entries (like the current `UserPromptSubmit` hook for discord-notify). Add the new hook entries alongside existing ones.

- [ ] **Step 2: Verify hooks fire**

Open a new Claude Code session in a registered project. Check server logs for incoming hook requests.

---

### Task 6: Frontend — WebSocket Event Handling

**Files:**
- Modify: `web/src/services/state.ts`

- [ ] **Step 1: Add new event types to WebSocket handler**

In `state.ts`, find the `ws.onmessage` handler where `RealtimeMessage` types are parsed. Add handling for the new event types:

```typescript
    // In the message handler switch/if block:
    if (msg.type === 'chat_message') {
      callbacks.onChatMessage?.(msg);
    } else if (msg.type === 'tool_event') {
      callbacks.onToolEvent?.(msg);
    } else if (msg.type === 'hook_session_update') {
      callbacks.onHookSessionUpdate?.(msg);
    }
```

Add to the `WebSocketCallbacks` interface:
```typescript
  onChatMessage?: (msg: any) => void;
  onToolEvent?: (msg: any) => void;
  onHookSessionUpdate?: (msg: any) => void;
```

- [ ] **Step 2: Commit**

```bash
git add web/src/services/state.ts
git commit -m "feat: handle new WebSocket event types for hook tracking"
```

---

### Task 7: Frontend — LIVE_CHAT Real-time Messages

**Files:**
- Modify: `web/src/components/ChatHistoryModal.tsx`
- Modify: `web/src/App.tsx`

- [ ] **Step 1: Pass WebSocket chat messages to ChatHistoryModal**

In `App.tsx`, when setting up WebSocket callbacks, add `onChatMessage` handler that stores messages in state and passes them down to `ChatHistoryModal`.

- [ ] **Step 2: In ChatHistoryModal, append real-time messages**

When the component receives new `chat_message` events via props, append them to the message list if they match the currently viewed session.

- [ ] **Step 3: Handle session status updates**

When `hook_session_update` with `event: "start"` arrives, find the matching workspace window and set status to BUSY. On `event: "end"`, set to IDLE.

- [ ] **Step 4: Build and verify**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/web && npm run build`

- [ ] **Step 5: Commit**

```bash
git add web/src/components/ChatHistoryModal.tsx web/src/App.tsx
git commit -m "feat: real-time chat messages and session status from hooks"
```

---

### Task 8: Build, Deploy, and Test

- [ ] **Step 1: Build backend**

```bash
cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo build --release
```

- [ ] **Step 2: Build frontend**

```bash
cd /Volumes/program/project-code/repos/ai-tracker/web && npm run build
```

- [ ] **Step 3: Deploy**

```bash
./scripts/deploy.sh --tauri
```

- [ ] **Step 4: Manual test — hook message flow**

1. Open a new Claude Code session in the ai-tracker project
2. Send a message to Claude
3. Check tracker Web UI — the message should appear in LIVE_CHAT in real-time
4. Check DB: `sqlite3 ~/.config/agent-tracker/data/tracker.db "SELECT * FROM conversation_messages WHERE source='hook' ORDER BY id DESC LIMIT 5"`

- [ ] **Step 5: Manual test — tool tracking**

1. Ask Claude to read a file
2. Check DB: `sqlite3 ~/.config/agent-tracker/data/tracker.db "SELECT tool_name, tool_use_id FROM tool_usage WHERE claude_session_id != '' ORDER BY id DESC LIMIT 5"`

- [ ] **Step 6: Manual test — session lifecycle**

1. Open Claude Code → check status shows BUSY
2. Exit Claude Code → check status shows IDLE
3. Wait 10 minutes with no activity → check auto-close

- [ ] **Step 7: Run health check**

```bash
./scripts/deploy.sh --check
```
