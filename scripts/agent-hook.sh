#!/bin/bash
# agent-hook.sh — Unified Claude Code hook handler
# Combines task status management (old /api/hook) and conversation tracking (new /api/hook/*).
# Reads JSON from stdin, routes to both endpoints.
#
# Configured in ~/.claude/settings.json for:
#   UserPromptSubmit, Stop, SubagentStop, PostToolUse, SessionStart, SessionEnd,
#   PermissionRequest, Notification

INPUT=$(cat)
EVENT=$(echo "$INPUT" | jq -r '.hook_event_name // ""' 2>/dev/null)

[ -z "$EVENT" ] && exit 0

# Always read token fresh from config (ignore stale env vars)
TRACKER_TOKEN=""
for cfg in \
  "$HOME/Library/Application Support/com.agent-tracker.menubar/agent-config.json" \
  "$HOME/.config/agent-tracker/agent-config.json"; do
  if [ -f "$cfg" ]; then
    TRACKER_TOKEN=$(jq -r '.auth.token // ""' "$cfg" 2>/dev/null)
    [ -n "$TRACKER_TOKEN" ] && break
  fi
done

# URL from env or default
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$SCRIPT_DIR/env.sh" ] && source "$SCRIPT_DIR/env.sh"

TOKEN="${TRACKER_TOKEN:-}"
URL="${TRACKER_URL:-http://127.0.0.1:3099}"

[ -z "$TOKEN" ] && exit 0

AUTH=(-H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json")

# --- 1. Legacy task status endpoint (/api/hook) ---
# Handles: UserPromptSubmit, Stop, PermissionRequest, Notification
# Sends X-Hook-Event header for the old handler
case "$EVENT" in
  UserPromptSubmit|Stop|PermissionRequest|Notification)
    curl -sf -X POST "$URL/api/hook" \
      "${AUTH[@]}" \
      -H "X-Hook-Event: $EVENT" \
      -H "X-Tmux-Pane: ${TMUX_PANE:-}" \
      -d "$INPUT" \
      --max-time 5 >/dev/null 2>&1 &
    ;;
esac

# --- 2. New conversation/tool/session endpoints (/api/hook/*) ---
case "$EVENT" in
  UserPromptSubmit|Stop|SubagentStop)  EP="/api/hook/message" ;;
  PostToolUse)                          EP="/api/hook/tool" ;;
  SessionStart|SessionEnd)              EP="/api/hook/session" ;;
  *)                                    EP="" ;;
esac

if [ -n "$EP" ]; then
  curl -sf -X POST "$URL$EP" \
    "${AUTH[@]}" \
    -d "$INPUT" \
    --max-time 3 >/dev/null 2>&1 &
fi

# --- 3. Desktop notification on Stop ---
if [ "$EVENT" = "Stop" ]; then
  NOTIFY_SCRIPT="$SCRIPT_DIR/notify.py"
  if [ -f "$NOTIFY_SCRIPT" ]; then
    python3 "$NOTIFY_SCRIPT" '{"type":"agent-turn-complete"}' >/dev/null 2>&1 &
  fi
fi

exit 0
