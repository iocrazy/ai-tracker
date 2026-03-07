//! Port management module
//!
//! Handles dynamic port allocation, port availability checks, and process cleanup.

use std::net::TcpListener;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

use crate::agent::TMUX_BIN;

/// Port range configuration
pub struct PortRangeConfig {
    /// Base port for frontend (default: 3001, so session1 starts at 3011)
    pub frontend_base: u16,
    /// Base port for backend (default: 8001, so session1 starts at 8011)
    pub backend_base: u16,
    /// Ports per session (default: 10)
    pub ports_per_session: u16,
}

impl Default for PortRangeConfig {
    fn default() -> Self {
        Self {
            frontend_base: 3001,
            backend_base: 8001,
            ports_per_session: 10,
        }
    }
}

/// Port management operations
pub struct PortManager;

impl PortManager {
    /// Calculate port based on base and window index (legacy)
    ///
    /// Port = PORT_BASE + window_index
    pub fn calculate_port(base: u16, window_index: u32) -> u16 {
        base.saturating_add(window_index as u16)
    }

    /// Calculate port based on session index and window index
    ///
    /// Port ranges by session:
    /// - session1: 3011-3020 (frontend), 8011-8020 (backend)
    /// - session2: 3021-3030 (frontend), 8021-8030 (backend)
    /// - sessionN: 30N1-30N0 (frontend), 80N1-80N0 (backend)
    ///
    /// Formula: base + (session_index * ports_per_session) + window_index
    pub fn calculate_session_port(
        base: u16,
        session_index: u32,
        window_index: u32,
        ports_per_session: u16,
    ) -> u16 {
        let session_offset = (session_index as u16).saturating_mul(ports_per_session);
        base.saturating_add(session_offset)
            .saturating_add(window_index as u16)
    }

    /// Extract session index from session name
    ///
    /// Examples:
    /// - "session1" -> 1
    /// - "session2" -> 2
    /// - "myproject3" -> 3
    /// - "dev" -> 1 (default)
    pub fn extract_session_index(session_name: &str) -> u32 {
        // Try to find trailing number in session name
        let digits: String = session_name
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();

        digits.parse().unwrap_or(1)
    }

    /// Allocate ports for a workspace using session-based ranges
    ///
    /// Returns (frontend_port, backend_port) based on session and window indices
    pub fn allocate_session_ports(
        session_name: &str,
        window_index: u32,
        config: &PortRangeConfig,
    ) -> (u16, u16) {
        let session_index = Self::extract_session_index(session_name);
        let frontend_port = Self::calculate_session_port(
            config.frontend_base,
            session_index,
            window_index,
            config.ports_per_session,
        );
        let backend_port = Self::calculate_session_port(
            config.backend_base,
            session_index,
            window_index,
            config.ports_per_session,
        );
        (frontend_port, backend_port)
    }

    /// Get port range for a session
    ///
    /// Returns (start_port, end_port) for the session's frontend ports
    pub fn get_session_port_range(session_name: &str, config: &PortRangeConfig) -> (u16, u16) {
        let session_index = Self::extract_session_index(session_name);
        let start = Self::calculate_session_port(
            config.frontend_base,
            session_index,
            0,
            config.ports_per_session,
        );
        let end = start.saturating_add(config.ports_per_session - 1);
        (start, end)
    }

    /// Check if a port is currently in use
    pub fn is_port_in_use(port: u16) -> bool {
        TcpListener::bind(("127.0.0.1", port)).is_err()
    }

    /// Find the next available port starting from base
    pub fn find_available_port(base: u16, max_attempts: u16) -> Option<u16> {
        for offset in 0..max_attempts {
            let port = base.saturating_add(offset);
            if !Self::is_port_in_use(port) {
                return Some(port);
            }
        }
        None
    }

