# Self-Contained Tauri App — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Consolidate all runtime files into the Tauri `.app` bundle (read-only assets in Resources/) and macOS Application Support (writable data), eliminating the need for `~/.config/agent-tracker/` when running as a Tauri app.

**Architecture:** Server reads two env vars (`TRACKER_RESOURCES_DIR`, `TRACKER_DATA_DIR`) set by Tauri at sidecar launch. When env vars are absent, falls back to `~/.config/agent-tracker/` for standalone mode. First-launch migration copies existing data to Application Support.

**Tech Stack:** Rust (axum server), Tauri v2 (tauri-plugin-shell sidecar), SQLite, macOS Application Support

---

### Task 1: Create `paths.rs` — TrackerPaths struct

**Files:**
- Create: `src/rust/crates/tracker-server/src/paths.rs`

**Step 1: Create the paths module**

```rust
//! Centralized path resolution for tracker-server.
//!
//! Supports two modes:
//! - Tauri sidecar: reads TRACKER_RESOURCES_DIR + TRACKER_DATA_DIR env vars
//! - Standalone: falls back to ~/.config/agent-tracker/ for everything

use std::path::PathBuf;
use tracing::info;

/// All resolved paths used by the server.
#[derive(Debug, Clone)]
pub struct TrackerPaths {
    /// Read-only assets root (web-dist, scripts, default config)
    pub resources_dir: PathBuf,
    /// Writable data root (db, logs, backups, config, run)
    pub data_dir: PathBuf,
    // Derived paths (convenience)
    pub config_path: PathBuf,
    pub db_path: PathBuf,
    pub log_dir: PathBuf,
    pub log_path: PathBuf,
    pub backup_dir: PathBuf,
    pub web_dist_dir: PathBuf,
    pub scripts_dir: PathBuf,
    pub run_dir: PathBuf,
}

impl TrackerPaths {
    /// Resolve all paths from env vars with fallback chain.
    pub fn resolve() -> Self {
        let legacy_dir = Self::legacy_config_dir();

        // Writable data directory
        let data_dir = std::env::var("TRACKER_DATA_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| legacy_dir.clone());

        // Read-only resources directory
        let resources_dir = std::env::var("TRACKER_RESOURCES_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                // Try relative to executable: ../../Resources/ (inside .app bundle)
                std::env::current_exe().ok().and_then(|exe| {
                    let candidate = exe
                        .parent()?       // MacOS/
                        .parent()?       // Contents/
                        .join("Resources");
                    if candidate.join("web-dist").exists() {
                        Some(candidate)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| legacy_dir.clone());

        let paths = Self {
            config_path: data_dir.join("agent-config.json"),
            db_path: data_dir.join("data").join("tracker.db"),
            log_dir: data_dir.join("logs"),
            log_path: data_dir.join("logs").join("tracker-server.log"),
            backup_dir: data_dir.join("backups"),
            run_dir: data_dir.join("run"),
            web_dist_dir: resources_dir.join("web-dist"),
            scripts_dir: resources_dir.join("scripts"),
            resources_dir,
            data_dir,
        };

        info!("TrackerPaths resolved:");
        info!("  resources_dir: {:?}", paths.resources_dir);
        info!("  data_dir:      {:?}", paths.data_dir);
        info!("  db_path:       {:?}", paths.db_path);
        info!("  web_dist_dir:  {:?}", paths.web_dist_dir);
        info!("  scripts_dir:   {:?}", paths.scripts_dir);

        paths
    }

    /// Create writable data directories if they don't exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.data_dir.join("data"))?;
        std::fs::create_dir_all(&self.log_dir)?;
        std::fs::create_dir_all(&self.backup_dir)?;
        std::fs::create_dir_all(&self.run_dir)?;
        Ok(())
    }

    /// Migrate data from legacy ~/.config/agent-tracker/ if this is a fresh data dir.
    /// Only runs when TRACKER_DATA_DIR is set (Tauri mode) and the data dir is empty.
    pub fn migrate_if_needed(&self) {
        // Only migrate if we're using a non-legacy data dir
        let legacy = Self::legacy_config_dir();
        if self.data_dir == legacy {
            return; // Standalone mode, no migration needed
        }

        let legacy_db = legacy.join("data").join("tracker.db");
        if self.db_path.exists() || !legacy_db.exists() {
            return; // Already migrated or nothing to migrate
        }

        info!("First launch detected — migrating data from {:?}", legacy);

        // Copy database
        if let Err(e) = std::fs::copy(&legacy_db, &self.db_path) {
            tracing::error!("Failed to migrate database: {}", e);
            return;
        }
        info!("  Migrated tracker.db ({:.1}MB)",
            std::fs::metadata(&self.db_path).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0));

        // Copy config
        let legacy_config = legacy.join("agent-config.json");
        if legacy_config.exists() && !self.config_path.exists() {
            if let Err(e) = std::fs::copy(&legacy_config, &self.config_path) {
                tracing::warn!("Failed to migrate config: {}", e);
            } else {
                info!("  Migrated agent-config.json");
            }
        }

        // Copy run/ contents
        let legacy_run = legacy.join("run");
        if legacy_run.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&legacy_run) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let dest = self.run_dir.join(entry.file_name());
                    let _ = std::fs::copy(entry.path(), dest);
                }
                info!("  Migrated run/ contents");
            }
        }

        info!("Migration complete. You can now delete {:?}", legacy);
    }

    /// Legacy config directory path (~/.config/agent-tracker)
    fn legacy_config_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("agent-tracker")
    }
}
```

