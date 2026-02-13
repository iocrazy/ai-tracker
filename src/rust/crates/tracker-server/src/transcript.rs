//! Transcript parser for Claude JSONL files
//!
//! Parses conversation messages, tool usage, and git commits from Claude's transcript files.
//! Also builds a timeline of thinking → text → tool_call → tool_result entries.

use anyhow::Result;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
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
    thinking: Option<String>,
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

// ============================================================================
// Timeline types (for rich history view)
// ============================================================================

/// Tool call detail for timeline entries
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallDetail {
    pub tool_use_id: String,
    pub tool_name: String,
    pub args_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_full: Option<String>,
}

/// Tool result detail for timeline entries
#[derive(Debug, Clone, Serialize)]
pub struct ToolResultDetail {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// A single entry in the conversation timeline, preserving JSONL order.
/// The timeline captures thinking → text → tool_call → tool_result sequences.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub entry_type: String,  // "text" | "thinking" | "tool_call" | "tool_result"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub role: String,        // "user" | "assistant"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<ToolCallDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResultDetail>,
}

/// Result of parsing a transcript
#[derive(Debug, Default)]
pub struct TranscriptParseResult {
    pub messages: Vec<ConversationMessage>,
    pub tool_usages: Vec<ToolUsage>,
    pub commits: Vec<GitCommit>,
    pub timeline: Vec<TimelineEntry>,
}

// ============================================================================
// Content extraction helpers
// ============================================================================

/// Extract text content from message content (simple, text-only)
fn extract_content(entry_type: &str, content: &serde_json::Value) -> String {
    let (text, _) = extract_content_rich(entry_type, content);
    text
}

/// Extract text + thinking from message content
fn extract_content_rich(entry_type: &str, content: &serde_json::Value) -> (String, Option<String>) {
    match entry_type {
        "user" => {
            if let Some(text) = content.as_str() {
                return (text.to_string(), None);
            }
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
                if !texts.is_empty() {
                    return (texts.join("\n"), None);
                }
            }
            (String::new(), None)
        }
        "assistant" => {
            if let Some(items) = content.as_array() {
                let mut texts: Vec<String> = Vec::new();
                let mut thinking_parts: Vec<String> = Vec::new();

                for item in items {
                    if let Ok(ci) = serde_json::from_value::<ContentItem>(item.clone()) {
                        match ci.content_type.as_deref() {
                            Some("text") => {
                                if let Some(t) = ci.text {
                                    if !t.trim().is_empty() {
                                        texts.push(t);
                                    }
                                }
                            }
                            Some("thinking") => {
                                if let Some(t) = ci.thinking {
                                    if !t.trim().is_empty() {
                                        thinking_parts.push(t);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }

                let text = texts.join("\n");
                let thinking = if thinking_parts.is_empty() {
                    None
                } else {
                    Some(thinking_parts.join("\n\n"))
                };
                return (text, thinking);
            }
            (String::new(), None)
        }
        _ => (String::new(), None),
    }
}

/// Truncate string to max chars (UTF-8 safe)
fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Generate args summary for tool call
fn tool_args_summary(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Bash" => {
            let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or("");
            truncate_str(cmd, 200)
        }
        "Read" | "Write" | "Edit" => {
            input.get("file_path").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        "Grep" => {
            let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
            let path = input.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            format!("{} in {}", pattern, path)
        }
        "Glob" => {
            input.get("pattern").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        "Task" => {
            let desc = input.get("description").and_then(|d| d.as_str()).unwrap_or("");
            truncate_str(desc, 100)
        }
        _ => {
            let json_str = serde_json::to_string(input).unwrap_or_default();
            truncate_str(&json_str, 200)
        }
    }
}

// ============================================================================
// Parsers
// ============================================================================

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
                    history_id: 0,
                    role: entry.entry_type,
                    content,
                    created_at: timestamp,
                });
            }
        }
    }

    Ok(messages)
}

/// Parse transcript and filter messages by time range
pub fn parse_transcript_for_task(
    path: &Path,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
) -> Result<Vec<ConversationMessage>> {
    let all_messages = parse_transcript(path)?;

    if started_at.is_none() && completed_at.is_none() {
        return Ok(all_messages);
    }

    let buffer = chrono::Duration::seconds(5);
    let started_with_buffer = started_at.map(|t| t - buffer);
    let completed_with_buffer = completed_at.map(|t| t + buffer);

    let filtered: Vec<ConversationMessage> = all_messages
        .into_iter()
        .filter(|msg| {
            if let Some(msg_time) = msg.created_at {
                let after_start = started_with_buffer.map_or(true, |s| msg_time >= s);
                let before_end = completed_with_buffer.map_or(true, |e| msg_time <= e);
                after_start && before_end
            } else {
                true
            }
        })
        .collect();

    Ok(filtered)
}

