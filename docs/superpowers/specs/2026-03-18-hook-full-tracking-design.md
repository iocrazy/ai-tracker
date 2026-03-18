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

### Hook Script: `agent-hook.sh`

Single unified script for all 6 hooks. Reads JSON from stdin, routes to the correct endpoint.

```bash
#!/bin/bash
INPUT=$(cat)
EVENT=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('hook_event_name',''))" 2>/dev/null)
TOKEN="${TRACKER_TOKEN:-}"
URL="${TRACKER_URL:-http://127.0.0.1:3099}"

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

All 3 endpoints are public (no JWT), authenticated via bearer token (same `auth_token` as existing hooks).

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
2. Resolve `cwd` to git_dir (run `git -C <cwd> rev-parse --show-toplevel`)
3. Match git_dir to registered workspace → discard if unmatched
4. Determine role: `UserPromptSubmit` → "user", `Stop`/`SubagentStop` → "assistant"
5. Extract content: `prompt` for user, `last_assistant_message` for assistant
6. Find or create active history_id for this session_id + window context
7. INSERT into `conversation_messages` (full content, no truncation)
8. WebSocket broadcast: `{ type: "chat_message", session_id, role, content, timestamp }`

**Response:** `{ "success": true }`

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
2. Serialize `tool_input` to JSON string
3. Truncate `tool_response` to 500 characters for `result_summary`
4. INSERT into `tool_usage` table
5. WebSocket broadcast: `{ type: "tool_event", session_id, tool_name, tool_input_preview, success, timestamp }`

**Response:** `{ "success": true }`

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
2. SessionStart:
   - Create or update task entry with `status: in_progress`
   - Store `model`, `transcript_path` metadata
   - WebSocket broadcast state update (triggers UI status → BUSY)
3. SessionEnd:
   - Update task entry with `status: completed`
   - Store `reason` in completion_note
   - WebSocket broadcast state update (triggers UI status → IDLE)

**Response:** `{ "success": true }`

## Backend: Database Changes

### Extend `conversation_messages` table

Migration 103:
```sql
ALTER TABLE conversation_messages ADD COLUMN session_id TEXT DEFAULT '';
ALTER TABLE conversation_messages ADD COLUMN source TEXT DEFAULT 'hook';
CREATE INDEX idx_conv_messages_session ON conversation_messages(session_id);
```

- `session_id`: Claude Code session ID (for correlating messages within a session)
- `source`: 'hook' (from this system) or 'jsonl_parse' (from legacy JSONL parsing)

### Extend `tool_usage` table (if needed)

Check existing schema. May need:
```sql
ALTER TABLE tool_usage ADD COLUMN session_id TEXT DEFAULT '';
ALTER TABLE tool_usage ADD COLUMN tool_use_id TEXT DEFAULT '';
```

## Backend: Git Dir Resolution

Hook data includes `cwd` (working directory) but not `git_dir`. The backend needs to resolve:

1. Cache: maintain in-memory `HashMap<PathBuf, Option<String>>` mapping cwd → git_dir
2. On miss: shell out to `git -C <cwd> rev-parse --show-toplevel`
3. Match against registered workspaces in `agent-config.json`
4. Cache result (both hit and miss) for performance

## Frontend Changes

### LIVE_CHAT Enhancement

Current: Parses JSONL files to show conversation history.
New: Also listens for WebSocket `chat_message` events for real-time messages.

- When a `chat_message` arrives, append it to the conversation list
- New messages from hooks appear instantly (no polling delay)
- Legacy JSONL-based messages still work as fallback

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
