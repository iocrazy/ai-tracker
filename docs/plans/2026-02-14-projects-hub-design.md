# Projects Hub — 完整设计文档

> ai-tracker 项目注册中心：统一管理所有项目、session、环境变量、worktree，
> 参考 Vercel Dashboard 模式，分 4 阶段实现。

## 目标

当前痛点：session 通过 tmux 轮询自动发现，没有一个总入口来浏览、启动、恢复项目。
环境变量只有 Project 层，缺少 Global 和 Worktree 层级。
没有项目级的统计、活动流、健康监控。

目标：打造一个项目管理中心，从一个入口管理所有项目的全生命周期。

---

## Phase 1: 基座（Projects Tab + 数据层）

### 1.1 DB 迁移框架

在服务器启动时自动执行 schema 迁移，保证升级不丢数据。

```rust
// src/rust/crates/tracker-server/src/main.rs
// 新增 migrations 模块

const MIGRATIONS: &[(&str, &str)] = &[
    ("001", "CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER PRIMARY KEY,
        applied_at TEXT DEFAULT (datetime('now'))
    )"),
    ("002", "CREATE TABLE IF NOT EXISTS global_env_vars (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        key TEXT NOT NULL UNIQUE,
        value TEXT NOT NULL,
        is_secret INTEGER DEFAULT 0,
        sort_order INTEGER DEFAULT 0,
        created_at TEXT DEFAULT (datetime('now')),
        updated_at TEXT DEFAULT (datetime('now'))
    )"),
    ("003", "CREATE TABLE IF NOT EXISTS worktree_env_vars (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_name TEXT NOT NULL,
        slot INTEGER NOT NULL,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        is_secret INTEGER DEFAULT 0,
        sort_order INTEGER DEFAULT 0,
        created_at TEXT DEFAULT (datetime('now')),
        updated_at TEXT DEFAULT (datetime('now')),
        UNIQUE(session_name, slot, key)
    )"),
    ("004", "ALTER TABLE projects ADD COLUMN description TEXT DEFAULT ''"),
    ("005", "ALTER TABLE projects ADD COLUMN status TEXT DEFAULT 'active'"),
    ("006", "ALTER TABLE projects ADD COLUMN tags TEXT DEFAULT ''"),
    ("007", "ALTER TABLE projects ADD COLUMN created_at TEXT DEFAULT ''"),
    ("008", "ALTER TABLE projects ADD COLUMN scan_paths TEXT DEFAULT ''"),
];

// 启动时：
// 1. 创建 schema_version 表（如果不存在）
// 2. 查询已执行的最高 version
// 3. 按序执行未执行的 migration
// 4. 记录到 schema_version
```

### 1.2 项目注册机制

三种注册方式：

**方式一：首次使用自动注册**
当 tmux session 被检测到并关联到 git_dir 时，自动注册到 projects 表。
（现有逻辑已部分实现，需确保每次 resolve_git_dir 后 upsert projects 表）

**方式二：目录扫描**
在 Settings 页面配置扫描根目录（如 ~/projects, /Volumes/program/project-code/repos）。
服务器启动时 + 手动触发时扫描这些目录，找到包含 .git 的子目录并注册。

```
Settings → SCAN PATHS:
  /Volumes/program/project-code/repos
  ~/projects
  [+ ADD PATH]        [SCAN NOW]
```

存储到 projects.scan_paths 或独立的 scan_paths 表。

**方式三：API/CLI 注册**

```
POST /api/projects/register
{ "git_dir": "/path/to/project", "name": "my-project" }
```

### 1.3 Projects 顶级 Tab

在 Workstations / Timeline / Console / Settings 之间新增 PROJECTS tab。

**项目列表视图（默认）：**

```
┌──────────────────────────────────────────────────────────┐
│  PROJECTS (12)          🔍 [search...]   [+ ADD PROJECT] │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  ┌─ ai-tracker ───────────────── ● ACTIVE (2 windows) ─┐│
│  │  /Volumes/program/project-code/repos/ai-tracker      ││
│  │  Last: 3 min ago  │  Session: 1-tracker              ││
│  │  [OPEN]  [SETTINGS]                                  ││
│  └──────────────────────────────────────────────────────┘│
│                                                          │
│  ┌─ mediahub ─────────────────── ● ACTIVE (5 windows) ─┐│
│  │  /Volumes/program/project-code/repos/mediahub        ││
│  │  Last: 10 min ago │  Session: 2-mediahub             ││
│  │  [OPEN]  [SETTINGS]                                  ││
│  └──────────────────────────────────────────────────────┘│
│                                                          │
│  ┌─ my-blog ──────────────────── ○ INACTIVE ────────────┐│
│  │  ~/projects/my-blog                                   ││
│  │  Last: 3 days ago │  History: 42 tasks                ││
│  │  [START SESSION]  [SETTINGS]                          ││
│  └──────────────────────────────────────────────────────┘│
│                                                          │
└──────────────────────────────────────────────────────────┘
```

