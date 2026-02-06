# 归档与历史功能增强设计

> 日期: 2026-02-03
> 状态: Draft

## 概述

增强 Agent Tracker 的归档和历史功能：
1. 移除 30 分钟自动超时
2. 任务/笔记归档与恢复（软删除模式）
3. 历史功能增强（分组、搜索、统计、对话恢复）
4. TUI 和 Web 同步实现

## 已知问题

- **任务计时存在问题** - 开发时需检查修复

## 数据模型变更

### 移除 30 分钟超时

```go
// tracker-server/main.go - 删除自动超时逻辑
// 原: if task.status == "in_progress" && time.Since(task.started_at) > 30*time.Minute
// 改: 完全移除，任务只能手动归档或完成
```

### Task 生命周期

```
  start_task     finish_task      archive_task
      │               │                │
      ▼               ▼                ▼
  ┌────────┐    ┌───────────┐    ┌──────────┐
  │running │───►│ completed │───►│ archived │
  └────────┘    └───────────┘    └──────────┘
      │                               │
      │         archive_task          │   restore_task
      └──────────────────────────────►│◄─────────────────
```

### 新增 IPC 命令

```rust
// tracker-core/src/ipc.rs
pub const TASK_ARCHIVE: &str = "task_archive";     // 任务归档
pub const TASK_RESTORE: &str = "task_restore";     // 任务恢复
pub const NOTE_RESTORE: &str = "note_restore";     // 笔记恢复
pub const HISTORY_QUERY: &str = "history_query";   // 历史查询
pub const HISTORY_STATS: &str = "history_stats";   // 历史统计
```

## REST API 设计

### 归档/恢复

```
POST /api/task/archive      { "task_id": "xxx" }
POST /api/task/restore      { "history_id": 123 }
POST /api/note/archive      { "note_id": "xxx" }
POST /api/note/restore      { "note_id": "xxx" }
```

### 历史查询

```
GET  /api/history?limit=50&offset=0&search=keyword&session=xxx&date=2026-02-03
GET  /api/history/:id       # 单条详情 + 对话消息
GET  /api/history/stats     # 统计数据
```

### 对话恢复

```
POST /api/history/:id/resume   # 返回 claude --resume 命令或直接执行
```

### 响应格式

#### 历史查询响应（分组）

```json
{
  "groups": [
    {
      "label": "Today",
      "records": [
        {
          "id": 123,
          "session": "tracker",
          "window": "fix:auth",
          "summary": "Fix authentication bug...",
          "completion_note": "Fixed the issue...",
          "duration_seconds": 342,
          "started_at": "2026-02-03T10:30:00Z",
          "message_count": 8
        }
      ]
    },
    { "label": "Yesterday", "records": [...] },
    { "label": "This Week", "records": [...] },
    { "label": "Earlier", "records": [...] }
  ],
  "total": 156
}
```

#### 统计响应

```json
{
  "total_tasks": 156,
  "total_duration_hours": 42.5,
  "today": { "count": 5, "duration_hours": 2.3 },
  "this_week": { "count": 23, "duration_hours": 15.2 },
  "by_session": [
    { "session": "tracker", "count": 45 },
    { "session": "mediahub", "count": 32 }
  ]
}
```

## TUI 界面设计

### 新增快捷键

| Tab | 快捷键 | 功能 |
|-----|--------|------|
| Tasks | `a` | 归档任务 (archive) |
| Tasks | `A` (Shift) | 查看已归档任务 |
| Notes | `a` | 归档笔记 |
| Notes | `A` (Shift) | 查看已归档笔记 |
| Notes | `r` | 恢复归档笔记 (在归档视图中) |
| History | `r` | 恢复任务 (restore) |
| History | `R` (Shift) | Resume 对话 (`claude --resume`) |
| History | `s` | 统计面板 (stats) |
| History | `g` | 切换分组视图 (grouped) |

### History Tab 分组视图

```
┌─ History (156) ─────────────────────────── [g:grouped] [s:stats] ┐
│                                                                   │
│ ▼ Today (5)                                                       │
│   #156 tracker:fix:auth   Fix authentication...  → Fixed...  5m  │
│   #155 tracker:feat:ui    Add new button...      → Added...  12m │
│                                                                   │
│ ▼ Yesterday (8)                                                   │
│   #154 mediahub:upload    Implement upload...    → Done...   23m │
│   ...                                                             │
│                                                                   │
│ ▶ This Week (23)  [展开查看]                                      │
│ ▶ Earlier (120)   [展开查看]                                      │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│ j/k:nav  l:detail  r:restore  R:resume  /:search  g:group  s:stats│
└───────────────────────────────────────────────────────────────────┘
```

### 统计面板 (按 `s`)

