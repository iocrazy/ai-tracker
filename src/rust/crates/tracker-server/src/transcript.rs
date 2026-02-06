//! Transcript parser for Claude JSONL files
//!
//! Parses conversation messages, tool usage, and git commits from Claude's transcript files.

use anyhow::Result;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracker_core::{ConversationMessage, GitCommit, ToolUsage};

/// A raw message from the JSONL transcript
#[derive(Debug, Deserialize)]
struct TranscriptEntry {
    #[serde(rename = "type")]
    entry_type: String,
    message: Option<MessageContent>,
    timestamp: Option<String>,
}

/// Message content structure
#[derive(Debug, Deserialize)]
struct MessageContent {
    content: serde_json::Value,
}

/// Content item in assistant messages
#[derive(Debug, Deserialize)]
struct ContentItem {
    #[serde(rename = "type")]
    content_type: Option<String>,
    text: Option<String>,
}

/// Tool use block in assistant messages
#[derive(Debug, Deserialize)]
struct ToolUseBlock {
    #[serde(rename = "type")]
    block_type: String,
    id: String,
    name: String,
    input: serde_json::Value,
}

/// Tool result in user messages
#[derive(Debug, Deserialize)]
struct ToolResultBlock {
    #[serde(rename = "type")]
    block_type: String,
    tool_use_id: String,
    content: serde_json::Value,
    #[serde(default)]
    is_error: bool,
}

/// Result of parsing a transcript
#[derive(Debug, Default)]
pub struct TranscriptParseResult {
    pub messages: Vec<ConversationMessage>,
    pub tool_usages: Vec<ToolUsage>,
    pub commits: Vec<GitCommit>,
}

/// Parse a transcript JSONL file and extract user/assistant messages
pub fn parse_transcript(path: &Path) -> Result<Vec<ConversationMessage>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();
    let mut id_counter = 0i64;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(&line) {
            // Only process user and assistant messages
            if entry.entry_type != "user" && entry.entry_type != "assistant" {
                continue;
            }

            let timestamp = entry.timestamp
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));

            if let Some(msg) = entry.message {
                let content = extract_content(&entry.entry_type, &msg.content);

                // Skip empty messages and tool results
                if content.is_empty() {
                    continue;
                }

                id_counter += 1;
                messages.push(ConversationMessage {
                    id: id_counter,
                    history_id: 0, // Will be set when saving
                    role: entry.entry_type,
                    content,
                    created_at: timestamp,
                });
            }
        }
    }

    Ok(messages)
}

/// Extract text content from message content
fn extract_content(entry_type: &str, content: &serde_json::Value) -> String {
    match entry_type {
        "user" => {
            // User message content can be:
            // - A string (simple text input)
            // - An array with text items or tool results
            if let Some(text) = content.as_str() {
                return text.to_string();
            }
            // Also handle array content with text items (like in continued sessions)
            if let Some(items) = content.as_array() {
                let texts: Vec<String> = items
                    .iter()
                    .filter_map(|item| {
                        // Try to extract text from {"type":"text","text":"..."} items
                        if let Ok(ci) = serde_json::from_value::<ContentItem>(item.clone()) {
                            if ci.content_type.as_deref() == Some("text") {
                                return ci.text;
                            }
                        }
                        None
                    })
                    .collect();
                if !texts.is_empty() {
                    return texts.join("\n");
                }
            }
            String::new()
        }
        "assistant" => {
            // Assistant message content is an array of items
            if let Some(items) = content.as_array() {
                let texts: Vec<String> = items
                    .iter()
                    .filter_map(|item| {
                        if let Ok(ci) = serde_json::from_value::<ContentItem>(item.clone()) {
                            if ci.content_type.as_deref() == Some("text") {
                                return ci.text;
                            }
                        }
                        None
                    })
                    .collect();
                return texts.join("\n");
            }
            String::new()
        }
        _ => String::new(),
    }
}

/// Parse transcript and filter messages by time range
pub fn parse_transcript_for_task(
    path: &Path,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
) -> Result<Vec<ConversationMessage>> {
    let all_messages = parse_transcript(path)?;

    // If no time range specified, return all messages
    if started_at.is_none() && completed_at.is_none() {
        return Ok(all_messages);
    }

    // Add 5 second buffer to account for timing differences
    // User prompt is sent slightly before start_task is processed
    let buffer = chrono::Duration::seconds(5);
    let started_with_buffer = started_at.map(|t| t - buffer);
    let completed_with_buffer = completed_at.map(|t| t + buffer);

    // Filter messages within the time range (with buffer)
    let filtered: Vec<ConversationMessage> = all_messages
        .into_iter()
        .filter(|msg| {
            if let Some(msg_time) = msg.created_at {
                let after_start = started_with_buffer.map_or(true, |s| msg_time >= s);
                let before_end = completed_with_buffer.map_or(true, |e| msg_time <= e);
                after_start && before_end
            } else {
                // Include messages without timestamps
                true
            }
        })
        .collect();

    Ok(filtered)
}

