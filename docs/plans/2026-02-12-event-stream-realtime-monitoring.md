# Event Stream Real-Time Monitoring Design

## Problem

Current monitoring has four critical issues:

1. **Can't see what agent is doing** — Web UI only shows "in_progress", no tool-level detail
2. **Task status inaccurate** — hook failures cause stale states; no health check
3. **Data inconsistent** — Timeline ALL vs project view show different data
4. **Interaction inconvenient** — limited real-time interaction from Web UI

Root cause: only 4 hook events are captured (start/finish/pause/idle), missing tool-level granularity. Single `curl` delivery with no retry means events get lost.

## Solution: Full Hook Event Stream

Transform the hook system from lifecycle-only tracking to comprehensive event sourcing.

### Architecture

```
Claude Code (per-window agent)
  │
  ├─ UserPromptSubmit  ──→ agent-event.sh start
  ├─ PreToolUse        ──→ agent-event.sh tool_start    [NEW]
  ├─ PostToolUse       ──→ agent-event.sh tool_end      [NEW]
  ├─ PermissionRequest ──→ agent-event.sh pause
  ├─ Notification      ──→ agent-event.sh pause
  └─ Stop              ──→ agent-event.sh finish
                              │
                              ├─→ curl POST (primary, fast path)
                              │     ↓
                              │   tracker-server /api/command ──→ update state + broadcast WS
                              │
                              └─→ SQLite event_queue (fallback on curl failure)
                                    ↓
                                  server drain loop (2s interval) ──→ process + broadcast WS
```

---

## Phase 1: Hook Event Expansion

### 1a. Add PreToolUse hook to settings.json

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "type": "command",
        "command": "INPUT=$(cat); tool_name=$(echo \"$INPUT\" | jq -r '.tool_name // \"unknown\"'); tool_input=$(echo \"$INPUT\" | jq -r '.tool_input // {}' | jq -c '.' | cut -c1-200); \"$HOME/.config/agent-tracker/scripts/agent-event.sh\" tool_start --agent claude --tool \"$tool_name\" --tool-input \"$tool_input\"",
        "async": true
      }
    ]
  }
}
```

Data available from PreToolUse stdin (JSON):
- `tool_name` — "Read", "Edit", "Bash", "Task", "Write", etc.
- `tool_input` — full tool parameters (file paths, commands, etc.)

### 1b. Add PostToolUse hook to settings.json

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "type": "command",
        "command": "INPUT=$(cat); tool_name=$(echo \"$INPUT\" | jq -r '.tool_name // \"unknown\"'); \"$HOME/.config/agent-tracker/scripts/agent-event.sh\" tool_end --agent claude --tool \"$tool_name\"",
        "async": true
      }
    ]
  }
}
```