**Step 2: Register the module in main.rs**

In `src/rust/crates/tracker-server/src/main.rs`, add `mod paths;` after line 11 (after `mod env_file;`):

```rust
mod paths;
```

**Step 3: Verify it compiles**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server`
Expected: compiles with no errors (paths.rs is defined but not yet used, which is fine)

**Step 4: Commit**

```bash
git add src/rust/crates/tracker-server/src/paths.rs src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: add paths.rs module for centralized path resolution"
```

---

### Task 2: Update `config.rs` — Use TRACKER_DATA_DIR for config path

**Files:**
- Modify: `src/rust/crates/tracker-server/src/config.rs:236-244` (default_path)
- Modify: `src/rust/crates/tracker-server/src/config.rs:322-334` (save)

**Step 1: Update `default_path()` to check env var**

Replace `AgentConfig::default_path()` (lines 238-244):

```rust
    /// Get the default config file path.
    /// Checks TRACKER_DATA_DIR env var first (Tauri mode), falls back to ~/.config/agent-tracker/.
    pub fn default_path() -> PathBuf {
        if let Ok(data_dir) = std::env::var("TRACKER_DATA_DIR") {
            if !data_dir.is_empty() {
                return PathBuf::from(data_dir).join("agent-config.json");
            }
        }
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("agent-tracker")
            .join("agent-config.json")
    }
```

**Step 2: Verify it compiles**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server`
Expected: compiles with no errors

**Step 3: Commit**

```bash
git add src/rust/crates/tracker-server/src/config.rs
git commit -m "feat: config.rs respects TRACKER_DATA_DIR env var"
```

---

### Task 3: Update `db.rs` — Use TRACKER_DATA_DIR for db path

**Files:**
- Modify: `src/rust/crates/tracker-server/src/db.rs:2487-2494` (default_db_path)

**Step 1: Update `default_db_path()` to check env var**

Replace the `default_db_path` function (lines 2487-2494):

```rust
/// Get default database path.
/// Checks TRACKER_DATA_DIR env var first (Tauri mode), falls back to ~/.config/agent-tracker/.
pub fn default_db_path() -> std::path::PathBuf {
    if let Ok(data_dir) = std::env::var("TRACKER_DATA_DIR") {
        if !data_dir.is_empty() {
            return std::path::PathBuf::from(data_dir).join("data").join("tracker.db");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".config")
        .join("agent-tracker")
        .join("data")
        .join("tracker.db")
}
```

