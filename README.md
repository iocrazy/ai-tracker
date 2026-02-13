# Agent Tracker

Tmux-aware agent task and note tracker for Claude Code parallel development.

## Build, Install & Restart

```bash
./scripts/install_brew_service.sh
```

This script:
1. Builds `tracker-server` from source
2. Installs it via Homebrew
3. Restarts the brew service

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           CLAUDE CODE 进程                                   │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        Claude Code Runtime                          │   │
│  │                                                                     │   │
│  │   用户输入 ──→ AI 处理 ──→ Tool 调用 ──→ 响应完成                    │   │
│  │       │                                      │                      │   │
│  │       ▼                                      ▼                      │   │
│  │  ┌──────────┐                         ┌──────────┐                  │   │
│  │  │  Hook:   │                         │  Hook:   │                  │   │
│  │  │ Submit   │                         │  Stop    │                  │   │
│  │  └────┬─────┘                         └────┬─────┘                  │   │
│  └───────┼─────────────────────────────────────┼───────────────────────┘   │
│          │                                     │                           │
└──────────┼─────────────────────────────────────┼───────────────────────────┘
           │                                     │
           ▼                                     ▼
    ┌──────────────┐                      ┌──────────────────────┐
    │tracker-client│                      │ tracker-client       │
    │ start_task   │                      │ finish_task          │
    └──────┬───────┘                      ├──────────────────────┤
                                          │ notify.py            │
                                          │ (macOS notification) │
                                          └──────┬───────────────┘
           │                                     │
           │     ┌───────────────────────────────┘
           │     │
           ▼     ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                  HTTP/WebSocket (port 3099)                  │
    └─────────────────────────────────────────────────────────────┘
                              │
                              ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                   TRACKER-SERVER (Rust)                      │
    │  ┌─────────────────────────────────────────────────────────┐│
    │  │                    内存状态 + SQLite                     ││
    │  │  tasks: HashMap<String, Task>                           ││
    │  │    - SessionID, WindowID, PaneID                        ││
    │  │    - Status: in_progress | awaiting_input | completed   ││
    │  │    - Summary, CompletionNote                            ││
    │  │    - StartedAt, CompletedAt                             ││
    │  │                                                         ││
    │  │  notes: HashMap<String, Note>                           ││
    │  │  goals: HashMap<String, Goal>                           ││
    │  │  history: Vec<HistoryRecord>                            ││
    │  │                                                         ││
    │  │  broadcast_tx: broadcast::Sender<Envelope>              ││
    │  │    - WebSocket 订阅者                                    ││
    │  └─────────────────────────────────────────────────────────┘│
    │                         │                                    │
    │                         ▼                                    │
    │  ┌─────────────────────────────────────────────────────────┐│
    │  │  REST API: /api/state, /api/command, /api/history       ││
    │  │  WebSocket: /ws (实时状态推送)                            ││
    │  │  SQLite: data/tracker.db                                ││
    │  └─────────────────────────────────────────────────────────┘│
    └─────────────────────────────────────────────────────────────┘
                              │
                              │ JSON over WebSocket
                              ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                   TRACKER-TUI (Rust)                         │
    │  ┌─────────────────────────────────────────────────────────┐│
    │  │              实时显示任务状态                             ││
    │  │  ▶ ⠋ 正在处理...           Session/Window   00m32s      ││
    │  │  ▌ 🚧 等待输入              Session/Window   01m15s      ││
    │  │  ✓ 已完成                  Session/Window   05m22s      ││
    │  └─────────────────────────────────────────────────────────┘│
    └─────────────────────────────────────────────────────────────┘
```

---

## Communication Protocol

**通信方式：HTTP REST API + WebSocket**

```
Server: http://127.0.0.1:3099
WebSocket: ws://127.0.0.1:3099/ws
```

### REST API Endpoints

#### Core APIs

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | 健康检查 |
| GET | `/api/state` | 获取完整状态 |
| POST | `/api/command` | 发送命令 |
| GET | `/api/tasks` | 任务列表 |
| GET | `/api/notes` | 笔记列表 |
| GET | `/api/goals` | 目标列表 |
| GET | `/api/history` | 历史记录 |

#### Workspace APIs

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/workspace/start` | 启动工作区 (支持全栈模式、端口分配、布局模板、浏览器自动打开) |
| POST | `/api/workspace/destroy` | 销毁工作区 (支持端口进程清理、Git 分支删除) |
| POST | `/api/workspace/resume` | 恢复工作区 |
| POST | `/api/workspace/activate` | 窗口激活钩子 (刷新 lazygit、切换浏览器标签) |
| GET | `/api/workspace/metadata` | 获取工作区元数据 |

