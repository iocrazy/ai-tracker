//! Stream module for real-time pane output capture via tmux pipe-pane
//!
//! Architecture:
//! - tmux pipe-pane sends pane output to a named pipe (FIFO)
//! - Server reads from the pipe asynchronously
//! - Output is parsed (ANSI codes stripped) and broadcast via WebSocket

use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::AsyncBufReadExt;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

use crate::agent::TMUX_BIN;

/// Stream data sent to clients
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamChunk {
    /// Pane identifier (e.g., "%3")
    pub pane_id: String,
    /// Session:window target
    pub target: String,
    /// Raw content (may contain ANSI codes)
    pub raw: String,
    /// Cleaned content (ANSI codes stripped)
    pub text: String,
    /// Timestamp
    pub timestamp: String,
}

/// Manages active pane streams
pub struct StreamManager {
    /// Active streams: pane_id -> stream info
    active_streams: RwLock<HashMap<String, StreamInfo>>,
    /// Broadcast channel for stream chunks
    broadcast_tx: broadcast::Sender<StreamChunk>,
    /// Base directory for FIFOs
    fifo_dir: PathBuf,
}

struct StreamInfo {
    target: String,
    fifo_path: PathBuf,
    /// Handle to cancel the reader task
    cancel_tx: tokio::sync::oneshot::Sender<()>,
}

impl StreamManager {
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(256);
        let fifo_dir = PathBuf::from("/tmp/tracker-streams");

        // Ensure FIFO directory exists
        let _ = std::fs::create_dir_all(&fifo_dir);

        Self {
            active_streams: RwLock::new(HashMap::new()),
            broadcast_tx,
            fifo_dir,
        }
    }

    /// Subscribe to stream updates
    pub fn subscribe(&self) -> broadcast::Receiver<StreamChunk> {
        self.broadcast_tx.subscribe()
    }

    /// Start streaming output from a pane
    pub async fn start_stream(&self, session: &str, window: &str, pane: &str) -> Result<String, String> {
        let target = format!("{}:{}.{}", session, window, pane);
        let pane_id = pane.to_string();

        // Check if already streaming
        {
            let streams = self.active_streams.read().await;
            if streams.contains_key(&pane_id) {
                return Err(format!("Already streaming pane {}", pane_id));
            }
        }

        // Create FIFO path
        let fifo_path = self.fifo_dir.join(format!("stream-{}.fifo", pane_id.replace("%", "")));

        // Remove existing FIFO if any
        let _ = std::fs::remove_file(&fifo_path);

        // Create named pipe (FIFO)
        let fifo_path_str = fifo_path.to_string_lossy().to_string();
        let mkfifo_output = std::process::Command::new("mkfifo")
            .arg(&fifo_path_str)
            .output()
            .map_err(|e| format!("Failed to create FIFO: {}", e))?;

        if !mkfifo_output.status.success() {
            return Err(format!("mkfifo failed: {}", String::from_utf8_lossy(&mkfifo_output.stderr)));
        }

        // Start tmux pipe-pane
        let pipe_cmd = format!("cat >> {}", fifo_path_str);
        let output = std::process::Command::new(TMUX_BIN)
            .args(["pipe-pane", "-t", &target, &pipe_cmd])
            .output()
            .map_err(|e| format!("Failed to start pipe-pane: {}", e))?;

        if !output.status.success() {
            let _ = std::fs::remove_file(&fifo_path);
            return Err(format!("pipe-pane failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        info!("Started streaming pane {} -> {}", target, fifo_path_str);

        // Create cancel channel
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();

        // Store stream info
        {
            let mut streams = self.active_streams.write().await;
            streams.insert(pane_id.clone(), StreamInfo {
                target: target.clone(),
                fifo_path: fifo_path.clone(),
                cancel_tx,
            });
        }

        // Spawn reader task
        let broadcast_tx = self.broadcast_tx.clone();
        let pane_id_clone = pane_id.clone();
        let target_clone = target.clone();
        let fifo_path_clone = fifo_path.clone();

        tokio::spawn(async move {
            Self::read_fifo_loop(
                fifo_path_clone,
                pane_id_clone,
                target_clone,
                broadcast_tx,
                cancel_rx,
            ).await;
        });

        Ok(target)
    }

    /// Stop streaming from a pane
    pub async fn stop_stream(&self, pane: &str) -> Result<(), String> {
        let pane_id = pane.to_string();

        let stream_info = {
            let mut streams = self.active_streams.write().await;
            streams.remove(&pane_id)
        };

        if let Some(info) = stream_info {
            // Stop tmux pipe-pane (empty command stops it)
            let _ = std::process::Command::new(TMUX_BIN)
                .args(["pipe-pane", "-t", &info.target])
                .output();

            // Signal reader task to stop
            let _ = info.cancel_tx.send(());

            // Clean up FIFO
            let _ = std::fs::remove_file(&info.fifo_path);

            info!("Stopped streaming pane {}", pane_id);
            Ok(())
        } else {
            Err(format!("No active stream for pane {}", pane_id))
        }
    }

    /// List active streams
    pub async fn list_streams(&self) -> Vec<(String, String)> {
        let streams = self.active_streams.read().await;
        streams.iter().map(|(k, v)| (k.clone(), v.target.clone())).collect()
    }

    /// Read from FIFO and broadcast chunks
    async fn read_fifo_loop(
        fifo_path: PathBuf,
        pane_id: String,
        target: String,
        broadcast_tx: broadcast::Sender<StreamChunk>,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        // Open FIFO for reading (this blocks until writer connects)
        let file = match tokio::fs::File::open(&fifo_path).await {
            Ok(f) => f,
            Err(e) => {
                error!("Failed to open FIFO {}: {}", fifo_path.display(), e);
                return;
            }
        };

        let reader = tokio::io::BufReader::new(file);
        let mut lines = reader.lines();

        loop {
            tokio::select! {
                _ = &mut cancel_rx => {
                    debug!("Stream reader cancelled for {}", pane_id);
                    break;
                }
                result = lines.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            let text = strip_ansi_codes(&line);

                            // Skip empty lines and spinner-only lines
                            if text.trim().is_empty() {
                                continue;
                            }

                            let chunk = StreamChunk {
                                pane_id: pane_id.clone(),
                                target: target.clone(),
                                raw: line,
                                text,
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            };

                            if broadcast_tx.send(chunk).is_err() {
                                // No subscribers, but keep reading
                            }
                        }
                        Ok(None) => {
                            // EOF - pipe closed, wait a bit and try to reopen
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                        Err(e) => {
                            warn!("Error reading from FIFO: {}", e);
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                    }
                }
            }
        }
    }
}

/// Strip ANSI escape codes from text
fn strip_ansi_codes(text: &str) -> String {
    // Simple ANSI escape sequence removal
    // Handles: ESC[...m (colors), ESC[...H (cursor), etc.
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Start of escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit a letter (end of sequence)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() || next == '?' {
                        break;
                    }
                }
            }
        } else if c == '\r' {
            // Skip carriage return
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi_codes("\x1b[32mHello\x1b[0m"), "Hello");
        assert_eq!(strip_ansi_codes("\x1b[2J\x1b[HTest"), "Test");
        assert_eq!(strip_ansi_codes("Plain text"), "Plain text");
    }
}
