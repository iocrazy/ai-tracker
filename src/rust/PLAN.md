# Agent Tracker Rust 实现计划

## 目标

完成 Rust 版本的三大功能模块：
1. TUI 完善 - Tab 切换、Notes/Goals 显示、操作
2. Web API 连接 - tracker-web 连接 tracker-server
3. 高级功能 - 搜索、过滤

---

## Phase 1: TUI 完善

### Task 1.1: 添加 Tab 切换基础结构
**文件:** `crates/tracker-tui/src/main.rs`

步骤:
1. 添加 `ActiveTab` 枚举 (Tasks, Notes, Goals)
2. 在 `App` struct 中添加 `active_tab` 字段
3. 添加 Tab 键 (1/2/3 或 Tab) 切换逻辑
4. 更新 UI 渲染显示当前 Tab

验证: `cargo check -p tracker-tui`

### Task 1.2: 实现 Notes 列表视图
**文件:** `crates/tracker-tui/src/main.rs`

步骤:
1. 在 `App` 中添加 `notes: Vec<Note>` 和 `selected_note: usize`
2. 更新 `update_from_envelope` 解析 notes
3. 添加 `render_notes()` 函数，显示 scope、summary、completed 状态
4. 在 Notes tab 时调用 render_notes

验证: 启动 server 和 tui，切换到 Notes tab 查看

### Task 1.3: 实现 Goals 列表视图
**文件:** `crates/tracker-tui/src/main.rs`

步骤:
1. 在 `App` 中添加 `goals: Vec<Goal>` 和 `selected_goal: usize`
2. 更新 `update_from_envelope` 解析 goals
3. 添加 `render_goals()` 函数
4. 在 Goals tab 时调用 render_goals

验证: 启动 server 和 tui，切换到 Goals tab 查看

### Task 1.4: 添加操作快捷键
**文件:** `crates/tracker-tui/src/main.rs`

步骤:
1. 添加 `a` 键添加 (需要输入框或简单 prompt)
2. 添加 `d` 键删除当前选中项
3. 添加 `Enter` 或 `Space` 切换完成状态
4. 添加发送命令到 server 的函数

验证: 在 TUI 中测试添加/删除/切换操作

---

## Phase 2: Web API 连接 Server

### Task 2.1: 创建 Server 连接模块
**文件:** `crates/tracker-web/src/server_client.rs` (新建)

步骤:
1. 创建 `ServerClient` struct，包含 Unix socket 连接
2. 实现 `connect()` 方法
3. 实现 `send_command()` 方法
4. 实现 `get_state()` 方法

验证: `cargo check -p tracker-web`

### Task 2.2: 集成到 Web API
**文件:** `crates/tracker-web/src/main.rs`

步骤:
1. 在 `AppState` 中添加 `ServerClient`
2. 更新 `/api/tasks` 从 server 获取真实数据
3. 添加 `/api/notes` 和 `/api/goals` 端点
4. 更新 `/api/task/send` 发送命令到 server

验证: 启动 server 和 web，curl 测试各端点

### Task 2.3: 添加 WebSocket 实时更新
**文件:** `crates/tracker-web/src/main.rs`

步骤:
1. 添加 `/ws` WebSocket 端点
2. 订阅 server 的状态广播
3. 将状态变化推送给 WebSocket 客户端

验证: 使用 websocat 连接 ws://localhost:3000/ws 测试

---

## Phase 3: 高级功能

### Task 3.1: 添加搜索功能到 Server
**文件:** `crates/tracker-server/src/main.rs`

步骤:
1. 添加 `search` 命令处理
2. 在 tasks/notes/goals 中搜索 summary 包含关键词的项
3. 返回匹配结果

验证: 发送 search 命令测试

### Task 3.2: 添加过滤功能到 TUI
**文件:** `crates/tracker-tui/src/main.rs`

步骤:
1. 添加 `/` 键进入搜索模式
2. 添加搜索输入框
3. 实时过滤显示匹配项
4. `Esc` 退出搜索模式

验证: 在 TUI 中测试搜索过滤

### Task 3.3: 添加 Session 过滤
**文件:** `crates/tracker-tui/src/main.rs`

步骤:
1. 添加 `filter_session: Option<String>` 到 App
2. 添加 `s` 键切换只显示当前 session
3. 过滤 notes/goals 只显示匹配的 session

验证: 测试 session 过滤功能

---

## 执行顺序

1. **Phase 1** (TUI) - 先完成用户界面
2. **Phase 2** (Web) - 连接 API
3. **Phase 3** (高级功能) - 增强体验

每个 Phase 完成后进行代码审查。

---

## 依赖关系

```
Task 1.1 → Task 1.2 → Task 1.3 → Task 1.4
                                    ↓
Task 2.1 → Task 2.2 → Task 2.3
                        ↓
Task 3.1 → Task 3.2 → Task 3.3
```

Phase 1 和 Phase 2 可以并行开发，Phase 3 依赖前两者完成。
