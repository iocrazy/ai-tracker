//! HTTP client module for tracker-tui
//!
//! Connects to tracker-server via HTTP/WebSocket.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracker_core::{Goal, Note, Task};

/// Server URL
const SERVER_URL: &str = "http://127.0.0.1:3099";

/// Full state response from server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateResponse {
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub archived_tasks: Vec<Task>,
    pub notes: Vec<Note>,
    #[serde(default)]
    pub archived_notes: Vec<Note>,
    pub goals: Vec<Goal>,
    pub message: String,
}

/// Command request
#[derive(Debug, Serialize)]
pub struct CommandRequest {
    pub command: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub window_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub window: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pane: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub goal_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub transcript_path: String,
}

impl Default for CommandRequest {
    fn default() -> Self {
        Self {
            command: String::new(),
            session_id: String::new(),
            session: String::new(),
            window_id: String::new(),
            window: String::new(),
            pane: String::new(),
            summary: String::new(),
            note_id: String::new(),
            goal_id: String::new(),
            scope: String::new(),
            transcript_path: String::new(),
        }
    }
}

/// Command response
#[derive(Debug, Deserialize)]
pub struct CommandResponse {
    pub success: bool,
    pub message: String,
}

/// HTTP client for tracker-server
pub struct TrackerClient {
    client: reqwest::Client,
    base_url: String,
}

impl TrackerClient {
    /// Create a new client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: SERVER_URL.to_string(),
        }
    }

    /// Get the WebSocket URL
    pub fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:3099/ws")
    }

    /// Get current state
    pub async fn get_state(&self) -> Result<StateResponse> {
        let url = format!("{}/api/state", self.base_url);
        let response = self.client.get(&url).send().await?;
        let state = response.json::<StateResponse>().await?;
        Ok(state)
    }

    /// Send a command
    pub async fn send_command(&self, req: CommandRequest) -> Result<CommandResponse> {
        let url = format!("{}/api/command", self.base_url);
        let response = self.client.post(&url).json(&req).send().await?;
        let result = response.json::<CommandResponse>().await?;
        Ok(result)
    }

    /// Health check
    pub async fn health(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

impl Default for TrackerClient {
    fn default() -> Self {
        Self::new()
    }
}
