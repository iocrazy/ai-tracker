# Claude Code Hook Full-Chain Tracking Design

**Date**: 2026-03-18
**Status**: Approved

## Overview

Integrate 6 Claude Code hooks to enable real-time conversation capture, tool usage tracking, and session lifecycle management. All data flows through tracker-server HTTP API → SQLite DB → WebSocket broadcast to UI.

## Goals

1. **Solve empty history records** — Store every user prompt and Claude response in DB via hooks (no JSONL dependency)
2. **Tool usage tracking** — Record tool calls with name, input, and response preview (500 chars)
3. **Session lifecycle** — Accurate BUSY/IDLE/OFFLINE status via SessionStart/SessionEnd hooks
4. **Real-time UI** — WebSocket broadcast enables live conversation display and tool activity feed
5. **Subagent tracking** — Capture subagent completion and responses

## Scope

- Only registered workspaces receive data (unregistered project requests are silently discarded)
- Hook configuration is global (`~/.claude/settings.json`)
- Conversation messages stored in full; tool responses stored as 500-char preview

## Architecture

```
Claude Code Session
  ├─ UserPromptSubmit → POST /api/hook/message
  ├─ Stop             → POST /api/hook/message
  ├─ PostToolUse      → POST /api/hook/tool
  ├─ SessionStart     → POST /api/hook/session
  ├─ SessionEnd       → POST /api/hook/session
  └─ SubagentStop     → POST /api/hook/message
        ↓
  tracker-server
    1. Auth: verify bearer token
    2. Match: resolve cwd → git_dir → registered workspace (discard if unmatched)
    3. Store: INSERT into appropriate table
    4. Broadcast: WebSocket push to connected clients
        ↓
  Web UI
    - LIVE_CHAT: real-time bidirectional conversation
    - Timeline: tool usage events
    - Statistics: tool frequency, daily volume
    - Status: auto-sync from session lifecycle
```

## Hooks Configuration

6 hooks in `~/.claude/settings.json`:

| Hook | Event Data (stdin JSON) | Endpoint |
|------|------------------------|----------|
| **UserPromptSubmit** | `{ session_id, transcript_path, cwd, prompt }` | `/api/hook/message` |
| **Stop** | `{ session_id, transcript_path, cwd, last_assistant_message }` | `/api/hook/message` |
| **SubagentStop** | `{ session_id, transcript_path, cwd, last_assistant_message, agent_type }` | `/api/hook/message` |
| **PostToolUse** | `{ session_id, transcript_path, cwd, tool_name, tool_input, tool_response, tool_use_id }` | `/api/hook/tool` |
| **SessionStart** | `{ session_id, transcript_path, cwd, source, model }` | `/api/hook/session` |
| **SessionEnd** | `{ session_id, transcript_path, cwd, reason }` | `/api/hook/session` |

All hooks share common fields: `session_id`, `transcript_path`, `cwd`, `hook_event_name`.

**Important naming**: `session_id` from Claude Code hooks is the **Claude session ID** (e.g., "abc123"), NOT the tmux session name. Throughout this spec, this is referred to as `claude_session_id` in DB columns to avoid collision with the existing tmux-based `session_id` used in the `history` and `tasks` tables.

### Hook Script: `agent-hook.sh`

Single unified script for all 6 hooks. Reads JSON from stdin, routes to the correct endpoint. Uses `jq` for fast JSON parsing (no python3 dependency).

```bash
#!/bin/bash
INPUT=$(cat)
EVENT=$(echo "$INPUT" | jq -r '.hook_event_name // ""')
TOKEN="${TRACKER_TOKEN:-}"
URL="${TRACKER_URL:-http://127.0.0.1:3099}"

[ -z "$EVENT" ] && exit 0

case "$EVENT" in
  UserPromptSubmit|Stop|SubagentStop)  EP="/api/hook/message" ;;
  PostToolUse)                          EP="/api/hook/tool" ;;
  SessionStart|SessionEnd)              EP="/api/hook/session" ;;
  *)                                    exit 0 ;;
esac

curl -sf -X POST "$URL$EP" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d "$INPUT" \
  --max-time 3 >/dev/null 2>&1 &
```

### settings.json Configuration

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

## Backend: New API Endpoints

All 3 endpoints authenticated via bearer token (same `auth_token` as existing hooks). No JWT required.

### Error Response Contract

