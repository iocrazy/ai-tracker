//! Project, env var, service, and worktree slot handlers.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::db;
use crate::routes_history::{HistoryEntry, HistoryGroup, HistoryQueryParams, HistoryResponse};
use crate::routes_workspace::{generate_worktree_env_file, sync_worktree_envs};
use crate::AppState;

// ============================================================================
// Grouped history response types
// ============================================================================

/// A window group entry for the grouped timeline view
#[derive(Serialize)]
pub(crate) struct WindowGroupEntryResponse {
    pub group_key: String,
    pub session: String,
    pub window: String,
    pub entry_ids: Vec<i64>,
    pub task_count: i32,
    pub total_messages: i32,
    pub total_duration: f64,
    pub first_started: String,
    pub last_ended: String,
    pub summaries: Vec<String>,
}

/// A date group containing window groups
#[derive(Serialize)]
pub(crate) struct WindowGroupDateGroup {
    pub label: String,
    pub records: Vec<WindowGroupEntryResponse>,
}

/// Response for grouped history
#[derive(Serialize)]
pub(crate) struct WindowGroupResponse {
    pub groups: Vec<WindowGroupDateGroup>,
    pub total: i32,
    pub grouped: bool,
}

// ============================================================================
// Request / Response Types
// ============================================================================

// === Project Todos ===

#[derive(Deserialize)]
pub(crate) struct ProjectTodoQuery {
    pub git_dir: String,
}

#[derive(Deserialize)]
pub(crate) struct CreateProjectTodoRequest {
    pub git_dir: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_todo_status")]
    pub status: String,
    #[serde(default)]
    pub priority: i32,
}

fn default_todo_status() -> String {
    "todo".to_string()
}

#[derive(Deserialize)]
pub(crate) struct UpdateProjectTodoRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

#[derive(Deserialize)]
pub(crate) struct UpdateProjectTodoStatusRequest {
    pub status: String,
}

// === Project Env Vars ===

#[derive(Deserialize)]
pub(crate) struct ProjectEnvVarQuery {
    pub session_name: String,
}

#[derive(Deserialize)]
pub(crate) struct CreateProjectEnvVarRequest {
    pub session_name: String,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub is_secret: bool,
}

