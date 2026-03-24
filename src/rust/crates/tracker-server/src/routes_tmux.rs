//! tmux interaction route handlers
//!
//! Extracted from main.rs — all tmux-related structs and handler functions.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::agent;
use crate::browser;
use crate::layout;
use crate::AppState;
use crate::CommandResponse;

// ============================================================================
// Browser Types (used by browser handlers in this module)
// ============================================================================

/// Open browser request
#[derive(Deserialize)]
pub(crate) struct OpenBrowserRequest {
    browser: String,
    url: String,
}

/// Switch browser tab request
#[derive(Deserialize)]
pub(crate) struct SwitchBrowserTabRequest {
    browser: String,
    port: u16,
}

// ============================================================================
// tmux Types
// ============================================================================

/// Send keys to tmux pane request
#[derive(Deserialize)]
pub(crate) struct TmuxSendKeysRequest {
    session: String,
    window: String,
    pane: String,
    keys: String,
    /// Suffix key to send after the text (e.g., "Enter", "C-m", "C-s")
    /// Empty string means no suffix key
    #[serde(default)]
    suffix_key: Option<String>,
    /// Legacy: if true and suffix_key is None, append "Enter"
    #[serde(default)]
    enter: bool,
}

/// Capture pane query params
#[derive(Deserialize)]
pub(crate) struct TmuxCaptureParams {
    session: String,
    window: String,
    pane: String,
    lines: Option<u32>,
}

/// List panes query params
#[derive(Deserialize)]
pub(crate) struct TmuxListPanesParams {
    session: String,
    window: String,
}

/// Capture pane response
#[derive(Serialize)]
pub(crate) struct TmuxCaptureResponse {
    success: bool,
    content: String,
}

/// List sessions response
#[derive(Serialize)]
pub(crate) struct TmuxSessionsResponse {
    sessions: Vec<agent::SessionInfo>,
}

/// List panes response
#[derive(Serialize)]
pub(crate) struct TmuxPanesResponse {
    panes: Vec<agent::PaneInfo>,
}

/// List all windows response
#[derive(Serialize)]
pub(crate) struct TmuxWindowsResponse {
    windows: Vec<agent::TmuxWindowInfo>,
}

#[derive(Deserialize)]
pub(crate) struct ClaudeStatusParams {
    session: String,
    window: String,
}

#[derive(Serialize)]
pub(crate) struct ClaudeStatusResponse {
    success: bool,
    status: agent::ClaudeStatus,
}

/// Kill session request
#[derive(Deserialize)]
pub(crate) struct TmuxKillSessionRequest {
    session: String,
}

/// Kill window request
#[derive(Deserialize)]
pub(crate) struct TmuxKillWindowRequest {
    session: String,
    window: String,
}

/// New window request
#[derive(Deserialize)]
pub(crate) struct TmuxNewWindowRequest {
    session: String,
    name: String,
}

/// Closed window info for API response
#[derive(Serialize)]
pub(crate) struct ClosedWindowInfo {
    id: i64,
    session_name: String,
    window_name: String,
    working_dir: String,
    git_branch: String,
    pane_count: i32,
    closed_at: Option<String>,
}

/// Delete a closed window record
#[derive(Deserialize)]
pub(crate) struct DeleteClosedWindowRequest {
    id: i64,
}

/// Resume closed window request
#[derive(Deserialize)]
pub(crate) struct ResumeClosedWindowRequest {
    session: String,
    window_name: String,
    working_dir: String,
    #[serde(default)]
    layout: Option<String>,  // "default" or "workspace"
    #[serde(default)]
    closed_window_id: Option<i64>,  // ID to delete after resume
}