**项目卡片字段：**
- 项目名（projects.name）
- 状态：ACTIVE（有活跃 tmux session 关联）/ INACTIVE
- 路径（projects.git_dir）
- 最后活跃时间（projects.last_active_at）
- 关联 session name + window 数量
- History 任务数（projects.history_count）

**操作按钮：**
- ACTIVE 项目 → `OPEN`（切换到 Workstations 视图并高亮该 session）
- INACTIVE 项目 → `START SESSION`（创建 tmux session + cd 到项目目录）
- `SETTINGS` → 打开 ProjectSettings modal

**ADD PROJECT 对话框：**
- 输入项目路径（文本框）
- 自动检测 .git 目录和项目名
- 可选：自定义项目名

**搜索：** 按项目名和路径搜索，实时过滤。

### 1.4 项目详情视图

点击项目卡片进入详情。顶部面包屑导航：`PROJECTS > ai-tracker`

```
┌──────────────────────────────────────────────────────────┐
│  ← BACK    PROJECT: ai-tracker    ● ACTIVE    [OPEN]    │
│  /Volumes/program/project-code/repos/ai-tracker          │
├──────────────────────────────────────────────────────────┤
│  OVERVIEW  │  ENV VARS  │  WORKTREES  │  STATISTICS      │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  （子 tab 内容见下方各 section）                           │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

**顶栏信息：**
- 项目名 + 路径
- 状态 badge（● ACTIVE / ○ INACTIVE）
- 操作按钮（OPEN / START SESSION）

### 1.5 三层环境变量

**数据模型：**

```
Global (global_env_vars)
  ↓ 继承
Project (project_env_vars, keyed by session_name)
  ↓ 继承
Worktree (worktree_env_vars, keyed by session_name + slot)
```

下层同名 key 覆盖上层。

**新增 API：**

```
# Global env vars
GET    /api/global/env-vars
POST   /api/global/env-vars         { key, value, is_secret }
PUT    /api/global/env-vars/:id     { key?, value?, is_secret? }
DELETE /api/global/env-vars/:id

# Worktree env vars
GET    /api/project/worktree-env-vars?session_name=X&slot=N
POST   /api/project/worktree-env-vars   { session_name, slot, key, value, is_secret }
PUT    /api/project/worktree-env-vars/:id  { key?, value?, is_secret? }
DELETE /api/project/worktree-env-vars/:id

# Merged/effective env vars (read-only)
GET    /api/project/effective-env-vars?session_name=X&slot=N
→ Returns merged list with source annotation: { key, value, is_secret, source: "global"|"project"|"worktree" }
```

**ENV VARS 子 tab UI：**

```
┌─ ENV VARS ──────────────────────────────────────────────┐
│                                                          │
│  SCOPE:  [● EFFECTIVE]  [GLOBAL]  [PROJECT]  [WORKTREE▾] │
│                                                          │
│  ─── EFFECTIVE view (merged, read-only) ───              │
│  NAME              VALUE                  SOURCE         │
│  ANTHROPIC_KEY     sk-ant-••••••         🌐 GLOBAL       │
│  SUPABASE_URL      http://127...54321    📁 PROJECT      │
│  FRONTEND_PORT     5177                  🌿 WORKTREE #2  │
│  REDIS_DB          2                     🌿 WORKTREE #2  │
│  SECRET_KEY        ••••••••              📁 PROJECT      │
│                                                          │
│  ─── 切换到 GLOBAL/PROJECT/WORKTREE scope 显示 CRUD ──  │
│  （复用 ProjectSettings 的表格 + 添加行 UI）              │
│                                                          │
│  WORKTREE scope 时显示 slot 下拉选择器                    │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### 1.6 Session 创建/恢复

从 Projects 视图点击 `START SESSION`：

**后端 API：**

