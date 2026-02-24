# Self-Contained Tauri App — Design

**Date**: 2026-02-23
**Status**: Approved

## Problem

`~/.config/agent-tracker/` holds both read-only assets (web/dist, scripts) and writable data (database, logs, config). The Tauri app bundle only contains the binary. After installing the app, web frontend and scripts must be manually deployed. This makes updates fragile and the app not self-contained.

## Constraint

macOS `.app` bundles are code-signed → read-only. Writable data (database, logs, backups, config) cannot live inside the bundle.

## Architecture

```
Agent Tracker.app/Contents/
├── MacOS/
│   ├── agent-tracker-menubar        (Tauri host)
│   └── tracker-server               (sidecar binary)
├── Resources/
│   ├── web-dist/                    (bundled frontend)
│   │   ├── index.html
│   │   ├── assets/
│   │   └── ...
│   ├── scripts/                     (bundled scripts)
│   │   ├── agent-event.sh
│   │   ├── notify.py
│   │   └── ...
│   └── agent-config.default.json    (template config)
└── Info.plist

~/Library/Application Support/com.agent-tracker.menubar/
├── agent-config.json        (user config: auth token, agents, layouts)
├── data/
│   └── tracker.db           (SQLite ~9MB)
├── logs/
│   └── tracker-server.log
├── backups/                 (database backups)
└── run/                     (runtime state: latest_notified.txt, etc.)
```

## Path Resolution

Server determines paths via a priority chain:

### Read-only assets (web/dist, scripts)
1. `TRACKER_RESOURCES_DIR` env var (set by Tauri when spawning sidecar)
2. Relative to executable: `../../Resources/` (works inside .app bundle)
3. Fallback: `~/.config/agent-tracker/` (standalone/launchd mode)

### Writable data (db, logs, backups, config)
1. `TRACKER_DATA_DIR` env var (set by Tauri)
2. Fallback: `~/.config/agent-tracker/` (standalone/launchd mode)

This means:
- **Tauri sidecar**: uses Application Support for data, Resources for assets
- **Standalone binary**: uses `~/.config/agent-tracker/` for everything (backward compat)

## Tauri Sidecar Launch

```rust
// lib.rs — before spawning sidecar
let resources_dir = app.path().resource_dir()?;
let data_dir = app.path().app_data_dir()?; // ~/Library/Application Support/com.agent-tracker.menubar/

let cmd = app.shell().sidecar("tracker-server")?
    .env("TRACKER_RESOURCES_DIR", resources_dir.to_str().unwrap())
    .env("TRACKER_DATA_DIR", data_dir.to_str().unwrap());
```

## First Launch Migration

When `TRACKER_DATA_DIR` is set but empty, and `~/.config/agent-tracker/data/tracker.db` exists:
1. Create Application Support directory structure
2. Copy `data/tracker.db` → Application Support
3. Copy `agent-config.json` → Application Support
4. Copy `run/` contents → Application Support
5. Log migration success

After migration, `~/.config/agent-tracker/` can be manually cleaned up. The server does NOT delete it automatically.

## Changes

### New: `src/rust/crates/tracker-server/src/paths.rs`
- `TrackerPaths` struct with fields: `resources_dir`, `data_dir`, `config_path`, `db_path`, `log_path`, `backup_dir`, `web_dist_dir`, `scripts_dir`, `run_dir`
- `TrackerPaths::resolve()` — builds paths from env vars with fallback chain
- `TrackerPaths::ensure_dirs()` — creates data directories if missing
- `TrackerPaths::migrate_if_needed()` — first-launch migration from old location

### Modified: `src/rust/crates/tracker-server/src/main.rs`
- Initialize `TrackerPaths` early in `main()`
- Store in `AppState` alongside db pool
- Replace all ~8 hardcoded `~/.config/agent-tracker` references
- Web dist serving uses `paths.web_dist_dir`
- Log file path from `paths.log_path`
- Backup dir from `paths.backup_dir`

### Modified: `src/rust/crates/tracker-server/src/config.rs`
- `AgentConfig::default_path()` checks `TRACKER_DATA_DIR` env first
- Fallback to `~/.config/agent-tracker/agent-config.json`

### Modified: `tauri-menubar/src-tauri/src/lib.rs`
- Set `TRACKER_RESOURCES_DIR` and `TRACKER_DATA_DIR` env vars before spawning sidecar
- `read_local_token()` reads from Application Support path
- `save_local_token()` writes to Application Support path
- On first launch: create Application Support dirs, copy default config from Resources

### Modified: `tauri-menubar/src-tauri/tauri.conf.json`
- Add `resources` array to bundle web/dist and scripts into .app

### Build process
- `npm run build` in `web/` → outputs to `web/dist/`
- Tauri build reads from configured resource paths, bundles into `.app/Contents/Resources/`
- No more manual `cp -r web/dist ~/.config/agent-tracker/web/dist`

## Files in `~/.config/agent-tracker/` After Migration

| File/Dir | Status | Notes |
|----------|--------|-------|
| `bin/` | Can delete | Binary now in .app bundle |
| `web/` | Can delete | Frontend now in .app Resources |
| `scripts/` | Can delete | Scripts now in .app Resources |
| `data/` | Keep or delete | Migrated to Application Support |
| `logs/` | Keep or delete | New logs go to Application Support |
| `backups/` | Keep or delete | New backups go to Application Support |
| `agent-config.json` | Keep or delete | Migrated to Application Support |
| `run/` | Keep or delete | Migrated to Application Support |
