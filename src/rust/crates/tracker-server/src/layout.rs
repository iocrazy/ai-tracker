//! Layout module for tmux pane management
//!
//! Provides various layout templates for workspace creation.

use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

use crate::agent::TMUX_BIN;

/// Layout template types
#[derive(Debug, Clone)]
pub enum LayoutTemplate {
    /// Single service 3-pane layout
    /// ```text
    /// ┌────────────────────────┬──────────────────────┐
    /// │                        │      lazygit         │
    /// │       main             │      (pane 1)        │
    /// │      (pane 0)          ├──────────────────────┤
    /// │                        │    dev server        │
    /// │                        │      (pane 2)        │
    /// └────────────────────────┴──────────────────────┘
    /// ```
    SingleService {
        main_cmd: Option<String>,
        server_cmd: Option<String>,
    },

    /// Fullstack 4-pane layout
    /// ```text
    /// ┌────────────────────────┬──────────────────────┐
    /// │                        │      lazygit         │
    /// │       main             │      (pane 1)        │
    /// │      (pane 0)          ├──────────────────────┤
    /// │                        │  frontend server     │
    /// │                        │      (pane 2)        │
    /// │                        ├──────────────────────┤
    /// │                        │  backend server      │
    /// │                        │      (pane 3)        │
    /// └────────────────────────┴──────────────────────┘
    /// ```
    Fullstack {
        main_cmd: Option<String>,
        frontend_cmd: Option<String>,
        backend_cmd: Option<String>,
    },

    /// Workspace 5-pane layout (yazi + claude + lazygit + 2 servers)
    /// ```text
    /// ┌──────────────┬──────────────────────────────┐
    /// │ yazi         │ Claude                       │
    /// │ (pane 0)     │ (pane 1)                     │
    /// ├──────────────┼──────────────┬───────────────┤
    /// │ lazygit      │ backend srv  │ frontend srv  │
    /// │ (pane 2)     │ (pane 3)     │ (pane 4)      │
    /// └──────────────┴──────────────┴───────────────┘
    /// ```
    Workspace {
        agent_cmd: String,
        frontend_cmd: Option<String>,
        backend_cmd: Option<String>,
    },

    /// Simple 3-pane default layout (yazi + lazygit + agent)
    /// ```text
    /// ┌──────────────┬──────────────────────────────┐
    /// │    yazi      │                              │
    /// │   (pane 0)   │          agent               │
    /// ├──────────────┤          (pane 2)            │
    /// │   lazygit    │                              │
    /// │   (pane 1)   │                              │
    /// └──────────────┴──────────────────────────────┘
    /// ```
    Default { agent_cmd: String },
}

/// Layout renderer for creating tmux layouts
pub struct LayoutRenderer;

impl LayoutRenderer {
    /// Create a layout in a tmux window
    pub async fn create_layout(
        session: &str,
        window: &str,
        template: LayoutTemplate,
        working_dir: &Path,
    ) -> Result<()> {
        let target = format!("{}:{}", session, window);

        match template {
            LayoutTemplate::SingleService {
                main_cmd,
                server_cmd,
            } => {
                Self::create_single_service_layout(&target, working_dir, main_cmd, server_cmd)
                    .await
            }
            LayoutTemplate::Fullstack {
                main_cmd,
                frontend_cmd,
                backend_cmd,
            } => {
                Self::create_fullstack_layout(
                    &target,
                    working_dir,
                    main_cmd,
                    frontend_cmd,
                    backend_cmd,
                )
                .await
            }
            LayoutTemplate::Workspace {
                agent_cmd,
                frontend_cmd,
                backend_cmd,
            } => {
                Self::create_workspace_layout(
                    &target,
                    working_dir,
                    &agent_cmd,
                    frontend_cmd,
                    backend_cmd,
                )
                .await
            }
            LayoutTemplate::Default { agent_cmd } => {
                Self::create_default_layout(&target, working_dir, &agent_cmd).await
            }
        }
    }

    /// Create single service 3-pane layout
    async fn create_single_service_layout(
        target: &str,
        working_dir: &Path,
        main_cmd: Option<String>,
        server_cmd: Option<String>,
    ) -> Result<()> {
        let dir = working_dir.to_str().unwrap();

        // Step 1: Horizontal split (left 55%, right 45%)
        Self::split_window(target, dir, "h", 45).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 2: Vertical split on right pane (top 65%, bottom 35%)
        Self::split_window(target, dir, "v", 35).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Pane layout: 0=left, 1=top-right, 2=bottom-right

        // Start lazygit in top-right
        Self::send_keys_to_pane(target, "{top-right}", "lazygit").await?;

        // Start server in bottom-right if provided
        if let Some(cmd) = server_cmd {
            Self::send_keys_to_pane(target, "{bottom-right}", &cmd).await?;
        }

        // Start main command in left if provided
        if let Some(cmd) = main_cmd {
            Self::send_keys_to_pane(target, "{left}", &cmd).await?;
        }

        // Focus on left pane
        Self::select_pane(target, "{left}").await?;

        Ok(())
    }