```
┌─ Statistics ─────────────────────────────────────────────────────┐
│                                                                   │
│  TOTAL TASKS: 156          TOTAL TIME: 42h 30m                   │
│                                                                   │
│  ┌─ Today ────────┐  ┌─ This Week ────┐  ┌─ This Month ───┐     │
│  │  5 tasks       │  │  23 tasks      │  │  89 tasks      │     │
│  │  2h 18m        │  │  15h 12m       │  │  38h 45m       │     │
│  └────────────────┘  └────────────────┘  └────────────────┘     │
│                                                                   │
│  BY SESSION:                                                      │
│  ████████████████████  tracker    45 (29%)                       │
│  ████████████         mediahub   32 (21%)                        │
│  ████████             dotconfig  28 (18%)                        │
│  █████                other      51 (32%)                        │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│ Press any key to close                                            │
└───────────────────────────────────────────────────────────────────┘
```

## Web Console 界面设计

### History Tab (Timeline 页面增强)

```
┌─────────────────────────────────────────────────────────────────┐
│  ██ HISTORY ██                              [SEARCH] [STATS]    │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  FILTERS: [All Sessions ▼] [All Dates ▼] [________Search______] │
│                                                                  │
│  ▼ TODAY (5)                                                     │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │ #156 ░░ tracker:fix:auth                              5m   │ │
│  │ Fix authentication bug in login flow...                    │ │
│  │ → Fixed the JWT token validation issue                     │ │
│  │                                    [RESTORE] [RESUME]      │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                  │
│  ▶ YESTERDAY (8)                                                 │
│  ▶ THIS WEEK (23)                                                │
│  ▶ EARLIER (120)                                                 │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Stats 面板 (CRT 风格)

```
┌─────────────────────────────────────────────────────────────────┐
│  ██ STATISTICS ██                                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐            │
│  │ TOTAL TASKS  │ │ TOTAL TIME   │ │ AVG DURATION │            │
│  │     156      │ │   42:30:00   │ │    00:16:21  │            │
│  └──────────────┘ └──────────────┘ └──────────────┘            │
│                                                                  │
│  DAILY ACTIVITY (LAST 7 DAYS)                                   │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │     ██                                                   │   │
│  │     ██  ██      ██                                       │   │
│  │ ██  ██  ██  ██  ██  ██                                   │   │
│  │ ██  ██  ██  ██  ██  ██  ██                               │   │
│  │ Mon Tue Wed Thu Fri Sat Sun                              │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  BY SESSION                                                      │
│  tracker   ████████████████████░░░░░░  45 tasks (29%)           │
│  mediahub  ████████████░░░░░░░░░░░░░░  32 tasks (21%)           │
│  dotconfig ████████░░░░░░░░░░░░░░░░░░  28 tasks (18%)           │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 归档视图 (Workstations 页面)

在 Workstations 页面增加切换：`[ACTIVE] [ARCHIVED]`

## 实现阶段

### Phase 1: 后端核心
- [ ] 移除 30 分钟自动超时 (Go tracker-server)
- [ ] 检查并修复任务计时问题
- [ ] 添加 TASK_ARCHIVE / TASK_RESTORE 命令
- [ ] 添加 NOTE_RESTORE 命令
- [ ] 添加 HISTORY_QUERY / HISTORY_STATS 命令

### Phase 2: REST API
- [ ] POST /api/task/archive
- [ ] POST /api/task/restore
- [ ] POST /api/note/restore
- [ ] GET /api/history (分组查询)
- [ ] GET /api/history/stats
- [ ] POST /api/history/:id/resume

### Phase 3: TUI 实现
- [ ] Tasks Tab 归档/恢复快捷键
- [ ] Notes Tab 恢复快捷键
- [ ] History Tab 分组视图
- [ ] History Tab 统计面板
- [ ] History Tab Resume 功能

### Phase 4: Web 实现
- [ ] Timeline 页面分组显示
- [ ] 筛选器 (session/date/search)
- [ ] Stats 面板组件
- [ ] Restore/Resume 按钮
- [ ] Workstations 归档视图切换

## 文件修改清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/go/cmd/tracker-server/main.go` | 修改 | 移除超时，添加命令 |
| `src/rust/crates/tracker-core/src/ipc.rs` | 修改 | 添加新命令常量 |
| `src/rust/crates/tracker-server/src/main.rs` | 修改 | 实现新命令处理 |
| `src/rust/crates/tracker-server/src/db.rs` | 修改 | 添加查询/统计方法 |
| `src/rust/crates/tracker-web/src/main.rs` | 修改 | 添加 REST API |
| `src/rust/crates/tracker-tui/src/main.rs` | 修改 | TUI 快捷键和视图 |
| `web/src/pages/Timeline.tsx` | 修改 | 分组显示 + 筛选 |
| `web/src/pages/Stats.tsx` | 新建 | 统计面板 |
| `web/src/components/HistoryCard.tsx` | 新建 | 历史卡片组件 |
