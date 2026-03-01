# Design: Auto-capture Todos from Claude Code Prompt Prefixes

## Problem

When starting work with `feature:xxx` or `fix:xxx` prefixes in Claude Code, these should automatically appear as todos in the project's kanban board.

## Solution

Extend `handle_hook` to parse conventional commit prefixes from `UserPromptSubmit` prompts and create project todos automatically.

## Prefix Recognition

```
regex: ^(feat|feature|fix|bug|chore|refactor|docs|perf|test|style|ci|build)(!)?:\s*(.+)
```

### Priority Mapping

| Prefix | Priority | Notes |
|--------|----------|-------|
| Any with `!` suffix | 2 (urgent) | e.g. `fix!:`, `feat!:` |
| `fix:` / `bug:` | 1 (high) | Bug fixes |
| All others | 0 (normal) | Features, chores, docs, etc. |

## Data Flow

```
UserPromptSubmit → handle_hook
  → parse prompt prefix (regex match)
  → if no match: skip (existing START_TASK only)
  → if match:
    1. resolve tmux context (existing)
    2. get pane_current_path → git rev-parse --show-toplevel → git_dir
    3. deduplicate: check if same title exists with status != 'done'
    4. create project_todo: { git_dir, title: "[type] description", priority, status: "todo" }
    5. continue with existing START_TASK logic (unchanged)
```

## Todo Title Format

```
[feat] Add dark mode support
[fix!] Fix login crash on empty password
[chore] Clean up unused dependencies
```

## Deduplication

Before creating, query `project_todos` for same `git_dir` + `title` where `status != 'done'`. Skip if exists.

## Files to Modify

- `src/rust/crates/tracker-server/src/main.rs`: Add prefix parsing + todo creation in `handle_hook` UserPromptSubmit branch

## Implementation Steps

1. Add helper function `parse_todo_prefix(prompt) -> Option<(type, bang, title)>`
2. Add helper function `resolve_git_dir_from_pane(pane_id) -> Option<String>`
3. In `handle_hook` UserPromptSubmit branch, after extracting prompt:
   - Call `parse_todo_prefix`
   - If matched, resolve git_dir, check dedup, create todo via `db::create_project_todo`
4. Existing START_TASK flow unchanged
