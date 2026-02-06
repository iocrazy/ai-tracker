//! Agent module for tmux session management (async version)
//!
//! Architecture:
//! - Session = Workspace (project group)
//! - Window = Branch/Agent (programmer)
//!
//! Handles creating tmux sessions, windows, and panes for agent workspaces.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tokio::process::Command;

use crate::config::{AgentDef, LayoutConfig, PaneConfig};

/// tmux binary path (Homebrew on macOS)
pub const TMUX_BIN: &str = "/opt/homebrew/bin/tmux";

/// Information about an active agent window (branch)
#[derive(Debug, Clone, Serialize)]
pub struct AgentSession {
    /// tmux session name (workspace)
    pub session_name: String,
    /// Window name (branch)
    pub window_name: String,
    /// Workspace name
    pub workspace: String,
    /// Branch name
    pub branch: String,
    /// Whether the session is currently attached
    pub attached: bool,
}

/// AI agent status parsed from tmux pane (supports Claude Code and OpenCode)
#[derive(Debug, Clone, Serialize, Default)]
pub struct ClaudeStatus {
    /// Agent type: "claude" or "opencode"
    pub agent_type: Option<String>,
    /// Current action/status (e.g., "✻ Thinking… (1m 47s)" or "▢ Build")
    pub action: Option<String>,
    /// Current tool being executed (e.g., "Bash(npm run build...)")
    pub current_tool: Option<String>,
    /// Model name (e.g., "Opus 4.5" or "big-pickle")
    pub model: Option<String>,
    /// Context usage percentage
    pub context_percent: Option<f32>,
    /// Token count
    pub tokens: Option<u64>,
    /// Session cost in dollars
    pub cost: Option<f32>,
    /// Session duration (e.g., "2hr 29m" or "2.9s")
    pub session_duration: Option<String>,
    /// Pane number where agent is detected (e.g., "1", "2", "3")
    pub pane: Option<String>,
}

/// tmux operations for agent management (async)
pub struct TmuxAgent;

impl TmuxAgent {
    /// Generate a window name for a branch
    fn window_name(branch: &str) -> String {
        // Sanitize branch name for tmux (replace / with -)
        branch.replace('/', "-")
    }

    /// Check if a session exists (handles numbered prefix from tmux session manager)
    pub async fn session_exists(workspace: &str) -> bool {
        Self::find_session(workspace).await.is_some()
    }

