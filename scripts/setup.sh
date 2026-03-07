#!/usr/bin/env bash
# Agent Tracker — Post-install setup
# Configures Claude Code hooks and TRACKER_TOKEN env var.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/iocrazy/ai-tracker/main/scripts/setup.sh | bash
#   # or locally:
#   ./scripts/setup.sh

set -euo pipefail

# ============================================================================
# Colors
# ============================================================================

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

info()  { printf "${CYAN}[INFO]${NC}  %s\n" "$*"; }
ok()    { printf "${GREEN}[OK]${NC}    %s\n" "$*"; }
warn()  { printf "${YELLOW}[WARN]${NC}  %s\n" "$*"; }
err()   { printf "${RED}[ERR]${NC}   %s\n" "$*"; }
header(){ printf "\n${BOLD}=== %s ===${NC}\n\n" "$*"; }

# ============================================================================
# 1. Platform detection
# ============================================================================

header "Agent Tracker Setup"

OS="$(uname -s)"
case "$OS" in
  Darwin) PLATFORM="macos" ;;
  Linux)  PLATFORM="linux" ;;
  *)      err "Unsupported OS: $OS"; exit 1 ;;
esac

info "Platform: $PLATFORM"

# Detect install mode and config path
TAURI_DATA_DIR=""
STANDALONE_DIR="$HOME/.config/agent-tracker"
INSTALL_MODE=""
SCRIPTS_DIR=""
NOTIFY_PY=""

if [[ "$PLATFORM" == "macos" ]]; then
    TAURI_DATA_DIR="$HOME/Library/Application Support/com.agent-tracker.menubar"
    if [[ -d "/Applications/Agent Tracker.app" ]]; then
        INSTALL_MODE="tauri"
        SCRIPTS_DIR="/Applications/Agent Tracker.app/Contents/Resources/scripts"
        NOTIFY_PY="$SCRIPTS_DIR/notify.py"
    fi
elif [[ "$PLATFORM" == "linux" ]]; then
    TAURI_DATA_DIR="$HOME/.local/share/com.agent-tracker.menubar"
    # Check common Linux install paths
    for app_dir in "/usr/share/agent-tracker" "$HOME/.local/share/agent-tracker"; do
        if [[ -d "$app_dir" ]]; then
            INSTALL_MODE="tauri"
            SCRIPTS_DIR="$app_dir/scripts"
            NOTIFY_PY="$SCRIPTS_DIR/notify.py"
            break
        fi
    done
fi

# Fallback to standalone
if [[ -z "$INSTALL_MODE" ]] && [[ -d "$STANDALONE_DIR" ]]; then
    INSTALL_MODE="standalone"
    SCRIPTS_DIR="$STANDALONE_DIR/scripts"
    NOTIFY_PY="$SCRIPTS_DIR/notify.py"
fi

if [[ -z "$INSTALL_MODE" ]]; then
    err "Agent Tracker not found. Install the app first, then run this script."
    exit 1
fi

info "Install mode: $INSTALL_MODE"

# ============================================================================
# 2. Ensure server is running
# ============================================================================

header "Checking Server"

TRACKER_URL="${TRACKER_URL:-http://127.0.0.1:3099}"
SERVER_OK=false

for i in $(seq 1 6); do
    if curl -sf "$TRACKER_URL/health" >/dev/null 2>&1; then
        SERVER_OK=true
        break
    fi
    if [[ $i -eq 1 ]]; then
        if [[ "$INSTALL_MODE" == "tauri" ]]; then
            warn "Server not responding. Please open Agent Tracker app and try again."
        else
            warn "Server not responding. Run: ~/.config/agent-tracker/scripts/service.sh start"
        fi
        info "Retrying (${i}/6)..."
    else
        info "Retrying (${i}/6)..."
    fi
    sleep 5
done

if [[ "$SERVER_OK" != "true" ]]; then
    err "Server not reachable at $TRACKER_URL after 30s."
    err "Start the server first, then re-run this script."
    exit 1
fi

ok "Server running at $TRACKER_URL"

# ============================================================================
# 3. Read auth token from agent-config.json
# ============================================================================

header "Reading Auth Token"

CONFIG_FILE=""
if [[ -n "$TAURI_DATA_DIR" ]] && [[ -f "$TAURI_DATA_DIR/agent-config.json" ]]; then
    CONFIG_FILE="$TAURI_DATA_DIR/agent-config.json"
elif [[ -f "$STANDALONE_DIR/agent-config.json" ]]; then
    CONFIG_FILE="$STANDALONE_DIR/agent-config.json"