    /// Kill process(es) using a specific port
    ///
    /// Uses `lsof` on macOS/Linux to find PIDs and kills them
    pub async fn kill_port_process(port: u16) -> Result<bool> {
        // Find PIDs using the port
        let output = Command::new("lsof")
            .args(["-ti", &format!(":{}", port)])
            .output()
            .await
            .context("Failed to execute lsof")?;

        if !output.status.success() || output.stdout.is_empty() {
            // No process found on this port
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let pids: Vec<&str> = stdout.trim().lines().collect();

        if pids.is_empty() {
            return Ok(false);
        }

        // Kill each PID
        let mut killed = false;
        for pid in pids {
            let pid = pid.trim();
            if pid.is_empty() {
                continue;
            }

            let kill_output = Command::new("kill")
                .args(["-9", pid])
                .output()
                .await;

            if let Ok(o) = kill_output {
                if o.status.success() {
                    killed = true;
                    tracing::info!("Killed process {} on port {}", pid, port);
                }
            }
        }

        Ok(killed)
    }

    /// Kill multiple port processes
    pub async fn kill_ports(ports: &[u16]) -> Result<u32> {
        let mut killed_count = 0;
        for &port in ports {
            if Self::kill_port_process(port).await? {
                killed_count += 1;
            }
        }
        Ok(killed_count)
    }

    /// Get the tmux window index for a given session and window
    pub async fn get_window_index(session: &str, window: &str) -> Result<u32> {
        let target = format!("{}:{}", session, window);
        let output = Command::new(TMUX_BIN.as_str())
            .args(["display-message", "-t", &target, "-p", "#{window_index}"])
            .output()
            .await
            .context("Failed to get tmux window index")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("tmux display-message failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let index: u32 = stdout
            .trim()
            .parse()
            .context("Failed to parse window index")?;

        Ok(index)
    }

    /// Allocate ports for a fullstack workspace
    ///
    /// Returns (frontend_port, backend_port)
    pub fn allocate_fullstack_ports(
        frontend_base: u16,
        backend_base: u16,
        window_index: u32,
    ) -> (u16, u16) {
        let frontend_port = Self::calculate_port(frontend_base, window_index);
        let backend_port = Self::calculate_port(backend_base, window_index);
        (frontend_port, backend_port)
    }

    /// Allocate a single port for a workspace
    pub fn allocate_port(base: u16, window_index: u32) -> u16 {
        Self::calculate_port(base, window_index)
    }

    /// Check if port is available, if not try to find next available
    pub async fn ensure_port_available(preferred: u16, kill_if_in_use: bool) -> Result<u16> {
        if !Self::is_port_in_use(preferred) {
            return Ok(preferred);
        }

        if kill_if_in_use {
            Self::kill_port_process(preferred).await?;
            // Wait a bit for the port to be released
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            if !Self::is_port_in_use(preferred) {
                return Ok(preferred);
            }
        }

        // Try to find an alternative port
        if let Some(port) = Self::find_available_port(preferred.saturating_add(1), 100) {
            return Ok(port);
        }

        bail!("Could not find available port starting from {}", preferred);
    }
}

/// Port allocation result for a workspace
#[derive(Debug, Clone)]
pub struct AllocatedPorts {
    /// Single service port (non-fullstack mode)
    pub port: Option<u16>,
    /// Frontend port (fullstack mode)
    pub frontend_port: Option<u16>,
    /// Backend port (fullstack mode)
    pub backend_port: Option<u16>,
}

impl AllocatedPorts {
    /// Create for single service mode
    pub fn single(port: u16) -> Self {
        Self {
            port: Some(port),
            frontend_port: None,
            backend_port: None,
        }
    }

    /// Create for fullstack mode
    pub fn fullstack(frontend: u16, backend: u16) -> Self {
        Self {
            port: None,
            frontend_port: Some(frontend),
            backend_port: Some(backend),
        }
    }

    /// Get all allocated ports as a vector
    pub fn all_ports(&self) -> Vec<u16> {
        let mut ports = Vec::new();
        if let Some(p) = self.port {
            ports.push(p);
        }
        if let Some(p) = self.frontend_port {
            ports.push(p);
        }
        if let Some(p) = self.backend_port {
            ports.push(p);
        }
        ports
    }

    /// Get the primary port (for browser URL)
    pub fn primary_port(&self) -> Option<u16> {
        // Prefer frontend port in fullstack mode
        self.frontend_port.or(self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_port() {
        assert_eq!(PortManager::calculate_port(3000, 0), 3000);
        assert_eq!(PortManager::calculate_port(3000, 1), 3001);
        assert_eq!(PortManager::calculate_port(3000, 10), 3010);
    }

    #[test]
    fn test_extract_session_index() {
        assert_eq!(PortManager::extract_session_index("session1"), 1);
        assert_eq!(PortManager::extract_session_index("session2"), 2);
        assert_eq!(PortManager::extract_session_index("session10"), 10);
        assert_eq!(PortManager::extract_session_index("myproject3"), 3);
        assert_eq!(PortManager::extract_session_index("dev"), 1); // default
        assert_eq!(PortManager::extract_session_index(""), 1); // default
    }

    #[test]
    fn test_calculate_session_port() {
        let config = PortRangeConfig::default();

        // session1 frontend: 3011-3020
        assert_eq!(
            PortManager::calculate_session_port(config.frontend_base, 1, 0, config.ports_per_session),
            3011
        );
        assert_eq!(
            PortManager::calculate_session_port(config.frontend_base, 1, 9, config.ports_per_session),
            3020
        );

        // session2 frontend: 3021-3030
        assert_eq!(
            PortManager::calculate_session_port(config.frontend_base, 2, 0, config.ports_per_session),
            3021
        );
        assert_eq!(
            PortManager::calculate_session_port(config.frontend_base, 2, 9, config.ports_per_session),
            3030
        );

        // session1 backend: 8011-8020
        assert_eq!(
            PortManager::calculate_session_port(config.backend_base, 1, 0, config.ports_per_session),
            8011
        );
    }

    #[test]
    fn test_allocate_session_ports() {
        let config = PortRangeConfig::default();

        // session1, window 0
        let (fe, be) = PortManager::allocate_session_ports("session1", 0, &config);
        assert_eq!(fe, 3011);
        assert_eq!(be, 8011);

        // session2, window 5
        let (fe, be) = PortManager::allocate_session_ports("session2", 5, &config);
        assert_eq!(fe, 3026);
        assert_eq!(be, 8026);

        // project3, window 0
        let (fe, be) = PortManager::allocate_session_ports("project3", 0, &config);
        assert_eq!(fe, 3031);
        assert_eq!(be, 8031);
    }

    #[test]
    fn test_get_session_port_range() {
        let config = PortRangeConfig::default();

        let (start, end) = PortManager::get_session_port_range("session1", &config);
        assert_eq!(start, 3011);
        assert_eq!(end, 3020);

        let (start, end) = PortManager::get_session_port_range("session2", &config);
        assert_eq!(start, 3021);
        assert_eq!(end, 3030);
    }

    #[test]
    fn test_allocated_ports_single() {
        let ports = AllocatedPorts::single(3000);
        assert_eq!(ports.port, Some(3000));
        assert!(ports.frontend_port.is_none());
        assert_eq!(ports.all_ports(), vec![3000]);
        assert_eq!(ports.primary_port(), Some(3000));
    }

    #[test]
    fn test_allocated_ports_fullstack() {
        let ports = AllocatedPorts::fullstack(3000, 8000);
        assert!(ports.port.is_none());
        assert_eq!(ports.frontend_port, Some(3000));
        assert_eq!(ports.backend_port, Some(8000));
        assert_eq!(ports.all_ports(), vec![3000, 8000]);
        assert_eq!(ports.primary_port(), Some(3000));
    }
}
