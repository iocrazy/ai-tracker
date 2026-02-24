# Timeline Grouped View — Design

## Problem
Each Claude task execution is stored as a separate HistoryEntry. The Project Timeline displays them as disconnected fragments, making it hard to follow conversations that happen in the same tmux window.

## Solution
Group history entries by `session:window` and present them as unified conversation threads.

## User Choices
- Group by: session:window
- Detail view: continuous timeline with task boundary dividers
- Card info: detailed mode (session:window name + task count + short titles + total messages)

## Architecture

### Backend

#### 1. New `group_by=window` mode on `/api/projects/history`

When `group_by=window` is passed, SQL groups by `session, window`:

```sql
SELECT session, window,
       GROUP_CONCAT(id) as entry_ids,
       COUNT(*) as task_count,
       MIN(started_at) as first_started,
       MAX(COALESCE(completed_at, started_at)) as last_ended,
       SUM(duration_seconds) as total_duration,
       GROUP_CONCAT(summary, '|||') as summaries
FROM history
WHERE ...
GROUP BY session, window
ORDER BY MAX(started_at) DESC
LIMIT ? OFFSET ?
```

Response structure:
```rust
struct WindowGroupEntry {
    group_key: String,         // "session:window"
    session: String,
    window: String,
    entry_ids: Vec<i64>,
    task_count: i32,
    total_messages: i32,
    total_duration: f64,
    first_started: String,
    last_ended: String,
    summaries: Vec<String>,
}

struct WindowGroupResponse {
    groups: Vec<HistoryGroup<WindowGroupEntry>>,  // grouped by date
    total: i32,
}
```

#### 2. New `/api/history/grouped-detail?ids=1,2,3` endpoint

Fetches messages from multiple history entries, merged by time:
```rust
struct GroupedDetailResponse {
    segments: Vec<TaskSegment>,
    messages: Vec<ConversationMessageResponse>,
    tool_usage: Vec<ToolUsageResponse>,
    commits: Vec<CommitResponse>,
    timeline: Vec<TimelineEntry>,
}

struct TaskSegment {
    history_id: i64,
    summary: String,
    started_at: String,
    ended_at: String,
    message_start_index: usize,
}
```

### Frontend

#### ProjectTimeline.tsx
- Default to `group_by=window` mode
- Render WindowGroupEntry cards with: session:window name, task count, summaries list, total messages
- Pass `groupIds` to HistoryDetailModal

#### HistoryDetailModal.tsx
- Accept optional `groupIds: number[]` prop
- When groupIds present, call `/api/history/grouped-detail?ids=...`
- Messages tab: render all messages chronologically with task boundary dividers

#### services/history.ts
- New `WindowGroupEntry` type
- New `fetchGroupedDetail(ids: number[])` function
- Update `fetchProjectHistory` to accept `group_by` param

## Files to Modify
| File | Change |
|------|--------|
| `src/rust/.../routes_projects.rs` | group_by=window branch in get_project_history |
| `src/rust/.../routes_history.rs` | new grouped_detail handler |
| `src/rust/.../project_db.rs` | new load_history_grouped method |
| `src/rust/.../main.rs` | register new route |
| `web/src/services/history.ts` | new types + API functions |
| `web/src/components/ProjectTimeline.tsx` | render grouped cards |
| `web/src/components/HistoryDetailModal.tsx` | support grouped detail |

## Backward Compatibility
- Without `group_by` param, API behavior unchanged
- Single-entry detail API unaffected
- Toggle available to switch between grouped/flat mode