Data available from PostToolUse stdin (JSON):
- `tool_name` — same as PreToolUse
- `tool_output` — tool result (can be large, we don't send this)

### 1c. Enrich existing Stop hook

Current Stop hook extracts last assistant message from transcript. Add:
- Token count, cost (if available in stop event data)
- Session duration

---

## Phase 2: Agent-Event.sh Overhaul

### 2a. New actions: tool_start, tool_end

Add to `agent-event.sh`:

```bash
# New argument parsing
TOOL_NAME=""
TOOL_INPUT=""

# In argument parser:
--tool) TOOL_NAME="$2"; shift 2 ;;
--tool-input) TOOL_INPUT="$2"; shift 2 ;;

# New command mappings:
case "$ACTION" in
    start)      COMMAND="start_task" ;;
    finish)     COMMAND="finish_task" ;;
    pause)      COMMAND="pause_task" ;;
    tool_start) COMMAND="tool_start" ;;   # NEW
    tool_end)   COMMAND="tool_end" ;;     # NEW
esac
```

Payload includes new fields:
```json
{
  "command": "tool_start",
  "session_id": "$0",
  "window_id": "@1",
  "pane": "%3",
  "tool_name": "Bash",
  "tool_input": "cargo build --release"
}
```

### 2b. Reliable delivery with SQLite queue

Replace the single `curl` call with a two-tier delivery: try HTTP first, fall back to SQLite queue.

**Server-side: new `event_queue` table in global DB**

```sql
CREATE TABLE IF NOT EXISTS event_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL,
    processed INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_event_queue_pending
    ON event_queue(processed, created_at);
```

**Server-side: queue drain loop** (in main.rs background task)

```rust
// Every 2 seconds, process pending events from the queue
async fn drain_event_queue(app_state: Arc<AppState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let events = {
            let state = app_state.state.lock().unwrap();
            state.db.get_pending_events(50) // batch of 50
        };
        for event in events {
            if let Ok(req) = serde_json::from_str::<SendCommandRequest>(&event.payload) {
                let _ = handle_command(&app_state, req).await;
                let state = app_state.state.lock().unwrap();
                let _ = state.db.mark_event_processed(event.id);
            }
        }
    }
}
```

**Shell-side: agent-event.sh delivery**

```bash
QUEUE_DB="$HOME/.config/agent-tracker/data/tracker.db"

send_event() {
    local payload="$1"

    # Try direct HTTP delivery first (fast path)
    local response
    response=$(curl -s -m 3 -X POST "$TRACKER_URL/api/command" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>&1)

    if [[ $? -eq 0 ]] && echo "$response" | jq -e '.success' &>/dev/null; then
        return 0
    fi

    # Fallback: write to SQLite queue (guaranteed delivery)
    sqlite3 "$QUEUE_DB" "INSERT INTO event_queue (payload, created_at) VALUES ('$(echo "$payload" | sed "s/'/''/g")', datetime('now'))" 2>/dev/null
    log "Queued event to SQLite (HTTP delivery failed)"
}
```

Benefits over JSONL file queue:
- **ACID transactions** — no partial writes or corruption on crash
- **No drain logic in shell** — server's background task handles queue consumption
- **Single source of truth** — queue lives in the same DB the server already manages
- **Concurrent safe** — SQLite WAL handles shell writes + server reads without locking issues
- **Queryable** — can inspect/debug queue with standard SQL

### 2c. Rate limiting for high-frequency events

PreToolUse/PostToolUse can fire many times per second (e.g., multiple Read calls in parallel). Add throttling:

```bash
# Only send tool_start if last tool_start was > 100ms ago
LAST_TOOL_FILE="$QUEUE_DIR/.last_tool_ts"
if [[ "$ACTION" == "tool_start" ]]; then
    now=$(date +%s%N)
    last=$(cat "$LAST_TOOL_FILE" 2>/dev/null || echo 0)
    diff=$(( (now - last) / 1000000 ))  # ms
    if [[ $diff -lt 100 ]]; then
        log "Throttled tool_start (${diff}ms since last)"
        exit 0
    fi
    echo "$now" > "$LAST_TOOL_FILE"
fi
```

---

## Phase 3: Server-Side Event Processing

### 3a. Extend SendCommandRequest

**File**: `src/rust/crates/tracker-server/src/main.rs`

Add fields to `SendCommandRequest`:
```rust
#[derive(Deserialize)]
struct SendCommandRequest {
    // ... existing fields ...
    #[serde(default)]
    tool_name: String,
    #[serde(default)]
    tool_input: String,
}
```

### 3b. Add command constants

**File**: `src/rust/crates/tracker-core/src/ipc.rs`

```rust
pub const TOOL_START: &str = "tool_start";
pub const TOOL_END: &str = "tool_end";
```

### 3c. Extend Task struct with tool state

**File**: `src/rust/crates/tracker-core/src/lib.rs`

Add to `Task`:
```rust
pub struct Task {
    // ... existing fields ...
    pub current_tool: String,        // "" when thinking, "Bash" when executing tool
    pub current_tool_input: String,  // truncated tool args for display
    pub tool_started_at: Option<DateTime<Utc>>,
    pub tool_count: i32,             // number of tool calls in this task
    pub last_event_at: Option<DateTime<Utc>>,  // for health check
}
```

### 3d. Handle new commands in main.rs

```rust
commands::TOOL_START => {
    let key = format!("{}|{}|{}", req.session_id, req.window_id, req.pane);
    let mut state = app_state.state.lock().unwrap();

    if let Some(task) = state.tasks.get_mut(&key) {
        task.current_tool = req.tool_name.clone();
        task.current_tool_input = req.tool_input.chars().take(200).collect();
        task.tool_started_at = Some(Utc::now());
        task.tool_count += 1;
        task.last_event_at = Some(Utc::now());
        task.status = TaskStatus::InProgress;  // ensure status is correct

        let task_clone = task.clone();
        let _ = state.db.save_task(&task_clone);
    }
    drop(state);
    app_state.broadcast_state();
}

commands::TOOL_END => {
    let key = format!("{}|{}|{}", req.session_id, req.window_id, req.pane);
    let mut state = app_state.state.lock().unwrap();

    if let Some(task) = state.tasks.get_mut(&key) {
        task.current_tool = String::new();
        task.current_tool_input = String::new();
        task.tool_started_at = None;
        task.last_event_at = Some(Utc::now());

        let task_clone = task.clone();
        let _ = state.db.save_task(&task_clone);
    }
    drop(state);
    app_state.broadcast_state();
}
```

### 3e. Health check — detect stale tasks

In the existing 1-second poll loop, add staleness detection:

```rust
// In poll_tmux_state or a separate check:
let now = Utc::now();
let stale_threshold = chrono::Duration::minutes(5);

let state = self.state.lock().unwrap();
for (key, task) in &state.tasks {
    if let Some(last_event) = task.last_event_at {
        if now - last_event > stale_threshold {
            // Task hasn't received any event in 5 minutes
            // Check if the tmux window still exists
            // If window gone: auto-finish the task
            // If window exists but no events: mark as "stale" for UI warning
        }
    }
}
```

---

## Phase 4: Frontend — Real-Time Tool Display

### 4a. Extend WebSocket state

The `Task` object in WebSocket payload now includes:
- `current_tool` — tool being executed ("Bash", "Read", "Edit", "")
- `current_tool_input` — truncated args ("cargo build --release")
- `tool_count` — total tool calls in session
- `last_event_at` — for freshness indicator

### 4b. WorkstationsView enhancement

Each task card shows:
- **Thinking**: pulsing dot + "Thinking..." when `current_tool` is empty and status is in_progress
- **Executing tool**: tool icon + "Bash: cargo build..." when `current_tool` is set
- **Awaiting input**: pause icon + "Waiting for input"
- **Stale**: warning icon if `last_event_at` > 5 minutes ago

### 4c. Tool activity timeline (optional)

Small inline timeline showing recent tool calls:
```
Read → Edit → Bash → Read → [Thinking...]
```

---

## Phase 5: Data Consistency Fix

### 5a. ALL mode aggregation

Timeline ALL tab should merge:
1. Project DB history (from all registered projects)
2. JSONL session index (for sessions without a project)

Implementation:
- New endpoint `GET /api/history/all` that:
  - Iterates all registered projects via `list_projects()`
  - For each project, queries `load_history_paginated()` from project DB
  - Merges with JSONL session index (excluding entries that have a project match)
  - Sorts by `started_at DESC`
  - Paginates the merged result

### 5b. Project count accuracy

Update project counts not just on write operations, but also:
- On server startup (scan all project DBs)
- Periodically (every 5 minutes) to catch any drift

---

## Files to Modify

| File | Changes |
|------|---------|
| `~/.claude/settings.json` | Add PreToolUse + PostToolUse hooks |
| `~/.config/agent-tracker/scripts/agent-event.sh` | Add tool_start/tool_end actions, queue logic, throttling |
| `src/rust/crates/tracker-core/src/ipc.rs` | Add TOOL_START, TOOL_END constants |
| `src/rust/crates/tracker-core/src/lib.rs` | Add current_tool, tool_count, last_event_at to Task |
| `src/rust/crates/tracker-server/src/main.rs` | Add tool_start/tool_end handlers, health check, ALL aggregation endpoint |
| `src/rust/crates/tracker-server/src/db.rs` | Add current_tool columns to tasks table, event_queue table + CRUD, migration |
| `web/src/components/WorkstationsView.tsx` | Display tool status, freshness indicator |
| `web/src/components/TimelineView.tsx` | ALL mode aggregation |

## Implementation Order

1. **Phase 1+2**: Hook expansion + agent-event.sh overhaul (shell side)
2. **Phase 3a-3d**: Server-side tool event handling (Rust)
3. **Phase 4a-4b**: Frontend tool display (React)
4. **Phase 3e**: Health check (Rust)
5. **Phase 5**: Data consistency (Rust + React)
6. **Phase 4c**: Tool activity timeline (React, optional)

## Verification

1. Open Claude Code in tmux, ask it to read a file → Web UI shows "Read: /path/to/file"
2. Ask Claude to run a bash command → Web UI shows "Bash: npm run build"
3. Kill curl (simulate network failure) → events queue locally → next event drains queue
4. Kill Claude Code process → health check detects stale task within 5 minutes
5. Timeline ALL tab shows merged project + JSONL data
6. Rapid tool calls (10+ per second) → throttled to reasonable update rate