/// Send image request (supports single or multiple images)
#[derive(Deserialize)]
pub(crate) struct SendImageRequest {
    session: String,
    window_id: String,
    pane: String,
    #[serde(default)]
    image_base64: String,  // data:image/png;base64,xxx (single, backwards compat)
    #[serde(default)]
    images: Vec<String>,   // Multiple images as base64 data URLs
    #[serde(default)]
    message: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SendImageResponse {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_paths: Option<Vec<String>>,
}

/// Select window request
#[derive(Deserialize)]
pub(crate) struct TmuxSelectWindowRequest {
    session: String,
    window: String,
    #[serde(default)]
    window_id: Option<String>,  // tmux window ID like @9 for precise targeting
}

/// Swap window request (reorder windows within a session)
#[derive(Deserialize)]
pub(crate) struct TmuxSwapWindowRequest {
    session: String,
    source_index: u32,
    target_index: u32,
}

/// Rename window request
#[derive(Deserialize)]
pub(crate) struct TmuxRenameWindowRequest {
    session: String,
    window: String,
    name: String,
}

/// Rename session request
#[derive(Deserialize)]
pub(crate) struct TmuxRenameSessionRequest {
    session: String,
    name: String,
}

/// Reset layout request
#[derive(Deserialize)]
pub(crate) struct TmuxResetLayoutRequest {
    session: String,
    window: String,
}

// ============================================================================
// Browser Handlers
// ============================================================================

/// Open browser to a URL
pub(crate) async fn open_browser(Json(req): Json<OpenBrowserRequest>) -> Json<CommandResponse> {
    match browser::BrowserAutomation::open_url(&req.browser, &req.url).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: format!("Opened {} in {}", req.url, req.browser),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to open browser: {}", e),
        }),
    }
}

/// Switch browser to a tab with specific port
pub(crate) async fn switch_browser_tab(Json(req): Json<SwitchBrowserTabRequest>) -> Json<CommandResponse> {
    match browser::BrowserAutomation::switch_to_tab(&req.browser, req.port).await {
        Ok(found) => Json(CommandResponse {
            success: true,
            message: if found {
                format!("Switched to tab with port {}", req.port)
            } else {
                format!("No tab found with port {}", req.port)
            },
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to switch tab: {}", e),
        }),
    }
}

// ============================================================================
// tmux Handlers
// ============================================================================

/// Send keys to a tmux pane
pub(crate) async fn tmux_send_keys(Json(req): Json<TmuxSendKeysRequest>) -> Json<CommandResponse> {
    // Determine suffix key: use suffix_key if provided, fallback to "Enter" if enter=true
    let suffix_key = match &req.suffix_key {
        Some(key) if !key.is_empty() => Some(key.as_str()),
        _ if req.enter => Some("Enter"),
        _ => None,
    };

    // Debug logging
    info!(
        "send_keys: session={}, window={}, pane={}, keys={}, suffix_key={:?}, enter={}",
        req.session, req.window, req.pane, req.keys, suffix_key, req.enter
    );

    match agent::TmuxAgent::send_keys_with_suffix(
        &req.session,
        &req.window,
        &req.pane,
        &req.keys,
        suffix_key,
    )
    .await
    {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Keys sent successfully".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to send keys: {}", e),
        }),
    }
}

/// Capture content from a tmux pane
pub(crate) async fn tmux_capture(Query(params): Query<TmuxCaptureParams>) -> Json<TmuxCaptureResponse> {
    match agent::TmuxAgent::capture_pane(
        &params.session,
        &params.window,
        &params.pane,
        params.lines,
    )
    .await
    {
        Ok(content) => Json(TmuxCaptureResponse {
            success: true,
            content,
        }),
        Err(e) => Json(TmuxCaptureResponse {
            success: false,
            content: format!("Failed to capture: {}", e),
        }),
    }
}

/// Get Claude Code status from a tmux pane
pub(crate) async fn get_claude_status(Query(params): Query<ClaudeStatusParams>) -> Json<ClaudeStatusResponse> {
    match agent::TmuxAgent::get_claude_status(&params.session, &params.window).await {
        Ok(status) => Json(ClaudeStatusResponse {
            success: true,
            status,
        }),
        Err(_) => Json(ClaudeStatusResponse {
            success: false,
            status: agent::ClaudeStatus::default(),
        }),
    }
}

/// List all tmux sessions
pub(crate) async fn tmux_list_sessions() -> Json<TmuxSessionsResponse> {
    let sessions = agent::TmuxAgent::list_sessions()
        .await
        .unwrap_or_default();
    Json(TmuxSessionsResponse { sessions })
}

/// List panes in a tmux window
pub(crate) async fn tmux_list_panes(Query(params): Query<TmuxListPanesParams>) -> Json<TmuxPanesResponse> {
    let panes = agent::TmuxAgent::list_panes(&params.session, &params.window)
        .await
        .unwrap_or_default();
    Json(TmuxPanesResponse { panes })
}

/// List all tmux windows with full details
pub(crate) async fn tmux_list_all_windows() -> Json<TmuxWindowsResponse> {
    let windows = agent::TmuxAgent::list_all_windows()
        .await
        .unwrap_or_default();
    Json(TmuxWindowsResponse { windows })
}

