//! Pane registry: maps tmux pane IDs to Claude sessions and transcript paths.
//!
//! Populated from hook events carrying the `X-Tmux-Pane` header (agent-hook.sh
//! forwards `$TMUX_PANE`, which every Claude process inherits from its pane's shell).
//! This gives exact per-pane attribution: multiple Claude instances running in the
//! same directory are disambiguated by which pane reported the event — directory
//! heuristics cannot tell them apart.
//!
//! Design follows herdr's claude integration: the hook reports pane_id +
//! agent_session_id + transcript_path; the server owns the mapping.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::process::Command;
use tracing::debug;

use crate::agent::{TMUX_BIN, TMUX_SOCKET};
use crate::db::PaneBindingRow;

/// How long a cached tmux location (session/window) stays fresh before re-resolving.
/// Windows are rarely renamed or moved; 30s bounds the staleness window.
const LOCATION_TTL: Duration = Duration::from_secs(30);

/// A pane's current Claude session binding
#[derive(Debug, Clone)]
pub struct PaneBinding {
    pub pane_id: String,
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
    pub claude_session_id: String,
    /// Absolute path to the Claude Code transcript JSONL (from hook input)
    pub transcript_path: Option<String>,
    /// When the tmux location (session/window) was last resolved
    resolved_at: Instant,
    /// When any hook event last touched this binding
    updated_at: Instant,
}

/// Registry of pane → Claude session bindings, keyed by tmux pane ID (e.g. "%15")
#[derive(Default)]
pub struct PaneRegistry {
    bindings: Mutex<HashMap<String, PaneBinding>>,
}

impl PaneRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a hook event for a pane. Resolves the pane's tmux location when the
    /// cached location is missing or stale. Returns the fresh binding plus whether
    /// it materially changed (new pane, new Claude session, new transcript, or the
    /// pane moved) — the caller persists only on change. None if the pane no longer
    /// exists in tmux.
    pub async fn update(
        &self,
        pane_id: &str,
        claude_session_id: &str,
        transcript_path: Option<&str>,
    ) -> Option<(PaneBinding, bool)> {
        // Reuse the cached tmux location if still fresh (avoids a tmux call per hook event)
        let cached_location = {
            let map = self.bindings.lock().unwrap();
            map.get(pane_id).and_then(|b| {
                (b.resolved_at.elapsed() < LOCATION_TTL).then(|| {
                    (
                        b.session_name.clone(),
                        b.window_id.clone(),
                        b.window_name.clone(),
                        b.resolved_at,
                    )
                })
            })
        };

        let (session_name, window_id, window_name, resolved_at) = match cached_location {
            Some(loc) => loc,
            None => {
                let (s, wid, wname) = resolve_pane_location(pane_id).await?;
                (s, wid, wname, Instant::now())
            }
        };

        let mut map = self.bindings.lock().unwrap();
        // Keep the previous transcript_path when this event doesn't carry one
        // (same Claude session only — a new session must not inherit an old path)
        let transcript_path = match transcript_path {
            Some(tp) if !tp.is_empty() => Some(tp.to_string()),
            _ => map
                .get(pane_id)
                .filter(|prev| prev.claude_session_id == claude_session_id)
                .and_then(|prev| prev.transcript_path.clone()),
        };
        let binding = PaneBinding {
            pane_id: pane_id.to_string(),
            session_name,
            window_id,
            window_name,
            claude_session_id: claude_session_id.to_string(),
            transcript_path,
            resolved_at,
            updated_at: Instant::now(),
        };
        let changed = match map.get(pane_id) {
            None => true,
            Some(prev) => {
                prev.claude_session_id != binding.claude_session_id
                    || prev.transcript_path != binding.transcript_path
                    || prev.session_name != binding.session_name
                    || prev.window_id != binding.window_id
            }
        };
        map.insert(pane_id.to_string(), binding.clone());
        Some((binding, changed))
    }

    /// Restore persisted bindings at startup. Each pane is validated against tmux:
    /// live panes are re-inserted with a fresh location, dead panes are returned
    /// for deletion from the store.
    pub async fn restore(&self, rows: Vec<PaneBindingRow>) -> (usize, Vec<String>) {
        let mut restored = 0usize;
        let mut dead: Vec<String> = Vec::new();
        for row in rows {
            match resolve_pane_location(&row.pane_id).await {
                Some((session_name, window_id, window_name)) => {
                    let binding = PaneBinding {
                        pane_id: row.pane_id.clone(),
                        session_name,
                        window_id,
                        window_name,
                        claude_session_id: row.claude_session_id,
                        transcript_path: row.transcript_path,
                        resolved_at: Instant::now(),
                        updated_at: Instant::now(),
                    };
                    self.bindings.lock().unwrap().insert(row.pane_id, binding);
                    restored += 1;
                }
                None => dead.push(row.pane_id),
            }
        }
        (restored, dead)
    }

    /// Find the binding for a specific pane ID
    pub fn find_by_pane(&self, pane_id: &str) -> Option<PaneBinding> {
        self.bindings.lock().unwrap().get(pane_id).cloned()
    }

    /// Find the most recently active binding in a window.
    /// `window` matches either the window ID ("@4") or the window name ("master").
    pub fn find_by_window(&self, session_name: &str, window: &str) -> Option<PaneBinding> {
        let map = self.bindings.lock().unwrap();
        map.values()
            .filter(|b| {
                b.session_name == session_name
                    && (b.window_id == window || b.window_name == window)
            })
            .max_by_key(|b| b.updated_at)
            .cloned()
    }

    /// Drop the binding for a pane (e.g. on SessionEnd when the pane is gone)
    #[allow(dead_code)]
    pub fn remove(&self, pane_id: &str) {
        self.bindings.lock().unwrap().remove(pane_id);
    }

    /// Convert a binding to its persisted form
    pub fn to_row(binding: &PaneBinding) -> PaneBindingRow {
        PaneBindingRow {
            pane_id: binding.pane_id.clone(),
            session_name: binding.session_name.clone(),
            window_id: binding.window_id.clone(),
            window_name: binding.window_name.clone(),
            claude_session_id: binding.claude_session_id.clone(),
            transcript_path: binding.transcript_path.clone(),
        }
    }

    /// Insert a binding directly (tests only — bypasses tmux resolution)
    #[cfg(test)]
    fn insert_for_test(&self, binding: PaneBinding) {
        self.bindings
            .lock()
            .unwrap()
            .insert(binding.pane_id.clone(), binding);
    }
}

