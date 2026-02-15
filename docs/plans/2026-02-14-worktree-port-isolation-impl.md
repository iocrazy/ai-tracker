# Worktree Port Isolation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add project-level environment variables, service definitions, and automatic worktree port isolation to ai-tracker.

**Architecture:** Three new SQLite tables in the central database store project env vars, service definitions, and worktree slot allocations. CRUD API endpoints manage these. When a worktree is created, a slot is auto-allocated, and `.worktree.env` is generated. Mutations to vars/services trigger auto-sync to all worktree env files. Frontend adds Variables/Services/Worktrees tabs to a project settings view.

**Tech Stack:** Rust (axum, rusqlite), React (TypeScript, Tailwind), SQLite

**Design Doc:** `docs/plans/2026-02-14-worktree-port-isolation-design.md`

---

### Task 1: Add Database Tables

**Files:**
- Modify: `src/rust/crates/tracker-server/src/db.rs` (inside `init_schema()`, after projects table ~line 266)

**Step 1: Add three new tables to init_schema()**

Add after the `projects` table creation (line 266), before the index creation block:

```rust
// Project environment variables (custom key-value, like GitHub repo variables)
self.conn.execute(
    "CREATE TABLE IF NOT EXISTS project_env_vars (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        session_name TEXT NOT NULL,
        key          TEXT NOT NULL,
        value        TEXT NOT NULL DEFAULT '',
        is_secret    INTEGER NOT NULL DEFAULT 0,
        sort_order   INTEGER NOT NULL DEFAULT 0,
        created_at   TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
        UNIQUE(session_name, key)
    )",
    [],
)?;

// Project service definitions (port allocation)
self.conn.execute(
    "CREATE TABLE IF NOT EXISTS project_services (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        session_name TEXT NOT NULL,
        service_name TEXT NOT NULL,
        base_value   INTEGER NOT NULL,
        value_type   TEXT NOT NULL DEFAULT 'port',
        env_key      TEXT NOT NULL,
        sort_order   INTEGER NOT NULL DEFAULT 0,
        UNIQUE(session_name, service_name)
    )",
    [],
)?;

// Worktree slot allocation
self.conn.execute(
    "CREATE TABLE IF NOT EXISTS worktree_slots (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        session_name  TEXT NOT NULL,
        slot          INTEGER NOT NULL,
        branch        TEXT NOT NULL,
        worktree_path TEXT,
        created_at    TEXT NOT NULL DEFAULT (datetime('now')),
        UNIQUE(session_name, slot),
        UNIQUE(session_name, branch)
    )",
    [],
)?;
```

**Step 2: Add DB helper methods for env vars**

Add to the `impl Database` block in `db.rs`:

```rust
// =========================================================================
// Project environment variables
// =========================================================================

pub fn list_project_env_vars(&self, session_name: &str) -> Result<Vec<ProjectEnvVar>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, session_name, key, value, is_secret, sort_order, created_at, updated_at
         FROM project_env_vars WHERE session_name = ? ORDER BY sort_order, id"
    )?;
    let rows = stmt.query_map([session_name], |row| {
        Ok(ProjectEnvVar {
            id: row.get(0)?,
            session_name: row.get(1)?,
            key: row.get(2)?,
            value: row.get(3)?,
            is_secret: row.get(4)?,
            sort_order: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn create_project_env_var(&self, session_name: &str, key: &str, value: &str, is_secret: bool) -> Result<i64> {
    self.conn.execute(
        "INSERT INTO project_env_vars (session_name, key, value, is_secret) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![session_name, key, value, is_secret as i32],
    )?;
    Ok(self.conn.last_insert_rowid())
}

pub fn update_project_env_var(&self, id: i64, key: Option<&str>, value: Option<&str>, is_secret: Option<bool>, sort_order: Option<i32>) -> Result<String> {
    // Get session_name before update for sync
    let session_name: String = self.conn.query_row(
        "SELECT session_name FROM project_env_vars WHERE id = ?", [id],
        |row| row.get(0),
    )?;
    let mut sets = vec!["updated_at = datetime('now')".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];
    if let Some(k) = key { sets.push(format!("key = ?{}", params.len() + 1)); params.push(Box::new(k.to_string())); }
    if let Some(v) = value { sets.push(format!("value = ?{}", params.len() + 1)); params.push(Box::new(v.to_string())); }
    if let Some(s) = is_secret { sets.push(format!("is_secret = ?{}", params.len() + 1)); params.push(Box::new(s as i32)); }
    if let Some(o) = sort_order { sets.push(format!("sort_order = ?{}", params.len() + 1)); params.push(Box::new(o)); }
    params.push(Box::new(id));
    let sql = format!("UPDATE project_env_vars SET {} WHERE id = ?{}", sets.join(", "), params.len());
    self.conn.execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))?;
    Ok(session_name)
}

pub fn delete_project_env_var(&self, id: i64) -> Result<String> {
    let session_name: String = self.conn.query_row(
        "SELECT session_name FROM project_env_vars WHERE id = ?", [id],
        |row| row.get(0),
    )?;
    self.conn.execute("DELETE FROM project_env_vars WHERE id = ?", [id])?;
    Ok(session_name)
}
```

