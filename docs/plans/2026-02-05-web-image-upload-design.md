# Web UI 图片发送到 tmux 设计

## 目标

在 Web UI 的 Chat 对话框中添加图片发送功能，让用户可以方便地发送截图或设计稿给 Claude Agent。

## 使用场景

1. **发送截图调试** - 让 Claude 看到界面截图/错误截图，用于调试或讨论
2. **发送设计稿** - 让 Claude 参考设计图来实现功能

## 用户流程

1. 打开 Chat 对话框（点击头像径向菜单的 CHAT）
2. 三种方式添加图片：
   - **粘贴**: Cmd+V 粘贴剪贴板图片
   - **点击**: 点击 📎 按钮选择文件
   - **拖拽**: 拖图片到对话框区域
3. 图片预览显示在输入框上方
4. 可添加文字说明，点击发送
5. 图片通过 tmux send-keys 发送给 Claude

## UI 设计

```
┌─────────────────────────────────────────┐
│  Chat History Modal                     │
├─────────────────────────────────────────┤
│  [消息列表...]                          │
│                                         │
│  ┌─────────────────────────────────┐   │
│  │ 📷 预览图 (可删除)    [×]       │   │ ← 图片预览区
│  └─────────────────────────────────┘   │
│  ┌─────────────────────────────────┐   │
│  │ 📎 │ Type a message...    [发送] │   │ ← 📎 按钮
│  └─────────────────────────────────┘   │
└─────────────────────────────────────────┘
```

## 技术实现

### 数据流

```
图片 → 前端压缩/转 base64 → API POST /api/tmux/send-image
    → 后端写入临时文件 → tmux send-keys "图片路径" Enter
```

### 前端修改 (ChatHistoryModal.tsx)

1. **添加状态**
   - `pendingImage: { file: File, preview: string } | null`

2. **添加图片处理函数**
   - `handlePaste(e)` - 监听粘贴事件，提取图片
   - `handleDrop(e)` - 监听拖拽事件
   - `handleFileSelect(e)` - 文件选择器回调
   - `compressImage(file)` - 压缩大图片（可选，限制 4MB）

3. **UI 组件**
   - 📎 按钮触发隐藏的 `<input type="file">`
   - 图片预览区（带删除按钮）
   - 拖拽区域高亮效果

4. **发送逻辑修改**
   - 如果有图片，先调用 `/api/tmux/send-image`
   - 再发送文字消息

### 后端 API (main.rs)

**新增 endpoint: `POST /api/tmux/send-image`**

```rust
struct SendImageRequest {
    session: String,
    window_id: String,
    pane: String,
    image_base64: String,  // data:image/png;base64,xxx
    message: Option<String>,  // 可选的文字说明
}

struct SendImageResponse {
    success: bool,
    message: String,
    image_path: String,  // 临时文件路径
}
```

**实现逻辑**:
1. 解码 base64 图片数据
2. 写入临时文件 `/tmp/agent-tracker-img-{uuid}.png`
3. 构造发送内容：`{message} {image_path}` 或仅 `{image_path}`
4. 调用 `tmux send-keys -t {session}:{window_id}.{pane} "{content}" Enter`
5. 返回结果

### 临时文件管理

- 路径格式: `/tmp/agent-tracker-img-{uuid}.{ext}`
- 清理策略: 可选，24小时后自动清理或手动清理

## 文件修改清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `web/src/components/ChatHistoryModal.tsx` | 修改 | 添加图片上传 UI 和逻辑 |
| `web/src/services/api.ts` | 修改 | 添加 `sendImage()` API 函数 |
| `src/rust/crates/tracker-server/src/main.rs` | 修改 | 添加 `/api/tmux/send-image` endpoint |

## 验证步骤

1. 打开 Chat 对话框
2. 测试粘贴图片 (Cmd+V)
3. 测试点击上传
4. 测试拖拽图片
5. 确认图片预览显示
6. 发送后确认 Claude 收到图片路径
7. 确认 Claude 能正确读取图片

## 后续优化（可选）

- [ ] 图片压缩（超过 4MB 自动压缩）
- [ ] 多图片支持
- [ ] 图片历史记录显示
- [ ] 临时文件自动清理
