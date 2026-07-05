//! Agent module for tmux session management (async version)
//!
//! Architecture:
//! - Session = Workspace (project group)
//! - Window = Branch/Agent (programmer)
//!
//! Handles creating tmux sessions, windows, and panes for agent workspaces.

use std::path::Path;
use std::sync::LazyLock;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tokio::process::Command;

use crate::config::{AgentDef, LayoutConfig, PaneConfig};

/// tmux binary path — resolved at first use via $PATH, with platform fallback
pub static TMUX_BIN: LazyLock<String> = LazyLock::new(|| {
    if let Ok(output) = std::process::Command::new("which").arg("tmux").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return path;
            }
        }
    }
    if cfg!(target_os = "macos") {
        "/opt/homebrew/bin/tmux".to_string()
    } else {
        "/usr/bin/tmux".to_string()
    }
});

/// Default tmux socket path — resolved per platform using current UID
pub static TMUX_SOCKET: LazyLock<String> = LazyLock::new(|| {
    let uid = unsafe { libc::getuid() };
    if cfg!(target_os = "macos") {
        format!("/private/tmp/tmux-{}/default", uid)
    } else {
        format!("/tmp/tmux-{}/default", uid)
    }
});

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
    /// Whether the agent is showing a permission prompt (waiting for user input)
    #[serde(default)]
    pub awaiting_permission: bool,
    /// Whether the agent is at "Resume Session" picker (needs user to select a session)
    #[serde(default)]
    pub awaiting_resume: bool,
    /// Interactive menu detected from TUI (AskUserQuestion picker)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_menu: Option<PendingMenu>,
}

/// An interactive menu parsed from Claude Code's TUI pane
#[derive(Debug, Clone, Serialize)]
pub struct PendingMenu {
    pub header: String,
    /// Question text between header and first option
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    pub options: Vec<MenuOption>,
    /// Right-side preview panel content (box-drawing bordered area)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Whether this is a multi-select menu (checkboxes)
    #[serde(default)]
    pub multi_select: bool,
}

/// A single option in a TUI menu
#[derive(Debug, Clone, Serialize)]
pub struct MenuOption {
    pub index: usize,
    pub label: String,
    pub description: String,
    /// Currently highlighted by cursor (❯)
    pub selected: bool,
    /// Checkbox checked state (multi-select only)
    #[serde(default)]
    pub checked: bool,
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

    /// Public version of find_session for use in route handlers
    pub async fn find_session_public(workspace: &str) -> Option<String> {
        Self::find_session(workspace).await
    }