    /// Find the actual tmux session name by workspace label
    /// Handles numbered prefix from tmux session manager (e.g., "5-workspace")
    async fn find_session(workspace: &str) -> Option<String> {
        let output = Command::new(TMUX_BIN)
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let name = line.trim();
            // Check if it matches directly
            if name == workspace {
                return Some(name.to_string());
            }
            // Check if it matches after stripping numbered prefix (e.g., "5-workspace")
            if let Some(idx) = name.find('-') {
                let prefix = &name[..idx];
                if prefix.chars().all(|c| c.is_ascii_digit()) {
                    let label = &name[idx + 1..];
                    if label == workspace {
                        return Some(name.to_string());
                    }
                    // Also match if workspace is just the numeric prefix (e.g., "3" matches "3-teat111")
                    if prefix == workspace {
                        return Some(name.to_string());
                    }
                }
            }
        }
        None
    }

    /// Check if a window exists in a session
    async fn window_exists(session: &str, window: &str) -> bool {
        let output = Command::new(TMUX_BIN)
            .args([
                "list-windows",
                "-t",
                session,
                "-F",
                "#{window_name}",
            ])
            .output()
            .await
            .ok();

        if let Some(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                return stdout.lines().any(|line| line.trim() == window);
            }
        }
        false
    }

    /// Create a new workspace (session + window with layout)
    /// If session exists, adds a new window; otherwise creates new session
    pub async fn create_workspace(
        workspace: &str,
        branch: &str,
        working_dir: &Path,
        layout: &LayoutConfig,
        agent: &AgentDef,
    ) -> Result<String> {
        let window_name = Self::window_name(branch);

        // Check if session exists
        let session = Self::find_session(workspace).await;

        if let Some(ref session_name) = session {
            // Session exists, check if window already exists
            if Self::window_exists(session_name, &window_name).await {
                bail!(
                    "Window '{}' already exists in session '{}'",
                    window_name,
                    session_name
                );
            }

            // Create new window in existing session
            Self::create_window(session_name, &window_name, working_dir, layout, agent).await?;
        } else {
            // Create new session with the first window
            Self::create_session(workspace, &window_name, working_dir, layout, agent).await?;
        }

        // Return the actual session name (may have been renamed with prefix)
        let actual_session = Self::find_session(workspace)
            .await
            .unwrap_or_else(|| workspace.to_string());

        Ok(actual_session)
    }

    /// Create a new session with first window
    /// Layout: yazi (left) | lazygit (top-right) | claude (bottom-right)
    async fn create_session(
        workspace: &str,
        window_name: &str,
        working_dir: &Path,
        layout: &LayoutConfig,
        agent: &AgentDef,
    ) -> Result<()> {
        // Resolve agent command in layout
        let panes = Self::resolve_layout(layout, &agent.command);

        if panes.is_empty() {
            bail!("Layout has no panes");
        }

        // Create session with empty shell first
        let output = Command::new(TMUX_BIN)
            .args([
                "new-session",
                "-d", // detached
                "-s",
                workspace,
                "-n",
                window_name,
                "-c",
                working_dir.to_str().unwrap(),
            ])
            .output()
            .await
            .context("Failed to create tmux session")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux new-session failed: {}", stderr);
        }

        // Get actual session name after creation (may have numbered prefix)
        let actual_session = Self::find_session(workspace)
            .await
            .unwrap_or_else(|| workspace.to_string());

        // Target for the window
        let target = format!("{}:{}", actual_session, window_name);

        // Build the 3-pane layout:
        // ┌──────────────┬──────────────────────────────┐
        // │    yazi      │                              │
        // │   (左上)     │          Claude              │
        // ├──────────────┤          (右，全高)          │
        // │   lazygit    │                              │
        // │   (左下)     │                              │
        // └──────────────┴──────────────────────────────┘

        // Step 1: Horizontal split (left 30%, right 70%)
        // After split, focus is on the NEW pane (right)
        Self::split_window_percent(&target, working_dir, "h", 70).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 2: Select left pane and vertical split it
        Self::select_pane_position(&target, "left").await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Self::split_window_percent(&target, working_dir, "v", 50).await?;
        // Now: top-left, bottom-left, right

        // Wait for panes to be ready
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        // Send commands to panes:
        // top-left: yazi (first command)
        if !panes.is_empty() {
            Self::send_keys_to_position(&target, "top-left", &panes[0].cmd).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // bottom-left: lazygit (second command)
        if panes.len() > 1 {
            Self::send_keys_to_position(&target, "bottom-left", &panes[1].cmd).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // right: claude/agent (third command)
        if panes.len() > 2 {
            Self::send_keys_to_position(&target, "right", &panes[2].cmd).await?;
        }

        // Focus on the agent pane (right)
        Self::select_pane_position(&target, "right").await?;

        Ok(())
    }

    /// Create a new window in existing session
    /// Layout: yazi (left) | lazygit (top-right) | claude (bottom-right)
    async fn create_window(
        session_name: &str,
        window_name: &str,
        working_dir: &Path,
        layout: &LayoutConfig,
        agent: &AgentDef,
    ) -> Result<()> {
        // Resolve agent command in layout
        let panes = Self::resolve_layout(layout, &agent.command);

        if panes.is_empty() {
            bail!("Layout has no panes");
        }

        // Create new window with empty shell
        let output = Command::new(TMUX_BIN)
            .args([
                "new-window",
                "-t",
                session_name,
                "-n",
                window_name,
                "-c",
                working_dir.to_str().unwrap(),
            ])
            .output()
            .await
            .context("Failed to create tmux window")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux new-window failed: {}", stderr);
        }

        // Target for the new window
        let target = format!("{}:{}", session_name, window_name);

        // Build the 3-pane layout:
        // ┌──────────────┬──────────────────────────────┐
        // │    yazi      │                              │
        // │   (左上)     │          Claude              │
        // ├──────────────┤          (右，全高)          │
        // │   lazygit    │                              │
        // │   (左下)     │                              │
        // └──────────────┴──────────────────────────────┘

        // Step 1: Horizontal split (left 30%, right 70%)
        // After split, focus is on the NEW pane (right)
        Self::split_window_percent(&target, working_dir, "h", 70).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 2: Select left pane and vertical split it
        Self::select_pane_position(&target, "left").await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Self::split_window_percent(&target, working_dir, "v", 50).await?;
        // Now: top-left, bottom-left, right

        // Wait for panes to be ready
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        // Send commands to panes:
        // top-left: yazi (first command)
        if !panes.is_empty() {
            Self::send_keys_to_position(&target, "top-left", &panes[0].cmd).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // bottom-left: lazygit (second command)
        if panes.len() > 1 {
            Self::send_keys_to_position(&target, "bottom-left", &panes[1].cmd).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // right: claude/agent (third command)
        if panes.len() > 2 {
            Self::send_keys_to_position(&target, "right", &panes[2].cmd).await?;
        }

        // Focus on the agent pane (right)
        Self::select_pane_position(&target, "right").await?;

        Ok(())
    }

    /// Resolve layout by replacing {agent} placeholder
    fn resolve_layout(layout: &LayoutConfig, agent_cmd: &str) -> Vec<PaneConfig> {
        layout
            .panes
            .iter()
            .map(|pane| PaneConfig {
                cmd: pane.cmd.replace("{agent}", agent_cmd),
                size: pane.size.clone(),
            })
            .collect()
    }

    /// Split window with percentage
    /// direction: "h" for horizontal (left/right), "v" for vertical (top/bottom)
    async fn split_window_percent(
        target: &str,
        working_dir: &Path,
        direction: &str,
        percent: u32,
    ) -> Result<()> {
        let dir_flag = if direction == "h" { "-h" } else { "-v" };
        let percent_str = format!("{}", percent);
        let output = Command::new(TMUX_BIN)
            .args([
                "split-window",
                dir_flag,
                "-p",
                &percent_str,
                "-t",
                target,
                "-c",
                working_dir.to_str().unwrap(),
            ])
            .output()
            .await
            .context("Failed to split tmux window")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux split-window failed: {}", stderr);
        }

        Ok(())
    }

    /// Send keys to a pane (like tmux send-keys)
    async fn send_keys(target: &str, cmd: &str) -> Result<()> {
        let output = Command::new(TMUX_BIN)
            .args(["send-keys", "-t", target, cmd, "Enter"])
            .output()
            .await
            .context("Failed to send keys to tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux send-keys failed: {}", stderr);
        }

        Ok(())
    }

    /// Select a pane
    async fn select_pane(target: &str) -> Result<()> {
        let output = Command::new(TMUX_BIN)
            .args(["select-pane", "-t", target])
            .output()
            .await
            .context("Failed to select tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux select-pane failed: {}", stderr);
        }

        Ok(())
    }

    /// Send keys to a pane using relative position (left, top-right, bottom-right)
    async fn send_keys_to_position(window_target: &str, position: &str, cmd: &str) -> Result<()> {
        // Format: session:window.{position}
        let target = format!("{}.{{{}}}", window_target, position);
        let output = Command::new(TMUX_BIN)
            .args(["send-keys", "-t", &target, cmd, "Enter"])
            .output()
            .await
            .context("Failed to send keys to tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux send-keys to {} failed: {}", position, stderr);
        }

        Ok(())
    }

    /// Select a pane using relative position
    async fn select_pane_position(window_target: &str, position: &str) -> Result<()> {
        let target = format!("{}.{{{}}}", window_target, position);
        let output = Command::new(TMUX_BIN)
            .args(["select-pane", "-t", &target])
            .output()
            .await
            .context("Failed to select tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux select-pane {} failed: {}", position, stderr);
        }

        Ok(())
    }

    /// Create a simple new window in a session (public API)
    pub async fn simple_new_window(session: &str, name: &str) -> Result<()> {
        // Find actual session name (handles numbered prefix)
        let actual_session = Self::find_session(session)
            .await
            .unwrap_or_else(|| session.to_string());

        let output = Command::new(TMUX_BIN)
            .args(["new-window", "-t", &actual_session, "-n", name])
            .output()
            .await
            .context("Failed to create tmux window")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux new-window failed: {}", stderr);
        }

        Ok(())
    }

    /// Kill a window (or session if last window)
    pub async fn kill_window(workspace: &str, branch: &str) -> Result<()> {
        // Find the actual session name
        let session_name = match Self::find_session(workspace).await {
            Some(name) => name,
            None => bail!("Session for workspace '{}' not found", workspace),
        };

        let window_name = Self::window_name(branch);
        let target = format!("{}:{}", session_name, window_name);

        // Kill the window
        let output = Command::new(TMUX_BIN)
            .args(["kill-window", "-t", &target])
            .output()
            .await
            .context("Failed to kill tmux window")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux kill-window failed: {}", stderr);
        }

        Ok(())
    }

    /// Kill entire session (all windows in workspace)
    pub async fn kill_session(workspace: &str) -> Result<()> {
        // Find the actual session name
        let session_name = match Self::find_session(workspace).await {
            Some(name) => name,
            None => bail!("Session for workspace '{}' not found", workspace),
        };

        let output = Command::new(TMUX_BIN)
            .args(["kill-session", "-t", &session_name])
            .output()
            .await
            .context("Failed to kill tmux session")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux kill-session failed: {}", stderr);
        }

        Ok(())
    }

    /// Select (switch to) a window in a session
    /// This handles cross-session switching by using switch-client
    pub async fn select_window(workspace: &str, branch: &str) -> Result<()> {
        // Find the actual session name
        let session_name = match Self::find_session(workspace).await {
            Some(name) => name,
            None => bail!("Session for workspace '{}' not found", workspace),
        };

        let window_name = Self::window_name(branch);
        let target = format!("{}:{}", session_name, window_name);

        // Use switch-client to switch to the target session:window
        // This works across sessions (unlike select-window which only works within current session)
        let output = Command::new(TMUX_BIN)
            .args(["switch-client", "-t", &target])
            .output()
            .await
            .context("Failed to switch tmux client")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux switch-client failed: {}", stderr);
        }

        Ok(())
    }

    /// Select window by target - supports window ID (like @9) or window name
    /// Window ID is more reliable when multiple windows have the same name
    pub async fn select_window_by_target(workspace: &str, window_target: &str) -> Result<()> {
        // Find the actual session name
        let session_name = match Self::find_session(workspace).await {
            Some(name) => name,
            None => bail!("Session for workspace '{}' not found", workspace),
        };

        // If window_target looks like a tmux ID (@number), use it directly
        // Otherwise treat it as a window name and sanitize it
        let target = if window_target.starts_with('@') {
            format!("{}:{}", session_name, window_target)
        } else {
            let window_name = Self::window_name(window_target);
            format!("{}:{}", session_name, window_name)
        };

        // Use switch-client to switch to the target session:window
        let output = Command::new(TMUX_BIN)
            .args(["switch-client", "-t", &target])
            .output()
            .await
            .context("Failed to switch tmux client")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux switch-client failed: {}", stderr);
        }

        Ok(())
    }

    /// List all agent windows across all workspace sessions
    pub async fn list_windows() -> Result<Vec<AgentSession>> {
        let output = Command::new(TMUX_BIN)
            .args([
                "list-windows",
                "-a", // all sessions
                "-F",
                "#{session_name}\t#{window_name}\t#{session_attached}",
            ])
            .output()
            .await
            .context("Failed to list tmux windows")?;

        if !output.status.success() {
            // No windows is not an error
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let sessions: Vec<AgentSession> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() != 3 {
                    return None;
                }
                let session_name = parts[0];
                let window_name = parts[1];
                let attached = parts[1] == "1";

                // Extract workspace from session name (strip numbered prefix)
                let workspace = if let Some(idx) = session_name.find('-') {
                    let prefix = &session_name[..idx];
                    if prefix.chars().all(|c| c.is_ascii_digit()) {
                        &session_name[idx + 1..]
                    } else {
                        session_name
                    }
                } else {
                    session_name
                };

                // Branch is the window name (already sanitized)
                Some(AgentSession {
                    session_name: session_name.to_string(),
                    window_name: window_name.to_string(),
                    workspace: workspace.to_string(),
                    branch: window_name.to_string(),
                    attached,
                })
            })
            .collect();

        Ok(sessions)
    }

    /// List windows for a specific workspace
    pub async fn list_workspace_windows(workspace: &str) -> Result<Vec<AgentSession>> {
        let all = Self::list_windows().await?;
        Ok(all
            .into_iter()
            .filter(|s| s.workspace == workspace)
            .collect())
    }

    // ============ Public tmux interaction methods ============

    /// Send keys to a specific tmux pane (public API)
    ///
    /// target format: "session:window" or "session:window.pane"
    /// If pane is empty or "0", only use session:window (sends to active pane)
    pub async fn send_keys_to_pane(
        session: &str,
        window: &str,
        pane: &str,
        keys: &str,
        enter: bool,
    ) -> Result<()> {
        // Find actual session name (handles numbered prefix)
        let actual_session = Self::find_session(session)
            .await
            .unwrap_or_else(|| session.to_string());

        // Build target: session:={window} or session:={window}.pane
        // Use ={window} format to handle window names containing colons (e.g., "fix:display-historial-data")
        // If pane is empty, "0", or not a valid pane ID, just use session:={window} (sends to active pane)
        let target = if pane.is_empty() || pane == "0" {
            format!("{}:={}", actual_session, window)
        } else if pane.starts_with('%') {
            // Pane ID format like %14 - use directly
            pane.to_string()
        } else {
            format!("{}:={}.{}", actual_session, window, pane)
        };

        let mut args = vec!["send-keys", "-t", &target, keys];
        if enter {
            args.push("C-m");  // C-m is the correct way to send Enter in tmux
        }

        let output = Command::new(TMUX_BIN)
            .args(&args)
            .output()
            .await
            .context("Failed to send keys to tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux send-keys failed: {}", stderr);
        }

        Ok(())
    }

    /// Send keys to a specific tmux pane with custom suffix key
    ///
    /// Uses two-step approach for reliable key sending:
    /// 1. Send text with -l flag (literal, no special interpretation)
    /// 2. Send suffix key separately (e.g., C-m for Enter, C-s for Ctrl+S)
    pub async fn send_keys_with_suffix(
        session: &str,
        window: &str,
        pane: &str,
        keys: &str,
        suffix_key: Option<&str>,
    ) -> Result<()> {
        // Find actual session name (handles numbered prefix)
        let actual_session = Self::find_session(session)
            .await
            .unwrap_or_else(|| session.to_string());

        // Build target: session:window or session:window.pane
        let target = if pane.is_empty() || pane == "0" {
            format!("{}:{}", actual_session, window)
        } else if pane.starts_with('%') {
            pane.to_string()
        } else {
            format!("{}:{}.{}", actual_session, window, pane)
        };

        // Step 1: Send text with -l flag (literal mode, no special char interpretation)
        tracing::info!("send_keys_with_suffix: target={}, keys={}, suffix={:?}", target, keys, suffix_key);

        let output = Command::new(TMUX_BIN)
            .args(["send-keys", "-t", &target, "-l", keys])
            .output()
            .await
            .context("Failed to send keys to tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux send-keys (text) failed: {}", stderr);
        }
        tracing::info!("send_keys_with_suffix: text sent successfully");

        // Step 2: Send suffix key separately (if provided)
        // Add small delay to ensure text is processed before sending key
        if let Some(key) = suffix_key {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            let output = Command::new(TMUX_BIN)
                .args(["send-keys", "-t", &target, key])
                .output()
                .await
                .context("Failed to send suffix key to tmux pane")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("tmux send-keys (suffix) failed: {}", stderr);
            }
            tracing::info!("send_keys_with_suffix: suffix key '{}' sent successfully", key);
        }

        Ok(())
    }

    /// Capture content from a tmux pane
    ///
    /// Returns the visible content of the pane
    pub async fn capture_pane(
        session: &str,
        window: &str,
        pane: &str,
        lines: Option<u32>,
    ) -> Result<String> {
        // Find actual session name (handles numbered prefix)
        let actual_session = Self::find_session(session)
            .await
            .unwrap_or_else(|| session.to_string());

        // Use ={window} format to handle window names containing colons
        let target = format!("{}:={}.{}", actual_session, window, pane);

        let mut args = vec![
            "capture-pane".to_string(),
            "-t".to_string(),
            target,
            "-p".to_string(), // print to stdout
        ];

        // Optionally capture last N lines
        if let Some(n) = lines {
            args.push("-S".to_string());
            args.push(format!("-{}", n)); // -S -N means start N lines from end
        }

        let output = Command::new(TMUX_BIN)
            .args(&args)
            .output()
            .await
            .context("Failed to capture tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux capture-pane failed: {}", stderr);
        }

        let content = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(content)
    }

    /// Get Claude Code status from tmux pane
    ///
    /// Parses the status bar to extract:
    /// - Current action (thinking, Propagating, etc.)
    /// - Model info
    /// - Context usage percentage
    /// - Token count
    /// - Cost
    /// - Session duration
    pub async fn get_claude_status(
        session: &str,
        window: &str,
    ) -> Result<ClaudeStatus> {
        // Try multiple panes to find Claude Code (usually in pane 1, 2, or 3)
        for pane_idx in 1..=5 {
            if let Ok(content) = Self::capture_pane(session, window, &pane_idx.to_string(), Some(10)).await {
                let mut status = Self::parse_claude_status(&content);
                // If we found any Claude status info, record the pane and return
                if status.cost.is_some() || status.model.is_some() || status.action.is_some() {
                    status.pane = Some(pane_idx.to_string());
                    return Ok(status);
                }
            }
        }
        // Return empty status if not found
        Ok(ClaudeStatus::default())
    }

    /// Strip ANSI escape codes and terminal control sequences from text
    fn strip_ansi_codes(text: &str) -> String {
        // Remove ANSI escape sequences: ESC[ ... m (colors, styles)
        // Also remove other control sequences: ESC[ ... (cursor, etc.)
        let mut result = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // ESC character - skip the escape sequence
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume '['
                    // Skip until we hit a letter (the command character)
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next.is_ascii_alphabetic() || next == 'm' {
                            break;
                        }
                    }
                }
            } else if c.is_control() && c != '\n' && c != '\t' {
                // Skip other control characters except newline and tab
                continue;
            } else {
                result.push(c);
            }
        }

        result
    }

    /// Parse AI agent status from captured pane content (Claude Code or OpenCode)
    fn parse_claude_status(content: &str) -> ClaudeStatus {
        let mut status = ClaudeStatus::default();
        // Strip ANSI codes before parsing
        let clean_content = Self::strip_ansi_codes(content);
        let lines: Vec<&str> = clean_content.lines().collect();

        for line in &lines {
            let line = line.trim();

            // ===== OpenCode Detection =====
            // OpenCode session marker: "# New session - 2026-02-05T00:46:33.604Z"
            if line.starts_with("# New session -") {
                status.agent_type = Some("opencode".to_string());
            }

            // OpenCode status line: "▢ Build · big-pickle · 2.9s"
            // Pattern: square icon + tool · model · duration (must have at least 2 · separators)
            if (line.starts_with('▢') || line.starts_with('■') || line.starts_with('□')) && line.contains('·') {
                let parts: Vec<&str> = line.split('·').collect();
                // Only match if we have the expected format (tool · model · duration)
                if parts.len() >= 2 {
                    status.agent_type = Some("opencode".to_string());
                    // First part is the action/tool (e.g., "▢ Build")
                    let action = parts[0].trim();
                    status.action = Some(action.to_string());
                    status.current_tool = Some(action.trim_start_matches(['▢', '■', '□', ' ']).to_string());

                    // Second part is the model name (e.g., "big-pickle")
                    status.model = Some(parts[1].trim().to_string());

                    if parts.len() >= 3 {
                        // Third part is the duration (e.g., "2.9s")
                        status.session_duration = Some(parts[2].trim().to_string());
                    }
                }
            }

            // ===== Claude Code Detection =====
            // Parse action line - Claude Code spinner with specific patterns
            // The symbol before the action word keeps changing (animation)
            // Examples:
            //   "✢ Lollygagging… (thinking)"
            //   "✶ Transfiguring…"
            //   "✻ Thinking… (1m 47s)"
            //   "✽ Sprouting… (34m 19s · ↑ 48.4k tokens · thinking)"
            // Key pattern: starts with spinner symbol, then a word ending in …
            // Spinner symbols: ✢✣✤✥✦✧✨✩✪✫✬✭✮✯✰✱✲✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❄❅❆❇❈❉❊❋
            let spinner_chars = ['✢', '✣', '✤', '✥', '✦', '✧', '✨', '✩', '✪', '✫', '✬', '✭', '✮', '✯', '✰',
                                 '✱', '✲', '✳', '✴', '✵', '✶', '✷', '✸', '✹', '✺', '✻', '✼', '✽', '✾', '✿',
                                 '❀', '❁', '❂', '❃', '❄', '❅', '❆', '❇', '❈', '❉', '❊', '❋'];
            let first_char = line.chars().next();
            if let Some(c) = first_char {
                if spinner_chars.contains(&c) && line.contains('…') && status.action.is_none() {
                    status.agent_type = Some("claude".to_string());
                    status.action = Some(line.to_string());
                }
            }

            // Parse tool execution line (e.g., "⏺ Bash(npm run build...)")
            if line.starts_with('⏺') {
                status.agent_type = Some("claude".to_string());
                // Extract just the tool part without the icon
                let tool_part = line.trim_start_matches('⏺').trim();
                status.current_tool = Some(tool_part.to_string());
            }

            // Parse Model line (e.g., "Model: Opus 4.5  [███░░░] 69.3% (116,482)")
            if line.contains("Model:") {
                status.agent_type = Some("claude".to_string());
                // Extract model name
                if let Some(model_part) = line.split("Model:").nth(1) {
                    let model_part = model_part.trim();
                    if let Some(model) = model_part.split_whitespace().next() {
                        status.model = Some(model.to_string());
                    }
                }
                // Extract context percentage
                if let Some(pct_idx) = line.find('%') {
                    let start = line[..pct_idx].rfind(char::is_whitespace).unwrap_or(0);
                    if let Ok(pct) = line[start..pct_idx].trim().parse::<f32>() {
                        status.context_percent = Some(pct);
                    }
                }
                // Extract token count (in parentheses)
                if let Some(start) = line.find('(') {
                    if let Some(end) = line[start..].find(')') {
                        let token_str = &line[start + 1..start + end];
                        let token_str = token_str.replace(',', "");
                        if let Ok(tokens) = token_str.trim().parse::<u64>() {
                            status.tokens = Some(tokens);
                        }
                    }
                }
            }

            // Parse Cost line (e.g., "Cost: $10.58  Session: 2hr 29m")
            if line.contains("Cost:") {
                status.agent_type = Some("claude".to_string());
                // Extract cost
                if let Some(cost_part) = line.split("Cost:").nth(1) {
                    if let Some(cost_str) = cost_part.split_whitespace().next() {
                        let cost_str = cost_str.trim_start_matches('$');
                        if let Ok(cost) = cost_str.parse::<f32>() {
                            status.cost = Some(cost);
                        }
                    }
                }
                // Extract session duration
                if let Some(sess_idx) = line.find("Session:") {
                    let sess_part = &line[sess_idx + 8..];
                    if let Some(duration) = sess_part.split_whitespace().take(2).collect::<Vec<_>>().join(" ").split('[').next() {
                        status.session_duration = Some(duration.trim().to_string());
                    }
                }
            }
        }

        status
    }

    /// List all tmux sessions with their windows
    pub async fn list_sessions() -> Result<Vec<SessionInfo>> {
        let output = Command::new(TMUX_BIN)
            .args([
                "list-sessions",
                "-F",
                "#{session_name}\t#{session_windows}\t#{session_attached}",
            ])
            .output()
            .await
            .context("Failed to list tmux sessions")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let sessions: Vec<SessionInfo> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() != 3 {
                    return None;
                }
                Some(SessionInfo {
                    name: parts[0].to_string(),
                    windows: parts[1].parse().unwrap_or(0),
                    attached: parts[2] == "1",
                })
            })
            .collect();

        Ok(sessions)
    }

    /// List panes in a window
    pub async fn list_panes(session: &str, window: &str) -> Result<Vec<PaneInfo>> {
        let actual_session = Self::find_session(session)
            .await
            .unwrap_or_else(|| session.to_string());

        let target = format!("{}:{}", actual_session, window);

        let output = Command::new(TMUX_BIN)
            .args([
                "list-panes",
                "-t",
                &target,
                "-F",
                "#{pane_index}\t#{pane_current_command}\t#{pane_width}\t#{pane_height}\t#{pane_active}",
            ])
            .output()
            .await
            .context("Failed to list tmux panes")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux list-panes failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let panes: Vec<PaneInfo> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() != 5 {
                    return None;
                }
                Some(PaneInfo {
                    index: parts[0].to_string(),
                    command: parts[1].to_string(),
                    width: parts[2].parse().unwrap_or(0),
                    height: parts[3].parse().unwrap_or(0),
                    active: parts[4] == "1",
                })
            })
            .collect();

        Ok(panes)
    }

    /// Get mapping of session_id -> session_name and window_id -> window_name
    /// Returns (session_map, window_map) - sync version for use with Mutex
    pub fn get_tmux_name_mappings_sync() -> (
        std::collections::HashMap<String, String>,
        std::collections::HashMap<String, String>,
    ) {
        let mut session_map = std::collections::HashMap::new();
        let mut window_map = std::collections::HashMap::new();

        // Get session mappings: session_id -> session_name
        if let Ok(output) = std::process::Command::new(TMUX_BIN)
            .args(["list-sessions", "-F", "#{session_id}:#{session_name}"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if let Some((id, name)) = line.split_once(':') {
                        session_map.insert(id.to_string(), name.to_string());
                    }
                }
            }
        }

        // Get window mappings: window_id -> window_name
        if let Ok(output) = std::process::Command::new(TMUX_BIN)
            .args([
                "list-windows",
                "-a",
                "-F",
                "#{window_id}:#{window_name}",
            ])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if let Some((id, name)) = line.split_once(':') {
                        window_map.insert(id.to_string(), name.to_string());
                    }
                }
            }
        }

        (session_map, window_map)
    }

    /// List all tmux windows with full details (session_id, window_id, names)
    pub async fn list_all_windows() -> Result<Vec<TmuxWindowInfo>> {
        // Use pipe separator and explicit socket path for launchd compatibility
        let output = Command::new(TMUX_BIN)
            .args([
                "-S", "/private/tmp/tmux-501/default",
                "list-windows",
                "-a",
                "-F",
                "#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{window_panes}|#{window_active}|#{pane_current_path}",
            ])
            .output()
            .await
            .context("Failed to list tmux windows")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("tmux list-all-windows failed: status={:?}, stderr={}", output.status.code(), stderr);
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Collect unique session IDs to fetch git_dir for each
        let mut session_git_dirs: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();

        let windows: Vec<TmuxWindowInfo> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() != 7 {
                    return None;
                }
                let session_id = parts[0].to_string();
                session_git_dirs.entry(session_id.clone()).or_insert(None);

                let working_dir = {
                    let p = parts[6].trim();
                    if p.is_empty() { None } else { Some(p.to_string()) }
                };

                Some(TmuxWindowInfo {
                    session_id,
                    session_name: parts[1].to_string(),
                    window_id: parts[2].to_string(),
                    window_name: parts[3].to_string(),
                    pane_count: parts[4].parse().unwrap_or(1),
                    active: parts[5] == "1",
                    git_dir: None, // Will be filled later
                    working_dir,
                })
            })
            .collect();

        // Fetch git_dir for each unique session (from @agent_main_repo or first pane path)
        let session_ids: Vec<String> = session_git_dirs.keys().cloned().collect();
        for session_id in session_ids {
            let git_dir = Self::get_session_git_dir(&session_id).await;
            session_git_dirs.insert(session_id, git_dir);
        }

        // Update windows with git_dir
        let windows: Vec<TmuxWindowInfo> = windows
            .into_iter()
            .map(|mut w| {
                w.git_dir = session_git_dirs.get(&w.session_id).cloned().flatten();
                w
            })
            .collect();

        Ok(windows)
    }

    /// Get git directory for a session
    /// First tries @agent_main_repo option, then falls back to first pane's current path
    async fn get_session_git_dir(session_id: &str) -> Option<String> {
        // Try @agent_main_repo first (if set)
        let target = format!("{}:0", session_id);
        if let Ok(Some(main_repo)) = Self::get_window_option(&target, "agent_main_repo").await {
            if !main_repo.is_empty() {
                return Some(main_repo);
            }
        }

        // Fall back to first pane's current path
        let output = Command::new(TMUX_BIN)
            .args([
                "-S", "/private/tmp/tmux-501/default",
                "display-message",
                "-t", &format!("{}:0.0", session_id),
                "-p",
                "#{pane_current_path}",
            ])
            .output()
            .await
            .ok()?;

        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                // Try to find git root from this path
                return Self::find_git_root(&path).await.or(Some(path));
            }
        }

        None
    }

    /// Find git root directory from a given path
    async fn find_git_root(path: &str) -> Option<String> {
        let output = Command::new("git")
            .args(["-C", path, "rev-parse", "--show-toplevel"])
            .output()
            .await
            .ok()?;

        if output.status.success() {
            let git_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !git_root.is_empty() {
                return Some(git_root);
            }
        }

        None
    }

    /// Synchronous version of list_all_windows (for use in non-async contexts)
    pub fn list_all_windows_sync() -> Vec<TmuxWindowInfo> {
        // Use pipe separator instead of tab to avoid shell escaping issues
        // Use explicit socket path with /private/tmp to work with launchd
        let output = std::process::Command::new(TMUX_BIN)
            .args([
                "-S", "/private/tmp/tmux-501/default",
                "list-windows",
                "-a",
                "-F",
                "#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{window_panes}|#{window_active}|#{pane_current_path}",
            ])
            .output();

        match &output {
            Ok(out) => {
                let stdout_str = String::from_utf8_lossy(&out.stdout);
                let stderr_str = String::from_utf8_lossy(&out.stderr);
                // Log first line to see the format
                if let Some(first_line) = stdout_str.lines().next() {
                    tracing::info!("tmux list-windows: status={}, first_line={:?}, stderr={}",
                        out.status, first_line, stderr_str);
                }
            }
            Err(e) => {
                tracing::error!("Failed to execute tmux: {}", e);
            }
        }

        match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);

                // Collect unique session IDs
                let mut session_git_dirs: std::collections::HashMap<String, Option<String>> =
                    std::collections::HashMap::new();

                let windows: Vec<TmuxWindowInfo> = stdout
                    .lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.split('|').collect();
                        if parts.len() != 7 {
                            tracing::warn!("Skipping invalid line: {}", line);
                            return None;
                        }
                        let session_id = parts[0].to_string();
                        session_git_dirs.entry(session_id.clone()).or_insert(None);

                        let working_dir = {
                            let p = parts[6].trim();
                            if p.is_empty() { None } else { Some(p.to_string()) }
                        };

                        Some(TmuxWindowInfo {
                            session_id,
                            session_name: parts[1].to_string(),
                            window_id: parts[2].to_string(),
                            window_name: parts[3].to_string(),
                            pane_count: parts[4].parse().unwrap_or(1),
                            active: parts[5] == "1",
                            git_dir: None,
                            working_dir,
                        })
                    })
                    .collect();

                // Fetch git_dir for each unique session
                for session_id in session_git_dirs.keys().cloned().collect::<Vec<_>>() {
                    let git_dir = Self::get_session_git_dir_sync(&session_id);
                    session_git_dirs.insert(session_id, git_dir);
                }

                // Update windows with git_dir
                windows
                    .into_iter()
                    .map(|mut w| {
                        w.git_dir = session_git_dirs.get(&w.session_id).cloned().flatten();
                        w
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Synchronous version of get_session_git_dir
    fn get_session_git_dir_sync(session_id: &str) -> Option<String> {
        // Get first pane's current path
        let output = std::process::Command::new(TMUX_BIN)
            .args([
                "-S", "/private/tmp/tmux-501/default",
                "display-message",
                "-t", &format!("{}:0.0", session_id),
                "-p",
                "#{pane_current_path}",
            ])
            .output()
            .ok()?;

        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                // Try to find git root from this path
                return Self::find_git_root_sync(&path).or(Some(path));
            }
        }

        None
    }

    /// Synchronous version of find_git_root
    fn find_git_root_sync(path: &str) -> Option<String> {
        let output = std::process::Command::new("git")
            .args(["-C", path, "rev-parse", "--show-toplevel"])
            .output()
            .ok()?;

        if output.status.success() {
            let git_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !git_root.is_empty() {
                return Some(git_root);
            }
        }

        None
    }

    // ============ tmux Window Options (Metadata Storage) ============

    /// Set a custom option on a tmux window
    ///
    /// Stores metadata like @agent_port, @agent_dir, etc.
    /// Format: tmux set-option -w -t {target} @{key} "{value}"
    pub async fn set_window_option(target: &str, key: &str, value: &str) -> Result<()> {
        let option_name = format!("@{}", key);
        let output = Command::new(TMUX_BIN)
            .args(["set-option", "-w", "-t", target, &option_name, value])
            .output()
            .await
            .context("Failed to set tmux window option")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux set-option failed: {}", stderr);
        }

        Ok(())
    }

    /// Get a custom option from a tmux window
    ///
    /// Retrieves metadata like @agent_port, @agent_dir, etc.
    /// Returns None if option is not set
    pub async fn get_window_option(target: &str, key: &str) -> Result<Option<String>> {
        let format_str = format!("#{{@{}}}", key);
        let output = Command::new(TMUX_BIN)
            .args(["display-message", "-t", target, "-p", &format_str])
            .output()
            .await
            .context("Failed to get tmux window option")?;

        if !output.status.success() {
            // Window might not exist, return None
            return Ok(None);
        }

        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Empty string means option not set
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }

    /// Set multiple window options at once
    pub async fn set_window_options(target: &str, options: &[(&str, &str)]) -> Result<()> {
        for (key, value) in options {
            Self::set_window_option(target, key, value).await?;
        }
        Ok(())
    }

    /// Rename a tmux window
    /// Format: tmux rename-window -t {target} "{name}"
    pub async fn rename_window(target: &str, name: &str) -> Result<()> {
        let output = Command::new(TMUX_BIN)
            .args(["rename-window", "-t", target, name])
            .output()
            .await
            .context("Failed to rename tmux window")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux rename-window failed: {}", stderr);
        }

        Ok(())
    }

    /// Update window name with status icon prefix
    /// Status icons match session icons in tmux-status/left.sh:
    /// - in_progress/BUSY → ⏳
    /// - awaiting_input/PAUSED → 🚧
    /// - completed (unacknowledged) → 🔔
    /// - completed (acknowledged) / IDLE → no icon
    pub async fn update_window_status_icon(session_id: &str, window_id: &str, status: &str, acknowledged: bool) -> Result<()> {
        // Get base window name (stored or current)
        let target = format!("{}:{}", session_id, window_id);

        // Get stored base name or current window name
        let base_name = Self::get_window_option(&target, "agent_base_name")
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                // Fallback: get current window name without any icon prefix
                let current = Self::get_window_name_sync(&target);
                // Strip any existing icon prefix
                let stripped = current
                    .trim_start_matches("● ")
                    .trim_start_matches("◉ ")
                    .trim_start_matches("✓ ")
                    .trim_start_matches("⏸ ")
                    .trim_start_matches("⏳ ")
                    .trim_start_matches("🚧 ")
                    .trim_start_matches("🔔 ")
                    .trim_start_matches("✅ ")
                    .trim_start_matches("🔄 ")
                    .trim_start_matches("⏸️ ")
                    .to_string();
                if stripped.is_empty() { current } else { stripped }
            });

        // Store base name for future updates
        let _ = Self::set_window_option(&target, "agent_base_name", &base_name).await;

        // Add status icon prefix (simple Unicode for better compatibility)
        let icon = match (status, acknowledged) {
            ("BUSY", _) => "●",               // in_progress (filled circle)
            ("PAUSED", _) => "⏸",             // awaiting_input (pause)
            ("COMPLETED", false) => "✓",      // completed but not acknowledged
            ("COMPLETED", true) | _ => "",    // acknowledged or IDLE - no icon
        };

        let new_name = if icon.is_empty() {
            base_name
        } else {
            format!("{} {}", icon, base_name)
        };

        Self::rename_window(&target, &new_name).await
    }

    /// Get window name synchronously (for fallback)
    fn get_window_name_sync(target: &str) -> String {
        std::process::Command::new(TMUX_BIN)
            .args(["display-message", "-t", target, "-p", "#{window_name}"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }

    /// Get all agent-related options from a window
    pub async fn get_agent_metadata(target: &str) -> AgentMetadata {
        let port = Self::get_window_option(target, "agent_port")
            .await
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok());

        let frontend_port = Self::get_window_option(target, "agent_frontend_port")
            .await
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok());

        let backend_port = Self::get_window_option(target, "agent_backend_port")
            .await
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok());

        let dir = Self::get_window_option(target, "agent_dir")
            .await
            .ok()
            .flatten();

        let main_repo = Self::get_window_option(target, "agent_main_repo")
            .await
            .ok()
            .flatten();

        let browser = Self::get_window_option(target, "agent_browser")
            .await
            .ok()
            .flatten();

        let name = Self::get_window_option(target, "agent_name")
            .await
            .ok()
            .flatten();

        let fullstack = Self::get_window_option(target, "agent_fullstack")
            .await
            .ok()
            .flatten()
            .map(|s| s == "true")
            .unwrap_or(false);

        AgentMetadata {
            port,
            frontend_port,
            backend_port,
            dir,
            main_repo,
            browser,
            name,
            fullstack,
        }
    }
}

/// Agent metadata stored in tmux window options
#[derive(Debug, Clone, Default)]
pub struct AgentMetadata {
    pub port: Option<u16>,
    pub frontend_port: Option<u16>,
    pub backend_port: Option<u16>,
    pub dir: Option<String>,
    pub main_repo: Option<String>,
    pub browser: Option<String>,
    pub name: Option<String>,
    pub fullstack: bool,
}

/// Information about a tmux session
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub name: String,
    pub windows: u32,
    pub attached: bool,
}

/// Full tmux window information with IDs
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TmuxWindowInfo {
    pub session_id: String,
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
    pub pane_count: u32,
    pub active: bool,
    /// Git directory for the session (from @agent_main_repo or first pane's path)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_dir: Option<String>,
    /// Working directory of the first pane (from #{pane_current_path})
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

/// Information about a tmux pane
#[derive(Debug, Clone, Serialize)]
pub struct PaneInfo {
    pub index: String,
    pub command: String,
    pub width: u32,
    pub height: u32,
    pub active: bool,
}