**Start Workspace Request:**
```json
{
  "git_dir": "/path/to/project",
  "branch": "feature-test",
  "session": "optional-session-name",
  "agent": "claude",
  "layout": "workspace",
  "fullstack_mode": true,
  "port_base": 3000,
  "frontend_cmd": "npm run dev -- --port $PORT",
  "backend_cmd": "python -m uvicorn main:app --port $PORT",
  "auto_open_browser": true
}
```

**Destroy Workspace Request:**
```json
{
  "git_dir": "/path/to/project",
  "branch": "feature-test",
  "force": false,
  "kill_ports": true,
  "delete_branch": true
}
```

#### Port Management APIs

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/port/check/:port` | 检查端口是否被占用 |
| POST | `/api/port/kill` | 杀死占用端口的进程 |
| GET | `/api/port/allocate` | 分配可用端口 |

#### Browser Automation APIs

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/browser/open` | 打开浏览器 URL (支持 Chrome、Safari、Arc) |
| POST | `/api/browser/switch-tab` | 切换到包含指定端口的浏览器标签页 |

#### Tmux APIs

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/tmux/sessions` | 获取所有 tmux sessions |
| GET | `/api/tmux/windows` | 获取所有 windows |
| POST | `/api/tmux/send-keys` | 发送按键到指定 pane |
| GET | `/api/tmux/capture/:session/:window/:pane` | 捕获 pane 内容 |
| POST | `/api/tmux/kill-window` | 关闭窗口 (同时保存到 closed_windows) |
| GET | `/api/tmux/closed-windows/:session` | 获取已关闭的窗口列表 |
| DELETE | `/api/tmux/closed-windows` | 删除已关闭窗口记录 |

#### Closed Windows & Resume (窗口恢复系统)

##### gitDir 检测机制

系统通过实时 tmux 轮询自动检测每个 session 的 git 仓库路径，不依赖 session 名称：

```
检测流程:
  tmux pane_current_path (当前工作目录)
    → 优先: tmux window option @agent_main_repo (脚本预设)
    → 回退: git rev-parse --show-toplevel (自动发现 git root)
    → 回退: 直接使用 working directory

数据传递:
  tmux pane → TmuxWindowInfo.git_dir → WebSocket broadcast
    → mapTmuxToSessions() → AgentSession.gitDir → AddWindowModal prop
    → fetchGitBranches(gitDir) 查询 worktree 列表
```

**关键设计:**
- gitDir 是 **session 级别**，取自该 session 第一个 window 的第一个 pane 的工作目录
- 同一个 session 的所有 window 共享一个 gitDir
- 新建 session 时默认在 `~/`，用 yazi 导航到项目目录后，下次轮询会自动检测到 git root

##### 关闭窗口的保存

窗口关闭时通过两个途径保存记录：

| 触发方式 | 说明 | 写入位置 |
|----------|------|----------|
| 自动检测 | tmux 轮询发现窗口消失 | 全局 DB + 项目 DB (双写) |
| 手动 kill | Web UI 点击关闭窗口 | 全局 DB |

- 存储去重: `save_closed_window()` 写入前先删除同 `session_name + window_name` 的旧记录
- 查询去重: `load_closed_windows()` 用 `GROUP BY window_name` 只返回每个窗口名的最新记录
- 全局 DB: `~/.config/agent-tracker/data/tracker.db` → `closed_windows` 表
- 项目 DB: `<git-root>/.aitracker/tracker.db` → `closed_windows` 表

**数据结构:**
```json
{
  "id": 1,
  "session_name": "2-mediahub",
  "window_name": "feature-add-whisper-support",
  "working_dir": "/path/to/worktree",
  "git_branch": "feature/add-whisper-support",
  "pane_count": 3,
  "closed_at": "2026-02-12T06:59:23Z"
}
```

##### Web UI Resume (统一列表)

Add Window → Resume 标签页显示一个统一的可恢复列表，合并两个数据源：

```
数据源 1: git worktree list --porcelain (通过 gitDir)
  → 检测磁盘上存在但没有打开 tmux 窗口的 worktree
  → 显示 worktree 目录名 作为主标题 (不是分支名)
  → 分支名和目录名不一致时，显示 "branch: xxx" 辅助信息

