#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "$0")/env.sh"

F="$TRACKER_RUN_DIR/latest_notified.txt"
if [[ ! -f "$F" ]]; then
  exit 0
fi

# Read line and split by literal ':::' into sid, wid, pid robustly
# Extract fields robustly using awk with a literal ':::' separator
sid=$(awk -F ':::' 'NR==1{print $1}' "$F" | tr -d '\r\n')
wid=$(awk -F ':::' 'NR==1{print $2}' "$F" | tr -d '\r\n')
pid=$(awk -F ':::' 'NR==1{print $3}' "$F" | tr -d '\r\n')

if [[ -z "${sid:-}" || -z "${wid:-}" || -z "${pid:-}" ]]; then
  exit 0
fi

mkdir -p "$TRACKER_RUN_DIR"

# Record current location for jump-back
current=$(tmux display-message -p "#{session_id}:::#{window_id}:::#{pane_id}" | tr -d '\r\n')
if [[ -n "$current" ]]; then
  printf '%s\n' "$current" > "$TRACKER_RUN_DIR/jump_back.txt"
fi

# Mark as viewed (acknowledged) in tracker via HTTP API (graceful if unavailable)
curl -s -m 2 -X POST "$TRACKER_URL/api/command" \
  -H "Content-Type: application/json" \
  -d "{\"command\":\"acknowledge\",\"session_id\":\"$sid\",\"window_id\":\"$wid\",\"pane\":\"$pid\"}" >/dev/null 2>&1 || true

# Focus the tmux target
tmux switch-client -t "$sid" \; select-window -t "$wid" \; select-pane -t "$pid"
