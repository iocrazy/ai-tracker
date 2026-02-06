//! Browser automation module
//!
//! Handles browser opening and tab switching using macOS AppleScript.

use anyhow::{Context, Result};
use tokio::process::Command;

/// Browser automation operations
pub struct BrowserAutomation;

impl BrowserAutomation {
    /// Open a URL in the specified browser
    ///
    /// Supported browsers: chrome, safari, arc
    pub async fn open_url(browser: &str, url: &str) -> Result<()> {
        let script = Self::get_open_url_script(browser, url);

        let output = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await
            .context("Failed to execute osascript")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("Browser open failed (may be expected): {}", stderr);
            // Don't fail - browser might not be running
        }

        Ok(())
    }

    /// Switch to a tab containing the specified port in the URL
    ///
    /// Searches through all browser windows/tabs for a URL containing "localhost:{port}"
    pub async fn switch_to_tab(browser: &str, port: u16) -> Result<bool> {
        let script = Self::get_switch_tab_script(browser, port);

        let output = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await
            .context("Failed to execute osascript for tab switch")?;

        // osascript returns 0 even if no tab found, check output
        let stdout = String::from_utf8_lossy(&output.stdout);
        let found = stdout.trim() == "found" || output.status.success();

        Ok(found)
    }

    /// Open URL with delay (useful for waiting for dev server to start)
    pub async fn open_url_delayed(browser: &str, url: &str, delay_secs: u64) -> Result<()> {
        tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
        Self::open_url(browser, url).await
    }

    /// Get AppleScript for opening a URL
    fn get_open_url_script(browser: &str, url: &str) -> String {
        match browser.to_lowercase().as_str() {
            "chrome" | "google chrome" => {
                format!(
                    r#"tell application "Google Chrome" to open location "{}""#,
                    url
                )
            }
            "safari" => {
                format!(
                    r#"tell application "Safari" to open location "{}""#,
                    url
                )
            }
            "arc" => {
                format!(
                    r#"tell application "Arc" to open location "{}""#,
                    url
                )
            }
            _ => {
                // Fallback: use system default
                format!(r#"open "{}""#, url)
            }
        }
    }

    /// Get AppleScript for switching to a tab with specific port
    fn get_switch_tab_script(browser: &str, port: u16) -> String {
        match browser.to_lowercase().as_str() {
            "chrome" | "google chrome" => {
                format!(
                    r#"
                    tell application "Google Chrome"
                        set targetURL to "localhost:{}"
                        repeat with w in windows
                            set tabIndex to 0
                            repeat with t in tabs of w
                                set tabIndex to tabIndex + 1
                                if URL of t contains targetURL then
                                    set active tab index of w to tabIndex
                                    set index of w to 1
                                    return "found"
                                end if
                            end repeat
                        end repeat
                        return "not found"
                    end tell
                    "#,
                    port
                )
            }
            "safari" => {
                format!(
                    r#"
                    tell application "Safari"
                        set targetURL to "localhost:{}"
                        repeat with w in windows
                            set tabIndex to 0
                            repeat with t in tabs of w
                                set tabIndex to tabIndex + 1
                                if URL of t contains targetURL then
                                    set current tab of w to t
                                    set index of w to 1
                                    return "found"
                                end if
                            end repeat
                        end repeat
                        return "not found"
                    end tell
                    "#,
                    port
                )
            }
            "arc" => {
                format!(
                    r#"
                    tell application "Arc"
                        set targetURL to "localhost:{}"
                        repeat with w in windows
                            repeat with t in tabs of w
                                if URL of t contains targetURL then
                                    tell t to select
                                    return "found"
                                end if
                            end repeat
                        end repeat
                        return "not found"
                    end tell
                    "#,
                    port
                )
            }
            _ => {
                // Cannot switch tabs for unknown browsers
                "return \"not supported\"".to_string()
            }
        }
    }

    /// Refresh the current tab in a browser
    pub async fn refresh_tab(browser: &str) -> Result<()> {
        let script = match browser.to_lowercase().as_str() {
            "chrome" | "google chrome" => {
                r#"tell application "Google Chrome" to reload active tab of front window"#
            }
            "safari" => {
                r#"tell application "Safari" to do JavaScript "location.reload()" in front document"#
            }
            "arc" => {
                r#"tell application "Arc" to reload active tab of front window"#
            }
            _ => return Ok(()),
        };

        let _ = Command::new("osascript")
            .args(["-e", script])
            .output()
            .await;

        Ok(())
    }

    /// Check if a browser is running
    pub async fn is_browser_running(browser: &str) -> bool {
        let app_name = match browser.to_lowercase().as_str() {
            "chrome" | "google chrome" => "Google Chrome",
            "safari" => "Safari",
            "arc" => "Arc",
            _ => return false,
        };

        let script = format!(
            r#"tell application "System Events" to (name of processes) contains "{}""#,
            app_name
        );

        let output = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await;

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.trim() == "true"
            }
            Err(_) => false,
        }
    }

    /// Normalize browser name to standard form
    pub fn normalize_browser_name(browser: &str) -> &'static str {
        match browser.to_lowercase().as_str() {
            "chrome" | "google chrome" | "googlechrome" => "chrome",
            "safari" => "safari",
            "arc" => "arc",
            _ => "chrome", // Default to Chrome
        }
    }

    /// Build URL from template and ports
    pub fn build_url(
        template: &str,
        port: Option<u16>,
        frontend_port: Option<u16>,
        backend_port: Option<u16>,
    ) -> String {
        let mut url = template.to_string();

        if let Some(p) = port {
            url = url.replace("$PORT", &p.to_string());
            url = url.replace("${PORT}", &p.to_string());
        }
        if let Some(p) = frontend_port {
            url = url.replace("$FRONTEND_PORT", &p.to_string());
            url = url.replace("${FRONTEND_PORT}", &p.to_string());
        }
        if let Some(p) = backend_port {
            url = url.replace("$BACKEND_PORT", &p.to_string());
            url = url.replace("${BACKEND_PORT}", &p.to_string());
        }

        // Default URL if template is empty
        if url.is_empty() {
            if let Some(p) = frontend_port.or(port) {
                url = format!("http://localhost:{}", p);
            }
        }

        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_browser_name() {
        assert_eq!(BrowserAutomation::normalize_browser_name("Chrome"), "chrome");
        assert_eq!(
            BrowserAutomation::normalize_browser_name("Google Chrome"),
            "chrome"
        );
        assert_eq!(BrowserAutomation::normalize_browser_name("safari"), "safari");
        assert_eq!(BrowserAutomation::normalize_browser_name("Arc"), "arc");
        assert_eq!(BrowserAutomation::normalize_browser_name("unknown"), "chrome");
    }

    #[test]
    fn test_build_url() {
        // With template
        assert_eq!(
            BrowserAutomation::build_url("http://localhost:$PORT", Some(3000), None, None),
            "http://localhost:3000"
        );

        // With fullstack ports
        assert_eq!(
            BrowserAutomation::build_url(
                "http://localhost:$FRONTEND_PORT/api/$BACKEND_PORT",
                None,
                Some(3000),
                Some(8000)
            ),
            "http://localhost:3000/api/8000"
        );

        // Empty template with port
        assert_eq!(
            BrowserAutomation::build_url("", Some(3000), None, None),
            "http://localhost:3000"
        );

        // Empty template with frontend port
        assert_eq!(
            BrowserAutomation::build_url("", None, Some(3000), Some(8000)),
            "http://localhost:3000"
        );
    }
}