/// Parse a transcript JSONL file and extract all data (messages, tool usage, commits)
pub fn parse_transcript_full(path: &Path) -> Result<TranscriptParseResult> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut result = TranscriptParseResult::default();
    let mut id_counter = 0i64;
    let mut tool_id_counter = 0i64;
    let mut commit_id_counter = 0i64;

    // Track tool_use blocks to match with tool_result
    let mut pending_tools: HashMap<String, (ToolUsage, Option<String>)> = HashMap::new();

    // Regex to extract commit info from git commit output
    // Format: [branch hash] commit message
    let commit_regex = Regex::new(r"\[[\w\-/]+ ([a-f0-9]+)\] (.+)")?;
    let files_changed_regex = Regex::new(r"(\d+) files? changed")?;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(&line) {
            let timestamp = entry.timestamp
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));

            match entry.entry_type.as_str() {
                "user" => {
                    if let Some(msg) = &entry.message {
                        // Check for tool_result in user messages
                        if let Some(items) = msg.content.as_array() {
                            for item in items {
                                if let Ok(tool_result) = serde_json::from_value::<ToolResultBlock>(item.clone()) {
                                    if tool_result.block_type == "tool_result" {
                                        // Update pending tool with result
                                        if let Some((tool_usage, git_cmd)) = pending_tools.remove(&tool_result.tool_use_id) {
                                            let content_str = match &tool_result.content {
                                                serde_json::Value::String(s) => s.clone(),
                                                other => serde_json::to_string(other).unwrap_or_default(),
                                            };

                                            // Truncate result summary to 500 chars (UTF-8 safe)
                                            let result_summary = if content_str.chars().count() > 500 {
                                                let truncated: String = content_str.chars().take(497).collect();
                                                format!("{}...", truncated)
                                            } else {
                                                content_str.clone()
                                            };

                                            tool_id_counter += 1;
                                            result.tool_usages.push(ToolUsage {
                                                id: tool_id_counter,
                                                history_id: 0,
                                                tool_name: tool_usage.tool_name,
                                                tool_args: tool_usage.tool_args,
                                                result_summary,
                                                success: !tool_result.is_error,
                                                timestamp: tool_usage.timestamp,
                                            });

                                            // Check if this is a git commit command with successful result
                                            if let Some(_cmd) = git_cmd {
                                                if !tool_result.is_error {
                                                    // Parse commit info from result
                                                    if let Some(caps) = commit_regex.captures(&content_str) {
                                                        let commit_hash = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                                                        let commit_message = caps.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                                                        let files_changed = files_changed_regex
                                                            .captures(&content_str)
                                                            .and_then(|c| c.get(1))
                                                            .and_then(|m| m.as_str().parse::<i32>().ok())
                                                            .unwrap_or(0);

                                                        commit_id_counter += 1;
                                                        result.commits.push(GitCommit {
                                                            id: commit_id_counter,
                                                            history_id: 0,
                                                            commit_hash,
                                                            commit_message,
                                                            files_changed,
                                                            timestamp,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Extract user text message
                        let content = extract_content("user", &msg.content);
                        if !content.is_empty() {
                            id_counter += 1;
                            result.messages.push(ConversationMessage {
                                id: id_counter,
                                history_id: 0,
                                role: "user".to_string(),
                                content,
                                created_at: timestamp,
                            });
                        }
                    }
                }
                "assistant" => {
                    if let Some(msg) = &entry.message {
                        // Check for tool_use blocks in assistant messages
                        if let Some(items) = msg.content.as_array() {
                            for item in items {
                                if let Ok(tool_use) = serde_json::from_value::<ToolUseBlock>(item.clone()) {
                                    if tool_use.block_type == "tool_use" {
                                        let tool_args = serde_json::to_string(&tool_use.input).unwrap_or_default();

                                        // Check if this is a git commit command
                                        let git_cmd = if tool_use.name == "Bash" {
                                            tool_use.input.get("command")
                                                .and_then(|c| c.as_str())
                                                .filter(|cmd| cmd.contains("git commit"))
                                                .map(|s| s.to_string())
                                        } else {
                                            None
                                        };

                                        let pending = ToolUsage {
                                            id: 0,
                                            history_id: 0,
                                            tool_name: tool_use.name.clone(),
                                            tool_args,
                                            result_summary: String::new(),
                                            success: true,
                                            timestamp,
                                        };

                                        pending_tools.insert(tool_use.id.clone(), (pending, git_cmd));
                                    }
                                }
                            }
                        }

                        // Extract assistant text message
                        let content = extract_content("assistant", &msg.content);
                        if !content.is_empty() {
                            id_counter += 1;
                            result.messages.push(ConversationMessage {
                                id: id_counter,
                                history_id: 0,
                                role: "assistant".to_string(),
                                content,
                                created_at: timestamp,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Add any pending tools that didn't get results (shouldn't normally happen)
    for (_id, (tool_usage, _)) in pending_tools {
        tool_id_counter += 1;
        result.tool_usages.push(ToolUsage {
            id: tool_id_counter,
            history_id: 0,
            tool_name: tool_usage.tool_name,
            tool_args: tool_usage.tool_args,
            result_summary: "(no result)".to_string(),
            success: false,
            timestamp: tool_usage.timestamp,
        });
    }

    Ok(result)
}

/// Parse transcript with time filtering
pub fn parse_transcript_full_for_task(
    path: &Path,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
) -> Result<TranscriptParseResult> {
    let mut result = parse_transcript_full(path)?;

    // If no time range specified, return all
    if started_at.is_none() && completed_at.is_none() {
        return Ok(result);
    }

    // Add 5 second buffer
    let buffer = chrono::Duration::seconds(5);
    let started_with_buffer = started_at.map(|t| t - buffer);
    let completed_with_buffer = completed_at.map(|t| t + buffer);

    let in_range = |ts: Option<DateTime<Utc>>| -> bool {
        if let Some(t) = ts {
            let after_start = started_with_buffer.map_or(true, |s| t >= s);
            let before_end = completed_with_buffer.map_or(true, |e| t <= e);
            after_start && before_end
        } else {
            true
        }
    };

    result.messages.retain(|m| in_range(m.created_at));
    result.tool_usages.retain(|t| in_range(t.timestamp));
    result.commits.retain(|c| in_range(c.timestamp));

    Ok(result)
}
