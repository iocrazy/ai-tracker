#!/bin/bash
# 获取当前 pane 的 tracker 状态，用于 tmux 状态栏显示

SESSION_ID="$1"
WINDOW_ID="$2"
PANE_ID="$3"

if [ -z "$SESSION_ID" ] || [ -z "$WINDOW_ID" ] || [ -z "$PANE_ID" ]; then
    exit 0
fi

# 获取 tracker 状态
STATE=$("$HOME/.config/agent-tracker/bin/tracker-client" state 2>/dev/null)

if [ -z "$STATE" ]; then
    exit 0
fi

# 解析当前 pane 的任务状态
TASK=$(echo "$STATE" | jq -r --arg sid "$SESSION_ID" --arg wid "$WINDOW_ID" --arg pid "$PANE_ID" \
    '.tasks[] | select(.session_id == $sid and .window_id == $wid and .pane == $pid) | "\(.status)|\(.summary)"' 2>/dev/null | head -1)

if [ -z "$TASK" ]; then
    exit 0
fi

STATUS=$(echo "$TASK" | cut -d'|' -f1)
SUMMARY=$(echo "$TASK" | cut -d'|' -f2- | head -c 30)

case "$STATUS" in
    "in_progress")
        echo "#[fg=yellow,bold]⏳ $SUMMARY"
        ;;
    "awaiting_input")
        echo "#[fg=orange,bold]⏸ 等待确认"
        ;;
    "completed")
        echo "#[fg=green]✓ $SUMMARY"
        ;;
esac