/// Kill a tmux session
pub(crate) async fn tmux_kill_session(Json(req): Json<TmuxKillSessionRequest>) -> Json<CommandResponse> {
    match agent::TmuxAgent::kill_session(&req.session).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Session killed successfully".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to kill session: {}", e),
        }),
    }
}

/// Kill a tmux window (saves window info for resume before killing)
pub(crate) async fn tmux_kill_window(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TmuxKillWindowRequest>,
) -> Json<CommandResponse> {
    // Get window info before killing (for resume functionality)
    let window_info = get_window_info_before_close(&req.session, &req.window).await;

    // Save to closed_windows table if we got valid info
    if let Some((session_id, session_name, window_name, working_dir, git_branch, pane_count)) = window_info {
        let wd = working_dir.clone();
        let git_dir = tokio::task::spawn_blocking(move || {
            crate::agent::TmuxAgent::find_git_root_sync(&wd)
        }).await.ok().flatten().unwrap_or_default();
        let db = &state.state.lock().unwrap().db;
        if let Err(e) = db.save_closed_window(&session_id, &session_name, &window_name, &working_dir, &git_branch, pane_count, &git_dir) {
            warn!("Failed to save closed window info: {}", e);
        }
    }

    // Now kill the window
    match agent::TmuxAgent::kill_window(&req.session, &req.window).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Window killed successfully".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to kill window: {}", e),
        }),
    }
}

/// Get window info before closing (session_id, session_name, window_name, working_dir, git_branch, pane_count)
async fn get_window_info_before_close(session: &str, window: &str) -> Option<(String, String, String, String, String, i32)> {
    use tokio::process::Command;

    // Build target - handle window ID (starts with @) or window name
    let target = if window.starts_with('@') {
        format!("{}:{}", session, window)
    } else {
        format!("{}:={}", session, window)
    };

    // Get session_id, session_name, window_name, working_dir, and pane_count in one call
    let output = Command::new(agent::TMUX_BIN.as_str())
        .args([
            "display-message",
            "-t", &target,
            "-p",
            "#{session_id}|#{session_name}|#{window_name}|#{pane_current_path}|#{window_panes}",
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stdout.trim().split('|').collect();
    if parts.len() != 5 {
        return None;
    }

    let session_id = parts[0].to_string();
    let session_name = parts[1].to_string();
    let window_name = parts[2].to_string();
    let working_dir = parts[3].to_string();
    let pane_count = parts[4].parse::<i32>().unwrap_or(1);

    // Try to get git branch from working dir
    let git_branch = get_git_branch(&working_dir).await.unwrap_or_default();

    Some((session_id, session_name, window_name, working_dir, git_branch, pane_count))
}

/// Get current git branch from a directory
async fn get_git_branch(dir: &str) -> Option<String> {
    use tokio::process::Command;

    let output = Command::new("git")
        .args(["-C", dir, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Create a new tmux window
pub(crate) async fn tmux_new_window(Json(req): Json<TmuxNewWindowRequest>) -> Json<CommandResponse> {
    match agent::TmuxAgent::simple_new_window(&req.session, &req.name).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: format!("Window '{}' created", req.name),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to create window: {}", e),
        }),
    }
}

/// Get closed windows for a session (for resume functionality)
pub(crate) async fn get_closed_windows(
    State(state): State<Arc<AppState>>,
    AxumPath(session_name): AxumPath<String>,
) -> Json<Vec<ClosedWindowInfo>> {
    // Get currently open window names for this session
    let open_windows = agent::TmuxAgent::list_all_windows()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.session_name == session_name)
        .map(|w| w.window_name)
        .collect::<Vec<_>>();

    let state = state.state.lock().unwrap();
    let closed = state.db.load_closed_windows(&session_name, &open_windows)
        .unwrap_or_default()
        .into_iter()
        .map(|w| ClosedWindowInfo {
            id: w.id,
            session_name: w.session_name,
            window_name: w.window_name,
            working_dir: w.working_dir,
            git_branch: w.git_branch,
            pane_count: w.pane_count,
            closed_at: w.closed_at.map(|t| t.to_rfc3339()),
        })
        .collect();

    Json(closed)
}

/// Delete a closed window record
pub(crate) async fn delete_closed_window(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteClosedWindowRequest>,
) -> Json<CommandResponse> {
    let state = state.state.lock().unwrap();
    match state.db.delete_closed_window(req.id) {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: "Closed window record deleted".to_string(),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to delete: {}", e),
        }),
    }
}