    /// Find the actual tmux session name by workspace label
    /// Handles numbered prefix from tmux session manager (e.g., "5-workspace")
    async fn find_session(workspace: &str) -> Option<String> {
        let output = Command::new(TMUX_BIN.as_str())
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
    pub async fn window_exists(session: &str, window: &str) -> bool {
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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

        // Wait for tmux session-created hook to finish renaming (adds numbered prefix)
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Get actual session name after creation (may have numbered prefix from hook)
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
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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

        let output = Command::new(TMUX_BIN.as_str())
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

    /// Create a simple new window in a session with a specific working directory
    pub async fn simple_new_window_with_dir(session: &str, name: &str, working_dir: &str) -> Result<()> {
        let actual_session = Self::find_session(session)
            .await
            .unwrap_or_else(|| session.to_string());

        let output = Command::new(TMUX_BIN.as_str())
            .args(["new-window", "-t", &actual_session, "-n", name, "-c", working_dir])
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
        let output = Command::new(TMUX_BIN.as_str())
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

        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
            .args(["switch-client", "-t", &target])
            .output()
            .await
            .context("Failed to switch tmux client")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux switch-client failed: {}", stderr);
        }

        // Activate the terminal application on macOS so it comes to foreground
        #[cfg(target_os = "macos")]
        {
            // Detect terminal from tmux client's process ancestry
            let terminal_app = Self::detect_terminal_app().await;
            let script = format!("tell application \"{}\" to activate", terminal_app);
            let _ = Command::new("osascript")
                .args(["-e", &script])
                .output()
                .await;
        }

        Ok(())
    }

    /// Detect which terminal application owns the tmux client by tracing the process tree
    #[cfg(target_os = "macos")]
    async fn detect_terminal_app() -> String {
        // Get tmux client PID
        let client_pid = Command::new(TMUX_BIN.as_str())
            .args(["-S", TMUX_SOCKET.as_str(), "display-message", "-p", "#{client_pid}"])
            .output()
            .await
            .ok()
            .and_then(|o| if o.status.success() {
                String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok()
            } else {
                None
            });

        if let Some(pid) = client_pid {
            // Walk up the process tree to find a known terminal app
            let output = std::process::Command::new("bash")
                .args(["-c", &format!(
                    "PID={}; for i in 1 2 3 4 5 6; do PARENT=$(ps -p $PID -o ppid= 2>/dev/null | tr -d ' '); \
                     [ -z \"$PARENT\" ] && break; COMM=$(ps -p $PARENT -o comm= 2>/dev/null); \
                     echo \"$COMM\"; PID=$PARENT; done", pid
                )])
                .output()
                .ok();

            if let Some(out) = output {
                let tree = String::from_utf8_lossy(&out.stdout).to_lowercase();
                if tree.contains("iterm") {
                    return "iTerm".to_string();
                } else if tree.contains("alacritty") {
                    return "Alacritty".to_string();
                } else if tree.contains("wezterm") {
                    return "WezTerm".to_string();
                } else if tree.contains("kitty") {
                    return "kitty".to_string();
                }
            }
        }

        // Default fallback
        "Terminal".to_string()
    }

    /// List all agent windows across all workspace sessions
    pub async fn list_windows() -> Result<Vec<AgentSession>> {
        let output = Command::new(TMUX_BIN.as_str())
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

        let output = Command::new(TMUX_BIN.as_str())
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

        // Step 0: Exit copy-mode if active (prevents / from triggering search in copy-mode-vi)
        let _ = Command::new(TMUX_BIN.as_str())
            .args(["send-keys", "-t", &target, "-X", "cancel"])
            .output()
            .await;

        // Step 1: Send text with -l flag (literal mode, no special char interpretation)
        // Skip when keys is empty (e.g., raw key-only sends like Down/Space/Enter)
        tracing::info!("send_keys_with_suffix: target={}, keys={}, suffix={:?}", target, keys, suffix_key);

        if !keys.is_empty() {
            // Slash commands (e.g., /qa, /commit) need character-by-character input
            // so Claude Code's TUI can trigger the autocomplete menu.
            // Regular text uses -l (literal) for efficiency.
            if keys.starts_with('/') && keys.len() <= 50 && !keys.contains(' ') {
                tracing::info!("send_keys_with_suffix: sending slash command char-by-char");
                for ch in keys.chars() {
                    let ch_str = ch.to_string();
                    let output = Command::new(TMUX_BIN.as_str())
                        .args(["send-keys", "-t", &target, "-l", &ch_str])
                        .output()
                        .await
                        .context("Failed to send char to tmux pane")?;
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        bail!("tmux send-keys (char) failed: {}", stderr);
                    }
                    // Small delay between chars for TUI to process
                    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
                }
                // Extra delay after slash command for menu to appear
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            } else if keys.len() > 500 {
                // Large payloads: use tmux paste buffer for reliability.
                // 500 bytes threshold — Chinese text is 3 bytes/char, so ~170 chars.
                use tokio::io::AsyncWriteExt;
                let buf_name = format!("agent-tracker-{}", std::process::id());
                let mut child = tokio::process::Command::new(TMUX_BIN.as_str())
                    .args(["load-buffer", "-b", &buf_name, "-"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .context("Failed to spawn tmux load-buffer")?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(keys.as_bytes()).await
                        .context("Failed to write to tmux load-buffer stdin")?;
                }
                let output = child.wait_with_output().await
                    .context("Failed to wait for tmux load-buffer")?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!("tmux load-buffer failed: {}", stderr);
                }
                let output = Command::new(TMUX_BIN.as_str())
                    .args(["paste-buffer", "-b", &buf_name, "-d", "-t", &target])
                    .output()
                    .await
                    .context("Failed to paste tmux buffer")?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!("tmux paste-buffer failed: {}", stderr);
                }
            } else {
                let output = Command::new(TMUX_BIN.as_str())
                    .args(["send-keys", "-t", &target, "-l", keys])
                    .output()
                    .await
                    .context("Failed to send keys to tmux pane")?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!("tmux send-keys (text) failed: {}", stderr);
                }
            }
            tracing::info!("send_keys_with_suffix: text sent successfully");
        }

        // Step 2: Send suffix key separately (if provided)
        // Delay: 500ms after paste-buffer (Claude TUI needs time to process large paste),
        // 50ms after send-keys -l (instant for small text).
        if let Some(key) = suffix_key {
            let delay = if keys.len() > 500 { 500 } else { 50 };
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;

            let output = Command::new(TMUX_BIN.as_str())
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

    /// Paste text into a pane via tmux paste-buffer (simulates a real paste, not keystrokes).
    /// Claude Code's image-path auto-attachment only triggers on paste events, not typed input.
    pub async fn paste_text(
        session: &str,
        window: &str,
        pane: &str,
        text: &str,
        suffix_key: Option<&str>,
    ) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let actual_session = Self::find_session(session)
            .await
            .ok_or_else(|| anyhow::anyhow!("Session '{}' not found", session))?;
        let target = if pane.is_empty() || pane == "0" {
            format!("{}:{}", actual_session, window)
        } else if pane.starts_with('%') {
            pane.to_string()
        } else {
            format!("{}:{}.{}", actual_session, window, pane)
        };

        let buf_name = format!("agent-tracker-paste-{}", std::process::id());

        let mut child = tokio::process::Command::new(TMUX_BIN.as_str())
            .args(["load-buffer", "-b", &buf_name, "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn tmux load-buffer")?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).await
                .context("Failed to write to tmux load-buffer stdin")?;
        }
        let output = child.wait_with_output().await
            .context("Failed to wait for tmux load-buffer")?;
        if !output.status.success() {
            bail!("tmux load-buffer failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        let output = Command::new(TMUX_BIN.as_str())
            .args(["paste-buffer", "-b", &buf_name, "-d", "-t", &target])
            .output()
            .await
            .context("Failed to paste tmux buffer")?;
        if !output.status.success() {
            bail!("tmux paste-buffer failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        if let Some(key) = suffix_key {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let output = Command::new(TMUX_BIN.as_str())
                .args(["send-keys", "-t", &target, key])
                .output()
                .await
                .context("Failed to send suffix key")?;
            if !output.status.success() {
                bail!("tmux send-keys (suffix) failed: {}", String::from_utf8_lossy(&output.stderr));
            }
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

        let output = Command::new(TMUX_BIN.as_str())
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
        // First: detect Claude Code pane by process name (most reliable)
        let claude_pane_id = Self::find_claude_pane(session, window).await;

        // If we found a Claude pane by process, try to parse its status
        if let Some(ref pane_id) = claude_pane_id {
            // First pass: 10 lines for basic status
            if let Ok(content) = Self::capture_pane(session, window, pane_id, Some(10)).await {
                let mut status = Self::parse_claude_status(&content);
                status.pane = Some(pane_id.clone());
                // If menu footer detected, re-capture with more lines to get full menu
                if content.contains("Enter to select") && content.contains("to navigate") {
                    if let Ok(full_content) = Self::capture_pane(session, window, pane_id, Some(60)).await {
                        status.pending_menu = Self::parse_tui_menu(&full_content);
                    }
                }
                return Ok(status);
            }
        }

        // Fallback: scan panes by content parsing (for non-standard setups)
        for pane_idx in 1..=5 {
            if let Ok(content) = Self::capture_pane(session, window, &pane_idx.to_string(), Some(10)).await {
                let mut status = Self::parse_claude_status(&content);
                if status.cost.is_some() || status.model.is_some() || status.action.is_some() {
                    status.pane = Some(pane_idx.to_string());
                    return Ok(status);
                }
            }
        }
        // Return empty status if not found
        Ok(ClaudeStatus::default())
    }

    /// Find the pane running Claude Code or OpenCode by checking process names.
    /// Returns the pane ID (e.g., "%4") for reliable targeting.
    async fn find_claude_pane(session: &str, window: &str) -> Option<String> {
        let actual_session = Self::find_session(session)
            .await
            .unwrap_or_else(|| session.to_string());

        // Use window ID directly if it starts with '@', otherwise use '=' prefix
        // for exact name match. Note: window names with dots (e.g., "2.1.45") cause
        // tmux to misparse them as window.pane notation, so we also use -F filter
        // as a fallback.
        let target = if window.starts_with('@') {
            format!("{}:{}", actual_session, window)
        } else {
            format!("{}:={}", actual_session, window)
        };

        // List panes with their current command and pane ID
        let output = Command::new(TMUX_BIN.as_str())
            .args(["list-panes", "-t", &target, "-F", "#{pane_id} #{pane_current_command}"])
            .output()
            .await
            .ok()?;

        // If exact name match fails (e.g., dots in window name), try listing all
        // windows and filter by name manually
        if !output.status.success() && !window.starts_with('@') {
            return Self::find_claude_pane_by_window_name(session, &actual_session, window).await;
        }

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() != 2 {
                continue;
            }
            let pane_id = parts[0];   // e.g., "%4"
            let command = parts[1];    // e.g., "2.1.39" or "claude" or "opencode"

            // Match Claude Code: version-like command (e.g., "2.1.39") or "claude"
            if command.starts_with("2.") || command.starts_with("1.")
                || command == "claude" || command == "claude-code"
                || command == "opencode"
            {
                return Some(pane_id.to_string());
            }
        }

        None
    }

    /// Fallback: find Claude pane when window name contains dots (e.g., "2.1.45")
    /// which tmux misparses as window.pane notation.
    async fn find_claude_pane_by_window_name(
        _original_session: &str,
        actual_session: &str,
        window_name: &str,
    ) -> Option<String> {
        // List all windows to find the one matching by name
        let output = Command::new(TMUX_BIN.as_str())
            .args([
                "list-windows", "-t", actual_session,
                "-F", "#{window_id} #{window_name}",
            ])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let window_id = stdout.lines().find_map(|line| {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() == 2 && parts[1] == window_name {
                Some(parts[0].to_string())
            } else {
                None
            }
        })?;

        // Now list panes using window ID (safe, no dot issues)
        let target = format!("{}:{}", actual_session, window_id);
        let output = Command::new(TMUX_BIN.as_str())
            .args(["list-panes", "-t", &target, "-F", "#{pane_id} #{pane_current_command}"])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() != 2 {
                continue;
            }
            let pane_id = parts[0];
            let command = parts[1];
            if command.starts_with("2.") || command.starts_with("1.")
                || command == "claude" || command == "claude-code"
                || command == "opencode"
            {
                return Some(pane_id.to_string());
            }
        }

        None
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

            // Parse tool execution line (e.g., "⏺ Bash(npm run build...)" or "⏺ Read(...)")
            // Only match when followed by a known tool pattern (capitalized word + parens or known tool names)
            if line.starts_with('⏺') {
                let tool_part = line.trim_start_matches('⏺').trim();
                // Match tool patterns: "ToolName(args)" or "Skill(name)" etc.
                let is_tool = tool_part.starts_with("Bash")
                    || tool_part.starts_with("Read")
                    || tool_part.starts_with("Write")
                    || tool_part.starts_with("Edit")
                    || tool_part.starts_with("Grep")
                    || tool_part.starts_with("Glob")
                    || tool_part.starts_with("Skill")
                    || tool_part.starts_with("Agent")
                    || tool_part.starts_with("Task")
                    || tool_part.starts_with("mcp_")
                    || tool_part.starts_with("WebFetch")
                    || tool_part.starts_with("WebSearch");
                if is_tool {
                    status.agent_type = Some("claude".to_string());
                    status.current_tool = Some(tool_part.to_string());
                }
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

            // Parse new-style status bar (Claude Code v2.1.108+)
            // Format: "🤖 Opus 4.7 1M | 📁 project | 🌿 branch | ⚡️ 44.3% · 443.3k tokens"
            if line.contains("🤖") && line.contains('|') {
                status.agent_type = Some("claude".to_string());
                let parts: Vec<&str> = line.split('|').collect();
                // Part 0: "🤖 Opus 4.7 1M" → model
                if let Some(model_part) = parts.first() {
                    let model_str = model_part.trim().trim_start_matches('🤖').trim();
                    if !model_str.is_empty() {
                        status.model = Some(model_str.to_string());
                    }
                }
                // Find the part with ⚡️ (context/tokens)
                for part in &parts {
                    let p = part.trim();
                    if p.contains('⚡') || p.contains('%') {
                        // Extract percentage (e.g., "44.3%")
                        if let Some(pct_idx) = p.find('%') {
                            // Scan backwards for the number
                            let before = &p[..pct_idx];
                            let num_start = before.rfind(|c: char| !c.is_ascii_digit() && c != '.').map(|i| i + 1).unwrap_or(0);
                            if let Ok(pct) = before[num_start..].trim().parse::<f32>() {
                                status.context_percent = Some(pct);
                            }
                        }
                        // Extract token count (e.g., "443.3k tokens" or "59.0k")
                        if let Some(k_idx) = p.find('k') {
                            // Look for number before 'k'
                            let before_k = &p[..k_idx];
                            let num_start = before_k.rfind(|c: char| !c.is_ascii_digit() && c != '.').map(|i| i + 1).unwrap_or(0);
                            if let Ok(k_val) = before_k[num_start..].trim().parse::<f64>() {
                                status.tokens = Some((k_val * 1000.0) as u64);
                            }
                        }
                    }
                }
            }

            // Parse Cost line (e.g., "Cost: $10.58  Session: 2hr 29m") — legacy format
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

        // Detect permission prompts (Claude waiting for user to approve an action)
        // Patterns: "Do you want to", "Esc to cancel", permission choice lines
        for line in &lines {
            let line = line.trim();
            if line.contains("Do you want to")
                || line.contains("Esc to cancel")
                || line.contains("Yes, allow all")
                || (line.starts_with('>') && line.contains("Yes"))
            {
                status.awaiting_permission = true;
                break;
            }
        }

        // Detect "Resume Session" picker (claude --resume showing session list)
        // Patterns: "Resume Session", "current worktree", "Type to search"
        for line in &lines {
            let line = line.trim();
            if line.contains("Resume Session") || line.contains("Type to search") {
                status.awaiting_resume = true;
                status.agent_type = Some("claude".to_string());
                break;
            }
        }

        status
    }

    /// Parse an interactive TUI menu from pane content.
    /// Detects the "Enter to select · ↑/↓ to navigate" picker and extracts options.
    ///
    /// Menu format:
    /// ```
    /// ☐ Header text
    /// ❯ 1. Label (selected)
    ///      Description...
    ///   2. Label
    ///      Description...
    /// ───────────
    ///   N. Chat about this
    /// Enter to select · ↑/↓ to navigate · Esc to cancel
    /// ```
    fn parse_tui_menu(content: &str) -> Option<PendingMenu> {
        // Strip box-drawing characters and everything after them on each line.
        let clean_line = |line: &str| -> String {
            if let Some(pos) = line.find(|c: char| "┌┐└┘│┃╔╗╚╝║".contains(c)) {
                line[..pos].trim_end().to_string()
            } else {
                line.to_string()
            }
        };

        let lines: Vec<String> = content.lines().map(clean_line).collect();

        // Find the "Enter to select" footer line
        let footer_idx = lines.iter().rposition(|l| {
            let t = l.trim();
            t.contains("Enter to select") && t.contains("to navigate")
        })?;

        // Find the header (☐/☑) scanning backward from footer to find menu start
        let mut header = String::new();
        let mut menu_start = 0;
        for i in (0..footer_idx).rev() {
            let trimmed = lines[i].trim();
            if trimmed.starts_with('☐') || trimmed.starts_with('☑') {
                header = trimmed.trim_start_matches(['☐', '☑', ' ']).trim().to_string();
                menu_start = i + 1;
                break;
            }
        }

        // Forward parse: from menu_start to footer_idx
        let mut options: Vec<MenuOption> = Vec::new();
        let mut question_parts: Vec<String> = Vec::new();
        let mut found_first_option = false;

        let try_parse_option = |line: &str| -> Option<(usize, String, bool, bool)> {
            let trimmed = line.trim();
            let is_selected = trimmed.starts_with('❯');
            let clean = trimmed.trim_start_matches('❯').trim();
            let dot_pos = clean.find(". ")?;
            let idx = clean[..dot_pos].trim().parse::<usize>().ok()?;
            let after_dot = clean[dot_pos + 2..].trim();
            // Detect checkbox
            let (checked, label) = if after_dot.starts_with("[ ] ") {
                (true, after_dot[4..].to_string()) // has checkbox = multi-select mode
            } else if after_dot.starts_with("[✓] ") || after_dot.starts_with("[x] ") || after_dot.starts_with("[■] ") {
                (true, after_dot[after_dot.find("] ").map(|p| p + 2).unwrap_or(0)..].to_string())
            } else {
                (false, after_dot.to_string())
            };
            Some((idx, label, is_selected, checked))
        };

        for i in menu_start..footer_idx {
            let trimmed = lines[i].trim().to_string();
            if trimmed.is_empty() || trimmed.chars().all(|c| c == '─' || c == '━') {
                continue;
            }
            // Skip standalone non-numbered lines like "Submit"
            if trimmed == "Submit" {
                continue;
            }

            if let Some((idx, label, is_selected, checked)) = try_parse_option(&lines[i]) {
                found_first_option = true;
                options.push(MenuOption {
                    index: idx,
                    label,
                    description: String::new(),
                    selected: is_selected,
                    checked,
                });
            } else if found_first_option && !options.is_empty() && !trimmed.is_empty() {
                // Description line: append to the LAST option (the one above, since we go forward)
                if let Some(last) = options.last_mut() {
                    if !last.description.is_empty() {
                        last.description.push(' ');
                    }
                    last.description.push_str(&trimmed);
                }
            } else if !found_first_option && !trimmed.is_empty() {
                // Question text: lines between header and first option
                question_parts.push(trimmed);
            }
        }

        if options.is_empty() {
            return None;
        }

        // Detect multi-select: any option has a checkbox pattern
        let multi_select = options.iter().any(|o| o.checked) ||
            content.contains("[ ]") || content.contains("[✓]") || content.contains("[■]");

        // Extract right-side preview panel (box-drawing bordered area)
        let preview = Self::parse_preview_panel(content);

        let question = if question_parts.is_empty() { None } else { Some(question_parts.join(" ")) };

        Some(PendingMenu { header, question, options, preview, multi_select })
    }

    /// Extract text from a box-drawing bordered panel in pane content.
    /// Looks for ┌───┐ / │ text │ / └───┘ patterns.
    fn parse_preview_panel(content: &str) -> Option<String> {
        let lines: Vec<&str> = content.lines().collect();
        let mut in_box = false;
        let mut preview_lines: Vec<String> = Vec::new();

        for line in &lines {
            if line.contains('┌') && line.contains('┐') {
                in_box = true;
                continue;
            }
            if in_box && line.contains('└') && line.contains('┘') {
                in_box = false;
                continue;
            }
            if in_box {
                // Extract content between │ markers
                if let Some(start) = line.find('│') {
                    let rest = &line[start + '│'.len_utf8()..];
                    if let Some(end) = rest.rfind('│') {
                        let inner = rest[..end].trim_end().to_string();
                        preview_lines.push(inner);
                    }
                }
            }
        }

        if preview_lines.is_empty() {
            return None;
        }

        // Trim trailing empty lines
        while preview_lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            preview_lines.pop();
        }
        // Trim leading empty lines
        while preview_lines.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
            preview_lines.remove(0);
        }

        Some(preview_lines.join("\n"))
    }

    /// List all tmux sessions with their windows
    pub async fn list_sessions() -> Result<Vec<SessionInfo>> {
        let output = Command::new(TMUX_BIN.as_str())
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

        let output = Command::new(TMUX_BIN.as_str())
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
        if let Ok(output) = std::process::Command::new(TMUX_BIN.as_str())
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
        if let Ok(output) = std::process::Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
            .args([
                "-S", TMUX_SOCKET.as_str(),
                "list-windows",
                "-a",
                "-F",
                "#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{window_index}|#{window_panes}|#{window_active}|#{pane_current_path}|#{@agent_dir}",
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
                // agent_dir (parts[8]) may be absent on older tmux/format; accept >= 8
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() < 8 {
                    return None;
                }
                let session_id = parts[0].to_string();
                session_git_dirs.entry(session_id.clone()).or_insert(None);

                let working_dir = {
                    let p = parts[7].trim();
                    if p.is_empty() { None } else { Some(p.to_string()) }
                };
                let agent_dir = parts.get(8).map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string());

                Some(TmuxWindowInfo {
                    session_id,
                    session_name: parts[1].to_string(),
                    window_id: parts[2].to_string(),
                    window_name: parts[3].to_string(),
                    window_index: parts[4].parse().unwrap_or(0),
                    pane_count: parts[5].parse().unwrap_or(1),
                    active: parts[6] == "1",
                    git_dir: None, // Will be filled later
                    working_dir,
                    agent_dir,
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

    /// Move a window from src_index to dst_index within the same session.
    /// Uses sequential swaps to "bubble" the window to its new position,
    /// shifting intermediate windows rather than just swapping two positions.
    pub async fn move_window(session: &str, src_index: u32, dst_index: u32) -> Result<()> {
        if src_index == dst_index {
            return Ok(());
        }

        let step: i32 = if src_index < dst_index { 1 } else { -1 };
        let mut current = src_index as i32;

        while current != dst_index as i32 {
            let next = current + step;
            let src_target = format!("{}:{}", session, current);
            let dst_target = format!("{}:{}", session, next);

            let output = Command::new(TMUX_BIN.as_str())
                .args([
                    "-S", TMUX_SOCKET.as_str(),
                    "swap-window",
                    "-s", &src_target,
                    "-t", &dst_target,
                ])
                .output()
                .await
                .context("Failed to swap tmux windows")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("tmux swap-window failed at {}→{}: {}", current, next, stderr);
            }

            current = next;
        }

        Ok(())
    }

    /// Set a built-in tmux window option (e.g. automatic-rename, allow-rename)
    pub async fn set_builtin_window_option(target: &str, key: &str, value: &str) -> Result<()> {
        let output = Command::new(TMUX_BIN.as_str())
            .args([
                "-S", TMUX_SOCKET.as_str(),
                "set-option", "-w", "-t", target, key, value,
            ])
            .output()
            .await
            .context("Failed to set tmux window option")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux set-option failed: {}", stderr);
        }

        Ok(())
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
        let output = Command::new(TMUX_BIN.as_str())
            .args([
                "-S", TMUX_SOCKET.as_str(),
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
        // Use --show-toplevel first, then check if we're in a worktree.
        // For worktrees, resolve to the main repo root so git_dir is consistent.
        let output = Command::new("git")
            .args(["-C", path, "rev-parse", "--show-toplevel"])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }
        let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if toplevel.is_empty() {
            return None;
        }

        // Check if this is a worktree by looking at --git-common-dir
        // In a worktree, --git-common-dir points to the main repo's .git directory
        if let Ok(common) = Command::new("git")
            .args(["-C", path, "rev-parse", "--git-common-dir"])
            .output()
            .await
        {
            if common.status.success() {
                let common_dir = String::from_utf8_lossy(&common.stdout).trim().to_string();
                // If common dir ends with "/.git", the main repo root is its parent
                if let Some(main_root) = common_dir.strip_suffix("/.git") {
                    if main_root != toplevel.trim_end_matches('/') {
                        // We're in a worktree — return main repo root
                        return Some(main_root.to_string());
                    }
                }
            }
        }

        Some(toplevel)
    }

    /// Synchronous version of list_all_windows (for use in non-async contexts)
    pub fn list_all_windows_sync() -> Vec<TmuxWindowInfo> {
        // Use pipe separator instead of tab to avoid shell escaping issues
        // Use explicit socket path with /private/tmp to work with launchd
        let output = std::process::Command::new(TMUX_BIN.as_str())
            .args([
                "-S", TMUX_SOCKET.as_str(),
                "list-windows",
                "-a",
                "-F",
                "#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{window_index}|#{window_panes}|#{window_active}|#{pane_current_path}|#{@agent_dir}",
            ])
            .output();

        match &output {
            Ok(out) => {
                let stdout_str = String::from_utf8_lossy(&out.stdout);
                let stderr_str = String::from_utf8_lossy(&out.stderr);
                // Log first line to see the format
                if let Some(first_line) = stdout_str.lines().next() {
                    tracing::debug!("tmux list-windows: status={}, first_line={:?}, stderr={}",
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
                        // agent_dir (parts[8]) may be absent; accept >= 8 fields
                        let parts: Vec<&str> = line.split('|').collect();
                        if parts.len() < 8 {
                            tracing::warn!("Skipping invalid line: {}", line);
                            return None;
                        }
                        let session_id = parts[0].to_string();
                        session_git_dirs.entry(session_id.clone()).or_insert(None);

                        let working_dir = {
                            let p = parts[7].trim();
                            if p.is_empty() { None } else { Some(p.to_string()) }
                        };
                        let agent_dir = parts.get(8).map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string());

                        Some(TmuxWindowInfo {
                            session_id,
                            session_name: parts[1].to_string(),
                            window_id: parts[2].to_string(),
                            window_name: parts[3].to_string(),
                            window_index: parts[4].parse().unwrap_or(0),
                            pane_count: parts[5].parse().unwrap_or(1),
                            active: parts[6] == "1",
                            git_dir: None,
                            working_dir,
                            agent_dir,
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
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!(
                    "tmux list-windows failed: exit={}, stderr={}",
                    output.status, stderr
                );
                Vec::new()
            }
            Err(e) => {
                tracing::error!("tmux list-windows command error: {}", e);
                Vec::new()
            }
        }
    }

    /// Synchronous version of get_session_git_dir
    fn get_session_git_dir_sync(session_id: &str) -> Option<String> {
        // Get first pane's current path
        let output = std::process::Command::new(TMUX_BIN.as_str())
            .args([
                "-S", TMUX_SOCKET.as_str(),
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
    pub fn find_git_root_sync(path: &str) -> Option<String> {
        let start = std::time::Instant::now();
        let output = std::process::Command::new("git")
            .args(["-C", path, "rev-parse", "--show-toplevel"])
            .output()
            .ok()?;
        let elapsed = start.elapsed();
        if elapsed.as_secs() >= 2 {
            tracing::warn!("CMD_SLOW: git rev-parse --show-toplevel in {} took {}ms", path, elapsed.as_millis());
        }

        if !output.status.success() {
            return None;
        }
        let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if toplevel.is_empty() {
            return None;
        }

        // Check if this is a worktree — resolve to main repo root
        if let Ok(common) = std::process::Command::new("git")
            .args(["-C", path, "rev-parse", "--git-common-dir"])
            .output()
        {
            if common.status.success() {
                let common_dir = String::from_utf8_lossy(&common.stdout).trim().to_string();
                if let Some(main_root) = common_dir.strip_suffix("/.git") {
                    if main_root != toplevel.trim_end_matches('/') {
                        return Some(main_root.to_string());
                    }
                }
            }
        }

        Some(toplevel)
    }

    // ============ tmux Window Options (Metadata Storage) ============

    /// Set a custom option on a tmux window
    ///
    /// Stores metadata like @agent_port, @agent_dir, etc.
    /// Format: tmux set-option -w -t {target} @{key} "{value}"
    pub async fn set_window_option(target: &str, key: &str, value: &str) -> Result<()> {
        let option_name = format!("@{}", key);
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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
        let output = Command::new(TMUX_BIN.as_str())
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

    /// Rename a tmux session
    /// Format: tmux rename-session -t {target} "{name}"
    pub async fn rename_session(target: &str, name: &str) -> Result<()> {
        let output = Command::new(TMUX_BIN.as_str())
            .args(["rename-session", "-t", target, name])
            .output()
            .await
            .context("Failed to rename tmux session")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux rename-session failed: {}", stderr);
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
        std::process::Command::new(TMUX_BIN.as_str())
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

    /// Reset window layout to the default 3-pane layout (yazi + lazygit + agent).
    ///
    /// This kills all panes except the agent pane (claude/opencode), then
    /// rebuilds the layout around it. Safe because yazi and lazygit can be
    /// restarted without data loss.
    pub async fn reset_window_layout(session: &str, window: &str) -> Result<String> {
        let actual_session = Self::find_session(session)
            .await
            .unwrap_or_else(|| session.to_string());

        // Build target - handle window ID (@N) or window name
        let target = if window.starts_with('@') {
            format!("{}:{}", actual_session, window)
        } else {
            format!("{}:={}", actual_session, window)
        };

        // List panes with pane_id, command, and current_path
        let output = Command::new(TMUX_BIN.as_str())
            .args([
                "list-panes", "-t", &target,
                "-F", "#{pane_id} #{pane_current_command} #{pane_current_path}",
            ])
            .output()
            .await
            .context("Failed to list panes")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux list-panes failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let panes: Vec<(&str, &str, &str)> = stdout
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, ' ');
                let id = parts.next()?;
                let cmd = parts.next()?;
                let path = parts.next().unwrap_or("");
                Some((id, cmd, path))
            })
            .collect();

        if panes.is_empty() {
            bail!("No panes found in window");
        }

        // Find the agent pane (claude/opencode)
        let agent_pane = panes.iter().find(|(_, cmd, _)| {
            cmd.starts_with("2.") || cmd.starts_with("1.")
                || *cmd == "claude" || *cmd == "claude-code"
                || *cmd == "opencode"
        });

        let (agent_pane_id, working_dir) = match agent_pane {
            Some((id, _, path)) => (id.to_string(), path.to_string()),
            None => {
                // Fallback: keep the first pane and use its working dir
                let (id, _, path) = panes[0];
                (id.to_string(), path.to_string())
            }
        };

        // Kill all panes except the agent pane
        for (id, _, _) in &panes {
            if *id != agent_pane_id {
                let _ = Command::new(TMUX_BIN.as_str())
                    .args(["kill-pane", "-t", id])
                    .output()
                    .await;
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        }

        // Small delay for tmux to settle
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // Now only the agent pane remains, filling the entire window.
        // Split to create the default layout: left 30% (yazi + lazygit), right 70% (agent)

        // Create left pane: split horizontally, new pane to the left (-b), 30% width
        let output = Command::new(TMUX_BIN.as_str())
            .args([
                "split-window", "-h", "-b", "-p", "30",
                "-t", &target,
                "-c", &working_dir,
            ])
            .output()
            .await
            .context("Failed to split window horizontally")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("split-window -h failed: {}", stderr);
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Split the left pane vertically: top for yazi, bottom for lazygit
        let output = Command::new(TMUX_BIN.as_str())
            .args([
                "split-window", "-v", "-p", "50",
                "-t", &format!("{}.{{left}}", target),
                "-c", &working_dir,
            ])
            .output()
            .await
            .context("Failed to split window vertically")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("split-window -v failed: {}", stderr);
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Start yazi in top-left
        let _ = Command::new(TMUX_BIN.as_str())
            .args(["send-keys", "-t", &format!("{}.{{top-left}}", target), "yazi", "Enter"])
            .output()
            .await;

        // Start lazygit in bottom-left
        let _ = Command::new(TMUX_BIN.as_str())
            .args(["send-keys", "-t", &format!("{}.{{bottom-left}}", target), "lazygit", "Enter"])
            .output()
            .await;

        // Focus on the agent pane (right)
        let _ = Command::new(TMUX_BIN.as_str())
            .args(["select-pane", "-t", &format!("{}.{{right}}", target)])
            .output()
            .await;

        Ok(format!("Layout reset with working_dir={}", working_dir))
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
    pub window_index: u32,
    pub pane_count: u32,
    pub active: bool,
    /// Git directory for the session (from @agent_main_repo or first pane's path)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_dir: Option<String>,
    /// Working directory of the first pane (from #{pane_current_path})
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// Stable worktree/working path recorded at window creation (@agent_dir option).
    /// Unlike working_dir (volatile active-pane cwd), this is fixed identity used
    /// to detect whether a resumable worktree is already open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_dir: Option<String>,
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