**Step 2: Verify it compiles**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server`
Expected: compiles with no errors

**Step 3: Commit**

```bash
git add src/rust/crates/tracker-server/src/db.rs
git commit -m "feat: db.rs respects TRACKER_DATA_DIR env var"
```

---

### Task 4: Integrate TrackerPaths into main.rs — Add to AppState

**Files:**
- Modify: `src/rust/crates/tracker-server/src/main.rs:140-175` (AppState)
- Modify: `src/rust/crates/tracker-server/src/main.rs:2403-2440` (main fn init)

**Step 1: Add `paths` field to AppState**

At `main.rs:140`, add field to the struct:

```rust
pub(crate) struct AppState {
    state: Mutex<ServerState>,
    broadcast_tx: broadcast::Sender<RealtimeMessage>,
    last_tmux_windows: Mutex<Vec<agent::TmuxWindowInfo>>,
    stream_manager: stream::StreamManager,
    project_dbs: project_db::ProjectDbManager,
    chat_watcher: chat_watcher::ChatWatcher,
    auth_token: String,
    allowed_origins: Vec<String>,
    start_time: std::time::Instant,
    /// Resolved paths for all server directories
    paths: paths::TrackerPaths,
}
```

Update `AppState::new` to accept and store TrackerPaths (line 160):

```rust
    fn new(db: Database, auth_token: String, allowed_origins: Vec<String>, paths: paths::TrackerPaths) -> Result<Self> {
        let (broadcast_tx, _) = broadcast::channel(16);
        let mut state = ServerState::new(db);
        state.load_from_db()?;
        Ok(Self {
            state: Mutex::new(state),
            broadcast_tx,
            last_tmux_windows: Mutex::new(Vec::new()),
            stream_manager: stream::StreamManager::new(),
            project_dbs: project_db::ProjectDbManager::new(),
            chat_watcher: chat_watcher::ChatWatcher::new(),
            auth_token,
            allowed_origins,
            start_time: std::time::Instant::now(),
            paths,
        })
    }
```

**Step 2: Initialize TrackerPaths in main() and pass to AppState**

In the `main()` function (around line 2403), add TrackerPaths initialization right after logging init (after line 2411):

```rust
    // Resolve and initialize paths
    let paths = paths::TrackerPaths::resolve();
    if let Err(e) = paths.ensure_dirs() {
        error!("Failed to create data directories: {}", e);
    }
    paths.migrate_if_needed();
```

Update the `AppState::new` call at line 2440:

```rust
    let app_state = Arc::new(AppState::new(db, auth_token, allowed_origins, paths)?);