**Step 3: Add DB helper methods for services**

```rust
// =========================================================================
// Project services
// =========================================================================

pub fn list_project_services(&self, session_name: &str) -> Result<Vec<ProjectService>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, session_name, service_name, base_value, value_type, env_key, sort_order
         FROM project_services WHERE session_name = ? ORDER BY sort_order, id"
    )?;
    let rows = stmt.query_map([session_name], |row| {
        Ok(ProjectService {
            id: row.get(0)?,
            session_name: row.get(1)?,
            service_name: row.get(2)?,
            base_value: row.get(3)?,
            value_type: row.get(4)?,
            env_key: row.get(5)?,
            sort_order: row.get(6)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn create_project_service(&self, session_name: &str, service_name: &str, base_value: i32, value_type: &str, env_key: &str) -> Result<i64> {
    self.conn.execute(
        "INSERT INTO project_services (session_name, service_name, base_value, value_type, env_key) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![session_name, service_name, base_value, value_type, env_key],
    )?;
    Ok(self.conn.last_insert_rowid())
}

pub fn update_project_service(&self, id: i64, service_name: Option<&str>, base_value: Option<i32>, value_type: Option<&str>, env_key: Option<&str>, sort_order: Option<i32>) -> Result<String> {
    let session_name: String = self.conn.query_row(
        "SELECT session_name FROM project_services WHERE id = ?", [id],
        |row| row.get(0),
    )?;
    let mut sets = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];
    if let Some(v) = service_name { sets.push(format!("service_name = ?{}", params.len() + 1)); params.push(Box::new(v.to_string())); }
    if let Some(v) = base_value { sets.push(format!("base_value = ?{}", params.len() + 1)); params.push(Box::new(v)); }
    if let Some(v) = value_type { sets.push(format!("value_type = ?{}", params.len() + 1)); params.push(Box::new(v.to_string())); }
    if let Some(v) = env_key { sets.push(format!("env_key = ?{}", params.len() + 1)); params.push(Box::new(v.to_string())); }
    if let Some(v) = sort_order { sets.push(format!("sort_order = ?{}", params.len() + 1)); params.push(Box::new(v)); }
    if sets.is_empty() { return Ok(session_name); }
    params.push(Box::new(id));
    let sql = format!("UPDATE project_services SET {} WHERE id = ?{}", sets.join(", "), params.len());
    self.conn.execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))?;
    Ok(session_name)
}

pub fn delete_project_service(&self, id: i64) -> Result<String> {
    let session_name: String = self.conn.query_row(
        "SELECT session_name FROM project_services WHERE id = ?", [id],
        |row| row.get(0),
    )?;
    self.conn.execute("DELETE FROM project_services WHERE id = ?", [id])?;
    Ok(session_name)
}
```

**Step 4: Add DB helper methods for worktree slots**

