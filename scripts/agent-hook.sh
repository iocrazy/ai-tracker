#!/bin/bash
# agent-hook.sh — Unified Claude Code hook forwarder
# Routes hook events to tracker-server ingest endpoints.
# Reads JSON from stdin, uses jq for parsing.
#
# Usage: Configured in ~/.claude/settings.json as hook command for:
#   UserPromptSubmit, Stop, SubagentStop, PostToolUse, SessionStart, SessionEnd

INPUT=$(cat)
EVENT=$(echo "$INPUT" | jq -r '.hook_event_name // ""' 2>/dev/null)

[ -z "$EVENT" ] && exit 0

# Load env.sh if available (for TRACKER_TOKEN, TRACKER_URL)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$SCRIPT_DIR/env.sh" ] && source "$SCRIPT_DIR/env.sh"

# If token not set via env.sh, read directly from config files
if [ -z "${TRACKER_TOKEN:-}" ]; then
  for cfg in \
    "$HOME/Library/Application Support/com.agent-tracker.menubar/agent-config.json" \
    "$HOME/.config/agent-tracker/agent-config.json"; do
    if [ -f "$cfg" ]; then
      TRACKER_TOKEN=$(jq -r '.auth.token // ""' "$cfg" 2>/dev/null)
      [ -n "$TRACKER_TOKEN" ] && break
    fi
  done
fi

TOKEN="${TRACKER_TOKEN:-}"
URL="${TRACKER_URL:-http://127.0.0.1:3099}"

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
