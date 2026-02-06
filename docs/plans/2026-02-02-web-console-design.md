# Agent Tracker Web Console 设计文档

> 日期: 2026-02-02
> 状态: Draft

## 概述

为 Agent Tracker 添加 Web 监控前端，提供工位可视化、任务时间线、远程控制台功能。

## 需求

- **访问方式**: 局域网访问，简单密码保护
- **功能范围**: 综合仪表盘（工位看板 + 任务时间线 + 远程控制台）
- **数据更新**: WebSocket 实时推送
- **UI 风格**: CRT 复古像素风，荧光绿/霓虹色，暗色主题

## 架构

```
┌─────────────────────────────────────────────────────────────────┐
│                        用户浏览器                                │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              React SPA (CRT 复古风格)                      │  │
│  │  ┌─────────┬─────────┬─────────┬─────────┐               │  │
│  │  │ 工位看板 │任务时间线│远程控制台│  设置   │  ◄─ Tab 导航  │  │
│  │  └─────────┴─────────┴─────────┴─────────┘               │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
           │                              ▲
           │ HTTP (REST API)              │ WebSocket (实时状态)
           ▼                              │
┌─────────────────────────────────────────────────────────────────┐
│                    tracker-web (Rust/Axum)                      │
│  ┌──────────────┬──────────────┬──────────────┐                │
│  │ 静态文件托管  │   REST API   │  WebSocket   │                │
│  │ /assets/*    │  /api/*      │  /ws         │                │
│  └──────────────┴──────────────┴──────────────┘                │
│                          │                                      │
│                    简单 Token 认证                               │
└─────────────────────────────────────────────────────────────────┘
           │
           │ Unix Socket
           ▼
┌─────────────────────────────────────────────────────────────────┐
│                    tracker-server (状态管理)                     │
└─────────────────────────────────────────────────────────────────┘
```

## 技术栈

| 层级 | 技术 | 版本 | 说明 |
|------|------|------|------|
| 前端框架 | React | 19.x | 最新版本 |
| 构建工具 | Vite | 6.x | 快速 HMR |
| 语言 | TypeScript | 5.7+ | 类型安全 |
| 样式 | Tailwind CSS | 4.0 | CSS 变量优先 |
| 状态管理 | Zustand | 5.x | 轻量 (~1KB)，适合 WebSocket 推送 |
| 图标 | Pixelarticons | - |
| 字体 | VT323 / Press Start 2P | Google Fonts |
| 静态嵌入 | rust-embed | - |

## 页面设计

### Tab 1: 工位看板 (Workstations)

显示所有 tmux session 和 window，实时状态更新。

```
┌─────────────────────────────────────────────────────────────────┐
│  ██ AGENT TRACKER ██                    [LOGIN: heygo] [⚙️]    │
├─────────────────────────────────────────────────────────────────┤
│  ┌─ SESSION: tracker ─────────────────────────────────────────┐ │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐       │ │
│  │  │ fix:auth-bug │ │ feat:new-ui  │ │    master    │       │ │
│  │  │ ██████░░░░░░ │ │ ████████████ │ │ ░░░░░░░░░░░░ │       │ │
│  │  │ 🟢 RUNNING   │ │ ⏸️ WAITING   │ │ ✅ IDLE      │       │ │
│  │  │ 03:42       │ │ 00:15        │ │ --:--        │       │ │
│  │  └──────────────┘ └──────────────┘ └──────────────┘       │ │
│  └─────────────────────────────────────────────────────────────┘ │
│  ┌─ SESSION: mediahub ────────────────────────────────────────┐ │
│  │  ┌──────────────┐ ┌──────────────┐                        │ │
│  │  │ feat:upload  │ │ fix:player   │                        │ │
│  │  │ ████░░░░░░░░ │ │ ██████████░░ │                        │ │
│  │  │ 🟢 RUNNING   │ │ 🟢 RUNNING   │                        │ │
│  │  └──────────────┘ └──────────────┘                        │ │
│  └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

**数据来源:**
- `GET /api/workspace/list` - 获取所有 workspace
- `GET /api/tasks` - 获取任务状态
- `WS /ws` - 实时状态推送

### Tab 2: 任务时间线 (Timeline)

按时间顺序展示任务事件。

```
┌─────────────────────────────────────────────────────────────────┐
│  TODAY                                                          │
│  ├─ 09:15 ── fix:auth-bug ─── STARTED ──────────────────────── │
│  ├─ 09:42 ── fix:auth-bug ─── WAITING (permission: Bash) ───── │
│  ├─ 09:43 ── fix:auth-bug ─── RESUMED ──────────────────────── │
│  ├─ 10:30 ── feat:new-ui ──── STARTED ──────────────────────── │
│  ├─ 11:15 ── feat:new-ui ──── COMPLETED ─ "Added login form" ─ │
│  └─ ...                                                         │
└─────────────────────────────────────────────────────────────────┘
```

**数据来源:**
- `GET /api/history` - 获取历史记录（需新增）

### Tab 3: 远程控制台 (Console)

查看 pane 内容，发送命令布置任务。

```
┌─────────────────────────────────────────────────────────────────┐
│  TARGET: [tracker ▼] : [fix:auth-bug ▼] : [pane 3 ▼]           │
├─────────────────────────────────────────────────────────────────┤
│  > Analyzing authentication flow...                             │
│  > Found issue in src/auth/middleware.rs:42                     │
│  > Applying fix...                                              │
│  > ▌                                                            │
├─────────────────────────────────────────────────────────────────┤
│  SEND COMMAND: [____________________________________] [SEND]    │
└─────────────────────────────────────────────────────────────────┘
```

**数据来源:**
- `GET /api/pane/capture?session=X&window=Y&pane=Z` - 获取 pane 内容（需新增）
- `POST /api/pane/send-keys` - 发送命令（需新增）

### Tab 4: 设置 (Settings)

- 修改密码
- 主题切换（绿/琥珀/青）
- 通知设置

## CRT 风格实现

### 颜色主题

```css
@theme {
  --color-crt-green: #33ff33;
  --color-crt-amber: #ffaa00;
  --color-crt-cyan: #00ffff;
  --color-crt-red: #ff3333;
  --color-crt-bg: #0a0a0a;
  --color-crt-bg-secondary: #1a1a1a;
}
```

### 扫描线效果

```css
.scanlines::before {
  content: '';
  position: absolute;
  inset: 0;
  background: repeating-linear-gradient(
    0deg,
    rgba(0, 0, 0, 0.15),
    rgba(0, 0, 0, 0.15) 1px,
    transparent 1px,
    transparent 2px
  );
  pointer-events: none;
}
```

### 发光效果

```css
.glow-text {
  text-shadow:
    0 0 5px currentColor,
    0 0 10px currentColor,
    0 0 20px currentColor;
}

