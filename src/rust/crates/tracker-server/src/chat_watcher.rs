//! Chat watcher: monitors active JSONL files for new messages and broadcasts via WebSocket
//!
//! Uses file-size polling (500ms) instead of filesystem notifications because
//! macOS FSEvents debounces append-mode file changes unreliably.

use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::routes_history::ClaudeMessage;

/// Event broadcast to WebSocket clients when new chat messages arrive
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessageEvent {
    pub kind: String,           // "chat"
    pub session_file: String,
    pub messages: Vec<ClaudeMessage>,
}

/// Tracks a watched JSONL session file
struct WatchedSession {
    path: PathBuf,
    last_size: u64,
}

/// Watches active JSONL session files for new messages
pub struct ChatWatcher {
    sessions: std::sync::Mutex<HashMap<String, WatchedSession>>,
    broadcast_tx: broadcast::Sender<ChatMessageEvent>,
}

impl ChatWatcher {
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(64);
        Self {
            sessions: std::sync::Mutex::new(HashMap::new()),
            broadcast_tx,
        }
    }

    /// Subscribe to chat message events
    pub fn subscribe(&self) -> broadcast::Receiver<ChatMessageEvent> {
        self.broadcast_tx.subscribe()
    }

    /// Register or update a session file to watch.
    /// Called from the tmux polling loop when active tasks are discovered.
    pub fn watch_session(&self, session_file: String, path: PathBuf) {
        let mut sessions = self.sessions.lock().unwrap();
        if !sessions.contains_key(&session_file) {
            // Get current file size as starting offset (don't replay existing content)
            let last_size = path.metadata().map(|m| m.len()).unwrap_or(0);
            debug!("ChatWatcher: watching {}", session_file);
            sessions.insert(session_file, WatchedSession { path, last_size });
        }
    }

    /// Remove a session that is no longer active
    pub fn unwatch_session(&self, session_file: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.remove(session_file).is_some() {
            debug!("ChatWatcher: unwatching {}", session_file);
        }
    }

    /// Update the set of watched sessions from active task list.
    /// `active_files` is a list of (session_file_key, PathBuf) for currently active sessions.
    pub fn sync_sessions(&self, active_files: Vec<(String, PathBuf)>) {
        let mut sessions = self.sessions.lock().unwrap();

        // Remove sessions that are no longer active
        let active_keys: std::collections::HashSet<&str> = active_files.iter().map(|(k, _)| k.as_str()).collect();
        sessions.retain(|k, _| active_keys.contains(k.as_str()));

        // Add new sessions
        for (key, path) in active_files {
            if !sessions.contains_key(&key) {
                let last_size = path.metadata().map(|m| m.len()).unwrap_or(0);
                debug!("ChatWatcher: watching {}", key);
                sessions.insert(key, WatchedSession { path, last_size });
            }
        }
    }

    /// Poll all watched sessions for changes. Call this every ~500ms.
    pub fn poll(&self) {
        use std::io::{Read, Seek, SeekFrom};

        let mut sessions = self.sessions.lock().unwrap();

        for (key, session) in sessions.iter_mut() {
            // Check current file size
            let current_size = match session.path.metadata() {
                Ok(m) => m.len(),
                Err(_) => continue,
            };

            if current_size <= session.last_size {
                continue;
            }

            // Read new bytes from last_size to current_size
            let mut file = match std::fs::File::open(&session.path) {
                Ok(f) => f,
                Err(e) => {
                    warn!("ChatWatcher: failed to open {}: {}", key, e);
                    continue;
                }
            };

            if let Err(e) = file.seek(SeekFrom::Start(session.last_size)) {
                warn!("ChatWatcher: seek failed for {}: {}", key, e);
                continue;
            }

            let bytes_to_read = (current_size - session.last_size) as usize;
            let mut buf = vec![0u8; bytes_to_read];
            let bytes_read = match file.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    warn!("ChatWatcher: read failed for {}: {}", key, e);
                    continue;
                }
            };
            buf.truncate(bytes_read);

            session.last_size = session.last_size + bytes_read as u64;

            let content = String::from_utf8_lossy(&buf);

            // Skip first partial line if we started mid-stream
            // (This shouldn't happen since we track exact offsets, but be safe)
            let lines_str = if session.last_size > current_size - (bytes_read as u64) && !content.starts_with('{') {
                // Might have caught a partial first line
                content.splitn(2, '\n').nth(1).unwrap_or("")
            } else {
                &content
            };

            let mut new_messages: Vec<ClaudeMessage> = Vec::new();
            for line in lines_str.lines() {
                if line.is_empty() {
                    continue;
                }
                if let Some(msg) = crate::routes_history::parse_single_jsonl_entry(line) {
                    new_messages.push(msg);
                }
            }

            if !new_messages.is_empty() {
                let event = ChatMessageEvent {
                    kind: "chat".to_string(),
                    session_file: key.clone(),
                    messages: new_messages,
                };
                // Broadcast to all WS subscribers (ignore error if no receivers)
                let _ = self.broadcast_tx.send(event);
            }
        }
    }
}