```
POST /api/sessions/create
{
  "project_name": "my-blog",
  "git_dir": "/path/to/my-blog",
  "session_name": "3-myblog"  // 可选，自动生成
}
```

**后端执行：**
1. 生成 session_name（如果未指定）：`{next_id}-{project_name}`
2. 执行 `tmux new-session -d -s {session_name} -c {git_dir}`
3. 更新 projects 表的 last_session
4. 返回 session 信息
5. 前端跳转到 Workstations 视图，新 session 会通过 tmux 轮询自动出现
6. 用户通过 ADD WINDOW 按钮添加第一个窗口（复用现有流程）

**恢复已有 session：**
如果项目已有活跃 tmux session，`OPEN` 按钮跳转到 Workstations 视图并滚动到对应 session。

---

## Phase 2: 可视化（Dashboard + Statistics）

### 2.1 OVERVIEW 子 tab — Activity Feed

项目级活动流，聚合该项目所有 session/window 的事件。

```
┌─ OVERVIEW ──────────────────────────────────────────────┐
│                                                          │
│  ┌─ QUICK INFO ────────────────────────────────────────┐ │
│  │  Sessions: 1-tracker (2 windows)                    │ │
│  │  Worktrees: 3 active  │  Total tasks: 156           │ │
│  │  Last 24h: 15 tasks completed                       │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌─ RECENT ACTIVITY ──────────────────────────────────┐  │
│  │                                                     │  │
│  │  3 min ago                                          │  │
│  │  ✓ Task completed: "polish ProjectSettings UI"      │  │
│  │    window: main  │  duration: 4m 32s                │  │
│  │                                                     │  │
│  │  15 min ago                                         │  │
│  │  📝 Commit: feat: add search feature (a1b2c3d)     │  │
│  │    branch: main  │  +45 -12 (3 files)               │  │
│  │                                                     │  │
│  │  1 hour ago                                         │  │
│  │  📌 Note added: "需要重构 auth 模块"                 │  │
│  │    scope: session                                   │  │
│  │                                                     │  │
│  │  2 hours ago                                        │  │
│  │  🎯 Goal completed: "v2.0 release"                  │  │
│  │                                                     │  │
│  │  [LOAD MORE...]                                     │  │
│  └─────────────────────────────────────────────────────┘  │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

**数据来源：**
- `history` 表 → 已完成的 tasks（按 session_id 过滤）
- `commits` 表 → git commits（关联 history_id）
- `notes` 表 → 笔记（按 session_id 过滤）
- `goals` 表 → 目标（按 session_id 过滤）

**API：**

```
GET /api/projects/:git_dir/activity?limit=20&offset=0
→ Returns unified activity feed sorted by timestamp desc
```

### 2.2 WORKTREES 子 tab — Worktree Dashboard

类似 Vercel 的 Active Branches / Deployments 列表。

```
┌─ WORKTREES ─────────────────────────────────────────────┐
│                                                          │
│  ┌─ SLOT #0  main ────────────────── ● ACTIVE ─────────┐│
│  │  Window: master                    3 min ago         ││
│  │                                                      ││
│  │  FRONTEND_PORT  5175    BACKEND_PORT  8080           ││
│  │  REDIS_DB       0                                    ││
│  │                                                      ││
│  │  Recent:                                             ││
│  │  • "polish ProjectSettings UI" (4m 32s)              ││
│  │  • "add search feature" (12m 05s)                    ││
│  │                                                      ││
│  │  /Volumes/program/project-code/repos/ai-tracker      ││
│  │                                 [ENV VARS] [FREE]    ││
│  └──────────────────────────────────────────────────────┘│
│                                                          │
│  ┌─ SLOT #1  feature/payment ─────── ● ACTIVE ─────────┐│
│  │  Window: feature-payment           10 min ago        ││
│  │                                                      ││
│  │  FRONTEND_PORT  5176    BACKEND_PORT  8081           ││
│  │  REDIS_DB       1                                    ││
│  │                                                      ││
│  │  Recent:                                             ││
│  │  • "implement checkout flow" (25m 10s)               ││
│  │                                                      ││
│  │  /Volumes/.../ai-tracker-feature-payment             ││
│  │                                 [ENV VARS] [FREE]    ││
│  └──────────────────────────────────────────────────────┘│
│                                                          │
│  ┌─ SLOT #2  fix/auth-bug ────────── ○ INACTIVE ───────┐│
│  │  No active window                  2 days ago        ││
│  │                                                      ││
│  │  FRONTEND_PORT  5177    BACKEND_PORT  8082           ││
│  │                                                      ││
│  │  /Volumes/.../ai-tracker-fix-auth-bug                ││
│  │                          [RESUME] [ENV VARS] [FREE]  ││
│  └──────────────────────────────────────────────────────┘│
│                                                          │
└──────────────────────────────────────────────────────────┘
```

**每个 worktree 卡片：**
- Slot 编号 + branch 名 + 状态
- 关联的 tmux window 名 + 最后活跃时间
- 计算后的端口分配（base_value + slot）
- 最近 2-3 条 task 摘要
- Worktree 路径
- 操作：ENV VARS（编辑该 worktree 层变量）、FREE（释放 slot）、RESUME（创建窗口恢复 inactive worktree）

**数据聚合 API：**

```
GET /api/projects/:session_name/worktree-dashboard
→ Returns worktree_slots + computed ports + recent tasks + tmux window status
```

### 2.3 STATISTICS 子 tab

```
┌─ STATISTICS ────────────────────────────────────────────┐
│                                                          │
│  TIME RANGE: [24h] [7d] [30d] [All]                     │
│                                                          │
│  ┌─ TASKS ───────────┐  ┌─ AGENT TIME ────────────────┐ │
│  │  Completed: 23     │  │  Total: 4h 32m              │ │
│  │  ▓▓▓▓▓▓▓▓░░  87%  │  │  ████████░░                 │ │
│  │  In Progress: 2    │  │  BUSY:  3h 15m              │ │
│  │  Failed: 1         │  │  IDLE:  1h 17m              │ │
│  └────────────────────┘  └─────────────────────────────┘ │
│                                                          │
│  ┌─ TOP TOOLS ───────┐  ┌─ ACTIVITY CHART ────────────┐ │
│  │  Read      ████ 45 │  │  ┃        ▂▄█▆▃▁▂▅██▆▃     │ │
│  │  Edit      ███  38 │  │  ┃                          │ │
│  │  Bash      ██   22 │  │  ┗━━━━━━━━━━━━━━━━━━━━━━━━ │ │
│  │  Grep      █    15 │  │    6h ago           now     │ │
│  │  WebFetch  ░     4 │  │                             │ │
│  └────────────────────┘  └─────────────────────────────┘ │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