All endpoints return:
- `200 { "success": true }` — stored successfully
- `200 { "success": true, "skipped": true }` — unmatched workspace, silently discarded
- `401 { "error": "Unauthorized" }` — invalid or missing bearer token
- `400 { "error": "<details>" }` — malformed JSON or missing required fields
- `500 { "error": "<details>" }` — internal server error

### `POST /api/hook/message`

Handles: UserPromptSubmit, Stop, SubagentStop

**Request body** (JSON from hook stdin):
```json
{
  "hook_event_name": "Stop",
  "session_id": "abc123",
  "cwd": "/path/to/project",
  "transcript_path": "/path/to/transcript.jsonl",
  "last_assistant_message": "Here is the implementation...",
  "prompt": "implement feature X",
  "agent_type": "Explore"
}
```

**Processing:**
1. Auth: verify bearer token
2. Resolve `cwd` to git_dir (cached, see Git Dir Resolution)
3. Match git_dir to registered workspace → return `skipped` if unmatched
4. Determine role: `UserPromptSubmit` → "user", `Stop`/`SubagentStop` → "assistant"
5. Extract content: `prompt` for user, `last_assistant_message` for assistant
6. Resolve history_id (see History ID Resolution below)
7. INSERT into `conversation_messages` (full content, no truncation)
8. WebSocket broadcast: `{ type: "chat_message", session_id, role, content, timestamp }`

### `POST /api/hook/tool`

Handles: PostToolUse

**Request body:**
```json
{
  "hook_event_name": "PostToolUse",
  "session_id": "abc123",
  "cwd": "/path/to/project",
  "tool_name": "Bash",
  "tool_input": { "command": "cargo build" },
  "tool_response": "... full output ...",
  "tool_use_id": "toolu_xxx"
}
```

