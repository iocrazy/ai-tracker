# LIVE_CHAT Interaction Sync Design

**Date**: 2026-02-09
**Status**: Approved

## Problem

The Web UI LIVE_CHAT modal and the tmux terminal are out of sync:
1. When Claude uses AskUserQuestion, the terminal shows an interactive selection menu,
   but the Web UI only shows the text question without options
2. `<thinking>` tag content is rendered as raw text in the Web UI
3. Messages starting with `thinking` or `tool_use` content blocks are entirely skipped

## Solution: Parse JSONL tool_use blocks (Approach A)

Extend the backend JSONL parser to extract tool interaction data, and render
interactive elements in the frontend ChatHistoryModal.

## Backend Changes

### File: `src/rust/crates/tracker-server/src/main.rs`

#### 1. Extend `ClaudeMessage` struct

```rust
#[derive(Serialize, Clone)]
struct ClaudeMessage {
    role: String,
    timestamp: String,
    text: String,
    // New fields:
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interaction: Option<ToolInteraction>,
}

#[derive(Serialize, Clone)]
struct ToolInteraction {
    tool_name: String,
    questions: Vec<InteractiveQuestion>,
}

#[derive(Serialize, Clone)]
struct InteractiveQuestion {
    question: String,
    header: String,
    options: Vec<InteractiveOption>,
}

#[derive(Serialize, Clone)]
struct InteractiveOption {
    label: String,
    description: String,
}
```

#### 2. Rewrite `parse_claude_messages()` content handling

Current behavior (line ~2794):
```rust
// PROBLEM: skips entire message if first item is tool_use/thinking
if first_type == "tool_result" || first_type == "tool_use" || first_type == "thinking" {
    continue;
}
```

New behavior:
- Iterate ALL content blocks in the array
- Collect `text` blocks → concatenate into message text
- Collect `thinking` blocks → store as optional thinking field
- Detect `tool_use` where `name == "AskUserQuestion"` → parse `input.questions`
- Skip `tool_result` blocks (they're in user messages, not needed for display)

#### 3. Strip `<thinking>` tags from text content

Text content may contain inline `<thinking>...</thinking>` tags. Use regex
to extract thinking content and remove it from the display text:

```rust
let thinking_re = Regex::new(r"<thinking>([\s\S]*?)</thinking>").unwrap();
```

## Frontend Changes

### File: `web/src/components/ChatHistoryModal.tsx`

#### 1. Thinking block - collapsible

When a message has a `thinking` field, render a collapsed block:
```
[> Thinking...] (click to expand)
```
Expanded shows the full thinking text in a dimmed, monospace style.

#### 2. AskUserQuestion options - clickable buttons

When a message has an `interaction` field with AskUserQuestion data:
- Render numbered options below the message text
- Each option shows: number, label, description
- On click: `tmuxSendKeys(session, windowId, pane, "{number}", "Enter")`
  - Claude Code supports direct number input to select options
- After sending: mark the button as "sent" with visual feedback

#### 3. UI Layout

```
┌─────────────────────────────────────┐
│ AGENT | 02:54:43                     │
│ [> Thinking...] (collapsed)          │
│                                      │
│ What's the primary goal when you     │
│ check the mobile dashboard?          │
│                                      │
│ ┌─ 1. Monitor active work ──────┐   │
│ │ See what's downloading...      │   │ ← clickable
│ └────────────────────────────────┘   │
│ ┌─ 2. Browse statistics ────────┐   │
│ │ Keep current charts + stats... │   │ ← clickable
│ └────────────────────────────────┘   │
│ ┌─ 3. Both: quick status ───────┐   │
│ │ Top section shows live status  │   │ ← clickable
│ └────────────────────────────────┘   │
└─────────────────────────────────────┘
```

### File: `web/src/services/api.ts` + `web/src/types.ts`

Update TypeScript interfaces to match new backend response:

```typescript
interface ToolInteraction {
  tool_name: string;
  questions: InteractiveQuestion[];
}

interface InteractiveQuestion {
  question: string;
  header: string;
  options: InteractiveOption[];
}

interface InteractiveOption {
  label: string;
  description: string;
}

interface ClaudeMessage {
  role: string;
  timestamp: string;
  text: string;
  thinking?: string;
  interaction?: ToolInteraction;
}
```

## What's NOT Changed

- WebSocket logic (no change)
- Polling interval (3 seconds)
- Existing input box and image upload
- agent-event.sh hooks
- HistoryDetailModal (only ChatHistoryModal affected)

## Interaction Flow

1. Claude sends AskUserQuestion → JSONL records assistant message with text + tool_use blocks
2. Frontend polls /api/claude/messages → backend parses JSONL → returns message with interaction data
3. Frontend renders question text + clickable option buttons
4. User clicks option → `tmuxSendKeys(session, windowId, pane, "2", "Enter")`
5. Claude Code receives keystroke "2" + Enter → selects option 2
6. Next poll cycle (3s) → conversation continues normally