/// Parse a transcript JSONL file and extract all data (messages, tool usage, commits, timeline)
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
            let ts_str = timestamp.map(|t| t.to_rfc3339());

            match entry.entry_type.as_str() {
                "user" => {
                    if let Some(msg) = &entry.message {
                        // Process tool_result blocks
                        if let Some(items) = msg.content.as_array() {
                            for item in items {
                                if let Ok(tool_result) = serde_json::from_value::<ToolResultBlock>(item.clone()) {
                                    if tool_result.block_type == "tool_result" {
                                        let content_str = match &tool_result.content {
                                            serde_json::Value::String(s) => s.clone(),
                                            serde_json::Value::Array(arr) => {
                                                arr.iter()
                                                    .filter_map(|it| {
                                                        if it.get("type").and_then(|t| t.as_str()) == Some("text") {
                                                            it.get("text").and_then(|t| t.as_str())
                                                        } else {
                                                            None
                                                        }
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join("\n")
                                            }
                                            other => serde_json::to_string(other).unwrap_or_default(),
                                        };

                                        // Add to timeline
                                        result.timeline.push(TimelineEntry {
                                            entry_type: "tool_result".to_string(),
                                            timestamp: ts_str.clone(),
                                            role: "user".to_string(),
                                            text: None,
                                            thinking: None,
                                            tool_call: None,
                                            tool_result: Some(ToolResultDetail {
                                                tool_use_id: tool_result.tool_use_id.clone(),
                                                content: truncate_str(&content_str, 500),
                                                is_error: tool_result.is_error,
                                            }),
                                        });

                                        // Update pending tool with result (for tool_usages)
                                        if let Some((tool_usage, git_cmd)) = pending_tools.remove(&tool_result.tool_use_id) {
                                            let result_summary = truncate_str(&content_str, 500);

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

                                            // Check for git commit
                                            if let Some(_cmd) = git_cmd {
                                                if !tool_result.is_error {
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
                                content: content.clone(),
                                created_at: timestamp,
                            });

                            // Add text to timeline
                            result.timeline.push(TimelineEntry {
                                entry_type: "text".to_string(),
                                timestamp: ts_str.clone(),
                                role: "user".to_string(),
                                text: Some(content),
                                thinking: None,
                                tool_call: None,
                                tool_result: None,
                            });
                        }
                    }
                }
                "assistant" => {
                    if let Some(msg) = &entry.message {
                        // Process array content for timeline entries
                        if let Some(items) = msg.content.as_array() {
                            for item in items {
                                let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                match item_type {
                                    "thinking" => {
                                        if let Some(text) = item.get("thinking").and_then(|t| t.as_str()) {
                                            if !text.trim().is_empty() {
                                                result.timeline.push(TimelineEntry {
                                                    entry_type: "thinking".to_string(),
                                                    timestamp: ts_str.clone(),
                                                    role: "assistant".to_string(),
                                                    text: None,
                                                    thinking: Some(text.to_string()),
                                                    tool_call: None,
                                                    tool_result: None,
                                                });
                                            }
                                        }
                                    }
                                    "text" => {
                                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                            if !text.trim().is_empty() {
                                                result.timeline.push(TimelineEntry {
                                                    entry_type: "text".to_string(),
                                                    timestamp: ts_str.clone(),
                                                    role: "assistant".to_string(),
                                                    text: Some(text.to_string()),
                                                    thinking: None,
                                                    tool_call: None,
                                                    tool_result: None,
                                                });
                                            }
                                        }
                                    }
                                    "tool_use" => {
                                        if let Ok(tool_use) = serde_json::from_value::<ToolUseBlock>(item.clone()) {
                                            if tool_use.block_type == "tool_use" {
                                                let args_summary = tool_args_summary(&tool_use.name, &tool_use.input);
                                                let tool_args = serde_json::to_string(&tool_use.input).unwrap_or_default();

                                                // Add to timeline
                                                result.timeline.push(TimelineEntry {
                                                    entry_type: "tool_call".to_string(),
                                                    timestamp: ts_str.clone(),
                                                    role: "assistant".to_string(),
                                                    text: None,
                                                    thinking: None,
                                                    tool_call: Some(ToolCallDetail {
                                                        tool_use_id: tool_use.id.clone(),
                                                        tool_name: tool_use.name.clone(),
                                                        args_summary,
                                                        args_full: Some(tool_args.clone()),
                                                    }),
                                                    tool_result: None,
                                                });

                                                // Track for tool_usages matching
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
                                    _ => {}
                                }
                            }
                        }

                        // Extract assistant text+thinking for messages
                        let (content, _thinking) = extract_content_rich("assistant", &msg.content);
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

    // Add any pending tools that didn't get results
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

    if started_at.is_none() && completed_at.is_none() {
        return Ok(result);
    }

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

    let in_range_str = |ts_str: &Option<String>| -> bool {
        if let Some(ref s) = ts_str {
            if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                return in_range(Some(dt.with_timezone(&Utc)));
            }
        }
        true
    };

    result.messages.retain(|m| in_range(m.created_at));
    result.tool_usages.retain(|t| in_range(t.timestamp));
    result.commits.retain(|c| in_range(c.timestamp));
    result.timeline.retain(|t| in_range_str(&t.timestamp));

    Ok(result)
}