**4 个统计卡片：**
1. **TASKS** — 完成/进行中/失败数量 + 完成率进度条
2. **AGENT TIME** — 总活跃时长，BUSY vs IDLE 占比
3. **TOP TOOLS** — 最常用工具 top 5（水平条形图）
4. **ACTIVITY CHART** — 时间轴 sparkline（每小时 task 数量）

**时间范围选择器：** 24h / 7d / 30d / All

**数据来源：**
- `history` 表 → task 统计（duration, status）
- `tool_usage` 表 → 工具使用统计
- `tasks` 表 → 当前进行中的 task

**API：**

```
GET /api/projects/:session_name/statistics?range=24h
→ Returns { tasks: {...}, agent_time: {...}, top_tools: [...], activity: [...] }
```

### 2.4 Git 集成看板

在 OVERVIEW 或独立子区域展示 Git 信息。
通过在后端执行 git CLI 命令获取数据（不依赖 GitHub API）。

**展示内容：**
- 当前分支列表（git branch -a）
- 每个分支的最后 commit
- 未推送的 commits 数量
- 工作区状态（clean / dirty / conflicts）

**API：**

```
GET /api/projects/:git_dir/git-info
→ Runs git commands in project dir, returns:
{
  current_branch: "main",
  branches: [
    { name: "main", last_commit: "abc123", message: "feat: ...", ahead: 0, behind: 0 },
    { name: "feature/payment", last_commit: "def456", message: "wip: ...", ahead: 3, behind: 1 }
  ],
  status: { modified: 2, untracked: 1, conflicts: 0 }
}
```

---

## Phase 3: 效率（Command Palette + Templates）

### 3.1 Command Palette (Cmd+K)

全局快捷键 Cmd+K 打开命令面板，提供统一的搜索和操作入口。

