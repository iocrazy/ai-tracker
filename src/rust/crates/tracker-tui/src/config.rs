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

    /// Save config to default path
    pub fn save(&self) -> Result<()> {
        self.save_to(Self::default_path())
    }

    /// Save config to a specific path
    pub fn save_to(&self, path: PathBuf) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {:?}", parent))?;
        }

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize config")?;

        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write config to {:?}", path))
    }

    /// Create default config with example entries
    fn default_with_examples() -> Self {
        let mut config = Self::default();

        // Add default agents
        config.agents.insert(
            "claude".to_string(),
            AgentDef {
                command: "claude".to_string(),
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

    /// List all workspace names
    pub fn list_workspaces(&self) -> Vec<&str> {
        self.workspaces.keys().map(|s| s.as_str()).collect()
    }

    /// List all agent names
    pub fn list_agents(&self) -> Vec<&str> {
        self.agents.keys().map(|s| s.as_str()).collect()
    }

    /// List all layout names
    pub fn list_layouts(&self) -> Vec<&str> {
        self.layouts.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AgentConfig::default_with_examples();
        assert!(config.agents.contains_key("claude"));
        assert!(config.layouts.contains_key("default"));
    }

    #[test]
    fn test_serialize_deserialize() {
        let config = AgentConfig::default_with_examples();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.agents.len(), config.agents.len());
    }
}
