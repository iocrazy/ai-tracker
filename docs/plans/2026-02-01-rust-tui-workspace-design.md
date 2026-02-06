# Rust Tracker-TUI 工作区管理设计

> 设计日期: 2026-02-01
> 状态: 待实现

## 概述

将 Shell 脚本 (start-agent, resume-agent, destroy.sh, start-workspace) 的功能完全迁移到 Rust tracker-tui，实现多项目 Git Worktree 并行开发工作流。

## 设计决策

| 决策点 | 选择 |
|--------|------|
| 项目范围 | 多项目并行 (Git worktree) |
| 实现方式 | 完全替换 Shell 脚本 |
| 配置格式 | JSON |
| 布局支持 | 3-pane (yazi+lazygit+agent) + 5-pane |
| 触发方式 | TUI + CLI 双重入口 |

## 模块架构

```
tracker-tui/src/
├── main.rs
├── app.rs              # 现有 TUI 应用
├── workspace/          # 新增: Git worktree 管理
│   ├── mod.rs
│   ├── git.rs          # git worktree 操作
│   └── config.rs       # 项目配置读写
├── agent/              # 新增: Agent 生命周期
│   ├── mod.rs
│   ├── tmux.rs         # tmux session/window/pane
│   └── layout.rs       # 布局模板渲染
└── cli/                # 新增: CLI 子命令
    ├── mod.rs
    ├── start.rs
    ├── resume.rs
    ├── destroy.rs
    └── list.rs
```

## 配置文件 Schema

### 位置
- 全局: `~/.config/agent-tracker/agent-config.json`
- 项目级: `{project}/.agent-tracker.json` (覆盖全局)

### Schema

```json
{
  "workspaces": {
    "agent-tracker": {
      "base_path": "/Users/heygo/.config/agent-tracker",
      "main_branch": "master",
      "worktree_dir": ".worktrees"
    },
    "my-app": {
      "base_path": "/Users/heygo/projects/my-app",
      "main_branch": "main",
      "worktree_dir": ".worktrees"
    }
  },
  "agents": {
    "claude": {
      "command": "claude",
      "color": "#f5a623",
      "icon": "🤖"
    },
    "opencode": {
      "command": "opencode",
      "color": "#7ed321",
      "icon": "💻"
    }
  },
  "layouts": {
    "default": {
      "panes": [
        {"cmd": "yazi", "size": "30%"},
        {"cmd": "lazygit", "size": "30%"},
        {"cmd": "{agent}", "size": "40%"}
      ]
    },
    "focus": {
      "panes": [
        {"cmd": "{agent}", "size": "100%"}
      ]
    }
  },
  "defaults": {
    "layout": "default",
    "agent": "claude"
  }
}
```

## CLI 命令设计

### 命令结构

```bash
tracker-tui [subcommand] [options]

# 无子命令时启动 TUI
tracker-tui

# 子命令
tracker-tui start    # 启动新 agent 工作区
tracker-tui resume   # 恢复已有工作区
tracker-tui destroy  # 销毁工作区
tracker-tui list     # 列出所有活跃工作区
```

### `start` - 启动新工作区

```bash
tracker-tui start [OPTIONS]

Options:
  -w, --workspace <NAME>   项目名称 (必须已在 config 注册)
  -b, --branch <NAME>      分支名 (自动创建 worktree)
  -a, --agent <NAME>       使用的 agent (默认: claude)
  -l, --layout <NAME>      布局模板 (默认: default)
  --attach                 创建后自动 attach

# 示例
tracker-tui start -w agent-tracker -b feat/tui-upgrade -a claude --attach
```

### `resume` - 恢复工作区

```bash
tracker-tui resume [OPTIONS]

Options:
  -w, --workspace <NAME>   项目名称
  -b, --branch <NAME>      分支名
  --attach                 恢复后自动 attach

# 示例 - 交互式选择
tracker-tui resume

# 直接指定
tracker-tui resume -w agent-tracker -b feat/tui-upgrade --attach
```

### `destroy` - 销毁工作区

```bash
tracker-tui destroy [OPTIONS]

Options:
  -w, --workspace <NAME>   项目名称
  -b, --branch <NAME>      分支名
  --force                  跳过确认

# 示例
tracker-tui destroy -w agent-tracker -b feat/tui-upgrade
```

### `list` - 列出工作区

```bash
tracker-tui list [OPTIONS]

Options:
  -w, --workspace <NAME>   过滤特定项目
  --json                   JSON 输出

# 输出示例
WORKSPACE       BRANCH              AGENT    STATUS
agent-tracker   feat/tui-upgrade    claude   running
my-app          fix/login-bug       opencode paused
```

## TUI 快捷键集成

### 全局快捷键

| 按键 | 功能 |
|------|------|
| `n` | 新建工作区 (触发 start 流程) |
| `r` | 恢复工作区 (显示选择列表) |
| `d` | 销毁当前选中工作区 |
| `a` | attach 到选中工作区 |

### 工作区面板

| 按键 | 功能 |
|------|------|
| `j/k` | 上下选择 |
| `Enter` | attach |
| `D` | 销毁 (需确认) |

## 实现计划

### Phase 1: 基础架构
1. 添加 clap 依赖用于 CLI 参数解析
2. 创建 `config` 模块，实现 JSON 配置读写
3. 定义配置数据结构 (serde)

### Phase 2: Git Worktree 管理
1. 创建 `workspace/git.rs`
2. 实现 worktree 创建、列表、删除
3. 实现分支检测和冲突处理

### Phase 3: Tmux 集成
1. 创建 `agent/tmux.rs`
2. 实现 session/window/pane 创建
3. 实现布局模板渲染
4. 实现 attach/detach 操作

### Phase 4: CLI 子命令
1. 实现 `start` 子命令
2. 实现 `resume` 子命令
3. 实现 `destroy` 子命令
4. 实现 `list` 子命令

### Phase 5: TUI 集成
1. 添加工作区面板视图
2. 实现快捷键绑定
3. 添加交互式选择器

## 依赖项

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
dirs = "5"  # 获取 home 目录
```

## 参考

- 现有 Shell 脚本: `/Users/heygo/.config/agent-tracker/agent-scripts/`
- tracker-tui 源码: `/Users/heygo/.config/agent-tracker/src/rust/crates/tracker-tui/`