```rust
// =========================================================================
// Worktree slots
// =========================================================================

pub fn list_worktree_slots(&self, session_name: &str) -> Result<Vec<WorktreeSlot>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, session_name, slot, branch, worktree_path, created_at
         FROM worktree_slots WHERE session_name = ? ORDER BY slot"
    )?;
    let rows = stmt.query_map([session_name], |row| {
        Ok(WorktreeSlot {
            id: row.get(0)?,
            session_name: row.get(1)?,
            slot: row.get(2)?,
            branch: row.get(3)?,
            worktree_path: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn next_available_slot(&self, session_name: &str) -> Result<i32> {
    let mut stmt = self.conn.prepare(
        "SELECT slot FROM worktree_slots WHERE session_name = ? ORDER BY slot"
    )?;
    let used: Vec<i32> = stmt.query_map([session_name], |row| row.get(0))?
        .filter_map(|r| r.ok()).collect();
    for slot in 1..=15 {
        if !used.contains(&slot) {
            return Ok(slot);
        }
    }
    anyhow::bail!("All 15 slots are in use")
}

pub fn allocate_worktree_slot(&self, session_name: &str, slot: i32, branch: &str, worktree_path: &str) -> Result<i64> {
    self.conn.execute(
        "INSERT INTO worktree_slots (session_name, slot, branch, worktree_path) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![session_name, slot, branch, worktree_path],
    )?;
    Ok(self.conn.last_insert_rowid())
}

pub fn free_worktree_slot_by_branch(&self, session_name: &str, branch: &str) -> Result<()> {
    self.conn.execute(
        "DELETE FROM worktree_slots WHERE session_name = ? AND branch = ?",
        rusqlite::params![session_name, branch],
    )?;
    Ok(())
}

pub fn free_worktree_slot_by_id(&self, id: i64) -> Result<()> {
    self.conn.execute("DELETE FROM worktree_slots WHERE id = ?", [id])?;
    Ok(())
}
```

**Step 5: Add struct definitions**