#[derive(Deserialize)]
pub(crate) struct UpdateProjectEnvVarRequest {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub is_secret: Option<bool>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

// === Global Env Vars ===

#[derive(Deserialize)]
pub(crate) struct CreateGlobalEnvVarRequest {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub is_secret: bool,
}

#[derive(Deserialize)]
pub(crate) struct UpdateGlobalEnvVarRequest {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub is_secret: Option<bool>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

// === Worktree Env Vars ===

#[derive(Deserialize)]
pub(crate) struct WorktreeEnvVarQuery {
    pub session_name: String,
    pub slot: i32,
}

#[derive(Deserialize)]
pub(crate) struct CreateWorktreeEnvVarRequest {
    pub session_name: String,
    pub slot: i32,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub is_secret: bool,
}

#[derive(Deserialize)]
pub(crate) struct UpdateWorktreeEnvVarRequest {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub is_secret: Option<bool>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

// === Session creation ===

#[derive(Deserialize)]
pub(crate) struct CreateSessionRequest {
    pub project_name: String,
    pub git_dir: String,
    #[serde(default)]
    pub session_name: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreateSessionResponse {
    pub success: bool,
    pub session_name: String,
    pub message: String,
}

// === Project Services ===

#[derive(Deserialize)]
pub(crate) struct CreateProjectServiceRequest {
    pub session_name: String,
    pub service_name: String,
    pub base_value: i32,
    #[serde(default = "default_port_type")]
    pub value_type: String,
    pub env_key: String,
}

fn default_port_type() -> String {
    "port".to_string()
}

#[derive(Deserialize)]
pub(crate) struct UpdateProjectServiceRequest {
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub base_value: Option<i32>,
    #[serde(default)]
    pub value_type: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

// === Worktree Slots ===

#[derive(Deserialize)]
pub(crate) struct CreateWorktreeSlotRequest {
    pub session_name: String,
    pub branch: String,
    #[serde(default)]
    pub worktree_path: Option<String>,
}

// ============================================================================
// Handlers — Project Env Vars
// ============================================================================

/// List project env vars
pub(crate) async fn list_project_env_vars(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProjectEnvVarQuery>,
) -> Json<Vec<db::ProjectEnvVar>> {
    let server_state = state.state.lock().unwrap();
    let vars = server_state
        .db
        .list_project_env_vars(&params.session_name)
        .unwrap_or_default();
    Json(vars)
}

/// Create a project env var
pub(crate) async fn create_project_env_var(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectEnvVarRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state
            .db
            .create_project_env_var(&req.session_name, &req.key, &req.value, req.is_secret)
    };
    match result {
        Ok(id) => {
            sync_worktree_envs(&state, &req.session_name).await;
            Json(serde_json::json!({ "success": true, "id": id }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

/// Update a project env var
pub(crate) async fn update_project_env_var(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<UpdateProjectEnvVarRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.update_project_env_var(
            id,
            req.key.as_deref(),
            req.value.as_deref(),
            req.is_secret,
            req.sort_order,
        )
    };
    match result {
        Ok(session_name) => {
            sync_worktree_envs(&state, &session_name).await;
            Json(serde_json::json!({ "success": true }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

/// Delete a project env var
pub(crate) async fn delete_project_env_var(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.delete_project_env_var(id)
    };
    match result {
        Ok(session_name) => {
            sync_worktree_envs(&state, &session_name).await;
            Json(serde_json::json!({ "success": true }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

// ============================================================================
// Handlers — Global Env Vars
// ============================================================================

pub(crate) async fn list_global_env_vars(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<db::GlobalEnvVar>> {
    let server_state = state.state.lock().unwrap();
    Json(server_state.db.list_global_env_vars().unwrap_or_default())
}

pub(crate) async fn create_global_env_var(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateGlobalEnvVarRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state
            .db
            .create_global_env_var(&req.key, &req.value, req.is_secret)
    };
    match result {
        Ok(id) => Json(serde_json::json!({ "success": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

pub(crate) async fn update_global_env_var(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<UpdateGlobalEnvVarRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.update_global_env_var(
            id,
            req.key.as_deref(),
            req.value.as_deref(),
            req.is_secret,
            req.sort_order,
        )
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

pub(crate) async fn delete_global_env_var(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.delete_global_env_var(id)
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

// ============================================================================
// Handlers — Worktree Env Vars
// ============================================================================

pub(crate) async fn list_worktree_env_vars(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WorktreeEnvVarQuery>,
) -> Json<Vec<db::WorktreeEnvVar>> {
    let server_state = state.state.lock().unwrap();
    Json(
        server_state
            .db
            .list_worktree_env_vars(&params.session_name, params.slot)
            .unwrap_or_default(),
    )
}

pub(crate) async fn create_worktree_env_var(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateWorktreeEnvVarRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state
            .db
            .create_worktree_env_var(&req.session_name, req.slot, &req.key, &req.value, req.is_secret)
    };
    match result {
        Ok(id) => Json(serde_json::json!({ "success": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

pub(crate) async fn update_worktree_env_var(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<UpdateWorktreeEnvVarRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.update_worktree_env_var(
            id,
            req.key.as_deref(),
            req.value.as_deref(),
            req.is_secret,
            req.sort_order,
        )
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

pub(crate) async fn delete_worktree_env_var(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.delete_worktree_env_var(id)
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

// ============================================================================
// Handlers — Effective Env Vars
// ============================================================================

pub(crate) async fn get_effective_env_vars(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WorktreeEnvVarQuery>,
) -> Json<Vec<db::EffectiveEnvVar>> {
    let server_state = state.state.lock().unwrap();
    Json(
        server_state
            .db
            .get_effective_env_vars(&params.session_name, params.slot)
            .unwrap_or_default(),
    )
}

// ============================================================================
// Handlers — Session Creation
// ============================================================================

pub(crate) async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Json<CreateSessionResponse> {
    let workspace_name = req.session_name.unwrap_or_else(|| {
        req.project_name.replace(' ', "-").to_lowercase()
    });

    let working_dir = std::path::Path::new(&req.git_dir);

    // Load config for default layout and agent
    let cfg = match crate::config::AgentConfig::load() {
        Ok(c) => c,
        Err(_) => {
            // Fallback: create bare session if config fails
            return create_bare_session(&workspace_name, &req.git_dir, &req.project_name, &state);
        }
    };

    let agent_def = match cfg.get_agent(&cfg.defaults.agent) {
        Some(a) => a,
        None => {
            return create_bare_session(&workspace_name, &req.git_dir, &req.project_name, &state);
        }
    };
    let layout = match cfg.get_layout(&cfg.defaults.layout) {
        Some(l) => l,
        None => {
            return create_bare_session(&workspace_name, &req.git_dir, &req.project_name, &state);
        }
    };

    // Create workspace with 3-pane layout
    match crate::agent::TmuxAgent::create_workspace(
        &workspace_name,
        "main",
        working_dir,
        layout,
        agent_def,
    )
    .await
    {
        Ok(actual_session) => {
            // Register the project
            let _ = {
                let server_state = state.state.lock().unwrap();
                server_state
                    .db
                    .register_project(&req.git_dir, &req.project_name)
            };
            Json(CreateSessionResponse {
                success: true,
                session_name: actual_session.clone(),
                message: format!("Session '{}' created with layout", actual_session),
            })
        }
        Err(e) => Json(CreateSessionResponse {
            success: false,
            session_name: String::new(),
            message: format!("Failed to create session: {}", e),
        }),
    }
}

fn create_bare_session(
    session_name: &str,
    git_dir: &str,
    project_name: &str,
    state: &Arc<AppState>,
) -> Json<CreateSessionResponse> {
    use crate::agent::TMUX_BIN;

    let result = std::process::Command::new(TMUX_BIN)
        .args(["new-session", "-d", "-s", session_name, "-c", git_dir])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            let _ = {
                let server_state = state.state.lock().unwrap();
                server_state.db.register_project(git_dir, project_name)
            };
            Json(CreateSessionResponse {
                success: true,
                session_name: session_name.to_string(),
                message: format!("Session '{}' created", session_name),
            })
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Json(CreateSessionResponse {
                success: false,
                session_name: String::new(),
                message: format!("tmux error: {}", stderr.trim()),
            })
        }
        Err(e) => Json(CreateSessionResponse {
            success: false,
            session_name: String::new(),
            message: format!("Failed to run tmux: {}", e),
        }),
    }
}

// ============================================================================
// Handlers — Delete Project
// ============================================================================

pub(crate) async fn delete_project(
    State(state): State<Arc<AppState>>,
    AxumPath(git_dir): AxumPath<String>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.delete_project(&git_dir)
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

// ============================================================================
// Handlers — Project Services
// ============================================================================

/// List project services
pub(crate) async fn list_project_services(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProjectEnvVarQuery>,
) -> Json<Vec<db::ProjectService>> {
    let server_state = state.state.lock().unwrap();
    let services = server_state
        .db
        .list_project_services(&params.session_name)
        .unwrap_or_default();
    Json(services)
}

/// Create a project service
pub(crate) async fn create_project_service(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectServiceRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.create_project_service(
            &req.session_name,
            &req.service_name,
            req.base_value,
            &req.value_type,
            &req.env_key,
        )
    };
    match result {
        Ok(id) => {
            sync_worktree_envs(&state, &req.session_name).await;
            Json(serde_json::json!({ "success": true, "id": id }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

/// Update a project service
pub(crate) async fn update_project_service(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<UpdateProjectServiceRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.update_project_service(
            id,
            req.service_name.as_deref(),
            req.base_value,
            req.value_type.as_deref(),
            req.env_key.as_deref(),
            req.sort_order,
        )
    };
    match result {
        Ok(session_name) => {
            sync_worktree_envs(&state, &session_name).await;
            Json(serde_json::json!({ "success": true }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

/// Delete a project service
pub(crate) async fn delete_project_service(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.delete_project_service(id)
    };
    match result {
        Ok(session_name) => {
            sync_worktree_envs(&state, &session_name).await;
            Json(serde_json::json!({ "success": true }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

// ============================================================================
// Handlers — Worktree Slots
// ============================================================================

/// List worktree slots
pub(crate) async fn list_worktree_slots(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProjectEnvVarQuery>,
) -> Json<Vec<db::WorktreeSlot>> {
    let server_state = state.state.lock().unwrap();
    let slots = server_state
        .db
        .list_worktree_slots(&params.session_name)
        .unwrap_or_default();
    Json(slots)
}

/// Create (allocate) a worktree slot
pub(crate) async fn create_worktree_slot(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateWorktreeSlotRequest>,
) -> Json<serde_json::Value> {
    let worktree_path = req.worktree_path.as_deref().unwrap_or("");

    let allocation = {
        let server_state = state.state.lock().unwrap();
        let slot_num = match server_state.db.next_available_slot(&req.session_name) {
            Ok(s) => s,
            Err(e) => {
                return Json(
                    serde_json::json!({ "success": false, "message": format!("{}", e) }),
                );
            }
        };
        match server_state
            .db
            .allocate_worktree_slot(&req.session_name, slot_num, &req.branch, worktree_path)
        {
            Ok(id) => {
                let services = server_state
                    .db
                    .list_project_services(&req.session_name)
                    .unwrap_or_default();
                Ok((id, slot_num, services))
            }
            Err(e) => Err(e),
        }
    };

    match allocation {
        Ok((id, slot_num, services)) => {
            let mut ports = serde_json::Map::new();
            for svc in &services {
                let calculated = svc.base_value + slot_num;
                ports.insert(svc.env_key.clone(), serde_json::json!(calculated));
            }

            // Generate .worktree.env if path provided
            if !worktree_path.is_empty() {
                generate_worktree_env_file(
                    &state,
                    &req.session_name,
                    slot_num,
                    &req.branch,
                    worktree_path,
                )
                .await;
            }

            Json(serde_json::json!({
                "success": true,
                "id": id,
                "slot": slot_num,
                "branch": req.branch,
                "ports": ports,
            }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

/// Delete (free) a worktree slot
pub(crate) async fn delete_worktree_slot(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> Json<serde_json::Value> {
    let server_state = state.state.lock().unwrap();
    match server_state.db.free_worktree_slot_by_id(id) {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

// ============================================================================
// Handlers — Project API
// ============================================================================

/// Get all registered projects
pub(crate) async fn get_projects(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<db::ProjectInfo>> {
    let server_state = state.state.lock().unwrap();
    Json(server_state.db.list_projects().unwrap_or_default())
}

/// Get project history (reads from project's .aitracker/tracker.db)
///
/// Supports `group_by=window` to group entries by session:window.
pub(crate) async fn get_project_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQueryParams>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let project = match params.project {
        Some(ref p) if !p.is_empty() => p.clone(),
        _ => {
            return Json(HistoryResponse {
                groups: vec![],
                total: 0,
            })
            .into_response();
        }
    };

    let pdb = match state.project_dbs.get_or_open(&project) {
        Ok(db) => db,
        Err(e) => {
            error!("Failed to open project DB {}: {}", project, e);
            return Json(HistoryResponse {
                groups: vec![],
                total: 0,
            })
            .into_response();
        }
    };

    let (limit, offset) = if let Some(page) = params.page {
        let per_page = params.per_page.unwrap_or(50);
        let page_offset = (page - 1).max(0) * per_page;
        (per_page, page_offset)
    } else {
        (params.limit.unwrap_or(100), params.offset.unwrap_or(0))
    };

    // Build date range from range param
    let today = chrono::Local::now().date_naive();
    let (start_date, end_date): (Option<String>, Option<String>) = match params.range.as_deref() {
        Some("today") => (
            Some(format!("{}T00:00:00Z", today.format("%Y-%m-%d"))),
            None,
        ),
        Some("yesterday") => {
            let d = today - chrono::Duration::days(1);
            (
                Some(format!("{}T00:00:00Z", d.format("%Y-%m-%d"))),
                Some(format!("{}T23:59:59Z", d.format("%Y-%m-%d"))),
            )
        }
        Some("7days") => (
            Some(format!(
                "{}T00:00:00Z",
                (today - chrono::Duration::days(7)).format("%Y-%m-%d")
            )),
            None,
        ),
        Some("30days") => (
            Some(format!(
                "{}T00:00:00Z",
                (today - chrono::Duration::days(30)).format("%Y-%m-%d")
            )),
            None,
        ),
        _ => (None, None),
    };

    // ── Grouped mode: group_by=window ──
    if params.group_by.as_deref() == Some("window") {
        let (entries, total) = match pdb.load_history_grouped(
            limit,
            offset,
            start_date.as_deref(),
            end_date.as_deref(),
            params.search.as_deref(),
        ) {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to load grouped project history: {}", e);
                return Json(WindowGroupResponse {
                    groups: vec![],
                    total: 0,
                    grouped: true,
                })
                .into_response();
            }
        };

        // Convert to response entries
        let entries: Vec<WindowGroupEntryResponse> = entries
            .into_iter()
            .map(|e| WindowGroupEntryResponse {
                group_key: e.group_key,
                session: e.session,
                window: e.window,
                entry_ids: e.entry_ids,
                task_count: e.task_count,
                total_messages: e.total_messages,
                total_duration: e.total_duration,
                first_started: e.first_started,
                last_ended: e.last_ended,
                summaries: e.summaries,
            })
            .collect();

        // Group by date (use last_ended for date grouping)
        let mut today_groups = vec![];
        let mut yesterday_groups = vec![];
        let mut this_week_groups = vec![];
        let mut older_groups = vec![];
        let yesterday_date = today - chrono::Duration::days(1);

        for entry in entries {
            let date_str = if !entry.last_ended.is_empty() {
                &entry.last_ended
            } else {
                &entry.first_started
            };
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date_str) {
                let date = dt.date_naive();
                if date == today {
                    today_groups.push(entry);
                } else if date == yesterday_date {
                    yesterday_groups.push(entry);
                } else if (today - date).num_days() < 7 {
                    this_week_groups.push(entry);
                } else {
                    older_groups.push(entry);
                }
            } else {
                older_groups.push(entry);
            }
        }

        let mut groups = vec![];
        if !today_groups.is_empty() {
            groups.push(WindowGroupDateGroup {
                label: "Today".to_string(),
                records: today_groups,
            });
        }
        if !yesterday_groups.is_empty() {
            groups.push(WindowGroupDateGroup {
                label: "Yesterday".to_string(),
                records: yesterday_groups,
            });
        }
        if !this_week_groups.is_empty() {
            groups.push(WindowGroupDateGroup {
                label: "This Week".to_string(),
                records: this_week_groups,
            });
        }
        if !older_groups.is_empty() {
            groups.push(WindowGroupDateGroup {
                label: "Older".to_string(),
                records: older_groups,
            });
        }

        return Json(WindowGroupResponse {
            groups,
            total,
            grouped: true,
        })
        .into_response();
    }

    // ── Default: flat entry mode ──
    let project_name = std::path::Path::new(&project)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let (entries, total) = match pdb.load_history_paginated(
        limit,
        offset,
        start_date.as_deref(),
        end_date.as_deref(),
        params.search.as_deref(),
    ) {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to load project history: {}", e);
            return Json(HistoryResponse {
                groups: vec![],
                total: 0,
            })
            .into_response();
        }
    };

    // Convert project_db::HistoryEntry to main HistoryEntry
    let entries: Vec<HistoryEntry> = entries
        .into_iter()
        .map(|e| HistoryEntry {
            id: e.id,
            session: e.session,
            window: e.window,
            summary: e.summary,
            completion_note: e.completion_note,
            duration_seconds: e.duration_seconds,
            started_at: e.started_at,
            ended_at: e.ended_at,
            message_count: e.message_count,
            file_path: e.file_path,
            project: Some(project_name.clone()),
        })
        .collect();

    // Group by date
    let mut today_entries = vec![];
    let mut yesterday_entries = vec![];
    let mut this_week_entries = vec![];
    let mut older_entries = vec![];
    let yesterday_date = today - chrono::Duration::days(1);

    for entry in entries {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&entry.started_at) {
            let date = dt.date_naive();
            if date == today {
                today_entries.push(entry);
            } else if date == yesterday_date {
                yesterday_entries.push(entry);
            } else if (today - date).num_days() < 7 {
                this_week_entries.push(entry);
            } else {
                older_entries.push(entry);
            }
        } else {
            older_entries.push(entry);
        }
    }

    let mut groups = vec![];
    if !today_entries.is_empty() {
        groups.push(HistoryGroup {
            label: "Today".to_string(),
            records: today_entries,
        });
    }
    if !yesterday_entries.is_empty() {
        groups.push(HistoryGroup {
            label: "Yesterday".to_string(),
            records: yesterday_entries,
        });
    }
    if !this_week_entries.is_empty() {
        groups.push(HistoryGroup {
            label: "This Week".to_string(),
            records: this_week_entries,
        });
    }
    if !older_entries.is_empty() {
        groups.push(HistoryGroup {
            label: "Older".to_string(),
            records: older_entries,
        });
    }

    Json(HistoryResponse { groups, total }).into_response()
}

// ============================================================================
// Grouped history detail
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct GroupedDetailParams {
    pub project: String,
    pub ids: String, // comma-separated history IDs
}

/// A task segment within a grouped detail response
#[derive(Serialize)]
struct TaskSegment {
    history_id: i64,
    summary: String,
    started_at: String,
    ended_at: String,
    message_start_index: usize,
    message_count: usize,
}

/// Message with history_id tag for boundary detection
#[derive(Serialize)]
struct GroupedMessage {
    role: String,
    content: String,
    created_at: String,
    history_id: i64,
}

/// Tool usage with history_id
#[derive(Serialize)]
struct GroupedToolUsage {
    id: i64,
    tool_name: String,
    tool_args: String,
    result_summary: String,
    success: bool,
    timestamp: String,
    history_id: i64,
}

/// Commit with history_id
#[derive(Serialize)]
struct GroupedCommit {
    id: i64,
    commit_hash: String,
    commit_message: String,
    files_changed: i32,
    timestamp: String,
    history_id: i64,
}

/// Grouped detail response — messages from multiple history entries merged by time
#[derive(Serialize)]
pub(crate) struct GroupedDetailResponse {
    segments: Vec<TaskSegment>,
    messages: Vec<GroupedMessage>,
    tool_usage: Vec<GroupedToolUsage>,
    commits: Vec<GroupedCommit>,
}

/// Get merged detail for multiple history entries (same session:window)
///
/// IDs come from the per-project DB. We try loading detail data (messages, tools,
/// commits) from the per-project DB first, then fall back to the main DB by
/// matching on started_at timestamp.
pub(crate) async fn get_grouped_detail(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GroupedDetailParams>,
) -> Json<GroupedDetailResponse> {
    let empty = || {
        Json(GroupedDetailResponse {
            segments: vec![],
            messages: vec![],
            tool_usage: vec![],
            commits: vec![],
        })
    };

    let pdb = match state.project_dbs.get_or_open(&params.project) {
        Ok(db) => db,
        Err(e) => {
            error!("Failed to open project DB: {}", e);
            return empty();
        }
    };

    let ids: Vec<i64> = params
        .ids
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if ids.is_empty() {
        return empty();
    }

    // Step 1: Get segments from per-project DB
    let pconn = pdb.conn();
    let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let segment_sql = format!(
        "SELECT id, COALESCE(summary, ''), COALESCE(started_at, ''), COALESCE(completed_at, '')
         FROM history WHERE id IN ({}) ORDER BY started_at ASC",
        placeholders
    );
    let id_params: Vec<Box<dyn rusqlite::ToSql>> = ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>).collect();
    let id_refs: Vec<&dyn rusqlite::ToSql> = id_params.iter().map(|p| p.as_ref()).collect();

    let mut segments: Vec<TaskSegment> = vec![];
    let mut started_ats: Vec<String> = vec![]; // For main DB fallback lookup
    if let Ok(mut stmt) = pconn.prepare(&segment_sql) {
        if let Ok(rows) = stmt.query_map(id_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0).unwrap_or(0),
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, String>(3).unwrap_or_default(),
            ))
        }) {
            for row in rows.flatten() {
                started_ats.push(row.2.clone());
                segments.push(TaskSegment {
                    history_id: row.0,
                    summary: row.1,
                    started_at: row.2,
                    ended_at: row.3,
                    message_start_index: 0,
                    message_count: 0,
                });
            }
        }
    }

    if segments.is_empty() {
        drop(pconn);
        return empty();
    }

    // Step 2: Try loading messages from per-project DB first
    let msg_sql = format!(
        "SELECT cm.role, cm.content, COALESCE(cm.created_at, ''), cm.history_id
         FROM conversation_messages cm
         JOIN history h ON cm.history_id = h.id
         WHERE cm.history_id IN ({})
         AND TRIM(cm.content) != ''
         ORDER BY h.started_at ASC, cm.id ASC",
        placeholders
    );
    let mut messages: Vec<GroupedMessage> = vec![];
    if let Ok(mut stmt) = pconn.prepare(&msg_sql) {
        if let Ok(rows) = stmt.query_map(id_refs.as_slice(), |row| {
            Ok(GroupedMessage {
                role: row.get(0).unwrap_or_default(),
                content: row.get(1).unwrap_or_default(),
                created_at: row.get(2).unwrap_or_default(),
                history_id: row.get(3).unwrap_or(0),
            })
        }) {
            for msg in rows.flatten() {
                messages.push(msg);
            }
        }
    }

    // Load tool usage from per-project DB
    let tool_sql = format!(
        "SELECT tu.id, tu.tool_name, COALESCE(tu.tool_args, ''), COALESCE(tu.result_summary, ''), tu.success, COALESCE(tu.timestamp, ''), tu.history_id
         FROM tool_usage tu
         JOIN history h ON tu.history_id = h.id
         WHERE tu.history_id IN ({})
         ORDER BY h.started_at ASC, tu.id ASC",
        placeholders
    );
    let mut tool_usage: Vec<GroupedToolUsage> = vec![];
    if let Ok(mut stmt) = pconn.prepare(&tool_sql) {
        if let Ok(rows) = stmt.query_map(id_refs.as_slice(), |row| {
            Ok(GroupedToolUsage {
                id: row.get(0).unwrap_or(0),
                tool_name: row.get::<_, String>(1).unwrap_or_default(),
                tool_args: row.get::<_, String>(2).unwrap_or_default(),
                result_summary: row.get::<_, String>(3).unwrap_or_default(),
                success: row.get::<_, i32>(4).unwrap_or(1) != 0,
                timestamp: row.get::<_, String>(5).unwrap_or_default(),
                history_id: row.get(6).unwrap_or(0),
            })
        }) {
            for tool in rows.flatten() {
                tool_usage.push(tool);
            }
        }
    }

    // Load commits from per-project DB
    let commit_sql = format!(
        "SELECT c.id, c.commit_hash, c.commit_message, c.files_changed, COALESCE(c.timestamp, ''), c.history_id
         FROM commits c
         JOIN history h ON c.history_id = h.id
         WHERE c.history_id IN ({})
         ORDER BY h.started_at ASC, c.id ASC",
        placeholders
    );
    let mut commits: Vec<GroupedCommit> = vec![];
    if let Ok(mut stmt) = pconn.prepare(&commit_sql) {
        if let Ok(rows) = stmt.query_map(id_refs.as_slice(), |row| {
            Ok(GroupedCommit {
                id: row.get(0).unwrap_or(0),
                commit_hash: row.get::<_, String>(1).unwrap_or_default(),
                commit_message: row.get::<_, String>(2).unwrap_or_default(),
                files_changed: row.get::<_, i32>(3).unwrap_or(0),
                timestamp: row.get::<_, String>(4).unwrap_or_default(),
                history_id: row.get(5).unwrap_or(0),
            })
        }) {
            for commit in rows.flatten() {
                commits.push(commit);
            }
        }
    }
    drop(pconn);

    // Step 3: On-demand transcript parsing from Claude JSONL session files
    // This is the primary data source — per-project DB and main DB rarely have cached messages
    if messages.is_empty() {
        use std::io::{BufRead, BufReader};
        use chrono::{DateTime, Utc};

        // Determine Claude project directories from git_dir
        // Claude encodes paths: /Volumes/foo/bar -> -Volumes-foo-bar
        // Also encodes _ and . to - (e.g. _playground -> -playground, .config -> -config)
        // Also search worktree dirs: -Volumes-foo-bar--worktrees-*
        if let Some(home) = dirs::home_dir() {
            let encoded: String = params.project.chars().map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }
            }).collect();
            let projects_base = home.join(".claude/projects");

            // Collect all related directories (main + worktrees + sub-paths)
            let mut scan_dirs: Vec<std::path::PathBuf> = vec![];
            let main_dir = projects_base.join(&encoded);
            if main_dir.exists() {
                scan_dirs.push(main_dir);
            }
            // Also scan worktree/sub-path directories matching the prefix
            if let Ok(entries) = std::fs::read_dir(&projects_base) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name != encoded && name.starts_with(&encoded) && entry.path().is_dir() {
                        scan_dirs.push(entry.path());
                    }
                }
            }

            if !scan_dirs.is_empty() {
                // Build index: (first_timestamp, file_path) for each JSONL file across all dirs
                let mut file_index: Vec<(DateTime<Utc>, String)> = vec![];
                for scan_dir in &scan_dirs {
                    if let Ok(entries) = std::fs::read_dir(scan_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                                if let Ok(file) = std::fs::File::open(&path) {
                                    let reader = BufReader::new(file);
                                    for line in reader.lines().take(10) {
                                        if let Ok(line) = line {
                                            if line.trim().is_empty() { continue; }
                                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&line) {
                                                if let Some(ts) = data.get("timestamp").and_then(|t| t.as_str()) {
                                                    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
                                                        file_index.push((dt.with_timezone(&Utc), path.to_string_lossy().to_string()));
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                file_index.sort_by_key(|(ts, _)| *ts);

                if !file_index.is_empty() {
                    // Match each segment to its transcript file
                    // A segment belongs to the JSONL file whose first_ts is <= segment's started_at
                    let mut seg_to_file: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
                    for seg in &segments {
                        if let Ok(seg_start) = DateTime::parse_from_rfc3339(&seg.started_at) {
                            let seg_start = seg_start.with_timezone(&Utc);
                            let buffer = chrono::Duration::seconds(30);
                            // Find the file with the largest first_ts <= seg_start + buffer
                            if let Some((_, path)) = file_index.iter().rev()
                                .find(|(ts, _)| *ts <= seg_start + buffer)
                            {
                                seg_to_file.insert(seg.history_id, path.clone());
                            }
                        }
                    }

                    // Parse each unique transcript file once (cache)
                    let unique_files: std::collections::HashSet<String> = seg_to_file.values().cloned().collect();
                    let mut parse_cache: std::collections::HashMap<String, crate::transcript::TranscriptParseResult> = std::collections::HashMap::new();
                    for file_path in &unique_files {
                        let path = std::path::Path::new(file_path);
                        if path.exists() {
                            if let Ok(parsed) = crate::transcript::parse_transcript_full(path) {
                                parse_cache.insert(file_path.clone(), parsed);
                            }
                        }
                    }

                    // Pre-compute effective end times for each segment.
                    // When completed_at is NULL, use the next segment's started_at as the boundary
                    // to prevent message duplication across segments.
                    let mut effective_ends: std::collections::HashMap<i64, Option<DateTime<Utc>>> = std::collections::HashMap::new();
                    for (i, seg) in segments.iter().enumerate() {
                        let ended = if !seg.ended_at.is_empty() {
                            DateTime::parse_from_rfc3339(&seg.ended_at)
                                .ok().map(|dt| dt.with_timezone(&Utc))
                        } else {
                            // Use next segment's started_at as implicit end boundary
                            if i + 1 < segments.len() {
                                DateTime::parse_from_rfc3339(&segments[i + 1].started_at)
                                    .ok().map(|dt| dt.with_timezone(&Utc))
                            } else {
                                None // Last segment with no end — leave open
                            }
                        };
                        effective_ends.insert(seg.history_id, ended);
                    }

                    // Distribute parsed data to segments by time range
                    for seg in &segments {
                        if let Some(file_path) = seg_to_file.get(&seg.history_id) {
                            if let Some(parsed) = parse_cache.get(file_path) {
                                let started = DateTime::parse_from_rfc3339(&seg.started_at)
                                    .ok().map(|dt| dt.with_timezone(&Utc));
                                let ended = effective_ends.get(&seg.history_id).copied().flatten();
                                let buffer = chrono::Duration::seconds(5);

                                let in_range = |ts: Option<DateTime<Utc>>| -> bool {
                                    if let Some(t) = ts {
                                        let after = started.map_or(true, |s| t >= s - buffer);
                                        let before = ended.map_or(true, |e| t <= e + buffer);
                                        after && before
                                    } else {
                                        false // No timestamp = skip (don't assume in range)
                                    }
                                };

                                for msg in &parsed.messages {
                                    if msg.content.trim().is_empty() { continue; }
                                    if in_range(msg.created_at) {
                                        messages.push(GroupedMessage {
                                            role: msg.role.clone(),
                                            content: msg.content.clone(),
                                            created_at: msg.created_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
                                            history_id: seg.history_id,
                                        });
                                    }
                                }

                                for tool in &parsed.tool_usages {
                                    if in_range(tool.timestamp) {
                                        tool_usage.push(GroupedToolUsage {
                                            id: tool.id,
                                            tool_name: tool.tool_name.clone(),
                                            tool_args: tool.tool_args.clone(),
                                            result_summary: tool.result_summary.clone(),
                                            success: tool.success,
                                            timestamp: tool.timestamp.map(|t| t.to_rfc3339()).unwrap_or_default(),
                                            history_id: seg.history_id,
                                        });
                                    }
                                }

                                for commit in &parsed.commits {
                                    if in_range(commit.timestamp) {
                                        commits.push(GroupedCommit {
                                            id: commit.id,
                                            commit_hash: commit.commit_hash.clone(),
                                            commit_message: commit.commit_message.clone(),
                                            files_changed: commit.files_changed,
                                            timestamp: commit.timestamp.map(|t| t.to_rfc3339()).unwrap_or_default(),
                                            history_id: seg.history_id,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Fill in message_start_index and message_count for each segment
    for seg in &mut segments {
        let start = messages.iter().position(|m| m.history_id == seg.history_id);
        let count = messages.iter().filter(|m| m.history_id == seg.history_id).count();
        seg.message_start_index = start.unwrap_or(0);
        seg.message_count = count;
    }

    Json(GroupedDetailResponse {
        segments,
        messages,
        tool_usage,
        commits,
    })
}

// ============================================================================
// Project metadata update
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct UpdateProjectRequest {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub tech_stack: Option<String>,
}

pub(crate) async fn update_project(
    State(state): State<Arc<AppState>>,
    AxumPath(git_dir): AxumPath<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> Json<crate::CommandResponse> {
    let server_state = state.state.lock().unwrap();
    match server_state.db.update_project(
        &git_dir,
        req.description.as_deref(),
        req.status.as_deref(),
        req.tags.as_deref(),
        req.tech_stack.as_deref(),
    ) {
        Ok(()) => Json(crate::CommandResponse { success: true, message: "Updated".to_string() }),
        Err(e) => Json(crate::CommandResponse { success: false, message: format!("Failed: {}", e) }),
    }
}

// ============================================================================
// Project files API
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct ProjectFilesQuery {
    pub git_dir: String,
}

#[derive(Serialize)]
pub(crate) struct ProjectFileEntry {
    pub name: String,
    pub path: String,
    pub content: String,
    pub exists: bool,
}

#[derive(Serialize)]
pub(crate) struct ProjectFilesResponse {
    pub files: Vec<ProjectFileEntry>,
}

/// Get project-related doc files (CLAUDE.md, MEMORY.md, .worktree.env)
pub(crate) async fn get_project_files(
    Query(params): Query<ProjectFilesQuery>,
) -> Json<ProjectFilesResponse> {
    let git_dir = &params.git_dir;
    let mut files = Vec::new();

    // 1. CLAUDE.md — {git_dir}/CLAUDE.md
    let claude_md_path = std::path::Path::new(git_dir).join("CLAUDE.md");
    let claude_md_str = claude_md_path.to_string_lossy().to_string();
    files.push(if claude_md_path.exists() {
        ProjectFileEntry {
            name: "CLAUDE.md".to_string(),
            path: claude_md_str,
            content: std::fs::read_to_string(&claude_md_path).unwrap_or_default(),
            exists: true,
        }
    } else {
        ProjectFileEntry {
            name: "CLAUDE.md".to_string(),
            path: claude_md_str,
            content: String::new(),
            exists: false,
        }
    });

    // 2. MEMORY.md — ~/.claude/projects/{encoded}/memory/MEMORY.md
    // Encoding: non-alphanumeric non-hyphen chars → '-'
    if let Some(home) = dirs::home_dir() {
        let encoded: String = git_dir.chars().map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }
        }).collect();
        let memory_path = home.join(".claude/projects").join(&encoded).join("memory/MEMORY.md");
        let memory_str = memory_path.to_string_lossy().to_string();
        files.push(if memory_path.exists() {
            ProjectFileEntry {
                name: "MEMORY.md".to_string(),
                path: memory_str,
                content: std::fs::read_to_string(&memory_path).unwrap_or_default(),
                exists: true,
            }
        } else {
            ProjectFileEntry {
                name: "MEMORY.md".to_string(),
                path: memory_str,
                content: String::new(),
                exists: false,
            }
        });
    }

    // 3. .worktree.env — {git_dir}/.worktree.env
    let wt_env_path = std::path::Path::new(git_dir).join(".worktree.env");
    let wt_env_str = wt_env_path.to_string_lossy().to_string();
    files.push(if wt_env_path.exists() {
        ProjectFileEntry {
            name: ".worktree.env".to_string(),
            path: wt_env_str,
            content: std::fs::read_to_string(&wt_env_path).unwrap_or_default(),
            exists: true,
        }
    } else {
        ProjectFileEntry {
            name: ".worktree.env".to_string(),
            path: wt_env_str,
            content: String::new(),
            exists: false,
        }
    });

    Json(ProjectFilesResponse { files })
}

// ============================================================================
// Git info API
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct GitInfoQuery {
    pub git_dir: String,
}

#[derive(Serialize)]
pub(crate) struct GitInfoResponse {
    pub current_branch: String,
    pub branches: Vec<GitBranchInfo>,
    pub status: GitStatus,
}

#[derive(Serialize)]
pub(crate) struct GitBranchInfo {
    pub name: String,
    pub is_current: bool,
    pub last_commit: String,
    pub message: String,
    pub ahead: i32,
    pub behind: i32,
}

#[derive(Serialize)]
pub(crate) struct GitStatus {
    pub modified: i32,
    pub untracked: i32,
    pub staged: i32,
    pub conflicts: i32,
    pub is_clean: bool,
}

pub(crate) async fn get_git_info(
    Query(params): Query<GitInfoQuery>,
) -> Json<serde_json::Value> {
    let git_dir = &params.git_dir;

    // Check directory exists
    if !std::path::Path::new(git_dir).exists() {
        return Json(serde_json::json!({ "error": "Directory not found" }));
    }

    // Get current branch
    let current_branch = run_git(git_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|| "unknown".to_string())
        .trim()
        .to_string();

    // Get local branches with last commit info
    let mut branches = Vec::new();
    if let Some(branch_output) = run_git(git_dir, &["for-each-ref", "--format=%(refname:short)\t%(objectname:short)\t%(subject)", "refs/heads/"]) {
        for line in branch_output.lines() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() >= 3 {
                let name = parts[0].to_string();
                let is_current = name == current_branch;

                // Get ahead/behind vs upstream
                let (ahead, behind) = get_ahead_behind(git_dir, &name);

                branches.push(GitBranchInfo {
                    is_current,
                    last_commit: parts[1].to_string(),
                    message: parts[2].to_string(),
                    ahead,
                    behind,
                    name,
                });
            }
        }
    }

    // Sort: current branch first, then alphabetical
    branches.sort_by(|a, b| b.is_current.cmp(&a.is_current).then(a.name.cmp(&b.name)));

    // Get working tree status
    let status = get_git_status(git_dir);

    Json(serde_json::json!(GitInfoResponse {
        current_branch,
        branches,
        status,
    }))
}

fn run_git(dir: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

fn get_ahead_behind(dir: &str, branch: &str) -> (i32, i32) {
    let upstream = format!("{}@{{upstream}}", branch);
    let range = format!("{}...{}", branch, upstream);
    if let Some(output) = run_git(dir, &["rev-list", "--left-right", "--count", &range]) {
        let parts: Vec<&str> = output.trim().split('\t').collect();
        if parts.len() == 2 {
            let ahead = parts[0].parse().unwrap_or(0);
            let behind = parts[1].parse().unwrap_or(0);
            return (ahead, behind);
        }
    }
    (0, 0)
}

fn get_git_status(dir: &str) -> GitStatus {
    let mut modified = 0;
    let mut untracked = 0;
    let mut staged = 0;
    let mut conflicts = 0;

    if let Some(output) = run_git(dir, &["status", "--porcelain=2"]) {
        for line in output.lines() {
            if line.starts_with("1 ") || line.starts_with("2 ") {
                // Ordinary/rename entry: XY format at position 2-3
                let chars: Vec<char> = line.chars().collect();
                if chars.len() >= 4 {
                    let x = chars[2]; // index status
                    let y = chars[3]; // worktree status
                    if x != '.' { staged += 1; }
                    if y != '.' { modified += 1; }
                }
            } else if line.starts_with("u ") {
                conflicts += 1;
            } else if line.starts_with("? ") {
                untracked += 1;
            }
        }
    }

    GitStatus {
        is_clean: modified == 0 && untracked == 0 && staged == 0 && conflicts == 0,
        modified,
        untracked,
        staged,
        conflicts,
    }
}

// ============================================================================
// Project statistics API
// ============================================================================

#[derive(Deserialize)]
pub(crate) struct StatisticsQuery {
    pub session_name: String,
    #[serde(default = "default_range")]
    pub range: String,
}

fn default_range() -> String { "24h".to_string() }

#[derive(Serialize)]
pub(crate) struct ProjectStatistics {
    pub tasks: TaskStats,
    pub agent_time: AgentTimeStats,
    pub top_tools: Vec<ToolUsage>,
    pub activity: Vec<HourlyActivity>,
}

#[derive(Serialize)]
pub(crate) struct TaskStats {
    pub completed: i32,
    pub in_progress: i32,
    pub failed: i32,
    pub total: i32,
    pub completion_rate: f64,
}

#[derive(Serialize)]
pub(crate) struct AgentTimeStats {
    pub total_seconds: f64,
    pub busy_seconds: f64,
    pub idle_seconds: f64,
}

#[derive(Serialize)]
pub(crate) struct ToolUsage {
    pub tool: String,
    pub count: i32,
}

#[derive(Serialize)]
pub(crate) struct HourlyActivity {
    pub hour: String,
    pub count: i32,
}

pub(crate) async fn get_project_statistics(
    State(state): State<Arc<AppState>>,
    Query(params): Query<StatisticsQuery>,
) -> Json<ProjectStatistics> {
    let server_state = state.state.lock().unwrap();

    // Determine time cutoff from range
    let hours = match params.range.as_str() {
        "24h" => 24,
        "7d" => 168,
        "30d" => 720,
        _ => 24 * 365 * 10, // "all" — 10 years
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);
    let cutoff_str = cutoff.to_rfc3339();

    // Get project git_dir from session_name
    let git_dir = server_state.db.list_projects().ok()
        .and_then(|projects| projects.into_iter()
            .find(|p| p.last_session == params.session_name || p.name == params.session_name)
            .map(|p| p.git_dir));

    // Query history from project-specific DB if available
    let (tasks, agent_time, top_tools, activity) = if let Some(ref gd) = git_dir {
        get_project_stats_from_db(&server_state, gd, &cutoff_str, hours)
    } else {
        // Fallback: query central history table
        get_stats_from_central_db(&server_state, &params.session_name, &cutoff_str, hours)
    };

    Json(ProjectStatistics { tasks, agent_time, top_tools, activity })
}

fn get_project_stats_from_db(
    server_state: &crate::ServerState,
    git_dir: &str,
    cutoff_str: &str,
    hours: i64,
) -> (TaskStats, AgentTimeStats, Vec<ToolUsage>, Vec<HourlyActivity>) {
    // Try to open the project-specific DB
    let aitracker_dir = std::path::Path::new(git_dir).join(".aitracker");
    let db_path = aitracker_dir.join("tracker.db");

    if db_path.exists() {
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            return query_stats_from_conn(&conn, cutoff_str, hours);
        }
    }

    // Fallback to central DB stats
    get_stats_from_central_db(server_state, "", cutoff_str, hours)
}

fn get_stats_from_central_db(
    server_state: &crate::ServerState,
    session_name: &str,
    cutoff_str: &str,
    hours: i64,
) -> (TaskStats, AgentTimeStats, Vec<ToolUsage>, Vec<HourlyActivity>) {
    // Use history table from central DB
    let conn = &server_state.db.conn;

    // Task stats from history table
    let completed: i32 = conn.query_row(
        "SELECT COUNT(*) FROM history WHERE completed_at >= ?1 AND (session_id LIKE ?2 OR ?2 = '')",
        rusqlite::params![cutoff_str, format!("%{}%", session_name)],
        |row| row.get(0),
    ).unwrap_or(0);

    let total_duration: f64 = conn.query_row(
        "SELECT COALESCE(SUM(duration_seconds), 0) FROM history WHERE completed_at >= ?1 AND (session_id LIKE ?2 OR ?2 = '')",
        rusqlite::params![cutoff_str, format!("%{}%", session_name)],
        |row| row.get(0),
    ).unwrap_or(0.0);

    let tasks = TaskStats {
        completed,
        in_progress: 0,
        failed: 0,
        total: completed,
        completion_rate: if completed > 0 { 100.0 } else { 0.0 },
    };

    let agent_time = AgentTimeStats {
        total_seconds: total_duration,
        busy_seconds: total_duration * 0.75, // approximate
        idle_seconds: total_duration * 0.25,
    };

    // Hourly activity
    let activity = get_hourly_activity(conn, cutoff_str, session_name, hours);

    (tasks, agent_time, Vec::new(), activity)
}

fn query_stats_from_conn(
    conn: &rusqlite::Connection,
    cutoff_str: &str,
    hours: i64,
) -> (TaskStats, AgentTimeStats, Vec<ToolUsage>, Vec<HourlyActivity>) {
    // Check if history table exists in project DB
    let has_history: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='history'",
        [], |row| row.get::<_, i32>(0),
    ).unwrap_or(0) > 0;

    if !has_history {
        return (
            TaskStats { completed: 0, in_progress: 0, failed: 0, total: 0, completion_rate: 0.0 },
            AgentTimeStats { total_seconds: 0.0, busy_seconds: 0.0, idle_seconds: 0.0 },
            Vec::new(),
            Vec::new(),
        );
    }

    let completed: i32 = conn.query_row(
        "SELECT COUNT(*) FROM history WHERE completed_at >= ?1",
        rusqlite::params![cutoff_str],
        |row| row.get(0),
    ).unwrap_or(0);

    let total_duration: f64 = conn.query_row(
        "SELECT COALESCE(SUM(duration_seconds), 0) FROM history WHERE completed_at >= ?1",
        rusqlite::params![cutoff_str],
        |row| row.get(0),
    ).unwrap_or(0.0);

    let tasks = TaskStats {
        completed,
        in_progress: 0,
        failed: 0,
        total: completed,
        completion_rate: if completed > 0 { 100.0 } else { 0.0 },
    };

    let agent_time = AgentTimeStats {
        total_seconds: total_duration,
        busy_seconds: total_duration * 0.75,
        idle_seconds: total_duration * 0.25,
    };

    // Top tools — check if tool_usage column exists in history
    let top_tools = get_top_tools(conn, cutoff_str);

    // Hourly activity
    let activity = get_hourly_activity(conn, cutoff_str, "", hours);

    (tasks, agent_time, top_tools, activity)
}

fn get_top_tools(conn: &rusqlite::Connection, cutoff_str: &str) -> Vec<ToolUsage> {
    // Try to get tool stats from session_index or history summary fields
    // The history table stores tool usage in the summary text — parse it out
    let mut tools: std::collections::HashMap<String, i32> = std::collections::HashMap::new();

    if let Ok(mut stmt) = conn.prepare(
        "SELECT summary FROM history WHERE completed_at >= ?1 AND summary IS NOT NULL"
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![cutoff_str], |row| {
            row.get::<_, String>(0)
        }) {
            for row in rows.flatten() {
                // Parse tool mentions from summary (common patterns: "Read", "Edit", "Bash", "Grep", etc.)
                for tool in &["Read", "Edit", "Bash", "Grep", "Write", "Glob", "WebFetch", "WebSearch", "Task"] {
                    if row.contains(tool) {
                        *tools.entry(tool.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut result: Vec<ToolUsage> = tools.into_iter()
        .map(|(tool, count)| ToolUsage { tool, count })
        .collect();
    result.sort_by(|a, b| b.count.cmp(&a.count));
    result.truncate(5);
    result
}

fn get_hourly_activity(conn: &rusqlite::Connection, cutoff_str: &str, session_filter: &str, hours: i64) -> Vec<HourlyActivity> {
    let bucket_count = if hours <= 24 { hours } else if hours <= 168 { hours / 24 } else { 30 }.min(24);
    let bucket_hours = (hours as f64 / bucket_count as f64).ceil() as i64;

    let mut activity = Vec::new();
    let now = chrono::Utc::now();

    for i in (0..bucket_count).rev() {
        let bucket_start = now - chrono::Duration::hours(bucket_hours * (i + 1));
        let bucket_end = now - chrono::Duration::hours(bucket_hours * i);
        let start_str = bucket_start.to_rfc3339();
        let end_str = bucket_end.to_rfc3339();

        let count: i32 = if session_filter.is_empty() {
            conn.query_row(
                "SELECT COUNT(*) FROM history WHERE completed_at >= ?1 AND completed_at < ?2",
                rusqlite::params![start_str, end_str],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM history WHERE completed_at >= ?1 AND completed_at < ?2 AND session_id LIKE ?3",
                rusqlite::params![start_str, end_str, format!("%{}%", session_filter)],
                |row| row.get(0),
            ).unwrap_or(0)
        };

        let label = if hours <= 24 {
            bucket_start.format("%H:%M").to_string()
        } else if hours <= 168 {
            bucket_start.format("%a").to_string()
        } else {
            bucket_start.format("%m/%d").to_string()
        };

        activity.push(HourlyActivity { hour: label, count });
    }

    activity
}

// ============================================================================
// Handlers — Project Todos
// ============================================================================

pub(crate) async fn list_project_todos(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProjectTodoQuery>,
) -> Json<Vec<db::ProjectTodo>> {
    let server_state = state.state.lock().unwrap();
    let todos = server_state
        .db
        .list_project_todos(&params.git_dir)
        .unwrap_or_default();
    Json(todos)
}

pub(crate) async fn create_project_todo(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectTodoRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state
            .db
            .create_project_todo(&req.git_dir, &req.title, &req.description, &req.status, req.priority)
    };
    match result {
        Ok(id) => Json(serde_json::json!({ "success": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

pub(crate) async fn update_project_todo(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<UpdateProjectTodoRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.update_project_todo(
            id,
            req.title.as_deref(),
            req.description.as_deref(),
            req.status.as_deref(),
            req.priority,
            req.sort_order,
        )
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

pub(crate) async fn delete_project_todo(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.delete_project_todo(id)
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

pub(crate) async fn update_project_todo_status(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(req): Json<UpdateProjectTodoStatusRequest>,
) -> Json<serde_json::Value> {
    let result = {
        let server_state = state.state.lock().unwrap();
        server_state.db.update_project_todo(
            id,
            None,
            None,
            Some(&req.status),
            None,
            None,
        )
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

/// Get history entries linked to a specific todo
pub(crate) async fn get_todo_history(
    State(state): State<Arc<AppState>>,
    AxumPath(todo_id): AxumPath<i64>,
) -> Json<serde_json::Value> {
    let server_state = state.state.lock().unwrap();
    match server_state.db.get_history_by_todo_id(todo_id) {
        Ok(records) => {
            let entries: Vec<serde_json::Value> = records
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "summary": r.summary,
                        "completion_note": r.completion_note,
                        "started_at": r.started_at.map(|t| t.to_rfc3339()),
                        "completed_at": r.completed_at.map(|t| t.to_rfc3339()),
                        "duration_seconds": r.duration_seconds,
                    })
                })
                .collect();
            Json(serde_json::json!({ "history": entries }))
        }
        Err(e) => Json(serde_json::json!({ "history": [], "error": format!("{}", e) })),
    }
}
