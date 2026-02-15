//! Workspace, git, config, and port management route handlers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::agent;
use crate::browser;
use crate::config;
use crate::layout;
use crate::port;
use crate::workspace;
use crate::{AppState, CommandResponse};

// ============================================================================
// Workspace Types
// ============================================================================

/// Start workspace request
#[derive(Deserialize)]
pub(crate) struct StartWorkspaceRequest {
    git_dir: String,
    branch: String,
    /// Base branch to create new branch from (if branch doesn't exist)
    #[serde(default)]
    base_branch: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    layout: Option<String>,
    /// Enable fullstack mode (frontend + backend)
    #[serde(default)]
    fullstack_mode: Option<bool>,
    /// Base port for port allocation
    #[serde(default)]
    port_base: Option<u16>,
    /// Frontend port base (fullstack mode)
    #[serde(default)]
    frontend_port_base: Option<u16>,
    /// Backend port base (fullstack mode)
    #[serde(default)]
    backend_port_base: Option<u16>,
    /// Frontend start command (supports $PORT)
    #[serde(default)]
    frontend_cmd: Option<String>,
    /// Backend start command (supports $PORT)
    #[serde(default)]
    backend_cmd: Option<String>,
    /// Dev server command (single service mode, supports $PORT)
    #[serde(default)]
    dev_server_cmd: Option<String>,
    /// Auto-open browser after starting
    #[serde(default)]
    auto_open_browser: Option<bool>,
    /// Browser type (chrome, safari, arc)
    #[serde(default)]
    browser: Option<String>,
    /// Browser URL template (supports $PORT, $FRONTEND_PORT, $BACKEND_PORT)
    #[serde(default)]
    browser_url: Option<String>,
    /// Frontend directory (relative to worktree)
    #[serde(default)]
    frontend_dir: Option<String>,
    /// Backend directory (relative to worktree)
    #[serde(default)]
    backend_dir: Option<String>,
}

/// Start workspace response
#[derive(Serialize)]
pub(crate) struct StartWorkspaceResponse {
    success: bool,
    session_name: String,
    worktree_path: String,
    message: String,
    /// Allocated port (single service mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    /// Allocated frontend port (fullstack mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    frontend_port: Option<u16>,
    /// Allocated backend port (fullstack mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    backend_port: Option<u16>,
    /// Browser URL that was opened
    #[serde(skip_serializing_if = "Option::is_none")]
    browser_url: Option<String>,
}

/// Resume workspace request
#[derive(Deserialize)]
pub(crate) struct ResumeWorkspaceRequest {
    git_dir: String,
    branch: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    layout: Option<String>,
}

/// Destroy workspace request
#[derive(Deserialize)]
pub(crate) struct DestroyWorkspaceRequest {
    git_dir: String,
    branch: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    force: bool,
    /// Kill processes on allocated ports
    #[serde(default)]
    kill_ports: Option<bool>,
    /// Delete the git branch after removing worktree
    #[serde(default)]
    delete_branch: Option<bool>,
}

/// Workspace list response
#[derive(Serialize)]
pub(crate) struct WorkspaceListResponse {
    workspaces: Vec<agent::AgentSession>,
}

/// Config response
#[derive(Serialize)]
pub(crate) struct ConfigResponse {
    workspaces: HashMap<String, config::WorkspaceConfig>,
    agents: HashMap<String, config::AgentDef>,
    layouts: HashMap<String, config::LayoutConfig>,
    defaults: config::Defaults,
}

// ============================================================================
// Port Management Types
// ============================================================================

/// Check port status response
#[derive(Serialize)]
pub(crate) struct PortStatusResponse {
    port: u16,
    in_use: bool,
}

/// Kill port request
#[derive(Deserialize)]
pub(crate) struct KillPortRequest {
    port: u16,
}

/// Kill port response
#[derive(Serialize)]
pub(crate) struct KillPortResponse {
    success: bool,
    port: u16,
    killed: bool,
    message: String,
}

