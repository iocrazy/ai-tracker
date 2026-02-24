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

    /// Legacy config directory path (~/.config/agent-tracker).
    /// Public so config.rs, db.rs, main.rs can share this single source of truth.
    pub fn legacy_config_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("agent-tracker")
    }
}