    /// Create fullstack 4-pane layout
    async fn create_fullstack_layout(
        target: &str,
        working_dir: &Path,
        main_cmd: Option<String>,
        frontend_cmd: Option<String>,
        backend_cmd: Option<String>,
    ) -> Result<()> {
        let dir = working_dir.to_str().unwrap();

        // Step 1: Horizontal split
        Self::split_window(target, dir, "h", 45).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 2: Vertical split on right (for lazygit)
        Self::split_window(target, dir, "v", 66).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 3: Another vertical split on bottom-right
        Self::split_window(target, dir, "v", 50).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Pane layout: 0=left, 1=top-right, 2=middle-right, 3=bottom-right

        // Start lazygit in top-right
        Self::send_keys_to_pane(target, "{top-right}", "lazygit").await?;

        // Start frontend in middle-right
        if let Some(cmd) = frontend_cmd {
            // Use pane index for middle pane
            Self::send_keys_to_pane_index(target, 2, &cmd).await?;
        }

        // Start backend in bottom-right
        if let Some(cmd) = backend_cmd {
            Self::send_keys_to_pane(target, "{bottom-right}", &cmd).await?;
        }

        // Start main command in left if provided
        if let Some(cmd) = main_cmd {
            Self::send_keys_to_pane(target, "{left}", &cmd).await?;
        }

        // Focus on left pane
        Self::select_pane(target, "{left}").await?;

        Ok(())
    }

    /// Create workspace 5-pane layout
    async fn create_workspace_layout(
        target: &str,
        working_dir: &Path,
        agent_cmd: &str,
        frontend_cmd: Option<String>,
        backend_cmd: Option<String>,
    ) -> Result<()> {
        let dir = working_dir.to_str().unwrap();

        // Step 1: Horizontal split (left 30%, right 70%)
        Self::split_window(target, dir, "h", 70).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 2: Select left pane and vertical split
        Self::select_pane(target, "{left}").await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Self::split_window(target, dir, "v", 50).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 3: Select right pane and vertical split (for Claude top, servers bottom)
        Self::select_pane(target, "{right}").await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Self::split_window(target, dir, "v", 40).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 4: Split bottom-right horizontally for backend/frontend
        Self::split_window(target, dir, "h", 50).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // Pane layout:
        // 0=top-left (yazi)
        // 1=top-right (claude)
        // 2=bottom-left (lazygit)
        // 3=bottom-middle (backend)
        // 4=bottom-right (frontend)

        // Start yazi in top-left
        Self::send_keys_to_pane(target, "{top-left}", "yazi").await?;

        // Start lazygit in bottom-left
        Self::send_keys_to_pane(target, "{bottom-left}", "lazygit").await?;

        // Start agent in top-right (Claude area)
        Self::send_keys_to_pane_index(target, 1, agent_cmd).await?;

        // Start backend server
        if let Some(cmd) = backend_cmd {
            Self::send_keys_to_pane_index(target, 3, &cmd).await?;
        }

        // Start frontend server
        if let Some(cmd) = frontend_cmd {
            Self::send_keys_to_pane_index(target, 4, &cmd).await?;
        }

        // Focus on agent pane
        Self::select_pane_index(target, 1).await?;

        Ok(())
    }

    /// Create default 3-pane layout (yazi + lazygit + agent)
    async fn create_default_layout(
        target: &str,
        working_dir: &Path,
        agent_cmd: &str,
    ) -> Result<()> {
        let dir = working_dir.to_str().unwrap();

        // Step 1: Horizontal split (left 30%, right 70%)
        Self::split_window(target, dir, "h", 70).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 2: Select left pane and vertical split
        Self::select_pane(target, "{left}").await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Self::split_window(target, dir, "v", 50).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // Pane layout: 0=top-left, 1=right, 2=bottom-left

        // Start yazi in top-left
        Self::send_keys_to_pane(target, "{top-left}", "yazi").await?;

        // Start lazygit in bottom-left
        Self::send_keys_to_pane(target, "{bottom-left}", "lazygit").await?;

        // Start agent in right
        Self::send_keys_to_pane(target, "{right}", agent_cmd).await?;

        // Focus on agent pane
        Self::select_pane(target, "{right}").await?;

        Ok(())
    }

