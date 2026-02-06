# Claude Code Hooks 与 Skills 协同指南

## 核心观点

来自 [抓蛙师的文章](https://blog.csdn.net/leoisaking/article/details/156203326)：

> AI 有能力，但缺乏"制度约束"。Hooks 是纪律，Skills 是知识。

通过 `UserPromptSubmit` hook 注入强制评估指令，让 AI 在开始回答前必须评估并激活相关 Skills，技能激活率从 **25% 提升到 90%+**。

---

## Hook 事件时序图

```
用户启动 claude
    ↓
┌─────────────────────────┐
│  SessionStart           │  ← 会话开始
└─────────────────────────┘
    ↓
用户输入问题，按回车
    ↓
┌─────────────────────────┐
│  UserPromptSubmit       │  ← 用户提交后，Claude 思考前
│  【唯一可注入指令的时机】  │
└─────────────────────────┘
    ↓
Claude 开始思考和执行
    ↓
    ├── 需要调用工具时 ──→ PreToolUse (工具执行前)
    │                      PostToolUse (工具执行后)
    │
    ├── 需要用户确认时 ──→ Notification (permission_prompt)
    │                      这就是 pause 状态
    ↓
Claude 完成回答
    ↓
┌─────────────────────────┐
│  Stop                   │  ← 回合结束
└─────────────────────────┘
    ↓
等待用户下一个问题 (idle_prompt)
```

---

## Hook 状态对比

| Hook | 触发时机 | Claude 状态 | 能否注入指令让 Claude 执行 |
|------|---------|-------------|---------------------------|
| `SessionStart` | 会话启动 | 还没开始 | ❌ |
| **`UserPromptSubmit`** | 用户按回车后 | **即将开始思考** | ✅ 可以注入指令 |
| `PreToolUse` | 调用工具前 | 执行中 | ❌ |
| `Notification` | 等待确认 | 暂停中 | ❌ |
| `Stop` | 回答完成 | 已结束 | ❌ |

---

## 关键洞察

### 为什么只有 UserPromptSubmit 能注入指令？

- **Hooks 是外部 shell 脚本**，独立于 Claude 进程
- **Skills 需要 Claude 上下文**，通过 Skill tool 调用
- `UserPromptSubmit` 是唯一一个 Claude **还没开始但即将处理输入**的时机
- Hook 输出的文本会被 Claude 看到并执行

### Stop/Pause 时如何触发 Skill？

不能直接在 Stop hook 中调用 Skill，因为那时 Claude 回合已结束。

**解决方案**：在 `UserPromptSubmit` 注入**行为规则**：

```bash
echo '【强制规则】当任务完成或需要用户确认时，必须调用 /discord-notify skill'
```

Claude 看到规则后，会在**正确的时机**主动调用 Skill。

---

## 实际配置示例

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "echo '【强制规则】任务完成或需要确认时，必须调用 /discord-notify skill'"
          }
        ]
      }
    ]
  }
}
```

---

## 四个 Hook 的协同工作流（来自抓蛙师）

| Hook | 作用 |
|------|------|
| **SessionStart** | 显示项目状态、Git 分支、待办事项 |
| **UserPromptSubmit** | 强制技能评估，激活率 25%→90%+ |
| **PreToolUse** | 安全防护，拦截危险命令 |
| **Stop** | 总结反馈、推荐下一步操作 |

> **核心总结**：Hooks 是纪律，Skills 是知识，Commands 是流程，Agents 是分工。
