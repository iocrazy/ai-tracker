#!/usr/bin/env bash
# Agent Tracker service management script
# Usage: service.sh [start|stop|restart|status|logs|logs-follow|build|install]

set -euo pipefail

source "$(dirname "$0")/env.sh"

SYSTEMD_UNIT="agent-tracker"

# Platform detection
if [[ "$(uname)" == "Darwin" ]]; then
    PLATFORM="macos"
else
    PLATFORM="linux"
fi

# ============================================================================
# macOS (launchctl)
# ============================================================================

macos_start() {
    local launch_agents="$HOME/Library/LaunchAgents"
    for svc in dev.heygo.tracker-server; do
        launchctl load "$launch_agents/$svc.plist" 2>/dev/null || true
        echo "Started $svc"
    done
}

macos_stop() {
    local launch_agents="$HOME/Library/LaunchAgents"
    for svc in dev.heygo.tracker-server; do
        launchctl unload "$launch_agents/$svc.plist" 2>/dev/null || true
        echo "Stopped $svc"
    done
}

macos_status() {
    echo "=== Service Status ==="
    for svc in dev.heygo.tracker-server; do
        if launchctl list "$svc" &>/dev/null; then
            pid=$(launchctl list "$svc" | awk 'NR==2 {print $1}')
            echo "✓ $svc (PID: $pid)"
        else
            echo "✗ $svc (stopped)"
        fi
    done
}

# ============================================================================
# Linux (systemd --user)
# ============================================================================

linux_install() {
    local unit_dir="$HOME/.config/systemd/user"
    local script_dir
    script_dir="$(cd "$(dirname "$0")" && pwd)"
    local unit_src="$script_dir/agent-tracker.service"

    if [[ ! -f "$unit_src" ]]; then
        echo "Error: $unit_src not found"
        exit 1
    fi

    mkdir -p "$unit_dir"
    cp "$unit_src" "$unit_dir/${SYSTEMD_UNIT}.service"
    systemctl --user daemon-reload
    echo "Installed ${SYSTEMD_UNIT}.service to $unit_dir"
    echo "Run '$0 start' to start, or '$0 enable' to start on login."
}

linux_start() {
    systemctl --user start "$SYSTEMD_UNIT"
    echo "Started $SYSTEMD_UNIT"
}

linux_stop() {
    systemctl --user stop "$SYSTEMD_UNIT"
    echo "Stopped $SYSTEMD_UNIT"
}

linux_status() {
    echo "=== Service Status ==="
    systemctl --user status "$SYSTEMD_UNIT" --no-pager 2>/dev/null || echo "✗ $SYSTEMD_UNIT (not installed or stopped)"
}

linux_enable() {
    systemctl --user enable "$SYSTEMD_UNIT"
    echo "Enabled $SYSTEMD_UNIT (will start on login)"
}

linux_disable() {
    systemctl --user disable "$SYSTEMD_UNIT"
    echo "Disabled $SYSTEMD_UNIT (will not start on login)"
}

# ============================================================================
# Common / dispatch
# ============================================================================

case "${1:-status}" in
  start)
    if [[ "$PLATFORM" == "macos" ]]; then macos_start; else linux_start; fi
    ;;

  stop)
    if [[ "$PLATFORM" == "macos" ]]; then macos_stop; else linux_stop; fi
    ;;

  restart)
    "$0" stop
    sleep 1
    "$0" start
    ;;

  status)
    if [[ "$PLATFORM" == "macos" ]]; then macos_status; else linux_status; fi
    ;;

  install)
    if [[ "$PLATFORM" == "linux" ]]; then
        linux_install
    else
        echo "install is only needed on Linux (systemd). macOS uses launchctl plists."
    fi
    ;;

  enable)
    if [[ "$PLATFORM" == "linux" ]]; then linux_enable; else echo "Use launchctl on macOS."; fi
    ;;

  disable)
    if [[ "$PLATFORM" == "linux" ]]; then linux_disable; else echo "Use launchctl on macOS."; fi
    ;;

  logs)
    echo "=== tracker-server ==="
    tail -20 "$TRACKER_DATA/logs/tracker-server.log" 2>/dev/null || echo "(no logs)"
    ;;

  logs-follow)
    tail -f "$TRACKER_DATA/logs/tracker-server.log"
    ;;

  build)
    echo "Building Rust project..."
    (cd "$TRACKER_DATA/src/rust" && cargo build --release)
    echo "Done. Run '$0 restart' to apply changes."
    ;;

  *)
    echo "Usage: $0 [start|stop|restart|status|logs|logs-follow|build|install|enable|disable]"
    exit 1
    ;;
esac
