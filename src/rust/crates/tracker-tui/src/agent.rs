//! Agent module for tmux session management
//!
//! Handles creating tmux sessions, windows, and panes for agent workspaces.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::{AgentDef, LayoutConfig, PaneConfig};

/// Information about an active agent session
#[derive(Debug, Clone)]
pub struct AgentSession {
    /// tmux session name
    pub session_name: String,
    /// Workspace name
    pub workspace: String,
    /// Branch name
    pub branch: String,
    /// Agent name
    pub agent: String,
    /// Whether the session is currently attached
    pub attached: bool,
}

/// tmux operations for agent management
pub struct TmuxAgent;

impl TmuxAgent {
    /// Generate a session name for workspace + branch
    pub fn session_name(workspace: &str, branch: &str) -> String {
        // Sanitize branch name for tmux (replace / with -)
        let safe_branch = branch.replace('/', "-");
        format!("{}:{}", workspace, safe_branch)
    }

    /// Check if tmux is available
    pub fn is_available() -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Check if a session exists
    pub fn session_exists(session_name: &str) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", session_name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Create a new session with layout
    pub fn create_session(
        session_name: &str,
        working_dir: &Path,
        layout: &LayoutConfig,
        agent: &AgentDef,
    ) -> Result<()> {
        if Self::session_exists(session_name) {
            bail!("Session '{}' already exists", session_name);
        }

        // Resolve agent command in layout
        let panes = Self::resolve_layout(layout, &agent.command);

        if panes.is_empty() {
            bail!("Layout has no panes");
        }

        // Create session with first pane
        let first_cmd = &panes[0].cmd;
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d", // detached
                "-s",
                session_name,
                "-c",
                working_dir.to_str().unwrap(),
                first_cmd,
            ])
            .output()
            .context("Failed to create tmux session")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux new-session failed: {}", stderr);
        }

        // Add remaining panes
        for (i, pane) in panes.iter().skip(1).enumerate() {
            Self::split_pane(session_name, working_dir, &pane.cmd, pane.size.as_deref())?;
        }

        // Balance the layout
        Self::select_layout(session_name, "even-horizontal")?;

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

    /// Split a pane horizontally
    fn split_pane(
        session_name: &str,
        working_dir: &Path,
        cmd: &str,
        _size: Option<&str>,
    ) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "split-window",
                "-h", // horizontal split
                "-t",
                session_name,
                "-c",
                working_dir.to_str().unwrap(),
                cmd,
            ])
            .output()
            .context("Failed to split tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux split-window failed: {}", stderr);
        }

        Ok(())
    }

    /// Select a tmux layout
    fn select_layout(session_name: &str, layout: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["select-layout", "-t", session_name, layout])
            .output()
            .context("Failed to select tmux layout")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux select-layout failed: {}", stderr);
        }

        Ok(())
    }

    /// Attach to a session
    pub fn attach(session_name: &str) -> Result<()> {
        // Use exec to replace current process
        let status = Command::new("tmux")
            .args(["attach-session", "-t", session_name])
            .status()
            .context("Failed to attach to tmux session")?;

        if !status.success() {
            bail!("tmux attach-session failed");
        }

        Ok(())
    }

    /// Switch to a session (when already in tmux)
    pub fn switch_client(session_name: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["switch-client", "-t", session_name])
            .output()
            .context("Failed to switch tmux client")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux switch-client failed: {}", stderr);
        }

        Ok(())
    }

    /// Kill a session
    pub fn kill_session(session_name: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["kill-session", "-t", session_name])
            .output()
            .context("Failed to kill tmux session")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux kill-session failed: {}", stderr);
        }

        Ok(())
    }

    /// List all agent sessions (sessions matching workspace:branch pattern)
    pub fn list_sessions() -> Result<Vec<AgentSession>> {
        let output = Command::new("tmux")
            .args([
                "list-sessions",
                "-F",
                "#{session_name}\t#{session_attached}",
            ])
            .output()
            .context("Failed to list tmux sessions")?;

        if !output.status.success() {
            // No sessions is not an error
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let sessions = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() != 2 {
                    return None;
                }
                let session_name = parts[0];
                let attached = parts[1] == "1";

                // Parse workspace:branch format
                if let Some((workspace, branch)) = session_name.split_once(':') {
                    Some(AgentSession {
                        session_name: session_name.to_string(),
                        workspace: workspace.to_string(),
                        branch: branch.to_string(),
                        agent: String::new(), // Will be filled from state if needed
                        attached,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(sessions)
    }

    /// Check if currently inside tmux
    pub fn is_inside_tmux() -> bool {
        std::env::var("TMUX").is_ok()
    }

    /// Send keys to a session
    pub fn send_keys(session_name: &str, keys: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["send-keys", "-t", session_name, keys, "Enter"])
            .output()
            .context("Failed to send keys to tmux session")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux send-keys failed: {}", stderr);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_name() {
        assert_eq!(
            TmuxAgent::session_name("my-project", "feature/new-thing"),
            "my-project:feature-new-thing"
        );
    }

    #[test]
    fn test_resolve_layout() {
        let layout = LayoutConfig {
            panes: vec![
                PaneConfig {
                    cmd: "yazi".to_string(),
                    size: Some("30%".to_string()),
                },
                PaneConfig {
                    cmd: "{agent}".to_string(),
                    size: Some("70%".to_string()),
                },
            ],
        };

        let resolved = TmuxAgent::resolve_layout(&layout, "claude");
        assert_eq!(resolved[0].cmd, "yazi");
        assert_eq!(resolved[1].cmd, "claude");
    }
}