数据源 2: closed_windows DB 记录 (通过 sessionName)
  → 已关闭但有数据库记录的普通窗口
  → 自动排除已在 worktree 列表中出现的条目 (跨源去重)
```

**为什么显示目录名而不是分支名:**
- 用户在 lazygit、文件管理器中看到的是 worktree 目录名
- 目录名和分支名可能不一致 (在 worktree 内 checkout 了其他分支)
- 例: 目录 `feature-Points-capacity-payment-system` 但分支是 `feature/sidebar-architecture`

**条目类型:**

| 类型 | 来源 | 图标 | Badge 颜色 | 主标题 | 辅助信息 |
|------|------|------|-----------|--------|----------|
| worktree | git worktree list | GitBranch | 黄色 | 目录名 (basename) | 分支名 (若不同) |
| window | closed_windows DB | FolderOpen | 青色 | 窗口名 | 工作目录路径 |

**操作对比:**

| 操作 | 关闭窗口 | 删除数据库记录 | 删除 Worktree | 删除 Git 分支 |
|------|----------|----------------|---------------|---------------|
| Close Window | ✅ | ❌ | ❌ | ❌ |
| Resume | 创建新窗口 | ❌ | ❌ | ❌ |
| Delete | ❌ | ✅ | ❌ | ❌ |
| DESTROY | ❌ | ✅ | ✅ | ✅ |

**DESTROY 后数据保留情况:**

| 数据类型 | DESTROY 是否删除 | 存储位置 |
|---------|-----------------|----------|
| Timeline 历史记录 | ❌ 保留 | `history` 表 |
| 对话消息详情 | ❌ 保留 | `conversation_messages` 表 |
| 工具使用记录 | ❌ 保留 | `tool_usage` 表 |
| Git 提交记录 | ❌ 保留 | `commits` 表 |
| Claude 会话文件 | ❌ 保留 | `~/.claude/projects/` |
| Git Worktree | ✅ 删除 | 项目目录 |
| Git 分支 | ✅ 删除 | Git 仓库 |

> **注意**: DESTROY 只清理工作区相关文件，Timeline 中的历史记录会永久保留，可随时查看对话详情。

### Envelope Structure (`tracker-core/src/ipc.rs`)

```rust
pub struct Envelope {
    pub kind: String,       // 消息类型: "command" | "state" | "ack"
    pub command: String,    // 命令名: start_task | finish_task | pause_task | ...
    pub session_id: String, // tmux session id
    pub window_id: String,  // tmux window id
    pub pane: String,       // tmux pane id
    pub summary: String,    // 任务摘要
    pub tasks: Vec<Task>,   // 任务列表 (state 消息)
    pub notes: Vec<Note>,   // 笔记列表
    pub goals: Vec<Goal>,   // 目标列表
    pub history: Vec<HistoryRecord>, // 历史记录
    ...
}
```

### Task Status

| Status | Description |
|--------|-------------|
| `in_progress` | Claude 正在处理任务 |
| `awaiting_input` | Claude 暂停，等待用户输入/确认 |
| `completed` | 任务已完成 |

---

## Communication Flow

### Scenario A: User Submit Prompt (UserPromptSubmit)

```
┌──────────────────┐     stdin (JSON)      ┌───────────────────┐
│   Claude Code    │ ───────────────────→  │  Hook Shell 命令   │
│  触发 Hook       │   {                   │                   │
│                  │     "prompt": "...",  │  解析 JSON        │
│                  │     "session_id": ""  │  获取 tmux 上下文  │
└──────────────────┘   }                   └─────────┬─────────┘
                                                     │
                                                     ▼
                                           ┌───────────────────┐
                                           │  tracker-client   │
                                           │  command          │
                                           │  -session-id $SID │
                                           │  -window-id $WID  │
                                           │  -pane $PID       │
                                           │  -summary "..."   │
                                           │  start_task       │
                                           └─────────┬─────────┘
                                                     │
                              JSON: {"kind":"command","command":"start_task",...}
                                                     │
                                                     ▼
                                           ┌───────────────────┐
                                           │  tracker-server   │
                                           │                   │
                                           │  1. 解析命令      │
                                           │  2. 更新 tasks    │
                                           │     map           │
                                           │  3. broadcast     │
                                           │     StateAsync()  │
                                           └───────────────────┘