/// Resolve a tmux pane ID to its (session_name, window_id, window_name)
async fn resolve_pane_location(pane_id: &str) -> Option<(String, String, String)> {
    if !pane_id.starts_with('%') {
        return None;
    }
    let output = Command::new(TMUX_BIN.as_str())
        .args([
            "-S",
            TMUX_SOCKET.as_str(),
            "display-message",
            "-p",
            "-t",
            pane_id,
            "-F",
            "#{session_name}\t#{window_id}\t#{window_name}",
        ])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        debug!(
            "pane_registry: cannot resolve pane {} (gone?): {}",
            pane_id,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut parts = line.splitn(3, '\t');
    let session_name = parts.next()?.to_string();
    let window_id = parts.next()?.to_string();
    let window_name = parts.next()?.to_string();
    if session_name.is_empty() || window_id.is_empty() {
        return None;
    }
    Some((session_name, window_id, window_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(pane: &str, session: &str, window_id: &str, window_name: &str, csid: &str) -> PaneBinding {
        PaneBinding {
            pane_id: pane.to_string(),
            session_name: session.to_string(),
            window_id: window_id.to_string(),
            window_name: window_name.to_string(),
            claude_session_id: csid.to_string(),
            transcript_path: Some(format!("/tmp/{}.jsonl", csid)),
            resolved_at: Instant::now(),
            updated_at: Instant::now(),
        }
    }

    #[test]
    fn find_by_window_matches_id_and_name() {
        let reg = PaneRegistry::new();
        reg.insert_for_test(binding("%2", "1-mediahub", "@1", "main", "aaa"));
        reg.insert_for_test(binding("%11", "1-mediahub", "@4", "master", "bbb"));

        let by_name = reg.find_by_window("1-mediahub", "master").unwrap();
        assert_eq!(by_name.claude_session_id, "bbb");

        let by_id = reg.find_by_window("1-mediahub", "@1").unwrap();
        assert_eq!(by_id.claude_session_id, "aaa");

        assert!(reg.find_by_window("other-session", "master").is_none());
    }

    #[test]
    fn find_by_window_disambiguates_same_directory_windows() {
        // Three windows, same repo dir — the exact scenario directory heuristics fail on
        let reg = PaneRegistry::new();
        reg.insert_for_test(binding("%2", "1-mediahub", "@1", "main", "session-a"));
        reg.insert_for_test(binding("%8", "1-mediahub", "@3", "fix-coverage", "session-b"));
        reg.insert_for_test(binding("%11", "1-mediahub", "@4", "master", "session-c"));

        assert_eq!(reg.find_by_window("1-mediahub", "main").unwrap().claude_session_id, "session-a");
        assert_eq!(reg.find_by_window("1-mediahub", "fix-coverage").unwrap().claude_session_id, "session-b");
        assert_eq!(reg.find_by_window("1-mediahub", "master").unwrap().claude_session_id, "session-c");
    }

    #[tokio::test]
    async fn update_reports_material_change_only() {
        let reg = PaneRegistry::new();
        // Seed with a fresh location so update() uses the cache (no tmux call)
        reg.insert_for_test(binding("%77", "s", "@9", "w", "session-a"));

        // Same session, same transcript → unchanged
        let (_, changed) = reg.update("%77", "session-a", Some("/tmp/session-a.jsonl")).await.unwrap();
        assert!(!changed);

        // New Claude session in the same pane → changed
        let (b, changed) = reg.update("%77", "session-b", Some("/tmp/session-b.jsonl")).await.unwrap();
        assert!(changed);
        assert_eq!(b.claude_session_id, "session-b");

        // Event without transcript_path keeps the previous one, still unchanged
        let (b, changed) = reg.update("%77", "session-b", None).await.unwrap();
        assert!(!changed);
        assert_eq!(b.transcript_path.as_deref(), Some("/tmp/session-b.jsonl"));
    }

    #[test]
    fn find_by_pane_returns_exact_binding() {
        let reg = PaneRegistry::new();
        reg.insert_for_test(binding("%8", "1-mediahub", "@3", "fix-coverage", "session-b"));
        assert_eq!(reg.find_by_pane("%8").unwrap().transcript_path.as_deref(), Some("/tmp/session-b.jsonl"));
        assert!(reg.find_by_pane("%99").is_none());
    }
}
