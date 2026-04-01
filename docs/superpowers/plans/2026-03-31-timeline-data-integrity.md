# Timeline Data Integrity Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate duplicate history records created by legacy+new hook dual-write, auto-close orphaned entries, and clean up empty records.

**Architecture:** The legacy `/api/hook` endpoint creates history entries via `upsert_active_history` (keyed by tmux session/window/pane). The new `/api/hook/message|tool|session` endpoints create separate entries via `find_or_create_hook_history` (keyed by `claude_session_id`). For events like `UserPromptSubmit` and `Stop`, `agent-hook.sh` sends to BOTH endpoints, producing duplicate records. Fix: stop the legacy endpoint from creating history entries for events already covered by the new hook system, extend stale-session cleanup to cover legacy entries, and add a one-time DB migration to clean existing orphans.

**Tech Stack:** Rust (axum), SQLite, Bash

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `scripts/agent-hook.sh` | Modify | Stop sending UserPromptSubmit/Stop to legacy `/api/hook` for conversation tracking |
| `src/rust/crates/tracker-server/src/main.rs` | Modify | Remove `upsert_active_history` call from `handle_command(START_TASK)` |
| `src/rust/crates/tracker-server/src/db.rs` | Modify | Add stale-entry cleanup for legacy (non-claude_session_id) entries; add cleanup migration |

---

### Task 1: Stop legacy hook from creating duplicate history entries

The root cause: `agent-hook.sh` sends `UserPromptSubmit`/`Stop` to both `/api/hook` (legacy) AND `/api/hook/message` (new). The legacy handler calls `handle_command(START_TASK)` which calls `upsert_active_history`, creating a separate history row without `claude_session_id`.

**Fix approach:** Remove the `upsert_active_history` call from `START_TASK` in `handle_command`. The new hook endpoints already create history entries with `claude_session_id` and store messages/tools. The legacy endpoint should only manage task status (workstation view), not history.

**Files:**
- Modify: `src/rust/crates/tracker-server/src/main.rs:1164-1176`

- [ ] **Step 1: Remove upsert_active_history from START_TASK**

In `src/rust/crates/tracker-server/src/main.rs`, delete lines 1164-1176 (the block after `drop(state)` that calls `upsert_active_history`):

```rust
// DELETE this entire block (lines 1164-1176):
            // Also upsert a history entry so the session appears in timeline immediately
            {
                let git_dir = app_state.resolve_git_dir_for_window(&req.session_id, &req.window_id)
                    .unwrap_or_default();
                if !git_dir.is_empty() {
                    let server = app_state.state.lock().unwrap();
                    let _ = server.db.upsert_active_history(
                        &req.session_id, &req.session, &req.window_id, &req.window, &req.pane,
                        &req.summary, &git_dir,
                    );
                }
            }
```

After deletion, the START_TASK branch should end with:

```rust
            drop(state);
            app_state.broadcast_state();
        }
```

- [ ] **Step 2: Verify the build compiles**

Run:
```bash
cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server 2>&1 | tail -5
```

Expected: compiles successfully (no errors). `upsert_active_history` may show a dead-code warning — that's fine, we'll clean it up in Task 3.

- [ ] **Step 3: Commit**

```bash
git add src/rust/crates/tracker-server/src/main.rs
git commit -m "fix: stop legacy hook from creating duplicate history entries

Remove upsert_active_history call from START_TASK handler.
New /api/hook/message endpoint already creates history entries
with claude_session_id, so the legacy endpoint no longer needs
to create its own."
```

---

### Task 2: Extend stale-session cleanup to cover legacy entries

Currently `close_stale_hook_sessions` only closes entries with `claude_session_id != ''`. Legacy entries (from `upsert_active_history`, with empty `claude_session_id`) are never closed, leaving 1,814+ orphaned "in-progress" entries.

**Files:**
- Modify: `src/rust/crates/tracker-server/src/db.rs:3293-3301`

- [ ] **Step 1: Add cleanup for legacy stale entries**

In `src/rust/crates/tracker-server/src/db.rs`, modify `close_stale_hook_sessions` to also close legacy entries. Replace the existing method (lines 3293-3301):

```rust
    pub fn close_stale_hook_sessions(&self, stale_minutes: i64) -> Result<usize> {
        let threshold = format!("-{} minutes", stale_minutes);
        // Close stale hook sessions (with claude_session_id)
        let hook_count = self.conn.execute(
            "UPDATE history SET completed_at = datetime('now'), completion_note = 'auto-closed: stale'
             WHERE claude_session_id != '' AND completed_at IS NULL
             AND started_at < datetime('now', ?1)",
            params![threshold],
        )?;
        // Close stale legacy entries (without claude_session_id, from upsert_active_history)
        let legacy_count = self.conn.execute(
            "UPDATE history SET completed_at = datetime('now'), completion_note = 'auto-closed: legacy-stale'
             WHERE (claude_session_id = '' OR claude_session_id IS NULL) AND completed_at IS NULL
             AND started_at < datetime('now', ?1)",
            params![threshold],
        )?;
        Ok(hook_count + legacy_count)
    }
```

- [ ] **Step 2: Verify the build compiles**

Run:
```bash
cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server 2>&1 | tail -5
```