```
┌──────────────────────────────────────────────────────┐
│  🔍 Type a command or search...                      │
├──────────────────────────────────────────────────────┤
│                                                      │
│  RECENT                                              │
│  → ai-tracker          ● ACTIVE    Open project      │
│  → mediahub            ● ACTIVE    Open project      │
│                                                      │
│  PROJECTS                                            │
│  → my-blog             ○ INACTIVE  Start session     │
│  → api-gateway         ○ INACTIVE  Start session     │
│                                                      │
│  ACTIONS                                             │
│  → Add new project...                                │
│  → Scan for projects...                              │
│  → Open settings...                                  │
│                                                      │
│  SEARCH HISTORY                                      │
│  → "payment integration" (3 results)                 │
│  → "auth bug" (1 result)                             │
│                                                      │
└──────────────────────────────────────────────────────┘
```

**功能分类：**
1. **项目搜索** — 按名称/路径模糊搜索项目，回车跳转
2. **Session 操作** — 打开/创建/停止 session
3. **全局搜索** — 搜索 task 历史、notes、goals（跨项目）
4. **导航** — 跳转到任意 tab / 视图
5. **快捷操作** — 扫描项目、打开设置等

**实现要点：**
- 全局 Cmd+K 监听（useEffect on keydown）
- 模糊搜索算法（fuse.js 或简单 includes 匹配）
- 分组展示结果（Recent / Projects / Actions / Search）
- 键盘导航（↑↓选择，Enter 执行，Esc 关闭）

### 3.2 项目模板

预定义常用项目类型的 services + env vars 配置。

**内置模板：**

```yaml
Next.js:
  services:
    - { name: "frontend", base_value: 3000, type: "port", env_key: "PORT" }
    - { name: "api", base_value: 3001, type: "port", env_key: "API_PORT" }
  env_vars:
    - { key: "NODE_ENV", value: "development" }

Rust + React (Vite):
  services:
    - { name: "frontend", base_value: 5173, type: "port", env_key: "FRONTEND_PORT" }
    - { name: "backend", base_value: 8080, type: "port", env_key: "BACKEND_PORT" }
  env_vars:
    - { key: "RUST_LOG", value: "info" }

Full Stack (Supabase):
  services:
    - { name: "frontend", base_value: 5173, type: "port", env_key: "FRONTEND_PORT" }
    - { name: "backend", base_value: 8080, type: "port", env_key: "BACKEND_PORT" }
    - { name: "supabase", base_value: 54321, type: "port", env_key: "SUPABASE_PORT" }
    - { name: "redis", base_value: 6379, type: "port", env_key: "REDIS_PORT" }
    - { name: "redis_db", base_value: 0, type: "db_index", env_key: "REDIS_DB" }
  env_vars:
    - { key: "SUPABASE_URL", value: "http://127.0.0.1:54321" }
```

**UI 流程：**
项目注册时或 SETTINGS 中可选择模板，一键填充 services 和 env vars。

**存储：**
模板定义为 JSON，存储在 `~/.config/agent-tracker/templates/` 或硬编码在前端。
支持自定义模板：从当前项目配置导出为模板。

### 3.3 键盘快捷键

```
Cmd+K          打开 Command Palette
Cmd+1/2/3/4/5  切换顶级 Tab (Workstations/Projects/Timeline/Console/Settings)
Cmd+N          新建窗口 (在当前 session)
Escape         关闭当前 modal / palette
/              聚焦搜索框 (在列表视图中)
j/k            上下导航列表项
Enter          打开选中项
```

---

## Phase 4: 可靠性（Health + Logs + Backup）

### 4.1 健康检查 + 自动恢复

**监控对象：**
1. **tmux server** — `tmux list-sessions` 是否正常响应
2. **tracker-server 进程** — 自检 API `/api/health`
3. **SQLite DB** — `PRAGMA integrity_check`
4. **WebSocket 连接** — 前端连接状态

**健康状态 API：**

```
GET /api/health
→ {
    status: "healthy" | "degraded" | "unhealthy",
    checks: {
      tmux: { status: "ok", sessions: 3 },
      database: { status: "ok", size: "24MB" },
      disk: { status: "ok", free: "50GB" },
      uptime: "3d 4h 12m"
    }
  }
```

**前端展示：**
- Header 栏的 ONLINE/OFFLINE 指示器增强：显示健康评分
- 异常时弹出告警 banner："tmux server not responding"
- 提供 "RECOVER" 按钮执行恢复操作

**自动恢复策略：**
- tmux session 消失 → 从 closed_windows 表恢复（已有数据）
- DB 锁定 → 等待重试 + 超时告警
- WebSocket 断开 → 自动重连（见 4.2）

### 4.2 WebSocket 增强

