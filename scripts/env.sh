#!/usr/bin/env bash
# env.sh — Shared path resolution for Agent Tracker scripts.
# Source this at the top of any script:
#   source "$(dirname "$0")/env.sh"
#
# Provides: TRACKER_DATA, TRACKER_SCRIPTS_DIR, TRACKER_CONFIG,
#           TRACKER_DB, TRACKER_LOG_DIR, TRACKER_RUN_DIR, TRACKER_BACKUP_DIR

# Scripts directory (where this file lives)
TRACKER_SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Data directory resolution:
#   1. TRACKER_DATA_DIR env var (set by Tauri sidecar, or user override)
#   2. ~/Library/Application Support/com.agent-tracker.menubar/ (Tauri app installed)
#   3. ~/.config/agent-tracker/ (legacy/standalone)
if [[ -n "${TRACKER_DATA_DIR:-}" ]]; then
    TRACKER_DATA="${TRACKER_DATA_DIR}"
elif [[ "$(uname)" == "Darwin" ]] && [[ -f "$HOME/Library/Application Support/com.agent-tracker.menubar/data/tracker.db" ]]; then
    TRACKER_DATA="$HOME/Library/Application Support/com.agent-tracker.menubar"
else
    TRACKER_DATA="$HOME/.config/agent-tracker"
fi

# Derived paths
TRACKER_CONFIG="$TRACKER_DATA/agent-config.json"
TRACKER_DB="$TRACKER_DATA/data/tracker.db"
TRACKER_LOG_DIR="$TRACKER_DATA/logs"
TRACKER_RUN_DIR="$TRACKER_DATA/run"
TRACKER_BACKUP_DIR="$TRACKER_DATA/backups"
TRACKER_URL="${TRACKER_URL:-http://127.0.0.1:3099}"

# Auth token (read from config, cached for the script lifetime)
if [[ -z "${TRACKER_TOKEN:-}" && -f "$TRACKER_CONFIG" ]]; then
    TRACKER_TOKEN=$(python3 -c "import json; print(json.load(open('$TRACKER_CONFIG')).get('auth',{}).get('token',''))" 2>/dev/null || true)
fi
TRACKER_TOKEN="${TRACKER_TOKEN:-}"