/// Allocate port response
#[derive(Serialize)]
pub(crate) struct AllocatePortResponse {
    success: bool,
    port: u16,
    message: String,
}

// ============================================================================
// Workspace Activate Types
// ============================================================================

/// Activate workspace request (window focus hook)
#[derive(Deserialize)]
pub(crate) struct ActivateWorkspaceRequest {
    session: String,
    window: String,
}

/// Activate workspace response
#[derive(Serialize)]
pub(crate) struct ActivateWorkspaceResponse {
    success: bool,
    message: String,
    refreshed_lazygit: bool,
    switched_browser_tab: bool,
}

/// Workspace metadata response
#[derive(Serialize)]
pub(crate) struct WorkspaceMetadataResponse {
    session: String,
    window: String,
    port: Option<u16>,
    frontend_port: Option<u16>,
    backend_port: Option<u16>,
    dir: Option<String>,
    main_repo: Option<String>,
    browser: Option<String>,
    name: Option<String>,
    fullstack: bool,
}

// ============================================================================
// Git Types
// ============================================================================

#[derive(Serialize)]
struct BranchInfo {
    name: String,
    has_worktree: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree_path: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct GitBranchesResponse {
    branches: Vec<String>,
    local: Vec<String>,
    remote: Vec<String>,
    /// Branches with worktree status
    branches_with_status: Vec<BranchInfo>,
}

#[derive(Deserialize)]
pub(crate) struct GitBranchesQuery {
    git_dir: Option<String>,
}

// ============================================================================
// Workspace Handlers
// ============================================================================

/// Start a new workspace (enhanced with port management, layouts, browser)
pub(crate) async fn start_workspace(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StartWorkspaceRequest>,
) -> Json<StartWorkspaceResponse> {
    let git_dir = Path::new(&req.git_dir);

    // Validation
    if !git_dir.exists() {
        return Json(StartWorkspaceResponse {
            success: false,
            session_name: String::new(),
            worktree_path: String::new(),
            message: format!("Directory '{}' does not exist", req.git_dir),
            port: None,
            frontend_port: None,
            backend_port: None,
            browser_url: None,
        });
    }

    if !git_dir.join(".git").exists() {
        return Json(StartWorkspaceResponse {
            success: false,
            session_name: String::new(),
            worktree_path: String::new(),
            message: format!("Directory '{}' is not a git repository", req.git_dir),
            port: None,
            frontend_port: None,
            backend_port: None,
            browser_url: None,
        });
    }

    // Load config
    let cfg = match config::AgentConfig::load() {
        Ok(c) => c,
        Err(e) => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name: String::new(),
                worktree_path: String::new(),
                message: format!("Failed to load config: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    let agent_name = req.agent.unwrap_or_else(|| cfg.defaults.agent.clone());
    let agent_def = match cfg.get_agent(&agent_name) {
        Some(a) => a,
        None => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name: String::new(),
                worktree_path: String::new(),
                message: format!("Agent '{}' not found in config", agent_name),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    let layout_name = req.layout.clone().unwrap_or_else(|| cfg.defaults.layout.clone());
    let layout = match cfg.get_layout(&layout_name) {
        Some(l) => l,
        None => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name: String::new(),
                worktree_path: String::new(),
                message: format!("Layout '{}' not found in config", layout_name),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    // Determine if using feature directory structure
    let use_feature_dir = req.fullstack_mode.unwrap_or(false)
        || req.frontend_cmd.is_some()
        || req.backend_cmd.is_some()
        || req.dev_server_cmd.is_some();

    // Create worktree
    let git = workspace::GitWorktree::new(git_dir);
    let worktree_path = if use_feature_dir {
        match git.create_feature_dir(&req.branch, req.base_branch.as_deref()).await {
            Ok(p) => p,
            Err(e) => {
                return Json(StartWorkspaceResponse {
                    success: false,
                    session_name: String::new(),
                    worktree_path: String::new(),
                    message: format!("Failed to create feature worktree: {}", e),
                    port: None,
                    frontend_port: None,
                    backend_port: None,
                    browser_url: None,
                });
            }
        }
    } else {
        match git.create(&req.branch).await {
            Ok(p) => p,
            Err(e) => {
                return Json(StartWorkspaceResponse {
                    success: false,
                    session_name: String::new(),
                    worktree_path: String::new(),
                    message: format!("Failed to create worktree: {}", e),
                    port: None,
                    frontend_port: None,
                    backend_port: None,
                    browser_url: None,
                });
            }
        }
    };

    let session_name = req.session.clone().unwrap_or_else(|| {
        git_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string())
    });

    // Create tmux window first (needed for port allocation based on window index)
    let actual_session = match agent::TmuxAgent::create_workspace(
        &session_name,
        &req.branch,
        &worktree_path,
        layout,
        agent_def,
    )
    .await
    {
        Ok(name) => name,
        Err(e) => {
            // Cleanup on failure
            if use_feature_dir {
                let _ = git.cleanup_feature_dir(&req.branch, true).await;
            } else {
                let _ = git.remove(&req.branch, true).await;
            }
            return Json(StartWorkspaceResponse {
                success: false,
                session_name: String::new(),
                worktree_path: String::new(),
                message: format!("Failed to create tmux window: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    // Port allocation
    let fullstack = req.fullstack_mode.unwrap_or(false);
    let mut allocated_port: Option<u16> = None;
    let mut allocated_frontend_port: Option<u16> = None;
    let mut allocated_backend_port: Option<u16> = None;

    // Get window index for port calculation
    if let Ok(window_index) = port::PortManager::get_window_index(&actual_session, &req.branch).await {
        if fullstack {
            let frontend_base = req.frontend_port_base.unwrap_or(3000);
            let backend_base = req.backend_port_base.unwrap_or(8000);
            let (fp, bp) = port::PortManager::allocate_fullstack_ports(frontend_base, backend_base, window_index);
            allocated_frontend_port = Some(fp);
            allocated_backend_port = Some(bp);
        } else if req.dev_server_cmd.is_some() || req.port_base.is_some() {
            let base = req.port_base.unwrap_or(9100);
            allocated_port = Some(port::PortManager::allocate_port(base, window_index));
        }
    }

    // Auto-allocate worktree slot and generate .worktree.env
    let slot_allocated = {
        let server_state = state.state.lock().unwrap();
        let services = server_state.db.list_project_services(&session_name).unwrap_or_default();
        if !services.is_empty() {
            if let Ok(slot) = server_state.db.next_available_slot(&session_name) {
                let wt_path_str = worktree_path.to_string_lossy().to_string();
                if server_state.db.allocate_worktree_slot(&session_name, slot, &req.branch, &wt_path_str).is_ok() {
                    // Override port allocation with slot-based values
                    for svc in &services {
                        let val = svc.base_value + slot;
                        match svc.env_key.as_str() {
                            "FRONTEND_PORT" => { allocated_frontend_port = Some(val as u16); }
                            "BACKEND_PORT" => { allocated_backend_port = Some(val as u16); }
                            _ => {}
                        }
                    }
                    Some((slot, wt_path_str))
                } else { None }
            } else { None }
        } else { None }
    }; // lock dropped here

    if let Some((slot, wt_path_str)) = slot_allocated {
        generate_worktree_env_file(&state, &session_name, slot, &req.branch, &wt_path_str).await;
    }

    // Store metadata in tmux window options
    let window_name = req.branch.replace('/', "-");
    let target = format!("{}:{}", actual_session, window_name);
    let browser_type = req.browser.clone().unwrap_or_else(|| "chrome".to_string());

    // Set tmux window options for metadata
    if let Some(p) = allocated_port {
        let _ = agent::TmuxAgent::set_window_option(&target, "agent_port", &p.to_string()).await;
    }
    if let Some(fp) = allocated_frontend_port {
        let _ = agent::TmuxAgent::set_window_option(&target, "agent_frontend_port", &fp.to_string()).await;
    }
    if let Some(bp) = allocated_backend_port {
        let _ = agent::TmuxAgent::set_window_option(&target, "agent_backend_port", &bp.to_string()).await;
    }
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_dir", &worktree_path.to_string_lossy()).await;
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_main_repo", &req.git_dir).await;
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_browser", &browser_type).await;
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_name", &req.branch).await;
    let _ = agent::TmuxAgent::set_window_option(&target, "agent_fullstack", &fullstack.to_string()).await;

    // Write feature.json if using feature directory
    let feature_dir = git.get_feature_dir(&req.branch);
    if use_feature_dir && feature_dir.exists() {
        let _ = layout::write_feature_json(
            &feature_dir,
            &req.branch,
            &worktree_path,
            &req.branch,
            "main",
            fullstack,
            allocated_port,
            allocated_frontend_port,
            allocated_backend_port,
            &browser_type,
        ).await;

        let _ = layout::write_agent_info(
            &feature_dir,
            &worktree_path,
            &req.branch,
            "main",
            allocated_port,
            allocated_frontend_port,
            allocated_backend_port,
        ).await;
    }

    // Build browser URL
    let browser_url = if req.auto_open_browser.unwrap_or(true) {
        let url_template = req.browser_url.clone().unwrap_or_default();
        let url = browser::BrowserAutomation::build_url(
            &url_template,
            allocated_port,
            allocated_frontend_port,
            allocated_backend_port,
        );
        if !url.is_empty() {
            // Open browser after a delay (let dev server start)
            let browser = browser_type.clone();
            let url_clone = url.clone();
            tokio::spawn(async move {
                let _ = browser::BrowserAutomation::open_url_delayed(&browser, &url_clone, 3).await;
            });
            Some(url)
        } else {
            None
        }
    } else {
        None
    };

    Json(StartWorkspaceResponse {
        success: true,
        session_name: actual_session,
        worktree_path: worktree_path.to_string_lossy().to_string(),
        message: "Workspace started successfully".to_string(),
        port: allocated_port,
        frontend_port: allocated_frontend_port,
        backend_port: allocated_backend_port,
        browser_url,
    })
}

/// Resume a workspace (list or attach)
pub(crate) async fn resume_workspace(Json(req): Json<ResumeWorkspaceRequest>) -> Json<StartWorkspaceResponse> {
    let git_dir = Path::new(&req.git_dir);

    if !git_dir.exists() {
        return Json(StartWorkspaceResponse {
            success: false,
            session_name: String::new(),
            worktree_path: String::new(),
            message: format!("Directory '{}' does not exist", req.git_dir),
            port: None,
            frontend_port: None,
            backend_port: None,
            browser_url: None,
        });
    }

    // Determine session name
    let session_name = req.session.clone().unwrap_or_else(|| {
        git_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string())
    });

    // Determine window name from branch
    let window_name = req.branch.replace('/', "-");

    // Find worktree path
    let worktree_path = match workspace::GitWorktree::find_worktree(git_dir, &req.branch).await {
        Ok(Some(path)) => path,
        Ok(None) => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name,
                worktree_path: String::new(),
                message: format!("Worktree for branch '{}' not found", req.branch),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
        Err(e) => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name,
                worktree_path: String::new(),
                message: format!("Failed to find worktree: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    };

    // First, create the tmux window
    match agent::TmuxAgent::simple_new_window(&session_name, &window_name).await {
        Ok(_) => {}
        Err(e) => {
            return Json(StartWorkspaceResponse {
                success: false,
                session_name,
                worktree_path: worktree_path.display().to_string(),
                message: format!("Failed to create tmux window: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            });
        }
    }

    // Small delay to let tmux create the window
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Determine layout
    let layout_type = req.layout.as_deref().unwrap_or("default");
    let agent_cmd = req.agent.clone().unwrap_or_else(|| "claude --dangerously-skip-permissions".to_string());
    let layout = match layout_type {
        "workspace" => layout::LayoutTemplate::Workspace {
            agent_cmd,
            frontend_cmd: None,
            backend_cmd: None,
        },
        _ => layout::LayoutTemplate::Default { agent_cmd },
    };

    // Create tmux layout in the window
    match layout::LayoutRenderer::create_layout(&session_name, &window_name, layout, &worktree_path).await {
        Ok(_) => {
            Json(StartWorkspaceResponse {
                success: true,
                session_name,
                worktree_path: worktree_path.display().to_string(),
                message: format!("Resumed workspace '{}' with {} layout", window_name, layout_type),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            })
        }
        Err(e) => {
            Json(StartWorkspaceResponse {
                success: false,
                session_name,
                worktree_path: worktree_path.display().to_string(),
                message: format!("Failed to create layout: {}", e),
                port: None,
                frontend_port: None,
                backend_port: None,
                browser_url: None,
            })
        }
    }
}

/// Destroy a workspace (enhanced with port cleanup and branch deletion)
pub(crate) async fn destroy_workspace(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DestroyWorkspaceRequest>,
) -> Json<CommandResponse> {
    let git_dir = Path::new(&req.git_dir);

    if !git_dir.exists() {
        return Json(CommandResponse {
            success: false,
            message: format!("Directory '{}' does not exist", req.git_dir),
        });
    }

    let session_name = req.session.clone().unwrap_or_else(|| {
        git_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string())
    });

    // Free worktree slot
    {
        let server_state = state.state.lock().unwrap();
        let _ = server_state.db.free_worktree_slot_by_branch(&session_name, &req.branch);
    }

    let window_name = req.branch.replace('/', "-");
    let target = format!("{}:{}", session_name, window_name);

    // Get port metadata before killing window
    let mut ports_to_kill = Vec::new();
    if req.kill_ports.unwrap_or(false) {
        // Try to get ports from tmux window options
        if let Ok(Some(port_str)) = agent::TmuxAgent::get_window_option(&target, "agent_port").await {
            if let Ok(port) = port_str.parse::<u16>() {
                ports_to_kill.push(port);
            }
        }
        if let Ok(Some(port_str)) = agent::TmuxAgent::get_window_option(&target, "agent_frontend_port").await {
            if let Ok(port) = port_str.parse::<u16>() {
                ports_to_kill.push(port);
            }
        }
        if let Ok(Some(port_str)) = agent::TmuxAgent::get_window_option(&target, "agent_backend_port").await {
            if let Ok(port) = port_str.parse::<u16>() {
                ports_to_kill.push(port);
            }
        }
    }

    // Kill port processes
    if !ports_to_kill.is_empty() {
        if let Err(e) = port::PortManager::kill_ports(&ports_to_kill).await {
            warn!("Failed to kill port processes: {}", e);
        }
    }

    // Kill tmux window
    if agent::TmuxAgent::session_exists(&session_name).await {
        if let Err(e) = agent::TmuxAgent::kill_window(&session_name, &req.branch).await {
            warn!("Failed to kill window: {}", e);
        }
    }

    let git = workspace::GitWorktree::new(git_dir);

    // Try to remove feature directory first, then fall back to regular worktree
    let feature_dir = git.get_feature_dir(&req.branch);
    if feature_dir.exists() {
        if let Err(e) = git.cleanup_feature_dir(&req.branch, req.force).await {
            warn!("Failed to cleanup feature directory: {}", e);
            // Try regular worktree removal as fallback
            if let Err(e2) = git.remove(&req.branch, req.force).await {
                return Json(CommandResponse {
                    success: false,
                    message: format!("Failed to remove worktree: {} (feature dir: {})", e2, e),
                });
            }
        }
    } else {
        // Regular worktree removal
        if let Err(e) = git.remove(&req.branch, req.force).await {
            return Json(CommandResponse {
                success: false,
                message: format!("Failed to remove worktree: {}", e),
            });
        }
    }

    // Delete git branch if requested
    if req.delete_branch.unwrap_or(false) {
        if let Err(e) = git.delete_branch(&req.branch, req.force).await {
            warn!("Failed to delete branch '{}': {}", req.branch, e);
            // Don't fail the whole operation if branch deletion fails
        }
    }

    Json(CommandResponse {
        success: true,
        message: "Workspace destroyed successfully".to_string(),
    })
}

/// List all active workspaces
pub(crate) async fn list_workspaces() -> Json<WorkspaceListResponse> {
    let windows = agent::TmuxAgent::list_windows().await.unwrap_or_default();
    Json(WorkspaceListResponse { workspaces: windows })
}

/// List git branches for a repository
pub(crate) async fn list_git_branches(Query(query): Query<GitBranchesQuery>) -> Json<GitBranchesResponse> {
    // Use provided git_dir or try to detect from current session
    let git_dir = query.git_dir.unwrap_or_else(|| ".".to_string());
    let git_dir = std::path::PathBuf::from(&git_dir);

    tracing::info!("list_git_branches: git_dir={:?}, exists={}", git_dir, git_dir.exists());

    let worktree = workspace::GitWorktree::new(&git_dir);

    let local = match worktree.list_branches().await {
        Ok(branches) => {
            tracing::info!("list_branches OK: {} branches", branches.len());
            branches
        }
        Err(e) => {
            tracing::error!("list_branches ERROR: {}", e);
            Vec::new()
        }
    };

    let remote = match worktree.list_remote_branches().await {
        Ok(branches) => branches,
        Err(e) => {
            tracing::warn!("list_remote_branches skip: {}", e);
            Vec::new()
        }
    };

    let all = worktree.list_all_branches().await.unwrap_or_default();

    // Get branches that have worktrees (branch_name -> worktree_path)
    let worktree_branches: HashMap<String, String> =
        get_worktree_branches(&git_dir).await;

    // Build branch info with worktree status
    let branches_with_status: Vec<BranchInfo> = all
        .iter()
        .map(|name| {
            let wt_path = worktree_branches.get(name).cloned();
            BranchInfo {
                name: name.clone(),
                has_worktree: wt_path.is_some(),
                worktree_path: wt_path,
            }
        })
        .collect();

    Json(GitBranchesResponse {
        branches: all,
        local,
        remote,
        branches_with_status,
    })
}

/// Get branches that have worktrees, returning branch_name -> worktree_path
async fn get_worktree_branches(git_dir: &Path) -> HashMap<String, String> {
    use tokio::process::Command;

    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(git_dir)
        .output()
        .await;

    let mut branches = HashMap::new();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut current_path = String::new();
            for line in stdout.lines() {
                if let Some(path) = line.strip_prefix("worktree ") {
                    current_path = path.to_string();
                } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                    branches.insert(branch.to_string(), current_path.clone());
                }
            }
        }
    }

    branches
}

/// Get config
pub(crate) async fn get_config() -> Json<ConfigResponse> {
    let cfg = config::AgentConfig::load().unwrap_or_default();
    Json(ConfigResponse {
        workspaces: cfg.workspaces,
        agents: cfg.agents,
        layouts: cfg.layouts,
        defaults: cfg.defaults,
    })
}

/// Activate workspace (window focus hook)
/// Called when a tmux window is activated to sync lazygit and browser
pub(crate) async fn activate_workspace(Json(req): Json<ActivateWorkspaceRequest>) -> Json<ActivateWorkspaceResponse> {
    let window_name = req.window.replace('/', "-");
    let target = format!("{}:{}", req.session, window_name);

    let mut refreshed_lazygit = false;
    let mut switched_browser_tab = false;

    // Get metadata from tmux window options
    let agent_port = agent::TmuxAgent::get_window_option(&target, "agent_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let frontend_port = agent::TmuxAgent::get_window_option(&target, "agent_frontend_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let browser_type = agent::TmuxAgent::get_window_option(&target, "agent_browser")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "chrome".to_string());

    // Refresh lazygit (send 'R' key to pane 1 or bottom-left)
    // Try to find lazygit pane and send refresh
    if let Ok(panes) = agent::TmuxAgent::list_panes(&req.session, &req.window).await {
        for pane in &panes {
            if pane.command == "lazygit" {
                if agent::TmuxAgent::send_keys_to_pane(&req.session, &req.window, &pane.index, "R", false).await.is_ok() {
                    refreshed_lazygit = true;
                    break;
                }
            }
        }
    }

    // Switch browser tab to matching port
    let port_to_switch = frontend_port.or(agent_port);
    if let Some(port) = port_to_switch {
        if let Ok(found) = browser::BrowserAutomation::switch_to_tab(&browser_type, port).await {
            switched_browser_tab = found;
        }
    }

    Json(ActivateWorkspaceResponse {
        success: true,
        message: "Workspace activated".to_string(),
        refreshed_lazygit,
        switched_browser_tab,
    })
}

/// Get workspace metadata from tmux window options
pub(crate) async fn get_workspace_metadata(Query(params): Query<ActivateWorkspaceRequest>) -> Json<WorkspaceMetadataResponse> {
    let window_name = params.window.replace('/', "-");
    let target = format!("{}:{}", params.session, window_name);

    let port = agent::TmuxAgent::get_window_option(&target, "agent_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let frontend_port = agent::TmuxAgent::get_window_option(&target, "agent_frontend_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let backend_port = agent::TmuxAgent::get_window_option(&target, "agent_backend_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());

    let dir = agent::TmuxAgent::get_window_option(&target, "agent_dir")
        .await
        .ok()
        .flatten();

    let main_repo = agent::TmuxAgent::get_window_option(&target, "agent_main_repo")
        .await
        .ok()
        .flatten();

    let browser = agent::TmuxAgent::get_window_option(&target, "agent_browser")
        .await
        .ok()
        .flatten();

    let name = agent::TmuxAgent::get_window_option(&target, "agent_name")
        .await
        .ok()
        .flatten();

    let fullstack = agent::TmuxAgent::get_window_option(&target, "agent_fullstack")
        .await
        .ok()
        .flatten()
        .map(|s| s == "true")
        .unwrap_or(false);

    Json(WorkspaceMetadataResponse {
        session: params.session,
        window: params.window,
        port,
        frontend_port,
        backend_port,
        dir,
        main_repo,
        browser,
        name,
        fullstack,
    })
}

// ============================================================================
// Port Management Handlers
// ============================================================================

/// Check if a port is in use
pub(crate) async fn check_port(AxumPath(port): AxumPath<u16>) -> Json<PortStatusResponse> {
    Json(PortStatusResponse {
        port,
        in_use: port::PortManager::is_port_in_use(port),
    })
}

/// Kill process on a port
pub(crate) async fn kill_port(Json(req): Json<KillPortRequest>) -> Json<KillPortResponse> {
    match port::PortManager::kill_port_process(req.port).await {
        Ok(killed) => Json(KillPortResponse {
            success: true,
            port: req.port,
            killed,
            message: if killed {
                "Process killed".to_string()
            } else {
                "No process found on port".to_string()
            },
        }),
        Err(e) => Json(KillPortResponse {
            success: false,
            port: req.port,
            killed: false,
            message: format!("Failed to kill process: {}", e),
        }),
    }
}

/// Allocate an available port
pub(crate) async fn allocate_port(Query(params): Query<HashMap<String, String>>) -> Json<AllocatePortResponse> {
    let base: u16 = params
        .get("base")
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    match port::PortManager::find_available_port(base, 100) {
        Some(port) => Json(AllocatePortResponse {
            success: true,
            port,
            message: format!("Port {} is available", port),
        }),
        None => Json(AllocatePortResponse {
            success: false,
            port: 0,
            message: format!("No available port found starting from {}", base),
        }),
    }
}

// ============================================================================
// Worktree Env Helpers
// ============================================================================

/// Generate a .worktree.env file for a specific worktree slot
pub(crate) async fn generate_worktree_env_file(
    state: &Arc<AppState>,
    session_name: &str,
    slot: i32,
    branch: &str,
    worktree_path: &str,
) {
    let (vars, services) = {
        let server_state = state.state.lock().unwrap();
        let vars = server_state.db.list_project_env_vars(session_name).unwrap_or_default();
        let services = server_state.db.list_project_services(session_name).unwrap_or_default();
        (vars, services)
    };

    if vars.is_empty() && services.is_empty() {
        return;
    }

    let sanitized_name = branch.replace('/', "-");
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");

    let mut content = format!(
        "# ============================================\n\
         # Auto-generated by ai-tracker (do not edit)\n\
         # ============================================\n\
         # Project: {}\n\
         # Branch:  {}\n\
         # Slot:    {}\n\
         # Updated: {}\n\n",
        session_name, branch, slot, now
    );

    if !vars.is_empty() {
        content.push_str("# Project Variables\n");
        for var in &vars {
            content.push_str(&format!("{}={}\n", var.key, var.value));
        }
        content.push('\n');
    }

    if !services.is_empty() {
        content.push_str(&format!("# Worktree Isolated (slot {})\n", slot));
        content.push_str(&format!("WORKTREE_NAME={}\n", sanitized_name));
        content.push_str(&format!("WORKTREE_SLOT={}\n", slot));
        for svc in &services {
            let calculated = svc.base_value + slot;
            content.push_str(&format!("{}={}\n", svc.env_key, calculated));
        }
    }

    let env_path = Path::new(worktree_path).join(".worktree.env");
    if let Err(e) = tokio::fs::write(&env_path, &content).await {
        warn!("Failed to write .worktree.env at {:?}: {}", env_path, e);
    } else {
        info!("Generated .worktree.env at {:?}", env_path);
        // Write MEMORY.md pointer (one-time, idempotent)
        write_memory_md_pointer(worktree_path).await;
    }
}

/// Write a MEMORY.md pointer so AI agents read .worktree.env
async fn write_memory_md_pointer(worktree_path: &str) {
    let encoded = worktree_path.replace('/', "-");
    let memory_dir = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("projects").join(&encoded).join("memory"),
        None => return,
    };

    if let Err(e) = tokio::fs::create_dir_all(&memory_dir).await {
        warn!("Failed to create memory dir: {}", e);
        return;
    }

    let memory_file = memory_dir.join("MEMORY.md");
    let pointer = "\n## Dev Environment\nWhen starting dev servers or configuring ports, read .worktree.env in the project root for isolated port assignments.\n";

    if let Ok(existing) = tokio::fs::read_to_string(&memory_file).await {
        if existing.contains(".worktree.env") {
            return; // Already has pointer
        }
        let new_content = format!("{}\n{}", existing.trim(), pointer);
        let _ = tokio::fs::write(&memory_file, new_content).await;
    } else {
        let _ = tokio::fs::write(&memory_file, pointer).await;
    }
    info!("Wrote MEMORY.md pointer at {:?}", memory_file);
}

/// Regenerate .worktree.env for all active slots in a session
pub(crate) async fn sync_worktree_envs(state: &Arc<AppState>, session_name: &str) {
    let slots = {
        let server_state = state.state.lock().unwrap();
        server_state.db.list_worktree_slots(session_name).unwrap_or_default()
    };
    for slot in &slots {
        if let Some(ref path) = slot.worktree_path {
            let p: &str = path.as_str();
            if !p.is_empty() && Path::new(p).exists() {
                generate_worktree_env_file(state, session_name, slot.slot, &slot.branch, p).await;
            }
        }
    }
}