```

### Scenario B: Claude Stop (Stop Hook)

```
┌──────────────────┐     stdin (JSON)      ┌───────────────────┐
│   Claude Code    │ ───────────────────→  │  Hook Shell 命令   │
│  触发 Stop Hook  │   {                   │                   │
│                  │     "transcript_     │  1. 读取 transcript│
│                  │      path": "..."    │  2. 提取最后消息   │
│                  │   }                   │  3. 获取 tmux 上下文│
└──────────────────┘                       └─────────┬─────────┘
                                                     │
                                        ┌────────────┴────────────┐
                                        │                         │
                                        ▼                         ▼
                              ┌───────────────────┐     ┌───────────────────┐
                              │  tracker-client   │     │    notify.py      │
                              │  finish_task      │     │                   │
                              └─────────┬─────────┘     │  1. 解析参数      │
                                        │               │  2. 获取 tmux 名称 │
                                        │               │  3. terminal-     │
                                        │               │     notifier      │
                                        │               └───────────────────┘
                                        ▼                         │
                              ┌───────────────────┐               │
                              │  tracker-server   │               ▼
                              │                   │     ┌───────────────────┐
                              │  status =         │     │  macOS 通知中心    │
                              │   "completed"     │     │  🔔 显示通知       │
                              └───────────────────┘     └───────────────────┘
```

### Scenario C: Permission Prompt (Notification Hook)

```
┌──────────────────┐     stdin (JSON)      ┌───────────────────┐
│   Claude Code    │ ───────────────────→  │  Hook Shell 命令   │
│  触发 Notification│   {                  │                   │
│  Hook            │     "tool": "Bash"   │  获取 tmux 上下文  │
│                  │   }                   │                   │
└──────────────────┘                       └─────────┬─────────┘
                                                     │
                                                     ▼
                                           ┌───────────────────┐
                                           │  tracker-client   │
                                           │  pause_task       │
                                           └─────────┬─────────┘
                                                     │
                                                     ▼
                                           ┌───────────────────┐
                                           │  tracker-server   │
                                           │                   │
                                           │  status =         │
                                           │   "awaiting_input"│
                                           │                   │
                                           │  ⚠️ 没有通知！     │
                                           └───────────────────┘
```

---

## Current Hook Configuration

**Location:** `~/.config/claude/settings.json`

```
Stop Hook:
  ├─ tracker-client finish_task  ✅
  └─ notify.py                   ✅  ← macOS 通知

Notification Hook (pause):
  └─ tracker-client pause_task   ✅
     (没有 notify.py!)           ❌  ← 缺失通知！
```

---

## TODO: Discord Notification Integration

### Problem

当 Claude 停止或暂停时，需要发送 Discord 通知，但存在架构上的挑战：

| 方式 | Hook + Webhook | Claude Skill |
|------|----------------|--------------|
| **触发者** | 系统自动 | Claude 主动 |
| **触发时机** | Claude 停止**后** | Claude 运行**中** |
| **可靠性** | 100% 自动 | 取决于 Claude 是否记得调用 |
| **信息丰富度** | ❌ 有限 (只有 Hook 输入) | ✅ 丰富 (完整上下文) |

### Proposed Solutions

#### Option A: Hook + Discord Webhook (Simple)

在 `notify.py` 中添加 Discord Webhook 调用：

```python
def send_discord_webhook(title, message, status):
    webhook_url = os.environ.get("DISCORD_WEBHOOK_URL")
    if not webhook_url:
        return

    color = 0x00ff00 if status == "completed" else 0xffa500
    emoji = "✅" if status == "completed" else "⏸️"

    payload = {
        "embeds": [{
            "title": f"{emoji} {title}",
            "description": message[:200],
            "color": color
        }]
    }
    requests.post(webhook_url, json=payload)
