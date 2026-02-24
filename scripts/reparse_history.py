#!/usr/bin/env python3
"""Re-parse transcripts for existing history records to add missing messages."""

import json
import os
import sqlite3
from datetime import datetime, timedelta, timezone

def _resolve_db_path():
    """Resolve DB path: TRACKER_DATA_DIR env → Application Support → legacy."""
    data_dir = os.environ.get("TRACKER_DATA_DIR", "")
    if data_dir:
        return os.path.join(data_dir, "data", "tracker.db")
    app_support = os.path.expanduser(
        "~/Library/Application Support/com.agent-tracker.menubar/data/tracker.db"
    )
    if os.path.exists(app_support):
        return app_support
    return os.path.expanduser("~/.config/agent-tracker/data/tracker.db")

DB_PATH = _resolve_db_path()

def parse_transcript(path: str, started_at: str | None, completed_at: str | None) -> list[dict]:
    """Parse a Claude transcript JSONL file and extract user/assistant messages."""
    if not os.path.exists(path):
        print(f"  Transcript not found: {path}")
        return []

    # Parse time range with 5 second buffer
    buffer = timedelta(seconds=5)
    start_time = None
    end_time = None

    if started_at:
        try:
            start_time = datetime.fromisoformat(started_at.replace('+00:00', '+00:00')) - buffer
        except:
            pass
    if completed_at:
        try:
            end_time = datetime.fromisoformat(completed_at.replace('+00:00', '+00:00')) + buffer
        except:
            pass

    messages = []
    with open(path, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
            except:
                continue

            entry_type = entry.get('type')
            if entry_type not in ('user', 'assistant'):
                continue

            # Parse timestamp
            ts_str = entry.get('timestamp')
            msg_time = None
            if ts_str:
                try:
                    # Handle Z suffix
                    ts_str = ts_str.replace('Z', '+00:00')
                    msg_time = datetime.fromisoformat(ts_str)
                except:
                    pass

            # Filter by time range
            if msg_time:
                if start_time and msg_time < start_time:
                    continue
                if end_time and msg_time > end_time:
                    continue

            # Extract content
            message = entry.get('message', {})
            content = message.get('content')

            if entry_type == 'user':
                # User content can be string or array
                if isinstance(content, str):
                    text = content
                else:
                    # Skip tool results
                    continue
            else:
                # Assistant content is array of items
                if isinstance(content, list):
                    texts = []
                    for item in content:
                        if isinstance(item, dict) and item.get('type') == 'text':
                            texts.append(item.get('text', ''))
                    text = '\n'.join(texts)
                else:
                    continue

            if not text.strip():
                continue

            messages.append({
                'role': entry_type,
                'content': text,
                'created_at': ts_str if ts_str else None
            })

    return messages

def main():
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()

    # Get all history records with transcript_path
    cursor.execute("""
        SELECT id, started_at, completed_at, transcript_path
        FROM history
        WHERE transcript_path <> ''
        ORDER BY id DESC
    """)
    records = cursor.fetchall()

    print(f"Found {len(records)} history records with transcripts")

    total_added = 0
    for history_id, started_at, completed_at, transcript_path in records:
        # Delete existing messages for this record
        cursor.execute("DELETE FROM conversation_messages WHERE history_id = ?", (history_id,))
        deleted = cursor.rowcount

        # Parse transcript
        messages = parse_transcript(transcript_path, started_at, completed_at)

        # Insert new messages
        for msg in messages:
            cursor.execute("""
                INSERT INTO conversation_messages (history_id, role, content, created_at)
                VALUES (?, ?, ?, ?)
            """, (history_id, msg['role'], msg['content'], msg['created_at']))

        if messages:
            print(f"  #{history_id}: deleted {deleted}, added {len(messages)} messages ({sum(1 for m in messages if m['role']=='user')} user, {sum(1 for m in messages if m['role']=='assistant')} assistant)")
            total_added += len(messages)

    conn.commit()
    conn.close()

    print(f"\nDone! Added {total_added} messages total.")

if __name__ == "__main__":
    main()
