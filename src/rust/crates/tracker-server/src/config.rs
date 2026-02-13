//! Configuration module for agent-tracker
//!
//! Handles reading and writing JSON configuration files.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    /// Registered workspaces (project name -> config)
    #[serde(default)]
    pub workspaces: HashMap<String, WorkspaceConfig>,

    /// Available agents (agent name -> config)
    #[serde(default)]
    pub agents: HashMap<String, AgentDef>,

    /// Layout templates (layout name -> config)
    #[serde(default)]
    pub layouts: HashMap<String, LayoutConfig>,

    /// Default settings
    #[serde(default)]
    pub defaults: Defaults,

    /// Authentication settings
    #[serde(default)]
    pub auth: AuthConfig,
}

/// Workspace (project) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Base path to the project
    pub base_path: PathBuf,

    /// Main branch name (e.g., "main", "master")
    #[serde(default = "default_main_branch")]
    pub main_branch: String,

    /// Directory for worktrees (relative to base_path)
    #[serde(default = "default_worktree_dir")]
    pub worktree_dir: String,

    /// Port base for dev servers (default 3000)
    #[serde(default = "default_port_base")]
    pub port_base: u16,

    /// Enable fullstack mode (separate frontend/backend)
    #[serde(default)]
    pub fullstack_mode: bool,

    /// Frontend service configuration (fullstack mode)
    #[serde(default)]
    pub frontend: Option<ServiceConfig>,

    /// Backend service configuration (fullstack mode)
    #[serde(default)]
    pub backend: Option<ServiceConfig>,

    /// Browser configuration
    #[serde(default)]
    pub browser: BrowserConfig,

    /// Environment file sync configuration
    #[serde(default)]
    pub env_sync: Option<EnvSyncConfig>,
}

fn default_port_base() -> u16 {
    3000
}

/// Service configuration for frontend/backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// Subdirectory (e.g., "frontend", "backend")
    pub dir: String,

    /// Port base for this service
    #[serde(default = "default_port_base")]
    pub port_base: u16,

    /// Command to start the service (supports $PORT variable)
    pub cmd: String,
}

/// Browser automation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    /// Browser type: chrome, safari, arc
    #[serde(default = "default_browser_type")]
    pub browser_type: String,

    /// URL template (supports $PORT, $FRONTEND_PORT, $BACKEND_PORT)
    #[serde(default)]
    pub url: String,

    /// Auto-open browser when starting workspace
    #[serde(default = "default_true")]
    pub auto_open: bool,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            browser_type: default_browser_type(),
            url: String::new(),
            auto_open: true,
        }
    }
}

fn default_browser_type() -> String {
    "chrome".to_string()
}

fn default_true() -> bool {
    true
}

/// Environment file sync configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvSyncConfig {
    /// Backend .env file path (relative to backend dir)
    #[serde(default = "default_env_file")]
    pub backend_env_file: String,

    /// Backend port variable name (e.g., "APP_PORT")
    #[serde(default)]
    pub backend_port_var: String,

    /// Frontend .env file path (relative to frontend dir)
    #[serde(default = "default_env_file")]
    pub frontend_env_file: String,

    /// Frontend API URL variable name (e.g., "VITE_API_URL")
    #[serde(default)]
    pub frontend_api_var: String,

    /// Frontend API URL format (e.g., "http://localhost:$BACKEND_PORT")
    #[serde(default)]
    pub frontend_api_format: String,
}

fn default_env_file() -> String {
    ".env".to_string()
}

fn default_main_branch() -> String {
    "main".to_string()
}

fn default_worktree_dir() -> String {
    ".worktrees".to_string()
}

/// Agent definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    /// Command to run the agent
    pub command: String,

    /// Display color (hex)
    #[serde(default)]
    pub color: Option<String>,

    /// Display icon/emoji
    #[serde(default)]
    pub icon: Option<String>,
}

/// Layout template configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    /// Panes in this layout
    pub panes: Vec<PaneConfig>,
}

/// Pane configuration within a layout
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneConfig {
    /// Command to run (use "{agent}" as placeholder)
    pub cmd: String,

    /// Size (e.g., "30%", "40%")
    #[serde(default)]
    pub size: Option<String>,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Bearer token for API authentication
    #[serde(default)]
    pub token: String,

    /// Allowed CORS origins (empty = mirror request, token is the real gate)
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            allowed_origins: Vec::new(),
        }
    }
}

/// Default settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Defaults {
    /// Default layout name
    #[serde(default = "default_layout")]
    pub layout: String,

    /// Default agent name
    #[serde(default = "default_agent")]
    pub agent: String,
}

fn default_layout() -> String {
    "default".to_string()
}

fn default_agent() -> String {
    "claude".to_string()
}

impl AgentConfig {
    /// Get the default config file path
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("agent-tracker")
            .join("agent-config.json")
    }

    /// Load config from default path
    pub fn load() -> Result<Self> {
        Self::load_from(Self::default_path())
    }

    /// Load config from a specific path
    pub fn load_from(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            // Return default config if file doesn't exist
            return Ok(Self::default_with_examples());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {:?}", path))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config from {:?}", path))
    }

    /// Create default config with example entries
    fn default_with_examples() -> Self {
        let mut config = Self::default();

        // Add default agents
        config.agents.insert(
            "claude".to_string(),
            AgentDef {
                command: "claude --dangerously-skip-permissions".to_string(),
                color: Some("#f5a623".to_string()),
                icon: Some("🤖".to_string()),
            },
        );
        config.agents.insert(
            "opencode".to_string(),
            AgentDef {
                command: "opencode".to_string(),
                color: Some("#7ed321".to_string()),
                icon: Some("💻".to_string()),
            },
        );

        // Add default layout
        config.layouts.insert(
            "default".to_string(),
            LayoutConfig {
                panes: vec![
                    PaneConfig {
                        cmd: "yazi".to_string(),
                        size: Some("30%".to_string()),
                    },
                    PaneConfig {
                        cmd: "lazygit".to_string(),
                        size: Some("30%".to_string()),
                    },
                    PaneConfig {
                        cmd: "{agent}".to_string(),
                        size: Some("40%".to_string()),
                    },
                ],
            },
        );

        // Add focus layout
        config.layouts.insert(
            "focus".to_string(),
            LayoutConfig {
                panes: vec![PaneConfig {
                    cmd: "{agent}".to_string(),
                    size: Some("100%".to_string()),
                }],
            },
        );

        config
    }

    /// Save config to default path
    pub fn save(&self) -> Result<()> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir {:?}", parent))?;
        }
        let content = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize config")?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write config to {:?}", path))?;
        Ok(())
    }

    /// Get workspace config by name
    pub fn get_workspace(&self, name: &str) -> Option<&WorkspaceConfig> {
        self.workspaces.get(name)
    }

    /// Get agent definition by name
    pub fn get_agent(&self, name: &str) -> Option<&AgentDef> {
        self.agents.get(name)
    }

    /// Get layout config by name
    pub fn get_layout(&self, name: &str) -> Option<&LayoutConfig> {
        self.layouts.get(name)
    }
}
