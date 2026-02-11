# .aitracker Per-Project Storage Design

## Problem

Current architecture stores all data in a single global SQLite DB (`~/.config/agent-tracker/tracker.db`), keyed by tmux session_id (`$2`, `@3`). Session IDs are ephemeral — after tmux restart, `$2` might be a different project. This causes history records to mismatch and makes cross-machine resume impossible.

## Solution

Two-layer storage: global DB as index/runtime, per-project `.aitracker/` directory for persistent data.

## Architecture

### Global Layer (`~/.config/agent-tracker/tracker.db`)

Keeps runtime and index data only:

- `tasks` — active in-progress tasks (bound to tmux session lifecycle, cleared on close)
- `session_index` — JSONL file index cache (background scanner)
- `projects` — project registry table (NEW):

```sql
CREATE TABLE projects (
    git_dir TEXT PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    last_session TEXT DEFAULT '',
    last_window TEXT DEFAULT '',
    last_active_at TEXT,
    notes_count INTEGER DEFAULT 0,
    goals_count INTEGER DEFAULT 0,
    history_count INTEGER DEFAULT 0
);
```

### Project Layer (`<git-root>/.aitracker/tracker.db`)

Per-project persistent data:

- `history` — completed task records
- `conversation_messages` — linked to history
- `tool_usage` — linked to history
- `commits` — linked to history
- `notes` — project-scoped sticky notes
- `goals` — project-scoped goals
- `closed_windows` — resumable window info

### Directory Structure

```
<git-root>/.aitracker/
    tracker.db      # SQLite database
    .gitignore      # Contains: *
```

The `.gitignore` with `*` auto-ignores the entire directory without touching the project's root `.gitignore`.

## Project Discovery

When a task event arrives:

1. Get tmux pane working directory
2. Run `git rev-parse --show-toplevel` to find git root
3. Check if `<git-root>/.aitracker/` exists
4. If not: create directory, init DB schema, write `.gitignore`, register in global `projects` table
5. If yes: open existing DB connection
6. Write data to project DB + update global `projects` table counts/timestamps

## Write Flow (task completion)

1. Hook reports `task_complete` with session/window/pane
2. Server resolves pane → git root → project `.aitracker/tracker.db`
3. Write history + sub-tables to project DB
4. Update global `projects` table: `last_active_at`, `history_count`

## Read Flow (Web UI)

| View | Data Source |
|------|------------|
| WORKSTATIONS | Global DB (live tasks + tmux state) |
| Timeline (ALL) | Global `projects` → lazy-load each project's history, sorted by `last_active_at` |
| Timeline (project) | Direct read from that project's `.aitracker/tracker.db` |
| LIVE_CHAT | JSONL files (unchanged) |
| Resume | Global `projects` → `git_dir` → project DB `closed_windows` |

### Timeline Project Filter

- Add project selector at top of Timeline view
- Options: ALL (default) | per-project tabs from `projects` table
- Sorted by `last_active_at` descending
- ALL mode: lazy-load recent 2-3 active projects first, load rest on scroll

## API Changes

- `/api/history` — add `?project=<git_dir>` parameter
- `/api/sessions` — add project filter for JSONL files
- `/api/projects` — NEW: list registered projects
- `/api/projects/:git_dir/history` — NEW: project-specific history

## Data Migration (one-time)

On first startup of new version:

1. Detect if global DB has old `history` records
2. Try matching by session name → git_dir (via tmux or session_index project field)
3. Matched records: migrate to project `.aitracker/tracker.db`
4. Unmatched records: keep in global DB (no data loss)
5. No forced migration — old data stays, new data goes to new path

## Implementation Steps

1. Add `projects` table to global DB schema
2. Create `ProjectDb` struct — manages per-project `.aitracker/tracker.db` connections (with connection cache)
3. Add git root discovery utility (`git rev-parse --show-toplevel`)
4. Modify task completion flow: resolve git root → write to project DB + update global
5. Modify history/notes/goals read APIs to read from project DBs
6. Add `/api/projects` endpoint
7. Add project filter to Timeline frontend
8. Add `.aitracker/` auto-init with `.gitignore`
9. Implement one-time migration logic
10. Update resume flow to use project DB for closed_windows