```

**Step 3: Verify it compiles**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server`
Expected: compiles (warnings about unused `paths` field are OK — we'll use it next)

**Step 4: Commit**

```bash
git add src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: integrate TrackerPaths into AppState"
```

---

### Task 5: Replace hardcoded paths in main.rs — diagnostics, logs, backups

**Files:**
- Modify: `src/rust/crates/tracker-server/src/main.rs`

This task replaces all ~10 hardcoded `~/.config/agent-tracker` references in main.rs.

**Step 1: Replace diagnostics config_dir (line 1639)**

The diagnostics function at line 1630 uses `config_dir` for hooks, disk usage, and log checks. Since diagnostics already receives `State(state)`, use `state.paths`:

Replace lines 1638-1639:
```rust
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = format!("{}/.config/agent-tracker", home);
```
With:
```rust
    let config_dir = state.paths.data_dir.to_string_lossy().to_string();
```

Replace line 1762 (hook path — hooks are read-only assets):
```rust
        let hook_path = format!("{}/scripts/agent-event.sh", config_dir);
```
With:
```rust
        let hook_path = state.paths.scripts_dir.join("agent-event.sh").to_string_lossy().to_string();
```

Replace line 1821 (log path in log check):
```rust
            format!("{}/logs/tracker-server.log", config_dir),
```
With:
```rust
            state.paths.log_path.to_string_lossy().to_string(),
```

**Step 2: Replace admin_clear_logs (lines 1888-1892)**

The function `admin_clear_logs` needs access to paths. Change its signature to accept State:

```rust
async fn admin_clear_logs(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let log_paths = [
        "/opt/homebrew/var/log/agent-tracker-server.log".to_string(),
        state.paths.log_path.to_string_lossy().to_string(),
    ];
```

**Step 3: Replace get_logs (lines 1946-1952)**

Change `get_logs` to accept State:

```rust
async fn get_logs(
    Query(params): Query<LogQuery>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let log_paths = [
        "/opt/homebrew/var/log/agent-tracker-server.log".to_string(),
        state.paths.log_path.to_string_lossy().to_string(),
    ];
```

(Remove the `let log_paths = [... format!("{}/.config/agent-tracker/...")]` block at line 1950-1953)

**Step 4: Replace create_backup (lines 2142-2146)**

```rust
async fn create_backup(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let backup_dir = state.paths.backup_dir.to_string_lossy().to_string();
```

**Step 5: Replace list_backups (lines 2159-2161)**

Change to accept State:

```rust
async fn list_backups(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let backup_dir = state.paths.backup_dir.to_string_lossy().to_string();
```

**Step 6: Replace auto-backup background task (lines 2532-2533)**

In the auto-backup `tokio::spawn` block, capture `backup_dir` before the spawn:

```rust
    // Background task: daily auto-backup
    let backup_dir_str = app_state.paths.backup_dir.to_string_lossy().to_string();
    let backup_state = app_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(6 * 3600));
        loop {
            interval.tick().await;
            let backup_dir = &backup_dir_str;
            let _ = std::fs::create_dir_all(backup_dir);
```

(Replace the `let home = ...` and `let backup_dir = format!(...)` lines with the captured variable)

**Step 7: Replace web_dist path resolution (lines 2723-2737)**

Replace the entire web dist path resolution block:

```rust
    // Static file serving for web frontend
    let web_dist = if app_state.paths.web_dist_dir.exists() {
        app_state.paths.web_dist_dir.clone()
    } else {
        // Fallback: try legacy path and ./web/dist
        let legacy = dirs::home_dir()
            .unwrap_or_default()
            .join(".config/agent-tracker/web/dist");
        let cwd = std::path::PathBuf::from("./web/dist");
        [legacy, cwd]
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or(app_state.paths.web_dist_dir.clone())
    };
```

**Step 8: Verify it compiles**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo check -p tracker-server`
Expected: compiles with no errors

**Step 9: Commit**

```bash
git add src/rust/crates/tracker-server/src/main.rs
git commit -m "feat: replace all hardcoded ~/.config/agent-tracker paths with TrackerPaths"
```

---

### Task 6: Update Tauri lib.rs — Set env vars and use app_data_dir

**Files:**
- Modify: `tauri-menubar/src-tauri/src/lib.rs`

**Step 1: Update `start_sidecar()` to pass env vars**

Replace the sidecar command creation at lib.rs lines 65-75. The function needs `app: &tauri::AppHandle` which it already has:

```rust
fn start_sidecar(app: &tauri::AppHandle) {
    const PORT: u16 = 3099;

    if is_port_in_use(PORT) {
        eprintln!("tracker-server already running on port {PORT}, reusing existing instance");
        app.manage(SidecarState {
            child: Mutex::new(None),
            source: "external",
        });
        return;
    }

    // Resolve Tauri standard directories for sidecar env vars
    let resources_dir = app.path().resource_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let data_dir = app.path().app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // Ensure data directory exists
    if !data_dir.is_empty() {
        let _ = std::fs::create_dir_all(&data_dir);
    }

    let cmd = match app.shell().sidecar("tracker-server") {
        Ok(cmd) => cmd
            .env("TRACKER_RESOURCES_DIR", &resources_dir)
            .env("TRACKER_DATA_DIR", &data_dir),
        Err(e) => {
            eprintln!("Failed to create sidecar command: {e}");
            app.manage(SidecarState {
                child: Mutex::new(None),
                source: "offline",
            });
            return;
        }
    };

    eprintln!("Sidecar env: TRACKER_RESOURCES_DIR={resources_dir}");
    eprintln!("Sidecar env: TRACKER_DATA_DIR={data_dir}");

    match cmd.spawn() {
```

(The rest of the `match cmd.spawn()` block stays the same)

**Step 2: Update `read_local_token()` to use app_data_dir**

Replace the function (lines 296-309):

```rust
/// Read auth token — try Application Support first, fall back to legacy path
#[tauri::command]
fn read_local_token(app: tauri::AppHandle) -> Result<String, String> {
    // Try Application Support (Tauri data dir)
    let paths: Vec<std::path::PathBuf> = [
        app.path().app_data_dir().ok().map(|p| p.join("agent-config.json")),
        {
            let home = std::env::var("HOME").ok();
            home.map(|h| std::path::PathBuf::from(h).join(".config/agent-tracker/agent-config.json"))
        },
    ]
    .into_iter()
    .flatten()
    .collect();

    for path in &paths {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(token) = json["auth"]["token"].as_str().filter(|s| !s.is_empty()) {
                    return Ok(token.to_string());
                }
            }
        }
    }

    Err("No token found in any config location".to_string())
}
```

Note: `read_local_token` signature changes from `fn read_local_token()` to `fn read_local_token(app: tauri::AppHandle)`. Tauri's invoke_handler auto-injects the AppHandle argument — no changes needed at the call site.

**Step 3: Update `save_local_token()` to use app_data_dir**

Replace the function (lines 355-369):

```rust
#[tauri::command]
fn save_local_token(app: tauri::AppHandle, token: String) -> Result<(), String> {
    // Save to Application Support (primary)
    let path = app.path().app_data_dir()
        .map(|p| p.join("agent-config.json"))
        .map_err(|e| format!("Cannot resolve app data dir: {}", e))?;

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Read existing or create new
    let mut json: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_else(|| serde_json::json!({"auth": {}}));

    json["auth"]["token"] = serde_json::Value::String(token);
    let output = serde_json::to_string_pretty(&json)
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, output)
        .map_err(|e| format!("Cannot write config: {}", e))?;
    Ok(())
}
```

**Step 4: Update `restart_sidecar()` to pass env vars**

In `restart_sidecar()` (lines 260-262), update the sidecar spawn to also pass env vars:

```rust
    // 3. Spawn new sidecar
    let resources_dir = app.path().resource_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let data_dir = app.path().app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let cmd = app.shell().sidecar("tracker-server")
        .map_err(|e| format!("Failed to create sidecar command: {e}"))?
        .env("TRACKER_RESOURCES_DIR", &resources_dir)
        .env("TRACKER_DATA_DIR", &data_dir);