fi

if [[ -z "$CONFIG_FILE" ]]; then
    err "agent-config.json not found."
    err "Looked in: $TAURI_DATA_DIR and $STANDALONE_DIR"
    exit 1
fi

info "Config: $CONFIG_FILE"

AUTH_TOKEN=$(python3 -c "
import json, sys
try:
    cfg = json.load(open('$CONFIG_FILE'))
    token = cfg.get('auth', {}).get('token', '')
    if not token:
        print('', end='')
        sys.exit(1)
    print(token, end='')
except Exception as e:
    print('', end='')
    sys.exit(1)
" 2>/dev/null) || true

if [[ -z "$AUTH_TOKEN" ]]; then
    err "Could not read auth token from $CONFIG_FILE"
    exit 1
fi

ok "Auth token found (${AUTH_TOKEN:0:8}...)"

# Verify token works
if curl -sf -H "Authorization: Bearer $AUTH_TOKEN" "$TRACKER_URL/api/auth/verify" >/dev/null 2>&1; then
    ok "Token verified against server"
else
    warn "Token verification failed — server may not be fully started yet"
fi

# ============================================================================
# 4. Configure Claude Code hooks
# ============================================================================

header "Configuring Claude Code Hooks"

CLAUDE_SETTINGS="$HOME/.claude/settings.json"
mkdir -p "$HOME/.claude"

# Backup existing settings
if [[ -f "$CLAUDE_SETTINGS" ]]; then
    BACKUP="$CLAUDE_SETTINGS.bak.$(date +%s)"
    cp "$CLAUDE_SETTINGS" "$BACKUP"
    info "Backed up settings to $BACKUP"
fi

# Resolve notify.py path — escape for JSON
NOTIFY_PY_ESCAPED=$(printf '%s' "$NOTIFY_PY" | sed 's/"/\\"/g')

# Use python3 to merge hooks into settings.json (idempotent)
python3 << 'PYEOF' - "$CLAUDE_SETTINGS" "$AUTH_TOKEN" "$NOTIFY_PY_ESCAPED"
import json
import sys
import os

settings_path = sys.argv[1]
token = sys.argv[2]
notify_py = sys.argv[3]

# Load existing settings or start fresh
settings = {}
if os.path.exists(settings_path):
    try:
        with open(settings_path) as f:
            settings = json.load(f)
    except (json.JSONDecodeError, IOError):
        settings = {}

# Ensure top-level keys
if "env" not in settings:
    settings["env"] = {}
if "hooks" not in settings:
    settings["hooks"] = {}

# Set TRACKER_TOKEN in env
settings["env"]["TRACKER_TOKEN"] = token

# Define the hook commands (marker comment for idempotency check)
MARKER = "agent-tracker"

def make_curl_hook(event: str) -> dict:
    return {
        "type": "command",
        "command": (
            f'INPUT=$(cat); curl -s -m 5 -X POST http://127.0.0.1:3099/api/hook '
            f'-H \'Content-Type: application/json\' '
            f'-H \'X-Hook-Event: {event}\' '
            f'-H "X-Tmux-Pane: ${{TMUX_PANE:-}}" '
            f'-H "Authorization: Bearer $TRACKER_TOKEN" '
            f'-d "$INPUT" >/dev/null 2>&1 || true'
        ),
        "async": True,
    }

def make_stop_hook() -> dict:
    notify_cmd = ""
    if notify_py:
        notify_cmd = (
            f'; python3 "{notify_py}" '
            '\'{"type":"agent-turn-complete"}\' 2>/dev/null || true'
        )
    return {
        "type": "command",
        "command": (
            'INPUT=$(cat); curl -s -m 5 -X POST http://127.0.0.1:3099/api/hook '
            '-H \'Content-Type: application/json\' '
            '-H \'X-Hook-Event: Stop\' '
            '-H "X-Tmux-Pane: ${TMUX_PANE:-}" '
            '-H "Authorization: Bearer $TRACKER_TOKEN" '
            '-d "$INPUT" >/dev/null 2>&1 || true'
            + notify_cmd
        ),
        "async": True,
    }

def has_tracker_hook(hook_list: list) -> bool:
    """Check if any hook entry already contains agent-tracker curl commands."""
    for entry in hook_list:
        hooks = entry.get("hooks", [])
        for h in hooks:
            cmd = h.get("command", "")
            if "127.0.0.1:3099/api/hook" in cmd:
                return True
    return False

# UserPromptSubmit
if not has_tracker_hook(settings["hooks"].get("UserPromptSubmit", [])):
    settings["hooks"].setdefault("UserPromptSubmit", []).append({
        "hooks": [make_curl_hook("UserPromptSubmit")]
    })
    print("  Added UserPromptSubmit hook")
else:
    print("  UserPromptSubmit hook already present — skipped")

# Stop
if not has_tracker_hook(settings["hooks"].get("Stop", [])):
    settings["hooks"].setdefault("Stop", []).append({
        "hooks": [make_stop_hook()]
    })
    print("  Added Stop hook")
else:
    print("  Stop hook already present — skipped")

# PermissionRequest
if not has_tracker_hook(settings["hooks"].get("PermissionRequest", [])):
    settings["hooks"].setdefault("PermissionRequest", []).append({
        "hooks": [make_curl_hook("PermissionRequest")]
    })
    print("  Added PermissionRequest hook")
else:
    print("  PermissionRequest hook already present — skipped")

# Notification (with idle_prompt matcher)
if not has_tracker_hook(settings["hooks"].get("Notification", [])):
    settings["hooks"].setdefault("Notification", []).append({
        "matcher": "idle_prompt",
        "hooks": [make_curl_hook("Notification")]
    })
    print("  Added Notification hook")
else:
    print("  Notification hook already present — skipped")

# Write back
with open(settings_path, "w") as f:
    json.dump(settings, f, indent=2)
    f.write("\n")

print("  Settings saved to", settings_path)
PYEOF

ok "Claude Code hooks configured"

# ============================================================================
# 5. Set TRACKER_TOKEN in shell profile
# ============================================================================

header "Setting TRACKER_TOKEN Environment Variable"

EXPORT_LINE="export TRACKER_TOKEN=\"$AUTH_TOKEN\""

# Determine shell profile
if [[ -n "${ZSH_VERSION:-}" ]] || [[ "$SHELL" == *zsh* ]]; then
    PROFILE="$HOME/.zshrc"
else
    PROFILE="$HOME/.bashrc"
fi

if [[ -f "$PROFILE" ]] && grep -q 'export TRACKER_TOKEN=' "$PROFILE"; then
    # Update existing line
    sed -i.bak "s|^export TRACKER_TOKEN=.*|$EXPORT_LINE|" "$PROFILE"
    rm -f "$PROFILE.bak"
    ok "Updated TRACKER_TOKEN in $PROFILE"
else
    # Append
    printf '\n# Agent Tracker auth token\n%s\n' "$EXPORT_LINE" >> "$PROFILE"
    ok "Added TRACKER_TOKEN to $PROFILE"
fi

# ============================================================================
# 6. Verify
# ============================================================================

header "Verification"

PASS=0
FAIL=0

# Server health
if curl -sf "$TRACKER_URL/health" >/dev/null 2>&1; then
    ok "Server health check passed"
    PASS=$((PASS + 1))
else
    err "Server health check failed"
    FAIL=$((FAIL + 1))
fi

# Auth works
if curl -sf -H "Authorization: Bearer $AUTH_TOKEN" "$TRACKER_URL/api/auth/verify" >/dev/null 2>&1; then
    ok "Auth verification passed"
    PASS=$((PASS + 1))
else
    err "Auth verification failed"
    FAIL=$((FAIL + 1))
fi

# Hooks in settings
if grep -q "127.0.0.1:3099/api/hook" "$CLAUDE_SETTINGS" 2>/dev/null; then
    ok "Claude Code hooks present"
    PASS=$((PASS + 1))
else
    err "Claude Code hooks not found in settings"
    FAIL=$((FAIL + 1))
fi

# Token in profile
if grep -q "TRACKER_TOKEN" "$PROFILE" 2>/dev/null; then
    ok "TRACKER_TOKEN in $PROFILE"
    PASS=$((PASS + 1))
else
    err "TRACKER_TOKEN not found in $PROFILE"
    FAIL=$((FAIL + 1))
fi

# ============================================================================
# Done
# ============================================================================

echo ""
if [[ $FAIL -eq 0 ]]; then
    printf "${GREEN}${BOLD}Setup complete!${NC} (%d/%d checks passed)\n\n" "$PASS" "$((PASS + FAIL))"
    echo "Next steps:"
    echo "  1. Run: source $PROFILE"
    echo "  2. Open a new Claude Code session — hooks will start tracking automatically."
    echo ""
else
    printf "${YELLOW}${BOLD}Setup completed with warnings.${NC} (%d/%d checks passed)\n\n" "$PASS" "$((PASS + FAIL))"
fi