**当前问题：**
- 断线后需要手动刷新
- 无离线缓存

**改进：**

```typescript
// 断线重连策略
class ReconnectingWebSocket {
  private retryCount = 0;
  private maxRetries = Infinity;
  private baseDelay = 1000;  // 1s
  private maxDelay = 30000;  // 30s

  reconnect() {
    const delay = Math.min(
      this.baseDelay * Math.pow(2, this.retryCount),
      this.maxDelay
    );
    // 指数退避 + jitter
    setTimeout(() => this.connect(), delay + Math.random() * 1000);
    this.retryCount++;
  }
}
```

**离线缓存：**
- 用 localStorage 缓存最近的项目列表和 session 状态
- 离线时显示缓存数据 + "OFFLINE — showing cached data" banner
- 重新连接后自动刷新

**连接状态 UI：**
```
● ONLINE           正常
◐ RECONNECTING...  断线重连中（显示重试次数）
○ OFFLINE          离线（显示缓存数据）
```

### 4.3 结构化日志 + Log Viewer

**后端结构化日志：**

```rust
// 使用 tracing crate（如果尚未使用）
// 输出 JSON 格式日志到文件

tracing::info!(
    session_name = %session_name,
    window_id = %window_id,
    event = "task_completed",
    duration_ms = duration,
    "Task completed"
);
```

**日志存储：**
- 写入 `~/.config/agent-tracker/logs/tracker-YYYY-MM-DD.log`
- 按天轮转，保留 30 天
- JSON 格式，方便查询

**Web UI Log Viewer：**

```
┌─ LOGS ──────────────────────────────────────────────────┐
│  LEVEL: [ALL] [INFO] [WARN] [ERROR]                     │
│  SESSION: [All sessions ▾]                               │
│  🔍 [filter...]                                          │
├──────────────────────────────────────────────────────────┤
│  12:34:56 INFO  [1-tracker] Task completed (4m 32s)     │
│  12:34:55 INFO  [1-tracker] Tool: Edit main.rs          │
│  12:30:22 WARN  [2-mediahub] WebSocket reconnect #3     │
│  12:28:01 ERROR [2-mediahub] tmux window not found      │
│  12:25:00 INFO  [system] Health check: all OK           │
│  ...                                                     │
│                                                          │
│  [LOAD MORE]  │  Auto-scroll: [ON]                      │
└──────────────────────────────────────────────────────────┘
```

**API：**

```
GET /api/logs?level=error&session=1-tracker&limit=100&offset=0
→ Returns parsed log entries
```

Log Viewer 可以作为 Console tab 的增强，或在 Settings 中作为子页面。

### 4.4 通知 + 告警系统

**告警规则（可配置）：**

```yaml
rules:
  - name: "task_stuck"
    condition: "task.status == 'active' && task.duration > 30m"
    action: "notify"
    message: "Task stuck for over 30 minutes"

  - name: "agent_error"
    condition: "task.status == 'error'"
    action: "notify"
    message: "Agent encountered an error"

  - name: "session_offline"
    condition: "session.status changed to 'offline'"
    action: "notify"
    message: "Session went offline"
```

**通知渠道：**
1. **浏览器通知** — Web Notification API（需用户授权）
2. **Discord** — 通过现有 Discord MCP 工具发送
3. **Web UI 内** — toast 提示 + 右上角通知铃铛

**实现：**
- 后端：定时检查告警规则（每 30s），触发时写入 notifications 表
- 前端：通过 WebSocket 推送通知，显示 toast
- 通知历史：可查看和清除

**新增表：**

```sql
CREATE TABLE notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,        -- 'task_stuck', 'agent_error', 'session_offline'
    session_name TEXT,
    message TEXT NOT NULL,
    read INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE alert_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    condition_type TEXT NOT NULL,  -- 'task_stuck', 'agent_error', 'session_offline'
    threshold_seconds INTEGER,     -- for time-based conditions
    enabled INTEGER DEFAULT 1,
    channels TEXT DEFAULT 'web',   -- 'web', 'discord', 'web,discord'
    created_at TEXT DEFAULT (datetime('now'))
);
```

### 4.5 自动备份 + 导出

**自动备份：**
- 每天自动备份 tracker.db → `~/.config/agent-tracker/backups/tracker-YYYY-MM-DD.db`
- 保留最近 30 天的备份
- 服务器启动时检查是否需要备份

**手动导出：**

