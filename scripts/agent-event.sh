#!/bin/bash
# agent-event.sh - Unified entry point for agent tracking
# Usage: agent-event.sh <action> --agent <name> [options]
#
# Actions: start, finish, pause
# Agents: claude, opencode, codex, cursor, ...

TRACKER_BIN="$HOME/.config/agent-tracker/bin/tracker-client"
NOTIFY_SCRIPT="$HOME/.config/agent-tracker/scripts/notify.py"
LOG_FILE="$HOME/.config/agent-tracker/logs/agent-event.log"

mkdir -p "$(dirname "$LOG_FILE")"

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

# Check tracker-client
if [[ ! -x "$TRACKER_BIN" ]]; then
    log "ERROR: tracker-client not found"
    exit 1
fi

log "ACTION=$ACTION AGENT=$AGENT SUMMARY=${SUMMARY:0:50}..."

# Build args
ARGS=("command")
[[ -n "$SESSION_ID" ]] && ARGS+=("--session-id" "$SESSION_ID")
[[ -n "$SESSION_NAME" ]] && ARGS+=("--session" "$SESSION_NAME")
[[ -n "$WINDOW_ID" ]] && ARGS+=("--window-id" "$WINDOW_ID")
[[ -n "$WINDOW_NAME" ]] && ARGS+=("--window" "$WINDOW_NAME")
[[ -n "$PANE" ]] && ARGS+=("--pane" "$PANE")
[[ -n "$SUMMARY" ]] && ARGS+=("--summary" "$SUMMARY")
[[ -n "$TRANSCRIPT" ]] && ARGS+=("--transcript" "$TRANSCRIPT")

case "$ACTION" in
    start)
        ARGS+=("start_task")
        ;;
    finish)
        ARGS+=("finish_task")
        ;;
    pause)
        ARGS+=("pause_task")
        ;;
    *)
        log "ERROR: Unknown action: $ACTION"
        exit 1
        ;;
esac

log "Running: $TRACKER_BIN ${ARGS[*]}"
"$TRACKER_BIN" "${ARGS[@]}" >> "$LOG_FILE" 2>&1 || true

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