Add at the top of `db.rs` (or a new `models.rs` if preferred, but inline is fine):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEnvVar {
    pub id: i64,
    pub session_name: String,
    pub key: String,
    pub value: String,
    pub is_secret: i32,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectService {
    pub id: i64,
    pub session_name: String,
    pub service_name: String,
    pub base_value: i32,
    pub value_type: String,
    pub env_key: String,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSlot {
    pub id: i64,
    pub session_name: String,
    pub slot: i32,
    pub branch: String,
    pub worktree_path: Option<String>,
    pub created_at: String,
}
```

**Step 6: Build and verify compilation**

Run: `cargo build -p tracker-server 2>&1 | tail -5`
Expected: successful compilation

**Step 7: Commit**

```bash
git add src/rust/crates/tracker-server/src/db.rs
git commit -m "feat: add project_env_vars, project_services, worktree_slots tables"
```

---

### Task 2: Backend CRUD API Endpoints

**Files:**
- Modify: `src/rust/crates/tracker-server/src/main.rs`

**Step 1: Add request/response structs**

Add near the other request structs (around line 860):

```rust
// === Project Env Vars ===

#[derive(Deserialize)]
struct ProjectEnvVarQuery {
    session_name: String,
}

#[derive(Deserialize)]
struct CreateProjectEnvVarRequest {
    session_name: String,
    key: String,
    value: String,
    #[serde(default)]
    is_secret: bool,
}

#[derive(Deserialize)]
struct UpdateProjectEnvVarRequest {
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    is_secret: Option<bool>,
    #[serde(default)]
    sort_order: Option<i32>,
}

// === Project Services ===

#[derive(Deserialize)]
struct CreateProjectServiceRequest {
    session_name: String,
    service_name: String,
    base_value: i32,
    #[serde(default = "default_port_type")]
    value_type: String,
    env_key: String,
}

fn default_port_type() -> String { "port".to_string() }

#[derive(Deserialize)]
struct UpdateProjectServiceRequest {
    #[serde(default)]
    service_name: Option<String>,
    #[serde(default)]
    base_value: Option<i32>,
    #[serde(default)]
    value_type: Option<String>,
    #[serde(default)]
    env_key: Option<String>,
    #[serde(default)]
    sort_order: Option<i32>,
}

// === Worktree Slots ===

#[derive(Deserialize)]
struct CreateWorktreeSlotRequest {
    session_name: String,
    branch: String,
    #[serde(default)]
    worktree_path: Option<String>,
}
```

**Step 2: Add handler functions for env vars**

```rust
async fn list_project_env_vars(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProjectEnvVarQuery>,
) -> Json<Vec<db::ProjectEnvVar>> {
    Json(state.db.list_project_env_vars(&params.session_name).unwrap_or_default())
}

async fn create_project_env_var(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectEnvVarRequest>,
) -> Json<CommandResponse> {
    match state.db.create_project_env_var(&req.session_name, &req.key, &req.value, req.is_secret) {
        Ok(id) => {
            sync_worktree_envs(&state, &req.session_name).await;
            Json(CommandResponse { success: true, message: format!("Created env var id={}", id) })
        }
        Err(e) => Json(CommandResponse { success: false, message: format!("Failed: {}", e) }),
    }
}

async fn update_project_env_var(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateProjectEnvVarRequest>,
) -> Json<CommandResponse> {
    match state.db.update_project_env_var(id, req.key.as_deref(), req.value.as_deref(), req.is_secret, req.sort_order) {
        Ok(session_name) => {
            sync_worktree_envs(&state, &session_name).await;
            Json(CommandResponse { success: true, message: "Updated".to_string() })
        }
        Err(e) => Json(CommandResponse { success: false, message: format!("Failed: {}", e) }),
    }
}

async fn delete_project_env_var(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<CommandResponse> {
    match state.db.delete_project_env_var(id) {
        Ok(session_name) => {
            sync_worktree_envs(&state, &session_name).await;
            Json(CommandResponse { success: true, message: "Deleted".to_string() })
        }
        Err(e) => Json(CommandResponse { success: false, message: format!("Failed: {}", e) }),
    }
}
```

**Step 3: Add handler functions for services**

```rust
async fn list_project_services(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProjectEnvVarQuery>,
) -> Json<Vec<db::ProjectService>> {
    Json(state.db.list_project_services(&params.session_name).unwrap_or_default())
}

async fn create_project_service(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProjectServiceRequest>,
) -> Json<CommandResponse> {
    match state.db.create_project_service(&req.session_name, &req.service_name, req.base_value, &req.value_type, &req.env_key) {
        Ok(id) => {
            sync_worktree_envs(&state, &req.session_name).await;
            Json(CommandResponse { success: true, message: format!("Created service id={}", id) })
        }
        Err(e) => Json(CommandResponse { success: false, message: format!("Failed: {}", e) }),
    }
}

async fn update_project_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateProjectServiceRequest>,
) -> Json<CommandResponse> {
    match state.db.update_project_service(id, req.service_name.as_deref(), req.base_value, req.value_type.as_deref(), req.env_key.as_deref(), req.sort_order) {
        Ok(session_name) => {
            sync_worktree_envs(&state, &session_name).await;
            Json(CommandResponse { success: true, message: "Updated".to_string() })
        }
        Err(e) => Json(CommandResponse { success: false, message: format!("Failed: {}", e) }),
    }
}

async fn delete_project_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<CommandResponse> {
    match state.db.delete_project_service(id) {
        Ok(session_name) => {
            sync_worktree_envs(&state, &session_name).await;
            Json(CommandResponse { success: true, message: "Deleted".to_string() })
        }
        Err(e) => Json(CommandResponse { success: false, message: format!("Failed: {}", e) }),
    }
}
```

**Step 4: Add handler functions for worktree slots**

```rust
async fn list_worktree_slots(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProjectEnvVarQuery>,
) -> Json<Vec<db::WorktreeSlot>> {
    Json(state.db.list_worktree_slots(&params.session_name).unwrap_or_default())
}

async fn create_worktree_slot(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateWorktreeSlotRequest>,
) -> Json<serde_json::Value> {
    let slot = match state.db.next_available_slot(&req.session_name) {
        Ok(s) => s,
        Err(e) => return Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    };
    let wt_path = req.worktree_path.unwrap_or_default();
    match state.db.allocate_worktree_slot(&req.session_name, slot, &req.branch, &wt_path) {
        Ok(id) => {
            // Generate .worktree.env if path exists
            if !wt_path.is_empty() {
                generate_worktree_env_file(&state, &req.session_name, slot, &req.branch, &wt_path).await;
            }
            // Build calculated ports for response
            let services = state.db.list_project_services(&req.session_name).unwrap_or_default();
            let ports: serde_json::Map<String, serde_json::Value> = services.iter().map(|svc| {
                (svc.env_key.clone(), serde_json::json!(svc.base_value + slot))
            }).collect();
            Json(serde_json::json!({
                "success": true,
                "id": id,
                "slot": slot,
                "branch": req.branch,
                "ports": ports,
            }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "message": format!("{}", e) })),
    }
}

async fn delete_worktree_slot(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<CommandResponse> {
    match state.db.free_worktree_slot_by_id(id) {
        Ok(()) => Json(CommandResponse { success: true, message: "Slot freed".to_string() }),
        Err(e) => Json(CommandResponse { success: false, message: format!("Failed: {}", e) }),
    }
}
```

**Step 5: Add the .worktree.env generation function**

```rust
async fn generate_worktree_env_file(
    state: &Arc<AppState>,
    session_name: &str,
    slot: i32,
    branch: &str,
    worktree_path: &str,
) {
    let vars = state.db.list_project_env_vars(session_name).unwrap_or_default();
    let services = state.db.list_project_services(session_name).unwrap_or_default();

    if vars.is_empty() && services.is_empty() {
        return; // Nothing to generate
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

    let env_path = std::path::Path::new(worktree_path).join(".worktree.env");
    if let Err(e) = tokio::fs::write(&env_path, &content).await {
        warn!("Failed to write .worktree.env at {:?}: {}", env_path, e);
    } else {
        info!("Generated .worktree.env at {:?}", env_path);
    }
}

async fn sync_worktree_envs(state: &Arc<AppState>, session_name: &str) {
    let slots = state.db.list_worktree_slots(session_name).unwrap_or_default();
    for slot in &slots {
        if let Some(ref path) = slot.worktree_path {
            if !path.is_empty() && std::path::Path::new(path).exists() {
                generate_worktree_env_file(state, session_name, slot.slot, &slot.branch, path).await;
            }
        }
    }
}
```

**Step 6: Register routes**

Add to the router builder (after the git routes, ~line 5532):

```rust
// Project environment & worktree isolation
.route("/api/project/env-vars", get(list_project_env_vars).post(create_project_env_var))
.route("/api/project/env-vars/:id", put(update_project_env_var).delete(delete_project_env_var))
.route("/api/project/services", get(list_project_services).post(create_project_service))
.route("/api/project/services/:id", put(update_project_service).delete(delete_project_service))
.route("/api/project/worktree-slots", get(list_worktree_slots).post(create_worktree_slot))
.route("/api/project/worktree-slots/:id", delete(delete_worktree_slot))
```

**Step 7: Build and verify**

Run: `cargo build -p tracker-server 2>&1 | tail -10`
Expected: successful compilation

**Step 8: Commit**

```bash
git add src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: add CRUD API for project env vars, services, and worktree slots"
```

---

### Task 3: Integrate with Workspace Start/Destroy

**Files:**
- Modify: `src/rust/crates/tracker-server/src/main.rs` (start_workspace ~line 3700, destroy_workspace ~line 3950)

**Step 1: Hook into start_workspace()**

After worktree creation and port allocation (~line 3701), before storing tmux options, add:

```rust
// Auto-allocate worktree slot and generate .worktree.env
let services = state.db.list_project_services(&session_name).unwrap_or_default();
if !services.is_empty() {
    if let Ok(slot) = state.db.next_available_slot(&session_name) {
        let wt_path_str = worktree_path.to_string_lossy().to_string();
        if let Ok(_) = state.db.allocate_worktree_slot(&session_name, slot, &req.branch, &wt_path_str) {
            generate_worktree_env_file(&state, &session_name, slot, &req.branch, &wt_path_str).await;
            // Override port allocation with slot-based values
            for svc in &services {
                let val = svc.base_value + slot;
                match svc.env_key.as_str() {
                    "FRONTEND_PORT" => { allocated_frontend_port = Some(val as u16); }
                    "BACKEND_PORT" => { allocated_backend_port = Some(val as u16); }
                    _ => {}
                }
            }
        }
    }
}
```

**Step 2: Hook into destroy_workspace()**

Before worktree removal (~line 3965), add:

```rust
// Free worktree slot
let _ = state.db.free_worktree_slot_by_branch(&session_name, &req.branch);
```

**Step 3: Build and verify**

Run: `cargo build -p tracker-server 2>&1 | tail -5`

**Step 4: Commit**

```bash
git add src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: auto-allocate worktree slot on workspace start, free on destroy"
```

---

### Task 4: Frontend API Service

**Files:**
- Modify: `web/src/services/api.ts`

**Step 1: Add TypeScript interfaces**

```typescript
// Project Environment Variables
export interface ProjectEnvVar {
  id: number;
  session_name: string;
  key: string;
  value: string;
  is_secret: number;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface ProjectService {
  id: number;
  session_name: string;
  service_name: string;
  base_value: number;
  value_type: string;
  env_key: string;
  sort_order: number;
}

export interface WorktreeSlot {
  id: number;
  session_name: string;
  slot: number;
  branch: string;
  worktree_path: string | null;
  created_at: string;
}
```

**Step 2: Add API functions**

```typescript
// Project Env Vars
export async function fetchProjectEnvVars(sessionName: string): Promise<ProjectEnvVar[]> {
  const res = await authFetch(`${API_BASE}/project/env-vars?session_name=${encodeURIComponent(sessionName)}`);
  return res.ok ? res.json() : [];
}

export async function createProjectEnvVar(sessionName: string, key: string, value: string, isSecret = false) {
  return authFetch(`${API_BASE}/project/env-vars`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session_name: sessionName, key, value, is_secret: isSecret }),
  }).then(r => r.json());
}

export async function updateProjectEnvVar(id: number, updates: { key?: string; value?: string; is_secret?: boolean; sort_order?: number }) {
  return authFetch(`${API_BASE}/project/env-vars/${id}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  }).then(r => r.json());
}

export async function deleteProjectEnvVar(id: number) {
  return authFetch(`${API_BASE}/project/env-vars/${id}`, { method: 'DELETE' }).then(r => r.json());
}

// Project Services
export async function fetchProjectServices(sessionName: string): Promise<ProjectService[]> {
  const res = await authFetch(`${API_BASE}/project/services?session_name=${encodeURIComponent(sessionName)}`);
  return res.ok ? res.json() : [];
}

export async function createProjectService(sessionName: string, serviceName: string, baseValue: number, valueType: string, envKey: string) {
  return authFetch(`${API_BASE}/project/services`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session_name: sessionName, service_name: serviceName, base_value: baseValue, value_type: valueType, env_key: envKey }),
  }).then(r => r.json());
}

export async function updateProjectService(id: number, updates: { service_name?: string; base_value?: number; value_type?: string; env_key?: string; sort_order?: number }) {
  return authFetch(`${API_BASE}/project/services/${id}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  }).then(r => r.json());
}

export async function deleteProjectService(id: number) {
  return authFetch(`${API_BASE}/project/services/${id}`, { method: 'DELETE' }).then(r => r.json());
}

// Worktree Slots
export async function fetchWorktreeSlots(sessionName: string): Promise<WorktreeSlot[]> {
  const res = await authFetch(`${API_BASE}/project/worktree-slots?session_name=${encodeURIComponent(sessionName)}`);
  return res.ok ? res.json() : [];
}

export async function createWorktreeSlot(sessionName: string, branch: string, worktreePath?: string) {
  return authFetch(`${API_BASE}/project/worktree-slots`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session_name: sessionName, branch, worktree_path: worktreePath }),
  }).then(r => r.json());
}