**Processing:**
1. Auth + workspace match (same as message)
2. Serialize `tool_input` to JSON string → store in existing `tool_args` column
3. Truncate `tool_response` to 500 characters → store in `result_summary`
4. `success` defaults to `1` (PostToolUse only fires after successful execution; PostToolUseFailure is a separate event we don't handle)
5. Deduplicate: `INSERT OR IGNORE` using `tool_use_id` UNIQUE constraint
6. INSERT into `tool_usage` table
7. WebSocket broadcast: `{ type: "tool_event", session_id, tool_name, tool_input_preview, timestamp }`

### `POST /api/hook/session`

Handles: SessionStart, SessionEnd

**Request body:**
```json
{
  "hook_event_name": "SessionStart",
  "session_id": "abc123",
  "cwd": "/path/to/project",
  "source": "startup",
  "model": "claude-opus-4-6"
}
```

**Processing:**
1. Auth + workspace match
2. **SessionStart:**
   - Resolve tmux context: find which tmux session:window is running in this cwd (use existing tmux window → git_dir mapping)
   - Create history row: `INSERT INTO history` with `claude_session_id`, tmux session/window, `status: in_progress`, summary "Claude session started"
   - If a previous session for the same workspace is still `in_progress`, auto-close it (set `completed`)
   - Store `model`, `transcript_path` in history metadata
   - WebSocket broadcast state update (triggers UI status → BUSY)
3. **SessionEnd:**
   - Find history row by `claude_session_id`
   - Update: `status: completed`, `reason` in completion_note
   - WebSocket broadcast state update (triggers UI status → IDLE)

**Staleness handling:** If no hook event arrives for a `claude_session_id` within 10 minutes, the server auto-transitions its history row to `completed` (status → IDLE). Checked via periodic background task (every 60s).

## History ID Resolution

The core challenge: hooks provide a `claude_session_id` but conversation_messages requires a `history_id` (linked to tmux session/window).

**Strategy:**

1. On **SessionStart**: create a `history` row with:
   - `claude_session_id` (new column) = Claude's session_id
   - `session_id` = tmux session name (resolved from cwd → git_dir → workspace → active tmux mapping)
   - `window_id` = tmux window (resolved similarly)
   - `summary` = "Claude session {claude_session_id}" (updated later as conversation progresses)
   - Return the new `history.id`

2. On **message/tool hooks**: look up `history.id` by `claude_session_id`:
   ```sql
   SELECT id FROM history WHERE claude_session_id = ? ORDER BY id DESC LIMIT 1
   ```
   If not found (SessionStart hook didn't fire or was lost), auto-create a history row with available context.

3. **Concurrency safety**: Use `INSERT OR IGNORE` with UNIQUE constraint on `claude_session_id` for history creation. The lookup + insert is wrapped in a transaction.

## Backend: Database Changes

### Migration 103: Add claude_session_id to history
```sql
ALTER TABLE history ADD COLUMN claude_session_id TEXT DEFAULT '';
CREATE INDEX idx_history_claude_session ON history(claude_session_id);
```

### Migration 104: Extend conversation_messages
```sql
ALTER TABLE conversation_messages ADD COLUMN claude_session_id TEXT DEFAULT '';
ALTER TABLE conversation_messages ADD COLUMN source TEXT DEFAULT 'hook';
```

### Migration 105: Extend tool_usage
```sql
ALTER TABLE tool_usage ADD COLUMN claude_session_id TEXT DEFAULT '';
ALTER TABLE tool_usage ADD COLUMN tool_use_id TEXT DEFAULT '';
```

### Migration 106: Add unique constraint for tool dedup
```sql
CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_usage_use_id ON tool_usage(tool_use_id) WHERE tool_use_id != '';
```

- `claude_session_id`: Claude Code session ID (distinct from tmux-based `session_id`)
- `source`: 'hook' (from this system) or 'jsonl_parse' (from legacy JSONL parsing)
- `tool_use_id`: Claude's tool use ID for deduplication

## Backend: Git Dir Resolution

Hook data includes `cwd` (working directory) but not `git_dir`. The backend needs to resolve:

1. **Startup pre-population**: Load all registered workspaces' `base_path` values into cache (eliminates cold-start misses for known projects)
2. **Runtime cache**: In-memory `HashMap<PathBuf, Option<String>>` mapping cwd → git_dir
3. **On cache miss**: Shell out to `git -C <cwd> rev-parse --show-toplevel` (async, with 3s timeout)
4. **Match**: Compare resolved git_dir against registered workspaces
5. **Cache both hit and miss** results for performance

## WebSocket Event Schema

### `chat_message`
```json
{
  "type": "chat_message",
  "claude_session_id": "abc123",
  "git_dir": "/path/to/project",
  "role": "user" | "assistant",
  "content": "full message text",
  "agent_type": "Explore" | null,
  "timestamp": "2026-03-18T09:00:00Z"
}
```

### `tool_event`
```json
{
  "type": "tool_event",
  "claude_session_id": "abc123",
  "git_dir": "/path/to/project",
  "tool_name": "Bash",
  "tool_input_preview": "{\"command\":\"cargo build\"}",
  "result_preview": "Compiling tracker-server...",
  "tool_use_id": "toolu_xxx",
  "timestamp": "2026-03-18T09:00:01Z"
}
```

## Frontend Changes

### LIVE_CHAT Enhancement

Current: Parses JSONL files to show conversation history.
New: Also listens for WebSocket `chat_message` events for real-time messages.

- When a `chat_message` arrives, append it to the conversation list
- New messages from hooks appear instantly (no polling delay)
- Legacy JSONL-based messages still work as fallback
- Filter by `git_dir` to show only messages for the currently viewed project

### Tool Activity Feed

New WebSocket `tool_event` messages displayed in:
- Timeline "工具" tab: real-time tool usage entries
- Workstation view: current tool indicator already exists (claudeStatus), now backed by actual data

### Statistics Enhancement

New data available for stats:
- Tool usage frequency (which tools used most)
- Daily message volume (user vs assistant)
- Active hours heatmap

### Status Auto-Sync

- `SessionStart` event → window status automatically BUSY (replaces 5s polling hack)
- `SessionEnd` event → window status automatically IDLE
- 10-minute staleness timeout → auto-IDLE if no hook events
- More accurate than current task-based status + Claude polling

## Backward Compatibility

- Existing `agent-event.sh` hook continues to work (task status reporting)
- New `agent-hook.sh` is additive (does not replace agent-event.sh)
- JSONL-based history loading remains as fallback for sessions without hook data
- `source` field distinguishes hook-captured vs JSONL-parsed messages

## Non-Goals

- Capturing data from unregistered projects
- Replacing the existing agent-event.sh hook system
- Real-time streaming of tool output (only post-completion capture)
- PreToolUse / PostCompact hooks (deferred to future iterations)
- Rate limiting on hook endpoints (auth token is sufficient for single-user system)
