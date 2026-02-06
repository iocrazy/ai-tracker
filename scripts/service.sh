#!/usr/bin/env bash
# Agent Tracker 服务管理脚本
# 用法: service.sh [start|stop|restart|status|logs]

set -euo pipefail

CONFIG_DIR="$HOME/.config/agent-tracker"
LAUNCHD_DIR="$CONFIG_DIR/launchd"
LAUNCH_AGENTS="$HOME/Library/LaunchAgents"

# Only tracker-server now (handles HTTP API + WebSocket on port 3099)
SERVICES=("dev.heygo.tracker-server")

case "${1:-status}" in
  start)
    for svc in "${SERVICES[@]}"; do
      launchctl load "$LAUNCH_AGENTS/$svc.plist" 2>/dev/null || true
      echo "Started $svc"
    done
    ;;

  stop)
    for svc in "${SERVICES[@]}"; do
      launchctl unload "$LAUNCH_AGENTS/$svc.plist" 2>/dev/null || true
      echo "Stopped $svc"
    done
    ;;

  restart)
    $0 stop
    sleep 1
    $0 start
    ;;

  status)
    echo "=== 服务状态 ==="
    for svc in "${SERVICES[@]}"; do
      if launchctl list "$svc" &>/dev/null; then
        pid=$(launchctl list "$svc" | awk 'NR==2 {print $1}')
        echo "✓ $svc (PID: $pid)"
      else
        echo "✗ $svc (stopped)"
      fi
    done
    ;;

  logs)
    echo "=== tracker-server ==="
    tail -20 "$CONFIG_DIR/logs/tracker-server.log" 2>/dev/null || echo "(no logs)"
    ;;

  logs-follow)
    tail -f "$CONFIG_DIR/logs/tracker-server.log"
    ;;

  build)
    echo "Building Rust project..."
    (cd "$CONFIG_DIR/src/rust" && cargo build --release)
    echo "Done. Run '$0 restart' to apply changes."
    ;;

  *)
    echo "用法: $0 [start|stop|restart|status|logs|logs-follow|build]"
    exit 1
    ;;
esac