/// Resume a closed window with optional layout
pub(crate) async fn resume_closed_window(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ResumeClosedWindowRequest>,
) -> Json<CommandResponse> {
    use std::path::Path;

    // Create new window with correct working directory
    if let Err(e) = agent::TmuxAgent::simple_new_window_with_dir(&req.session, &req.window_name, &req.working_dir).await {
        return Json(CommandResponse {
            success: false,
            message: format!("Failed to create window: {}", e),
        });
    }

    // Apply layout if requested and working_dir exists
    let layout_type = req.layout.as_deref().unwrap_or("simple");
    if layout_type != "simple" && !req.working_dir.is_empty() {
        let working_dir = Path::new(&req.working_dir);
        if working_dir.exists() {
            // Default agent command
            let agent_cmd = "claude --dangerously-skip-permissions".to_string();

            let template = match layout_type {
                "workspace" => layout::LayoutTemplate::Workspace {
                    agent_cmd,
                    frontend_cmd: None,
                    backend_cmd: None,
                },
                _ => layout::LayoutTemplate::Default { agent_cmd },  // "default" = 3-pane
            };

            // Give tmux time to create the window
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            if let Err(e) = layout::LayoutRenderer::create_layout(
                &req.session,
                &req.window_name,
                template,
                working_dir,
            ).await {
                warn!("Failed to apply layout: {}", e);
                // Continue anyway - window was created
            }
        }
    }

    // Delete closed window record if ID provided
    if let Some(id) = req.closed_window_id {
        let db = &state.state.lock().unwrap().db;
        let _ = db.delete_closed_window(id);
    }

    Json(CommandResponse {
        success: true,
        message: format!("Window '{}' resumed with {} layout", req.window_name, layout_type),
    })
}

/// Helper: decode base64 image data URL, save to temp file, return path
fn save_base64_image(base64_data: &str) -> Result<String, String> {
    use base64::Engine;
    use std::io::Write;

    let data_part = if let Some(comma_pos) = base64_data.find(',') {
        &base64_data[comma_pos + 1..]
    } else {
        base64_data
    };

    let ext = if base64_data.starts_with("data:image/png") {
        "png"
    } else if base64_data.starts_with("data:image/jpeg") || base64_data.starts_with("data:image/jpg") {
        "jpg"
    } else if base64_data.starts_with("data:image/gif") {
        "gif"
    } else if base64_data.starts_with("data:image/webp") {
        "webp"
    } else {
        "png"
    };

    let image_data = base64::engine::general_purpose::STANDARD.decode(data_part)
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

    let filename = format!("agent-tracker-img-{}.{}", Uuid::new_v4(), ext);
    let tmp_path = std::path::PathBuf::from("/tmp").join(&filename);

    let mut f = std::fs::File::create(&tmp_path)
        .map_err(|e| format!("Failed to create image file: {}", e))?;
    f.write_all(&image_data)
        .map_err(|e| format!("Failed to write image file: {}", e))?;

    Ok(tmp_path.to_string_lossy().to_string())
}