```

修改 `settings.json` 的 Hook 配置，在 `permission_prompt` 中添加 `notify.py` 调用。

#### Option B: Tracker-Server Webhook (Centralized)

在 `tracker-server` 中添加状态变更时的 webhook 调用：

```go
func (s *server) finishTask(target tmuxTarget, note string) error {
    // ... existing code ...

    // Send Discord notification
    go s.sendDiscordNotification("completed", target, note)

    return nil
}
```

#### Option C: Hybrid (Recommended)

1. Hook 发送简单通知 (确保不漏)
2. Claude Skill 发送详细通知 (提供上下文)
3. tracker-server 存储上下文，供 Hook 读取

### Required Changes

1. **修改 `notify.py`**
   - 添加 Discord Webhook 支持
   - 支持 `awaiting_input` 事件类型

2. **修改 `settings.json` Hook**
   - 在 `permission_prompt` 中添加 `notify.py` 调用

3. **环境变量**
   - `DISCORD_WEBHOOK_URL`: Discord Webhook URL

---

## Agent Integration

### Claude Code

Claude Code 原生支持通过 `settings.json` 中的 hooks 集成 agent-tracker。

**配置位置:** `~/.config/claude/settings.json`

```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{
        "type": "command",
        "command": "INPUT=$(cat); summary=$(echo \"$INPUT\" | jq -r '.prompt // \"working...\"' | head -c 100); \"$HOME/.config/agent-tracker/scripts/agent-event.sh\" start --agent claude --summary \"$summary\""
      }]
    }],
    "Stop": [{
      "hooks": [{
        "type": "command",
        "command": "INPUT=$(cat); transcript_path=$(echo \"$INPUT\" | jq -r '.transcript_path'); last_message=$(tail -20 \"$transcript_path\" 2>/dev/null | jq -rs '[.[] | select(.type == \"assistant\") | .message.content[] | select(.type == \"text\") | .text] | last // empty' | head -c 200); \"$HOME/.config/agent-tracker/scripts/agent-event.sh\" finish --agent claude --summary \"$last_message\" --transcript \"$transcript_path\""
      }]
    }],
    "Notification": [
      {"matcher": "permission_prompt", "hooks": [{"type": "command", "command": "INPUT=$(cat); tool_name=$(echo \"$INPUT\" | jq -r '.tool // .tool_name // \"确认\"'); \"$HOME/.config/agent-tracker/scripts/agent-event.sh\" pause --agent claude --summary \"$tool_name\""}]},
      {"matcher": "idle_prompt", "hooks": [{"type": "command", "command": "\"$HOME/.config/agent-tracker/scripts/agent-event.sh\" finish --agent claude --summary \"空闲\""}]}
    ]
  }
}
```

**特点:**
- 原生 hooks 系统，无需额外包装脚本
- 项目指令通过 `CLAUDE.md` 自动加载
- 直接运行 `claude` 命令即可

### OpenCode

OpenCode 通过 JavaScript 插件集成 agent-tracker。

**插件位置:** `~/.config/opencode/plugin/tracker-notify.js`

**启动方式:** 通过 `op` 命令（zsh function）启动，而非直接运行 `opencode`

```bash
# 正确方式
op