    // =========================================================================
    // Helper functions
    // =========================================================================

    /// Split window with percentage
    async fn split_window(target: &str, working_dir: &str, direction: &str, percent: u32) -> Result<()> {
        let dir_flag = if direction == "h" { "-h" } else { "-v" };
        let output = Command::new(TMUX_BIN)
            .args([
                "split-window",
                dir_flag,
                "-p",
                &percent.to_string(),
                "-t",
                target,
                "-c",
                working_dir,
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

    /// Send keys to a pane using position specifier
    async fn send_keys_to_pane(target: &str, position: &str, cmd: &str) -> Result<()> {
        let pane_target = format!("{}.{}", target, position);
        let output = Command::new(TMUX_BIN)
            .args(["send-keys", "-t", &pane_target, cmd, "Enter"])
            .output()
            .await
            .context("Failed to send keys to tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux send-keys to {} failed: {}", position, stderr);
        }

        Ok(())
    }

    /// Send keys to a pane by index
    async fn send_keys_to_pane_index(target: &str, index: u32, cmd: &str) -> Result<()> {
        let pane_target = format!("{}.{}", target, index);
        let output = Command::new(TMUX_BIN)
            .args(["send-keys", "-t", &pane_target, cmd, "Enter"])
            .output()
            .await
            .context("Failed to send keys to tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux send-keys to pane {} failed: {}", index, stderr);
        }

        Ok(())
    }

    /// Select a pane using position specifier
    async fn select_pane(target: &str, position: &str) -> Result<()> {
        let pane_target = format!("{}.{}", target, position);
        let output = Command::new(TMUX_BIN)
            .args(["select-pane", "-t", &pane_target])
            .output()
            .await
            .context("Failed to select tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux select-pane {} failed: {}", position, stderr);
        }

        Ok(())
    }

    /// Select a pane by index
    async fn select_pane_index(target: &str, index: u32) -> Result<()> {
        let pane_target = format!("{}.{}", target, index);
        let output = Command::new(TMUX_BIN)
            .args(["select-pane", "-t", &pane_target])
            .output()
            .await
            .context("Failed to select tmux pane")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux select-pane {} failed: {}", index, stderr);
        }

        Ok(())
    }
}

/// Write feature.json metadata file
pub async fn write_feature_json(
    feature_dir: &Path,
    name: &str,
    worktree: &Path,
    branch: &str,
    base_branch: &str,
    fullstack: bool,
    port: Option<u16>,
    frontend_port: Option<u16>,
    backend_port: Option<u16>,
    browser: &str,
) -> Result<()> {
    use serde_json::json;

    let mut data = json!({
        "name": name,
        "worktree": worktree.to_string_lossy(),
        "branch": branch,
        "base_branch": base_branch,
        "fullstack": fullstack,
        "browser": browser
    });

    if fullstack {
        if let Some(p) = frontend_port {
            data["frontend_port"] = json!(p);
        }
        if let Some(p) = backend_port {
            data["backend_port"] = json!(p);
        }
    } else if let Some(p) = port {
        data["port"] = json!(p);
    }

    let content = serde_json::to_string_pretty(&data)?;
    let file_path = feature_dir.join("feature.json");

    tokio::fs::write(&file_path, content)
        .await
        .with_context(|| format!("Failed to write feature.json to {:?}", file_path))?;

    Ok(())
}

/// Write .agent-info status file
pub async fn write_agent_info(
    feature_dir: &Path,
    worktree: &Path,
    branch: &str,
    base_branch: &str,
    port: Option<u16>,
    frontend_port: Option<u16>,
    backend_port: Option<u16>,
) -> Result<()> {
    let mut content = String::new();
    content.push('\n');
    content.push_str(&format!(
        "Host path: {}\n",
        worktree.to_string_lossy()
    ));
    content.push_str(&format!(
        "Created branch: {} (from {})\n",
        branch, base_branch
    ));

    if let (Some(fp), Some(bp)) = (frontend_port, backend_port) {
        content.push_str(&format!("Frontend: http://localhost:{}\n", fp));
        content.push_str(&format!("Backend: http://localhost:{}\n", bp));
    } else if let Some(p) = port {
        content.push_str(&format!("URL: http://localhost:{}\n", p));
    }

    content.push_str(&format!(
        "Config: {}\n",
        feature_dir.join("feature.json").to_string_lossy()
    ));
    content.push('\n');

    let file_path = feature_dir.join(".agent-info");
    tokio::fs::write(&file_path, content)
        .await
        .with_context(|| format!("Failed to write .agent-info to {:?}", file_path))?;

    Ok(())
}