Expected: compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add src/rust/crates/tracker-server/src/db.rs
git commit -m "fix: extend stale-session cleanup to close legacy history entries

Previously only entries with claude_session_id were auto-closed.
Legacy entries (from upsert_active_history) with empty
claude_session_id were left open indefinitely."
```

---

### Task 3: One-time cleanup migration — close all existing orphaned entries

There are ~1,814 existing unclosed entries in the DB. Add a DB migration to close them all and clean up truly empty records (no messages, no tools, no completion data).

**Files:**
- Modify: `src/rust/crates/tracker-server/src/db.rs` (migrations section, around line 560+)

- [ ] **Step 1: Find the last migration number**

Search for the highest migration number in `db.rs`:

```bash
cd /Volumes/program/project-code/repos/ai-tracker && grep -oP '\(\d+,' src/rust/crates/tracker-server/src/db.rs | sort -t'(' -k1 -n | tail -5
```

Use the next number after the highest found.

- [ ] **Step 2: Add migration to close orphaned entries and delete empty records**

Add two new migrations after the last one in the `migrations` array in `db.rs`:

```rust
            // Close all orphaned history entries (no completed_at, older than 10 min)
            (NEXT_NUM, "UPDATE history SET completed_at = datetime('now'), completion_note = 'migration: closed orphan' WHERE completed_at IS NULL AND started_at < datetime('now', '-10 minutes')"),
            // Delete empty history entries: no messages, no tools, no completion_note, no summary content
            (NEXT_NUM+1, "DELETE FROM history WHERE id NOT IN (SELECT DISTINCT history_id FROM conversation_messages) AND id NOT IN (SELECT DISTINCT history_id FROM tool_usage) AND (completion_note IS NULL OR completion_note = '') AND (summary IS NULL OR summary = '' OR summary LIKE 'Claude session %')"),
```

Replace `NEXT_NUM` and `NEXT_NUM+1` with the actual next migration numbers.

- [ ] **Step 3: Verify the build compiles**

Run:
```bash
cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server 2>&1 | tail -5
```

Expected: compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/rust/crates/tracker-server/src/db.rs
git commit -m "fix: add migration to close orphaned history and delete empty records

Closes ~1814 unclosed entries and removes entries with no
messages, no tools, and no meaningful content."
```

---

### Task 4: Remove dead code — upsert_active_history

After Task 1, `upsert_active_history` is no longer called. Clean it up.

**Files:**
- Modify: `src/rust/crates/tracker-server/src/db.rs:3178-3216`

- [ ] **Step 1: Check no remaining callers**

```bash
cd /Volumes/program/project-code/repos/ai-tracker && grep -rn "upsert_active_history" src/rust/
```

Expected: only the function definition in `db.rs`, no callers.

- [ ] **Step 2: Delete the function**

Remove the `upsert_active_history` method (lines 3171-3216 in `db.rs`), including its doc comment and section header.

- [ ] **Step 3: Verify the build compiles**

Run:
```bash
cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server 2>&1 | tail -5
```

Expected: compiles successfully with no warnings about dead code.

- [ ] **Step 4: Commit**

```bash
git add src/rust/crates/tracker-server/src/db.rs
git commit -m "refactor: remove unused upsert_active_history method

No longer called after removing duplicate history creation
from legacy hook handler."
```

---

### Task 5: Verify end-to-end data flow

Manual verification that the fix works correctly.

- [ ] **Step 1: Build and deploy**

```bash
cd /Volumes/program/project-code/repos/ai-tracker && ./scripts/deploy.sh --tauri
```

- [ ] **Step 2: Trigger a hook event and verify single history entry**

Open a new Claude Code session in a tracked project. After sending a prompt, check the DB:

```bash
sqlite3 "/Users/heygo/Library/Application Support/com.agent-tracker.menubar/data/tracker.db" \
  "SELECT id, session_id, claude_session_id, summary, completed_at FROM history ORDER BY id DESC LIMIT 5"
```

Verify:
- Only ONE new entry per Claude session (with `claude_session_id` set)
- No duplicate entry with tmux pane ID as session_id
- Entry has associated `conversation_messages`

- [ ] **Step 3: Verify orphaned entries were cleaned up**

```bash
sqlite3 "/Users/heygo/Library/Application Support/com.agent-tracker.menubar/data/tracker.db" \
  "SELECT COUNT(*) FROM history WHERE completed_at IS NULL"
```

Expected: 0 (or very few, only currently active sessions).

- [ ] **Step 4: Check timeline UI**

Open the web UI, navigate to a project's timeline. Verify:
- No duplicate entries visible
- Records show proper time ranges and message counts
- Grouped view works correctly

---

## Summary of changes

| Problem | Root Cause | Fix |
|---|---|---|
| Duplicate history records | `agent-hook.sh` dual-writes to both endpoints; legacy `START_TASK` calls `upsert_active_history` | Remove `upsert_active_history` from `START_TASK` (Task 1) |
| 1,814 unclosed entries | Legacy entries lack `claude_session_id`, skip stale cleanup | Extend `close_stale_hook_sessions` to cover all entries (Task 2) |
| 2,347 empty records | Legacy endpoint creates history without storing messages | Migration to delete empty records (Task 3) |
| Dead code | `upsert_active_history` no longer needed | Remove function (Task 4) |