export async function deleteWorktreeSlot(id: number) {
  return authFetch(`${API_BASE}/project/worktree-slots/${id}`, { method: 'DELETE' }).then(r => r.json());
}
```

**Step 3: Commit**

```bash
git add web/src/services/api.ts
git commit -m "feat: add frontend API functions for project env vars, services, and worktree slots"
```

---

### Task 5: Frontend — Project Settings Component (Variables Tab)

**Files:**
- Create: `web/src/components/ProjectSettings.tsx`

**Step 1: Create ProjectSettings component with Variables tab**

Create `web/src/components/ProjectSettings.tsx` — a modal/panel component with three tabs: Variables, Services, Worktrees. Start with Variables tab implementing:

- Table listing all env vars for the selected session
- Add new variable form (key, value, is_secret checkbox)
- Inline edit on click
- Delete button with confirmation
- Secret values masked with dots, revealed on edit
- Call `fetchProjectEnvVars`, `createProjectEnvVar`, `updateProjectEnvVar`, `deleteProjectEnvVar`

Follow existing component patterns (e.g., `AddWindowModal.tsx` for modal structure, tailwind classes).

**Step 2: Add Services tab**

Same table structure for service definitions:
- Columns: Service Name, Base Value, Type (port/db_index), Env Key
- Add/Edit/Delete operations
- Call `fetchProjectServices`, `createProjectService`, `updateProjectService`, `deleteProjectService`

**Step 3: Add Worktrees tab**

Read-only overview of allocated slots:
- Table: Slot #, Branch, Worktree Path, Calculated Ports (from services × slot)
- Delete button to free a slot
- "New Worktree" button that opens branch selector → calls `createWorktreeSlot`
- Call `fetchWorktreeSlots`, `fetchProjectServices` (for port calculation), `deleteWorktreeSlot`

**Step 4: Integrate into App.tsx**

Add a settings icon/button per session in the sidebar or session header that opens `ProjectSettings` with the session name.

**Step 5: Build and verify**

Run: `cd web && npm run build`
Expected: successful build

**Step 6: Commit**

```bash
git add web/src/components/ProjectSettings.tsx web/src/App.tsx
git commit -m "feat: add project settings UI with Variables, Services, and Worktrees tabs"
```

---

### Task 6: Deploy and Test End-to-End

**Step 1: Build and deploy frontend**

```bash
cd /Volumes/program/project-code/repos/ai-tracker/web && npm run build
cp -r dist/* ~/.config/agent-tracker/web/dist/
```

**Step 2: Build and deploy backend**

```bash
cd /Volumes/program/project-code/repos/ai-tracker
cargo build --release -p tracker-server
cp target/release/tracker-server /opt/homebrew/opt/agent-tracker-server/bin/tracker-server
codesign -fs - /opt/homebrew/opt/agent-tracker-server/bin/tracker-server
brew services restart agent-tracker-server
```

**Step 3: Test via curl**

```bash
# Create env var
curl -s -X POST http://localhost:3099/api/project/env-vars \
  -H "Content-Type: application/json" \
  -d '{"session_name":"mediahub","key":"REDIS_HOST","value":"127.0.0.1"}' | jq .

# Create service
curl -s -X POST http://localhost:3099/api/project/services \
  -H "Content-Type: application/json" \
  -d '{"session_name":"mediahub","service_name":"frontend","base_value":5175,"value_type":"port","env_key":"FRONTEND_PORT"}' | jq .

# Allocate slot
curl -s -X POST http://localhost:3099/api/project/worktree-slots \
  -H "Content-Type: application/json" \
  -d '{"session_name":"mediahub","branch":"feature/test","worktree_path":"/tmp/test-worktree"}' | jq .

# List slots
curl -s "http://localhost:3099/api/project/worktree-slots?session_name=mediahub" | jq .
```

**Step 4: Test Web UI**

Open http://localhost:3099, navigate to project settings, verify Variables/Services/Worktrees tabs work.

**Step 5: Commit if any fixes**

```bash
git add -A && git commit -m "fix: polish worktree port isolation feature"
```

---

### Task 7: MEMORY.md Pointer Writer (Optional, Post-Deploy)

**Files:**
- Modify: `src/rust/crates/tracker-server/src/main.rs` (in `generate_worktree_env_file` or `start_workspace`)

**Step 1: Add MEMORY.md writer function**

```rust
async fn write_memory_md_pointer(worktree_path: &str) {
    // Encode path for Claude Code memory directory
    let encoded = worktree_path.replace('/', "-");
    let memory_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("projects")
        .join(&encoded)
        .join("memory");

    if let Err(e) = tokio::fs::create_dir_all(&memory_dir).await {
        warn!("Failed to create memory dir: {}", e);
        return;
    }

    let memory_file = memory_dir.join("MEMORY.md");
    let pointer_line = "\n## Dev Environment\nWhen starting dev servers or configuring ports, read .worktree.env in the project root for isolated port assignments.\n";

    // Check if already present
    if let Ok(existing) = tokio::fs::read_to_string(&memory_file).await {
        if existing.contains(".worktree.env") {
            return; // Already has pointer
        }
        // Append
        let new_content = format!("{}\n{}", existing.trim(), pointer_line);
        let _ = tokio::fs::write(&memory_file, new_content).await;
    } else {
        // Create new
        let _ = tokio::fs::write(&memory_file, pointer_line).await;
    }
}
```

**Step 2: Call from start_workspace after env generation**

**Step 3: Build, deploy, commit**

```bash
cargo build --release -p tracker-server
git add src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: auto-write MEMORY.md pointer on worktree creation"
```