/// Save base64 image(s) to temp file and send path via tmux send-keys
pub(crate) async fn tmux_send_image(Json(req): Json<SendImageRequest>) -> Json<SendImageResponse> {
    // Collect all images (support both single and multi)
    let mut all_base64: Vec<&str> = Vec::new();
    if !req.images.is_empty() {
        for img in &req.images {
            all_base64.push(img);
        }
    } else if !req.image_base64.is_empty() {
        all_base64.push(&req.image_base64);
    }

    if all_base64.is_empty() {
        return Json(SendImageResponse {
            success: false,
            message: "No image data provided".to_string(),
            image_path: None,
            image_paths: None,
        });
    }

    // Save all images and send each path
    let mut saved_paths: Vec<String> = Vec::new();
    for base64_data in &all_base64 {
        let image_path = match save_base64_image(base64_data) {
            Ok(path) => path,
            Err(e) => {
                return Json(SendImageResponse {
                    success: false,
                    message: e,
                    image_path: saved_paths.first().cloned(),
                    image_paths: if saved_paths.is_empty() { None } else { Some(saved_paths) },
                });
            }
        };

        // Send image path + Enter (Claude Code attaches as [Image #N])
        if let Err(e) = agent::TmuxAgent::send_keys_with_suffix(
            &req.session,
            &req.window_id,
            &req.pane,
            &image_path,
            Some("Enter"),
        )
        .await
        {
            return Json(SendImageResponse {
                success: false,
                message: format!("Failed to send image path: {}", e),
                image_path: Some(image_path),
                image_paths: if saved_paths.is_empty() { None } else { Some(saved_paths) },
            });
        }

        saved_paths.push(image_path);

        // Wait for Claude Code to process the image attachment
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    // Send message text + Enter to submit (or just Enter if no message)
    let msg_text = req.message.as_deref().unwrap_or("").trim().to_string();
    match agent::TmuxAgent::send_keys_with_suffix(
        &req.session,
        &req.window_id,
        &req.pane,
        &msg_text,
        Some("Enter"),
    )
    .await
    {
        Ok(()) => Json(SendImageResponse {
            success: true,
            message: format!("{} image(s) sent successfully", saved_paths.len()),
            image_path: saved_paths.first().cloned(),
            image_paths: Some(saved_paths),
        }),
        Err(e) => Json(SendImageResponse {
            success: false,
            message: format!("Failed to send message: {}", e),
            image_path: saved_paths.first().cloned(),
            image_paths: Some(saved_paths),
        }),
    }
}

/// Select (switch to) a tmux window
pub(crate) async fn tmux_select_window(Json(req): Json<TmuxSelectWindowRequest>) -> Json<CommandResponse> {
    // If window_id is provided, use it for precise targeting
    let window_target = req.window_id.as_deref().unwrap_or(&req.window);

    match agent::TmuxAgent::select_window_by_target(&req.session, window_target).await {
        Ok(()) => Json(CommandResponse {
            success: true,
            message: format!("Switched to window '{}'", req.window),
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to select window: {}", e),
        }),
    }
}

/// Move a window to a new position within the same session (for drag-and-drop reordering).
/// Uses sequential swaps to shift intermediate windows, implementing insert-style reorder.
pub(crate) async fn tmux_swap_window(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TmuxSwapWindowRequest>,
) -> Json<CommandResponse> {
    match agent::TmuxAgent::move_window(&req.session, req.source_index, req.target_index).await {
        Ok(()) => {
            // Trigger broadcast so all clients see the new order
            state.broadcast_state();
            Json(CommandResponse {
                success: true,
                message: format!("Moved window {} → {}", req.source_index, req.target_index),
            })
        }
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to move window: {}", e),
        }),
    }
}

/// Rename a tmux window
pub(crate) async fn tmux_rename_window(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TmuxRenameWindowRequest>,
) -> Json<CommandResponse> {
    let target = format!("{}:{}", req.session, req.window);
    match agent::TmuxAgent::rename_window(&target, &req.name).await {
        Ok(()) => {
            // Disable automatic-rename so tmux doesn't overwrite the new name
            agent::TmuxAgent::set_builtin_window_option(&target, "automatic-rename", "off").await.ok();
            // Update agent_base_name so status icon system uses the new name
            agent::TmuxAgent::set_window_option(&target, "agent_base_name", &req.name).await.ok();
            // Broadcast in background to avoid blocking the response
            let state_clone = state.clone();
            tokio::task::spawn_blocking(move || state_clone.broadcast_state());
            Json(CommandResponse {
                success: true,
                message: format!("Renamed window to '{}'", req.name),
            })
        }
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to rename window: {}", e),
        }),
    }
}

/// Rename a tmux session
pub(crate) async fn tmux_rename_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TmuxRenameSessionRequest>,
) -> Json<CommandResponse> {
    match agent::TmuxAgent::rename_session(&req.session, &req.name).await {
        Ok(()) => {
            let state_clone = state.clone();
            tokio::task::spawn_blocking(move || state_clone.broadcast_state());
            Json(CommandResponse {
                success: true,
                message: format!("Renamed session to '{}'", req.name),
            })
        }
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to rename session: {}", e),
        }),
    }
}

/// Reset a window's layout to the default 3-pane arrangement (yazi + lazygit + agent)
pub(crate) async fn tmux_reset_layout(Json(req): Json<TmuxResetLayoutRequest>) -> Json<CommandResponse> {
    match agent::TmuxAgent::reset_window_layout(&req.session, &req.window).await {
        Ok(msg) => Json(CommandResponse {
            success: true,
            message: msg,
        }),
        Err(e) => Json(CommandResponse {
            success: false,
            message: format!("Failed to reset layout: {}", e),
        }),
    }
}
