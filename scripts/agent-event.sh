#!/bin/bash
# agent-event.sh - Unified entry point for agent tracking
# Usage: agent-event.sh <action> --agent <name> [options]
#
# Actions: start, finish, pause
# Agents: claude, opencode, codex, cursor, ...

source "$(dirname "$0")/env.sh"

NOTIFY_SCRIPT="$TRACKER_SCRIPTS_DIR/notify.py"
LOG_FILE="$TRACKER_LOG_DIR/agent-event.log"

mkdir -p "$TRACKER_LOG_DIR"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$LOG_FILE"
}

# Parse action
ACTION="${1:-}"
shift 2>/dev/null || true

# Default values
AGENT=""
SUMMARY=""
SESSION_ID=""
SESSION_NAME=""
WINDOW_ID=""
WINDOW_NAME=""
PANE=""
TRANSCRIPT=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --agent) AGENT="$2"; shift 2 ;;
        --summary) SUMMARY="$2"; shift 2 ;;
        --session-id) SESSION_ID="$2"; shift 2 ;;
        --window-id) WINDOW_ID="$2"; shift 2 ;;
        --pane) PANE="$2"; shift 2 ;;
        --transcript) TRANSCRIPT="$2"; shift 2 ;;
        *) shift ;;
    esac
done

# Auto-detect tmux context if not provided
if [[ -z "$SESSION_ID" || -z "$WINDOW_ID" || -z "$PANE" ]]; then
    TMUX_PANE="${TMUX_PANE:-}"
    if [[ -n "$TMUX_PANE" ]]; then
        # Get IDs
        tmux_info=$(tmux display-message -p -t "$TMUX_PANE" '#{session_id} #{window_id} #{pane_id}' 2>/dev/null || true)
        if [[ -n "$tmux_info" ]]; then
            read -r sid wid pid <<< "$tmux_info"
            SESSION_ID="${SESSION_ID:-$sid}"
            WINDOW_ID="${WINDOW_ID:-$wid}"
            PANE="${PANE:-$pid}"
        fi
        # Get names (for display in history)
        SESSION_NAME=$(tmux display-message -p -t "$TMUX_PANE" '#{session_name}' 2>/dev/null || true)
        WINDOW_NAME=$(tmux display-message -p -t "$TMUX_PANE" '#{window_name}' 2>/dev/null || true)
    fi
fi

# Map action to command
case "$ACTION" in
    start)   COMMAND="start_task" ;;
    finish)  COMMAND="finish_task" ;;
    pause)   COMMAND="pause_task" ;;
    *)
        log "ERROR: Unknown action: $ACTION"
        exit 1
        ;;
esac

log "ACTION=$ACTION AGENT=$AGENT SUMMARY=${SUMMARY:0:50}..."

# Build JSON payload and send via HTTP
PAYLOAD=$(jq -n \
    --arg command "$COMMAND" \
    --arg session_id "$SESSION_ID" \
    --arg session "$SESSION_NAME" \
    --arg window_id "$WINDOW_ID" \
    --arg window "$WINDOW_NAME" \
    --arg pane "$PANE" \
    --arg summary "$SUMMARY" \
    --arg transcript_path "$TRANSCRIPT" \
    '{command: $command, session_id: $session_id, session: $session, window_id: $window_id, window: $window, pane: $pane, summary: $summary, transcript_path: $transcript_path}')

log "POST $TRACKER_URL/api/command: $PAYLOAD"
AUTH_HEADER=""
if [[ -n "$TRACKER_TOKEN" ]]; then
    AUTH_HEADER="Authorization: Bearer $TRACKER_TOKEN"
fi
RESPONSE=$(curl -s -m 5 -X POST "$TRACKER_URL/api/command" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$PAYLOAD" 2>&1)
log "Response: $RESPONSE"

# Send notification for finish action
if [[ "$ACTION" == "finish" && -f "$NOTIFY_SCRIPT" ]]; then
    notification_json=$(jq -n \
        --arg msg "${SUMMARY:-done}" \
        --arg agent "$AGENT" \
        '{type: "agent-turn-complete", "last-assistant-message": $msg, agent: $agent}')
    log "Notify: $notification_json"
    python3 "$NOTIFY_SCRIPT" "$notification_json" >> "$LOG_FILE" 2>&1 || true
fi

log "Done"
