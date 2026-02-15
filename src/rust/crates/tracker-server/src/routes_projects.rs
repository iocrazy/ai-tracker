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
// Request / Response Types
// ============================================================================

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
    use crate::agent::TMUX_BIN;

    // Generate session name if not provided
    let session_name = if let Some(name) = req.session_name {
        name
    } else {
        // Count existing tmux sessions to generate prefix
        let output = std::process::Command::new(TMUX_BIN)
            .args(["list-sessions", "-F", "#{session_name}"])
            .output();
        let count = match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).lines().count(),
            Err(_) => 0,
        };
        format!(
            "{}-{}",
            count + 1,
            req.project_name.replace(' ', "-").to_lowercase()
        )
    };

    // Create tmux session
    let result = std::process::Command::new(TMUX_BIN)
        .args(["new-session", "-d", "-s", &session_name, "-c", &req.git_dir])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            // Register the project
            let _ = {
                let server_state = state.state.lock().unwrap();
                server_state
                    .db
                    .register_project(&req.git_dir, &req.project_name)
            };
            Json(CreateSessionResponse {
                success: true,
                session_name: session_name.clone(),
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
pub(crate) async fn get_project_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQueryParams>,
) -> Json<HistoryResponse> {
    let project = match params.project {
        Some(ref p) if !p.is_empty() => p.clone(),
        _ => {
            return Json(HistoryResponse {
                groups: vec![],
                total: 0,
            });
        }
    };

    let pdb = match state.project_dbs.get_or_open(&project) {
        Ok(db) => db,
        Err(e) => {
            error!("Failed to open project DB {}: {}", project, e);
            return Json(HistoryResponse {
                groups: vec![],
                total: 0,
            });
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
            });
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

    Json(HistoryResponse { groups, total })
}