# 错误方式（插件可能无法加载）
opencode
```

**`op` 命令原理:**
1. 创建临时配置目录 `$TMPDIR/opencode-home.XXXXXX`
2. 复制基础配置到临时目录
3. **符号链接 `plugin/` 目录**（确保 tracker 插件可用）
4. 加载项目级提示词（`.agent-prompts/*.md`）
5. 设置 `OPENCODE_CONFIG_DIR` 并启动 opencode
6. 退出时自动清理临时目录

**相关文件:**
- `~/.config/zsh/functions/op.zsh` - 入口函数
- `~/.config/zsh/functions/_op_common.zsh` - 核心逻辑
- `~/.config/opencode/plugin/tracker-notify.js` - tracker 插件

---

## Components

| Component | Language | Purpose |
|-----------|----------|---------|
| `tracker-server` | Rust | 状态管理，IPC 服务器，持久化 |
| `tracker-client` | Rust | TUI 界面，命令行接口 |
| `notify.py` | Python | macOS 通知 (+ Discord TODO) |
| `agent-event.sh` | Bash | 统一的 agent 事件入口脚本 |

## Data Storage

**全局数据** (`~/.config/agent-tracker/`):

| File | Purpose |
|------|---------|
| `data/tracker.db` | SQLite 主数据库 (tasks, notes, goals, history, closed_windows, projects) |
| `run/latest_notified.txt` | 最近通知的 tmux 目标 |
| `web/dist/` | 前端静态文件 |

**项目级数据** (`<git-root>/.aitracker/`):

| File | Purpose |
|------|---------|
| `tracker.db` | 项目专属 SQLite (history, notes, goals, closed_windows) |
| `.gitignore` | 自动生成，内容为 `*` (不提交到 git) |

双写机制: history、notes、goals、closed_windows 同时写入全局 DB 和项目 DB。
全局 DB 用于跨项目聚合查询，项目 DB 用于项目级过滤和持久化。

---

## Launchd Compatibility (macOS)

### 问题背景

当 `tracker-server` 通过 macOS launchd 服务运行时，调用 tmux 命令可能会失败或返回空结果。

### 已知问题及解决方案

#### 1. tmux Format 字符串转义问题

**问题**: launchd 环境下，tmux format 字符串中的 `\t` (tab) 会被错误解释为下划线 `_`。

```bash
# 预期输出
$0	1-tracker	@0	main	3	1

# 实际输出 (launchd 环境)
$0_1-tracker_@0_main_3_1
```

**解决方案**: 使用 `|` 作为字段分隔符代替 `\t`：

```rust
// 修改前
"-F", "#{session_id}\t#{session_name}\t..."

// 修改后
"-F", "#{session_id}|#{session_name}|..."
```

#### 2. tmux Socket 路径问题

**问题**: launchd 环境下，tmux 无法自动找到 socket 文件，因为 `TMPDIR` 环境变量可能指向不同位置。

**解决方案**: 显式指定 socket 路径：

```rust
// 修改前
Command::new("/opt/homebrew/bin/tmux")
    .args(["list-windows", "-a", ...])

// 修改后
Command::new("/opt/homebrew/bin/tmux")
    .args(["-S", "/private/tmp/tmux-501/default", "list-windows", "-a", ...])
```

**注意**: socket 路径中的 `501` 是用户 UID，不同用户需要修改。可通过 `id -u` 获取当前用户 UID。

#### 3. launchd plist 配置

确保 `~/Library/LaunchAgents/dev.heygo.tracker-server.plist` 包含正确的环境变量：

```xml
<key>EnvironmentVariables</key>
<dict>
    <key>PATH</key>
    <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    <key>TMPDIR</key>
    <string>/tmp</string>
</dict>
```

### 相关文件

| 文件 | 说明 |
|------|------|
| `src/rust/crates/tracker-server/src/agent.rs` | tmux 命令封装，需要使用 `\|` 分隔符和显式 socket 路径 |
| `~/Library/LaunchAgents/dev.heygo.tracker-server.plist` | launchd 服务配置 |

### 调试方法

```bash
# 检查 launchd 服务状态
launchctl list | grep tracker

# 查看服务日志
tail -f ~/.config/agent-tracker/logs/tracker-server.log

# 手动测试 tmux 命令
/opt/homebrew/bin/tmux -S /private/tmp/tmux-501/default list-sessions

# 测试 API 返回
curl -s http://localhost:3099/api/tmux/windows | jq '.windows | length'
```

---

## Roadmap: Web Console Extension

### 目标

扩展 Agent Tracker 为完整的 Web 控制台，支持：
- 公网访问 + 认证
- 多 Session 工位可视化
- 实时状态同步
- 远程布置任务

### 工位可视化概念

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Agent Tracker Web Console                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    Session 1: mediahub (办公区域 1)                  │   │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐               │   │
│  │  │ Window 1 │ │ Window 2 │ │ Window 3 │ │ Window 4 │  ...          │   │
│  │  │  工位 1   │ │  工位 2   │ │  工位 3   │ │  工位 4   │               │   │
│  │  │ 🟢 运行中 │ │ ⏸️ 等待  │ │ ✅ 完成  │ │ 🟢 运行中 │               │   │
│  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    Session 2: dotconfig (办公区域 2)                 │   │
│  │  ┌──────────┐ ┌──────────┐                                         │   │
│  │  │ Window 1 │ │ Window 2 │  ...                                    │   │
│  │  └──────────┘ └──────────┘                                         │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    Session 3: Demo (办公区域 3)                      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    Session 4: douyin-creator (办公区域 4)            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 目标架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              完整架构                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│                              公网用户                                        │
│                                  │                                          │
│                            HTTPS (认证)                                      │
│                                  │                                          │
│                                  ▼                                          │
│  ┌───────────────────────────────────────────────────────────────────────┐ │
│  │                         PocketBase                                     │ │
│  │  ┌─────────────┬─────────────┬─────────────┬─────────────────────┐   │ │
│  │  │ pb_public/  │   认证系统   │  SQLite DB  │   实时订阅          │   │ │
│  │  │ (React App) │  (内置)     │  (内置)     │  (WebSocket-like)  │   │ │
│  │  └─────────────┴─────────────┴─────────────┴─────────────────────┘   │ │
│  │                              │                                        │ │
│  │                    ┌─────────┴─────────┐                              │ │
│  │                    │   Go 扩展 Hooks    │                              │ │
│  │                    │  (自定义 API)      │                              │ │
│  │                    └─────────┬─────────┘                              │ │
│  └──────────────────────────────┼────────────────────────────────────────┘ │
│                                 │                                          │
│              ┌──────────────────┼──────────────────┐                       │
│              │                  │                  │                       │
│              ▼                  ▼                  ▼                       │
│     ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                 │
│     │    tmux     │    │  tracker-   │    │  Claude     │                 │
│     │  send-keys  │    │  server     │    │  Hooks      │                 │
│     │  capture    │    │  (状态同步)  │    │             │                 │
│     └─────────────┘    └─────────────┘    └─────────────┘                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 技术栈选择

**核心原则：Rust 做核心引擎，TypeScript 做 Web 界面**

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              技术栈                                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   【后端 - Rust】                       【前端 - TypeScript】                 │
│   ┌─────────────────────┐             ┌─────────────────────┐              │
│   │  tracker-server     │             │  React Web App      │              │
│   │  (Rust + Axum)      │◄──────────►│  (TypeScript)       │              │
│   │                     │    API      │                     │              │
│   │  - 核心状态管理      │             │  - 工位可视化        │              │
│   │  - HTTP REST API    │             │  - 任务管理          │              │
│   │  - WebSocket        │             │  - 历史查看          │              │
│   │  - SQLite 持久化    │             │  - 布置任务          │              │
│   │  - tmux 交互        │             │                     │              │
│   └─────────────────────┘             └─────────────────────┘              │
│            │                                                               │
│            ▼                                                               │
│   ┌─────────────────────┐                                                  │
│   │  tracker-tui        │                                                  │
│   │  (Rust + Ratatui)   │                                                  │
│   └─────────────────────┘                                                  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

| 层级 | 技术选择 | 说明 |
|------|----------|------|
| **核心引擎** | Rust (Axum) | HTTP/WebSocket 服务器，端口 3099 |
| **数据库** | SQLite (rusqlite) | 本地持久化 data/tracker.db |
| **前端框架** | React + TypeScript | 现代化、类型安全 |
| **UI 样式** | Tailwind CSS | 快速开发、响应式 |
| **TUI** | Rust (Ratatui) | 终端 UI 客户端 |
| 单文件部署 | ✅ 单个二进制 | ❌ 需要 node_modules |

### 与 Clawdbot 对比

[Clawdbot](https://github.com/clawdbot/clawdbot) 是一个类似的项目，使用 TypeScript 实现消息平台到 AI 的桥接。

| 对比项 | Clawdbot | Agent Tracker Web |
|--------|----------|-------------------|
| **核心语言** | TypeScript | Go + TypeScript |
| **定位** | 消息平台 → AI 桥接 | 多工位任务管理 |
| **AI 实例** | 单一 | 多个 (每个工位一个) |
| **任务管理** | 无 | 有 (状态跟踪) |
| **可视化** | 无 | 有 (工位看板) |
| **历史记录** | 基础 | 完整 (SQLite) |

### API 设计

```
GET  /api/sessions              # 获取所有 session (办公区域)
GET  /api/sessions/:id/windows  # 获取 session 下的 windows (工位)
GET  /api/tasks                 # 获取所有任务状态
GET  /api/history               # 获取历史记录

POST /api/task/send             # 布置任务
     {
       "session": "1-mediahub",
       "window": "1",
       "pane": "1",
       "command": "帮我实现 xxx 功能"
     }

GET  /api/pane/capture          # 获取 pane 内容
     ?session=1&window=1&pane=1

WS   /ws                        # WebSocket 实时状态推送
```

### 布置任务实现

通过 tmux send-keys 实现远程布置任务：

```bash
# 获取 pane 内容
tmux capture-pane -t "${session}:${window}.${pane}" -p

# 发送命令到 pane
tmux send-keys -t "${session}:${window}.${pane}" -l "$command"
tmux send-keys -t "${session}:${window}.${pane}" C-m
```

### 实现路线图

- [ ] **Phase 1: API 扩展**
  - [ ] tracker-server 添加 HTTP API
  - [ ] tracker-server 添加 WebSocket 支持
  - [ ] 实现 tmux session/window 列表 API
  - [ ] 实现 pane capture API
  - [ ] 实现 send-keys API

- [ ] **Phase 2: 认证集成**
  - [ ] 集成 PocketBase
  - [ ] 实现用户认证
  - [ ] 实现 API 鉴权

- [ ] **Phase 3: Web 前端**
  - [ ] React + TypeScript + Tailwind 项目搭建
  - [ ] 工位看板 UI
  - [ ] 任务状态实时展示
  - [ ] 布置任务表单
  - [ ] 历史记录查看

- [ ] **Phase 4: 通知集成**
  - [ ] Discord Webhook 通知
  - [ ] 修复 pause 状态通知缺失

---

## Roadmap: Rust Rewrite

### 目标

使用 Rust 重构整个项目，作为学习 Rust 的练手项目，同时获得更好的性能和类型安全。

### 当前代码量

| 组件 | 语言 | 大约行数 |
|------|------|----------|
| tracker-server | Go | ~1000 行 |
| tracker-client (TUI) | Go | ~2500 行 |
| notify.py | Python | ~200 行 |
| Shell 脚本 | Bash | ~500 行 |
| **总计** | | **~4200 行** |

### Rust 技术栈

| 功能 | Rust 库 | 说明 |
|------|---------|------|
| TUI | **ratatui** | 终端 UI 框架 |
| HTTP/WebSocket | **axum** | 高性能 Web 框架 |
| WebSocket Client | **tokio-tungstenite** | TUI 连接服务器 |
| JSON | **serde_json** | 序列化/反序列化 |
| SQLite | **rusqlite** | 本地持久化 |
| 异步运行时 | **tokio** | 异步 I/O |
| HTTP Client | **reqwest** | CLI 命令发送 |

### Go vs Rust 对比

| 对比 | Go | Rust |
|------|-----|------|
| 学习曲线 | 🟢 简单 | 🔴 陡峭 |
| 编译速度 | 🟢 快 | 🔴 慢 |
| 运行性能 | 🟢 好 | 🟢 极好 |
| 内存安全 | 🟡 GC | 🟢 编译时保证 |
| 二进制大小 | 🟡 中等 | 🟢 更小 |
| 开发速度 | 🟢 快 | 🟡 中等 |
| 错误处理 | 🟡 if err != nil | 🟢 Result<T, E> |

### 项目结构

```
agent-tracker/
├── rust/                        # Rust 重构
│   ├── Cargo.toml               # Workspace
│   ├── crates/
│   │   ├── tracker-core/        # 核心逻辑、IPC 协议
│   │   ├── tracker-server/      # 服务端
│   │   ├── tracker-tui/         # TUI 客户端
│   │   └── tracker-web/         # Web API
│   └── README.md
├── web/                         # React 前端
│   ├── src/
│   ├── package.json
│   └── ...
├── cmd/                         # 现有 Go 代码 (保留参考)
├── internal/
└── ...
```

### Rust 学习 + 重构路线图

- [ ] **Phase 0: Rust 基础**
  - [ ] 所有权、借用、生命周期
  - [ ] Result/Option 错误处理
  - [x] struct、impl、trait
  - [x] async/await 基础

- [x] **Phase 1: 核心重构 (tracker-server)** ✅
  - [x] 项目结构搭建 (Cargo workspace)
  - [x] IPC 协议定义 (serde)
  - [x] HTTP/WebSocket 服务端 (axum + tokio)
  - [x] 状态管理 (Arc<Mutex<State>>)
  - [x] 命令处理 (start_task, finish_task, pause_task)
  - [x] 状态广播 (tokio broadcast channel)

- [x] **Phase 2: TUI 客户端 (tracker-tui)** ✅
  - [x] ratatui 基础学习
  - [x] 事件循环
  - [x] 任务列表展示
  - [x] 笔记/目标视图
  - [x] 键盘交互
  - [x] 实时状态更新 (WebSocket)

- [x] **Phase 3: Web 扩展** ✅
  - [x] HTTP API (axum)
  - [x] WebSocket 实时推送
  - [x] tmux 交互封装
  - [ ] 认证中间件
  - [x] React 前端对接

- [x] **Phase 4: 完善** ✅
  - [x] SQLite 历史存储 (rusqlite)
  - [ ] Discord Webhook 通知
  - [x] 配置文件管理
  - [x] 日志系统 (tracing)
  - [x] 错误处理优化 (thiserror/anyhow)

### 推荐学习资源

| 资源 | 说明 |
|------|------|
| [The Rust Book](https://doc.rust-lang.org/book/) | 官方入门书 |
| [Rust by Example](https://doc.rust-lang.org/rust-by-example/) | 示例驱动学习 |
| [ratatui 文档](https://ratatui.rs/) | TUI 框架 |
| [Tokio 教程](https://tokio.rs/tokio/tutorial) | 异步运行时 |
| [Axum 文档](https://docs.rs/axum/latest/axum/) | Web 框架 |
| [Serde 文档](https://serde.rs/) | 序列化框架 |
