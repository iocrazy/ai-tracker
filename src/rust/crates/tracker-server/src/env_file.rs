//! Environment file management module
//!
//! Handles reading, updating, and writing .env files with variable substitution.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

/// Environment file management operations
pub struct EnvFileManager;

impl EnvFileManager {
    /// Update or add an environment variable in a .env file
    ///
    /// If the variable exists, its value is updated.
    /// If it doesn't exist, it's appended to the file.
    /// If the file doesn't exist, it's created.
    pub async fn update_var(file: &Path, key: &str, value: &str) -> Result<()> {
        let content = if file.exists() {
            tokio::fs::read_to_string(file)
                .await
                .with_context(|| format!("Failed to read env file {:?}", file))?
        } else {
            String::new()
        };

        let new_content = Self::update_var_in_content(&content, key, value);

        // Ensure parent directory exists
        if let Some(parent) = file.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Failed to create directory {:?}", parent))?;
            }
        }

        tokio::fs::write(file, new_content)
            .await
            .with_context(|| format!("Failed to write env file {:?}", file))?;

        Ok(())
    }

    /// Update a variable in the content string
    fn update_var_in_content(content: &str, key: &str, value: &str) -> String {
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let key_prefix = format!("{}=", key);
        let new_line = format!("{}={}", key, value);

        let mut found = false;
        for line in &mut lines {
            // Skip comments and empty lines when checking
            let trimmed = line.trim();
            if trimmed.starts_with(&key_prefix) {
                *line = new_line.clone();
                found = true;
                break;
            }
        }

        if !found {
            // Append the new variable
            lines.push(new_line);
        }

        // Join with newline, ensure file ends with newline
        let mut result = lines.join("\n");
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result
    }

    /// Substitute variables in a template string
    ///
    /// Supported placeholders:
    /// - $PORT or ${PORT}
    /// - $FRONTEND_PORT or ${FRONTEND_PORT}
    /// - $BACKEND_PORT or ${BACKEND_PORT}
    /// - Custom variables from the HashMap
    pub fn substitute_vars(template: &str, vars: &HashMap<String, String>) -> String {
        let mut result = template.to_string();

        for (key, value) in vars {
            // Replace ${KEY} format
            result = result.replace(&format!("${{{}}}", key), value);
            // Replace $KEY format (only if followed by non-alphanumeric or end)
            result = Self::replace_dollar_var(&result, key, value);
        }

        result
    }

    /// Replace $VAR format (more careful replacement)
    fn replace_dollar_var(content: &str, var_name: &str, value: &str) -> String {
        let pattern = format!("${}", var_name);
        let mut result = String::with_capacity(content.len());
        let pattern_chars: Vec<char> = pattern.chars().collect();

        let mut i = 0;
        let content_chars: Vec<char> = content.chars().collect();

        while i < content_chars.len() {
            // Check if we match the pattern at this position
            let matches = content_chars[i..]
                .iter()
                .take(pattern_chars.len())
                .zip(pattern_chars.iter())
                .all(|(a, b)| a == b);

            if matches && content_chars.len() >= i + pattern_chars.len() {
                // Check what comes after the pattern
                let next_pos = i + pattern_chars.len();
                let should_replace = if next_pos >= content_chars.len() {
                    true // End of string
                } else {
                    let next_char = content_chars[next_pos];
                    // Replace if next char is not alphanumeric or underscore
                    !next_char.is_alphanumeric() && next_char != '_'
                };

                if should_replace {
                    result.push_str(value);
                    i += pattern_chars.len();
                    continue;
                }
            }

            result.push(content_chars[i]);
            i += 1;
        }

        result
    }

    /// Read all variables from an env file
    pub async fn read_vars(file: &Path) -> Result<HashMap<String, String>> {
        if !file.exists() {
            return Ok(HashMap::new());
        }

        let content = tokio::fs::read_to_string(file)
            .await
            .with_context(|| format!("Failed to read env file {:?}", file))?;

        Ok(Self::parse_env_content(&content))
    }

    /// Parse env file content into a HashMap
    fn parse_env_content(content: &str) -> HashMap<String, String> {
        let mut vars = HashMap::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Find the first '=' to split key and value
            if let Some(pos) = line.find('=') {
                let key = line[..pos].trim().to_string();
                let mut value = line[pos + 1..].trim().to_string();

                // Remove surrounding quotes if present
                if (value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\''))
                {
                    if value.len() >= 2 {
                        value = value[1..value.len() - 1].to_string();
                    }
                }

                if !key.is_empty() {
                    vars.insert(key, value);
                }
            }
        }

        vars
    }

    /// Create port variables map for substitution
    pub fn port_vars(port: Option<u16>, frontend_port: Option<u16>, backend_port: Option<u16>) -> HashMap<String, String> {
        let mut vars = HashMap::new();

        if let Some(p) = port {
            vars.insert("PORT".to_string(), p.to_string());
        }
        if let Some(p) = frontend_port {
            vars.insert("FRONTEND_PORT".to_string(), p.to_string());
        }
        if let Some(p) = backend_port {
            vars.insert("BACKEND_PORT".to_string(), p.to_string());
        }

        vars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_var_in_content_new_var() {
        let content = "FOO=bar\n";
        let result = EnvFileManager::update_var_in_content(content, "BAZ", "qux");
        assert!(result.contains("FOO=bar"));
        assert!(result.contains("BAZ=qux"));
    }

    #[test]
    fn test_update_var_in_content_existing_var() {
        let content = "FOO=bar\nBAZ=old\n";
        let result = EnvFileManager::update_var_in_content(content, "BAZ", "new");
        assert!(result.contains("FOO=bar"));
        assert!(result.contains("BAZ=new"));
        assert!(!result.contains("BAZ=old"));
    }

    #[test]
    fn test_substitute_vars_dollar_format() {
        let mut vars = HashMap::new();
        vars.insert("PORT".to_string(), "3000".to_string());

        let result = EnvFileManager::substitute_vars("http://localhost:$PORT", &vars);
        assert_eq!(result, "http://localhost:3000");
    }

    #[test]
    fn test_substitute_vars_braces_format() {
        let mut vars = HashMap::new();
        vars.insert("PORT".to_string(), "3000".to_string());

        let result = EnvFileManager::substitute_vars("http://localhost:${PORT}", &vars);
        assert_eq!(result, "http://localhost:3000");
    }

    #[test]
    fn test_substitute_vars_multiple() {
        let mut vars = HashMap::new();
        vars.insert("FRONTEND_PORT".to_string(), "3000".to_string());
        vars.insert("BACKEND_PORT".to_string(), "8000".to_string());

        let result = EnvFileManager::substitute_vars(
            "Frontend: $FRONTEND_PORT, Backend: ${BACKEND_PORT}",
            &vars,
        );
        assert_eq!(result, "Frontend: 3000, Backend: 8000");
    }

    #[test]
    fn test_parse_env_content() {
        let content = r#"
# Comment
FOO=bar
BAZ="quoted value"
EMPTY=
NUMBER=123
"#;
        let vars = EnvFileManager::parse_env_content(content);
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(vars.get("BAZ"), Some(&"quoted value".to_string()));
        assert_eq!(vars.get("EMPTY"), Some(&"".to_string()));
        assert_eq!(vars.get("NUMBER"), Some(&"123".to_string()));
    }
}
