# History Feature Design

日期: 2026-01-30

## 概述

为 Agent Tracker 添加对话历史记录功能，让用户退出项目后再次打开时能快速了解之前的对话内容。

## 需求

- **目标用户**: 使用 Claude Code + tmux 的开发者
- **核心场景**: 退出项目后再次打开，快速回顾之前做了什么
- **数据量预估**: 中等 (1000-10000 条)

## 数据模型

### SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS conversations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT NOT NULL,
    session_id TEXT,
    started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    ended_at DATETIME,
    user_prompt TEXT,
    assistant_reply TEXT,
    transcript_path TEXT
);
CREATE INDEX idx_project_path ON conversations(project_path);
CREATE INDEX idx_started_at ON conversations(started_at);
```

### 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| id | INTEGER | 自增主键 |
| project_path | TEXT | 项目目录路径 (如 ~/projects/mediahub) |
| session_id | TEXT | Claude session ID，用于 --resume |
| started_at | DATETIME | 对话开始时间 |
| ended_at | DATETIME | 对话结束时间 |
| user_prompt | TEXT | 用户问题摘要 |
| assistant_reply | TEXT | Claude 回复摘要 |
| transcript_path | TEXT | 完整对话文件路径 |

## 数据采集

复用现有 Claude Hooks:

### UserPromptSubmit Hook

```bash
# 现有调用
tracker-client command ... start_task

# 新增调用
tracker-client command -project "$PWD" -prompt "$prompt" history_start
```

### Stop Hook

```bash
# 现有调用
tracker-client command ... finish_task

# 新增调用
tracker-client command -project "$PWD" -reply "$last_message" -transcript "$transcript_path" history_end
```

## 新增命令

### tracker-server

| 命令 | 参数 | 说明 |
|------|------|------|
| history_start | project, prompt | 创建历史记录 |
| history_end | project, reply, transcript | 更新历史记录 |
| history_query | project, limit, offset, search | 查询历史 |

### tracker-client

| 命令 | 说明 |
|------|------|
| history | 查询并显示历史记录 |

## 用户界面

在 tracker-client TUI 中新增 History 视图:

```
┌─ History ─ ~/projects/mediahub ──────────────────┐
│                                                   │
│  Today                                            │
│  ├─ 22:54  start-workspace 脚本布局修改          │
│  │         → backend/frontend 改为左右并排        │
│  ├─ 22:30  tmux 鼠标拖拽复制配置                 │
│  │         → 配置完成，支持自动同步剪贴板         │
│  │                                                │
│  Yesterday                                        │
│  ├─ 15:20  Agent Tracker awaiting_input 状态     │
│  │         → 添加 🚧 图标，Hooks 配置完成         │
│  └─ 14:00  Discord 通知功能讨论                  │
│            → 待实现                               │
│                                                   │
│  [Enter] 查看详情  [r] 恢复对话  [/] 搜索        │
└───────────────────────────────────────────────────┘
```

### 快捷键

| 键 | 功能 |
|----|------|
| h | 切换到 History 视图 |
| Enter | 展开查看完整摘要 |
| r | 调用 claude --resume 恢复对话 |
| / | 搜索历史记录 |

### 时间分组

- Today
- Yesterday
- This Week
- Earlier

## 文件结构

```
~/.config/agent-tracker/
├── cmd/tracker-server/main.go   # 修改: 新增 SQLite + history 命令
├── cmd/tracker-client/main.go   # 修改: 新增 History 视图
├── internal/history/
│   └── db.go                    # 新增: SQLite 操作封装
└── data/
    └── history.db               # 新增: SQLite 数据库文件
```

## 技术选型

- **语言**: Go (与现有系统一致)
- **SQLite 库**: modernc.org/sqlite (纯 Go，无需 CGO)
- **TUI**: 复用现有 tcell

## 实现步骤

1. 添加 SQLite 依赖 - `go get modernc.org/sqlite`
2. 创建 internal/history/db.go - 数据库初始化、CRUD 操作
3. 修改 tracker-server - 添加 history_start、history_end、history_query 命令
4. 修改 tracker-client - 添加 History 视图
5. 修改 Claude Hooks - 在现有 hooks 中追加 history 调用
6. 测试和调试