.glow-border {
  box-shadow:
    0 0 5px currentColor,
    inset 0 0 5px currentColor;
}
```

### 闪烁动画

```css
@keyframes flicker {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.98; }
}

.crt-flicker {
  animation: flicker 0.15s infinite;
}
```

## API 扩展（tracker-web 需新增）

### 认证

```
POST /api/auth/login
Request:  { "password": "xxx" }
Response: { "token": "xxx", "expires_at": "..." }

所有其他 API 需要 Header: Authorization: Bearer <token>
```

### tmux 操作

```
GET /api/tmux/sessions
Response: [{ "name": "tracker", "windows": [...] }]

GET /api/tmux/capture?session=X&window=Y&pane=Z
Response: { "content": "...", "cursor_x": 0, "cursor_y": 10 }

POST /api/tmux/send-keys
Request:  { "session": "X", "window": "Y", "pane": "Z", "keys": "..." }
Response: { "success": true }
```

### 历史记录

```
GET /api/history?limit=100&offset=0
Response: { "events": [{ "time": "...", "type": "...", "data": {...} }] }
```

## 目录结构

```
agent-tracker/
├── web/                          # 前端项目
│   ├── src/
│   │   ├── components/
│   │   │   ├── layout/
│   │   │   │   ├── Header.tsx
│   │   │   │   ├── TabNav.tsx
│   │   │   │   └── CRTScreen.tsx
│   │   │   ├── workstation/
│   │   │   │   ├── SessionCard.tsx
│   │   │   │   └── WindowCard.tsx
│   │   │   ├── timeline/
│   │   │   │   └── EventItem.tsx
│   │   │   └── console/
│   │   │       ├── PaneViewer.tsx
│   │   │       └── CommandInput.tsx
│   │   ├── pages/
│   │   │   ├── Workstations.tsx
│   │   │   ├── Timeline.tsx
│   │   │   ├── Console.tsx
│   │   │   └── Settings.tsx
│   │   ├── stores/
│   │   │   ├── authStore.ts
│   │   │   ├── taskStore.ts
│   │   │   └── wsStore.ts
│   │   ├── hooks/
│   │   │   └── useWebSocket.ts
│   │   ├── styles/
│   │   │   └── crt.css
│   │   ├── App.tsx
│   │   └── main.tsx
│   ├── public/
│   │   └── fonts/
│   ├── index.html
│   ├── package.json
│   ├── vite.config.ts
│   └── tailwind.config.ts
└── src/rust/crates/tracker-web/
    └── src/
        ├── main.rs              # 添加静态文件托管
        ├── auth.rs              # 新增: 认证中间件
        └── tmux.rs              # 新增: tmux 操作 API
```

## 实现阶段

### Phase 1: 基础框架
- [ ] 创建 React 项目 (Vite + React 19 + TypeScript)
- [ ] 配置 Tailwind CSS 4.0 + CRT 主题
- [ ] 实现基础布局 (Header + TabNav + CRTScreen)
- [ ] 添加像素字体

### Phase 2: 工位看板
- [ ] 实现 SessionCard / WindowCard 组件
- [ ] 连接 WebSocket 实时更新
- [ ] 状态指示器动画

### Phase 3: 认证系统
- [ ] tracker-web 添加简单 Token 认证
- [ ] 登录页面
- [ ] Token 存储和刷新

### Phase 4: 任务时间线
- [ ] tracker-web 添加历史 API
- [ ] Timeline 组件
- [ ] 日期筛选

### Phase 5: 远程控制台
- [ ] tracker-web 添加 tmux capture/send-keys API
- [ ] PaneViewer 组件（虚拟终端显示）
- [ ] CommandInput 组件

### Phase 6: 打包部署
- [ ] 构建优化
- [ ] rust-embed 嵌入静态文件
- [ ] 更新 launchd 服务