```

**Step 5: Verify it compiles**

Run: `cd /Volumes/program/project-code/repos/ai-tracker/tauri-menubar/src-tauri && cargo check`
Expected: compiles with no errors

**Step 6: Commit**

```bash
git add tauri-menubar/src-tauri/src/lib.rs
git commit -m "feat: Tauri passes TRACKER_RESOURCES_DIR and TRACKER_DATA_DIR to sidecar"
```

---

### Task 7: Update tauri.conf.json — Bundle web-dist and scripts

**Files:**
- Modify: `tauri-menubar/src-tauri/tauri.conf.json`

**Step 1: Add resources to bundle config**

Add a `resources` key inside the `bundle` object. Tauri v2 supports a map of `target -> source` relative to `tauri.conf.json`:

```json
{
  "productName": "Agent Tracker",
  "version": "0.1.0",
  "identifier": "com.agent-tracker.menubar",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "externalBin": ["bin/tracker-server"],
    "resources": {
      "web-dist": "../../web/dist",
      "scripts": "../../scripts"
    },
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "macOS": {
      "signingIdentity": "HeyGo Local Dev"
    }
  }
}
```

The paths `../../web/dist` and `../../scripts` are relative to `tauri-menubar/src-tauri/tauri.conf.json`, reaching up to the repo root's `web/dist/` and `scripts/` directories.

**Note:** Tauri v2 resource map copies the source directory to `Contents/Resources/<target-name>/`. So `web/dist/*` becomes `Contents/Resources/web-dist/*` and `scripts/*` becomes `Contents/Resources/scripts/*`. This matches the `web-dist` name in `TrackerPaths::resolve()`.

**Step 2: Verify the source directories exist**

Run: `ls /Volumes/program/project-code/repos/ai-tracker/web/dist/index.html && ls /Volumes/program/project-code/repos/ai-tracker/scripts/agent-event.sh`
Expected: both files exist

**Step 3: Commit**

```bash
git add tauri-menubar/src-tauri/tauri.conf.json
git commit -m "feat: bundle web-dist and scripts into Tauri app Resources"
```

---

### Task 8: Build, deploy, and verify

**Files:** None (build/deploy steps only)

**Step 1: Build the frontend**

```bash
cd /Volumes/program/project-code/repos/ai-tracker/web && npm run build
```

Expected: `web/dist/` directory populated with built assets.

**Step 2: Build the Rust server**

```bash
cd /Volumes/program/project-code/repos/ai-tracker/src/rust && cargo build --release -p tracker-server
```

Expected: binary at `target/release/tracker-server`.

**Step 3: Copy sidecar binary into Tauri bin/**

```bash
cp /Volumes/program/project-code/repos/ai-tracker/src/rust/target/release/tracker-server \
   /Volumes/program/project-code/repos/ai-tracker/tauri-menubar/src-tauri/bin/tracker-server-aarch64-apple-darwin
```

Note: Tauri sidecar naming convention requires the target triple suffix.

**Step 4: Build the Tauri app**

```bash
cd /Volumes/program/project-code/repos/ai-tracker/tauri-menubar && npm run tauri build
```

Expected: `.app` bundle at `tauri-menubar/src-tauri/target/release/bundle/macos/Agent Tracker.app`

**Step 5: Verify bundle contents**

```bash
ls -la "/Volumes/program/project-code/repos/ai-tracker/tauri-menubar/src-tauri/target/release/bundle/macos/Agent Tracker.app/Contents/Resources/"
```

Expected: `web-dist/` and `scripts/` directories present alongside Tauri's own resources.

**Step 6: Deploy and verify**

```bash
# Kill existing app
pkill -f "Agent Tracker" || true
sleep 1

# Copy to /Applications (or wherever the user's app lives)
cp -R "tauri-menubar/src-tauri/target/release/bundle/macos/Agent Tracker.app" "/Applications/Agent Tracker.app"

# Codesign
codesign -fs - "/Applications/Agent Tracker.app"

# Launch
open "/Applications/Agent Tracker.app"
sleep 5

# Verify health
curl -s http://localhost:3099/health | python3 -m json.tool
```

Expected: health check returns `"status": "ok"`.

**Step 7: Verify Application Support was created**

```bash
ls -la ~/Library/Application\ Support/com.agent-tracker.menubar/
```

Expected: directory created with `agent-config.json`, `data/tracker.db` (migrated from legacy), `logs/`, `backups/`, `run/`.

**Step 8: Verify web frontend loads**

Open the Tauri dashboard (click tray icon → Dashboard). The web UI should load from the bundled Resources, not from `~/.config/agent-tracker/web/dist`.

Check server logs for the resolved paths:
```bash
curl -s http://localhost:3099/api/logs?limit=20 | python3 -c "import sys,json; [print(e['message']) for e in json.load(sys.stdin)['entries'] if 'TrackerPaths' in e['message'] or 'Serving static' in e['message']]"
```

Expected: logs show `TrackerPaths resolved:` with Resources and Application Support paths, and `Serving static files from` pointing to the bundled web-dist.

**Step 9: Commit (if any final fixes were needed)**

```bash
git add -A && git commit -m "chore: build and verify self-contained Tauri app"
```

---

### Post-implementation notes

After verifying everything works:

1. The legacy `~/.config/agent-tracker/` directory can be cleaned up:
   - Delete `bin/`, `web/`, `scripts/` (now in app bundle)
   - Keep `data/`, `logs/`, `backups/` as archives, or delete after confirming Application Support has the data

2. Future deploys only need `npm run tauri build` — no manual file copying.

3. Standalone server (launchd) still works: without `TRACKER_DATA_DIR` set, it falls back to `~/.config/agent-tracker/`.