```
GET /api/backup/export?project=ai-tracker
→ 下载 ZIP 包含：
  - project_env_vars.json
  - project_services.json
  - worktree_slots.json
  - history.json (最近 1000 条)
  - notes.json
  - goals.json
```

**导入：**

```
POST /api/backup/import
→ 上传 ZIP，合并到当前数据库
```

---

## 新增 DB 表汇总

```sql
-- Phase 1
CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE global_env_vars (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL,
    is_secret INTEGER DEFAULT 0,
    sort_order INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE worktree_env_vars (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_name TEXT NOT NULL,
    slot INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    is_secret INTEGER DEFAULT 0,
    sort_order INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now')),
    UNIQUE(session_name, slot, key)
);

-- ALTER TABLE projects ADD COLUMN description TEXT DEFAULT '';
-- ALTER TABLE projects ADD COLUMN status TEXT DEFAULT 'active';
-- ALTER TABLE projects ADD COLUMN tags TEXT DEFAULT '';
-- ALTER TABLE projects ADD COLUMN created_at TEXT DEFAULT '';
-- ALTER TABLE projects ADD COLUMN scan_paths TEXT DEFAULT '';

-- Phase 4
CREATE TABLE notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    session_name TEXT,
    message TEXT NOT NULL,
    read INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE alert_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    condition_type TEXT NOT NULL,
    threshold_seconds INTEGER,
    enabled INTEGER DEFAULT 1,
    channels TEXT DEFAULT 'web',
    created_at TEXT DEFAULT (datetime('now'))
);
```

## 新增 API 端点汇总

```
# Phase 1 — Projects
GET    /api/projects                          列出所有注册项目
POST   /api/projects/register                 注册新项目
POST   /api/projects/scan                     扫描目录注册项目
DELETE /api/projects/:git_dir                 删除项目注册

# Phase 1 — Global env vars
GET    /api/global/env-vars
POST   /api/global/env-vars
PUT    /api/global/env-vars/:id
DELETE /api/global/env-vars/:id

# Phase 1 — Worktree env vars
GET    /api/project/worktree-env-vars?session_name=X&slot=N
POST   /api/project/worktree-env-vars
PUT    /api/project/worktree-env-vars/:id
DELETE /api/project/worktree-env-vars/:id

# Phase 1 — Effective env vars (merged)
GET    /api/project/effective-env-vars?session_name=X&slot=N

# Phase 1 — Session management
POST   /api/sessions/create

# Phase 2 — Activity & Stats
GET    /api/projects/:session_name/activity?limit=20
GET    /api/projects/:session_name/worktree-dashboard
GET    /api/projects/:session_name/statistics?range=24h
GET    /api/projects/:git_dir/git-info

# Phase 4 — Health & Logs
GET    /api/health
GET    /api/logs?level=error&session=X&limit=100
GET    /api/notifications?unread=true
PUT    /api/notifications/:id/read
GET    /api/backup/export?project=X
POST   /api/backup/import
```

## 前端新增文件预估

```
web/src/components/
  ProjectsView.tsx          — 项目列表视图（Phase 1）
  ProjectDetail.tsx         — 项目详情视图（Phase 1）
  ProjectEnvVarsTab.tsx     — 三层环境变量 tab（Phase 1）
  ProjectOverviewTab.tsx    — Activity Feed + Quick Info（Phase 2）
  WorktreeDashboard.tsx     — Worktree 卡片视图（Phase 2）
  ProjectStatistics.tsx     — 统计面板 4 卡片（Phase 2）
  CommandPalette.tsx        — Cmd+K 命令面板（Phase 3）
  ProjectTemplates.tsx      — 模板选择器（Phase 3）
  LogViewer.tsx             — 日志查看器（Phase 4）
  NotificationCenter.tsx    — 通知中心（Phase 4）
  HealthStatus.tsx          — 健康状态指示器（Phase 4）

web/src/services/
  api.ts                    — 新增 API 函数（扩展现有文件）
```

## 实施顺序

Phase 1 内部建议顺序：
1. DB 迁移框架（后续所有表都依赖它）
2. global_env_vars + worktree_env_vars 表和 API
3. projects 表扩展 + 注册 API
4. ProjectsView.tsx（项目列表）
5. ProjectDetail.tsx + ENV VARS tab
6. Session 创建 API + UI
7. 项目扫描功能

Phase 2-4 可根据实际使用需求灵活调整。
