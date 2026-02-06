# 修复历史对话显示功能

> 日期: 2026-02-03
> 状态: Implementing

## 问题分析

| 问题 | 根因 | 位置 |
|------|------|------|
| Session/Window 显示 ID 而非名字 | `agent-event.sh` 只传递 ID，没有传递名字 | `agent-event.sh:67-68` |
| message_count = 0 | conversation_messages 未返回到前端 | 后端 API |
| HISTORY 按钮显示假数据 | `handleViewHistory` 使用 `generateMockChat()` | `App.tsx:319` |
| Timeline 详情缺少实际对话 | API 不返回 conversation_messages | 后端 API |

## 修复方案

### 1. 修复 Session/Window 名字

修改 `agent-event.sh`，获取并传递实际名字：

```bash
# 获取 session/window 名字
SESSION_NAME=$(tmux display-message -p -t "$TMUX_PANE" '#{session_name}')
WINDOW_NAME=$(tmux display-message -p -t "$TMUX_PANE" '#{window_name}')

# 传递名字参数
[[ -n "$SESSION_NAME" ]] && ARGS+=("--session" "$SESSION_NAME")
[[ -n "$WINDOW_NAME" ]] && ARGS+=("--window" "$WINDOW_NAME")
```

### 2. 修复 HISTORY 按钮功能

数据流：
```
点击 HISTORY 按钮
       │
       ▼
检查该 window 是否有活跃 task
(status = RUNNING | WAITING)
       │
       ├─ YES ──► /api/tmux/capture 获取实时内容
       │
       └─ NO ───► /api/claude/messages 获取最近对话
```

### 3. 修复 Timeline 详情

后端 `/api/history/:id` 返回 `messages` 字段（从 conversation_messages 表加载）

### 4. 后端 API

- 从 master 合并 `/api/claude/messages`
- 修改 `get_history_detail` 返回 messages

## 修改清单

| # | 文件 | 改动 |
|---|------|------|
| 1 | `scripts/agent-event.sh` | 获取并传递 session_name/window_name |
| 2 | `tracker-server/main.rs` | 合并 `/api/claude/messages`，修改 `get_history_detail` |
| 3 | `web/src/services/api.ts` | 新增 `fetchClaudeMessages()`, `fetchTmuxCapture()` |
| 4 | `web/src/App.tsx` | 重写 `handleViewHistory` 和 `handleTimelineDetails` |
