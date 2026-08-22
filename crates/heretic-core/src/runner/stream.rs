//! Turning raw agent output into something worth watching.
//!
//! Claude Code's `--output-format stream-json` emits one JSON object per line.
//! Raw, it is unreadable; parsed, it becomes a live account of what the agent is
//! doing. Anything we do not recognise is passed through untouched rather than
//! dropped, so no output is ever silently lost.

use serde::Serialize;

/// How a backend prints its progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OutputFormat {
    /// Ordinary lines of text.
    Plain,
    /// Claude Code's `--output-format stream-json`.
    ClaudeStreamJson,
    /// Codex's `--json`: one JSON event per line.
    CodexJsonl,
}

/// A single item in a run's activity feed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Prose the agent produced.
    Text { text: String },
    /// The agent used a tool.
    Tool {
        name: String,
        detail: Option<String>,
    },
    /// A line we could not interpret — printed as-is.
    Raw { text: String },
    /// Something the agent or its CLI reported as an error.
    Error { message: String },
    /// The agent's closing summary, emitted once at the end of a run.
    Result {
        text: Option<String>,
        is_error: bool,
        duration_ms: Option<u64>,
        cost_usd: Option<f64>,
    },
}

impl AgentEvent {
    /// A one-line rendering for compact views and Flux comments.
    pub fn summary(&self) -> String {
        match self {
            AgentEvent::Text { text } => text.clone(),
            AgentEvent::Tool { name, detail } => match detail {
                Some(d) if !d.is_empty() => format!("{name}: {d}"),
                _ => name.clone(),
            },
            AgentEvent::Raw { text } => text.clone(),
            AgentEvent::Error { message } => format!("Error: {message}"),
            AgentEvent::Result { text, .. } => text.clone().unwrap_or_default(),
        }
    }
}

/// Parse one line of agent output.
///
/// Returns `None` for lines that carry no information — blank lines, and
/// protocol bookkeeping such as `system`, `ping` or `turn.started`.
pub fn parse_line(line: &str, format: OutputFormat) -> Option<AgentEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if format == OutputFormat::Plain {
        return Some(AgentEvent::Raw {
            text: trimmed.to_string(),
        });
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        // Not JSON after all — a warning on stdout, or a CLI banner.
        return Some(AgentEvent::Raw {
            text: trimmed.to_string(),
        });
    };

    match format {
        OutputFormat::CodexJsonl => parse_codex(&value),
        _ => parse_claude(&value),
    }
}

/// Codex's `--json` stream.
///
/// Shapes confirmed against codex-cli 0.149:
/// `{"type":"item.completed","item":{"type":"agent_message","text":"…"}}`,
/// `{"type":"turn.completed","usage":{…}}`, and `{"type":"error","message":"…"}`.
fn parse_codex(value: &serde_json::Value) -> Option<AgentEvent> {
    match value.get("type").and_then(|t| t.as_str()) {
        // Only completed items are shown: started/updated would double up.
        Some("item.completed") => parse_codex_item(value.get("item")?),
        Some("error") | Some("turn.failed") => Some(AgentEvent::Error {
            message: value
                .get("message")
                .or_else(|| value.get("error").and_then(|e| e.get("message")))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
                .to_string(),
        }),
        // Session and turn bookkeeping carries nothing worth showing.
        _ => None,
    }
}

fn parse_codex_item(item: &serde_json::Value) -> Option<AgentEvent> {
    let kind = item.get("type").and_then(|t| t.as_str())?;

    let text_of = |key: &str| {
        item.get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    match kind {
        "agent_message" => Some(AgentEvent::Text {
            text: text_of("text").or_else(|| text_of("message"))?,
        }),
        "error" => Some(AgentEvent::Error {
            message: text_of("message").unwrap_or_else(|| "unknown error".into()),
        }),
        "command_execution" => Some(AgentEvent::Tool {
            name: "Shell".into(),
            detail: text_of("command").map(|c| truncate(&c, 120)),
        }),
        "file_change" | "patch_apply" => Some(AgentEvent::Tool {
            name: "Edit".into(),
            detail: changed_paths(item),
        }),
        "mcp_tool_call" => Some(AgentEvent::Tool {
            name: text_of("tool").unwrap_or_else(|| "MCP".into()),
            detail: text_of("server"),
        }),
        "web_search" => Some(AgentEvent::Tool {
            name: "Search".into(),
            detail: text_of("query"),
        }),
        // Reasoning and todo lists are internal chatter, not progress.
        "reasoning" | "todo_list" => None,
        // Anything new in a later Codex still shows up rather than vanishing.
        other => Some(AgentEvent::Tool {
            name: other.to_string(),
            detail: None,
        }),
    }
}

/// The files a `file_change` item touched.
fn changed_paths(item: &serde_json::Value) -> Option<String> {
    let changes = item.get("changes")?.as_array()?;
    let paths: Vec<String> = changes
        .iter()
        .filter_map(|change| {
            change
                .get("path")
                .and_then(|p| p.as_str())
                .map(str::to_string)
        })
        .collect();

    (!paths.is_empty()).then(|| truncate(&paths.join(", "), 120))
}

/// Claude Code's `--output-format stream-json`.
fn parse_claude(value: &serde_json::Value) -> Option<AgentEvent> {
    match value.get("type").and_then(|t| t.as_str()) {
        Some("assistant") => parse_assistant(value),
        Some("result") => Some(parse_result(value)),
        Some("error") => Some(AgentEvent::Error {
            message: value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
                .to_string(),
        }),
        // system init, user tool results, stream bookkeeping: nothing to show.
        Some(_) => None,
        None => None,
    }
}

fn parse_assistant(value: &serde_json::Value) -> Option<AgentEvent> {
    let content = value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())?;

    let mut texts: Vec<String> = Vec::new();

    for block in content {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    let text = text.trim();
                    if !text.is_empty() {
                        texts.push(text.to_string());
                    }
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool")
                    .to_string();
                return Some(AgentEvent::Tool {
                    name,
                    detail: tool_detail(block.get("input")),
                });
            }
            _ => {}
        }
    }

    if texts.is_empty() {
        None
    } else {
        Some(AgentEvent::Text {
            text: texts.join(" "),
        })
    }
}

/// Pick the most informative field out of a tool's input, so the feed reads
/// "Edit: src/main.rs" rather than a wall of JSON.
fn tool_detail(input: Option<&serde_json::Value>) -> Option<String> {
    let input = input?.as_object()?;
    for key in ["file_path", "path", "command", "pattern", "url", "query"] {
        if let Some(value) = input.get(key).and_then(|v| v.as_str()) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(truncate(value, 120));
            }
        }
    }
    None
}

fn parse_result(value: &serde_json::Value) -> AgentEvent {
    AgentEvent::Result {
        text: value
            .get("result")
            .and_then(|r| r.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        is_error: value
            .get("is_error")
            .and_then(|e| e.as_bool())
            .unwrap_or(false),
        duration_ms: value.get("duration_ms").and_then(|d| d.as_u64()),
        cost_usd: value.get("total_cost_usd").and_then(|c| c.as_f64()),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let kept: String = value.chars().take(limit).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_text_becomes_a_text_event() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Looking at the tests."}]}}"#;
        assert_eq!(
            parse_line(line, OutputFormat::ClaudeStreamJson),
            Some(AgentEvent::Text {
                text: "Looking at the tests.".into()
            })
        );
    }

    #[test]
    fn tool_use_reports_the_file_it_touched() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/main.rs"}}]}}"#;
        assert_eq!(
            parse_line(line, OutputFormat::ClaudeStreamJson),
            Some(AgentEvent::Tool {
                name: "Edit".into(),
                detail: Some("src/main.rs".into())
            })
        );
    }

    #[test]
    fn the_final_result_carries_cost_and_duration() {
        let line = r#"{"type":"result","result":"Done.","is_error":false,"duration_ms":4200,"total_cost_usd":0.031}"#;
        assert_eq!(
            parse_line(line, OutputFormat::ClaudeStreamJson),
            Some(AgentEvent::Result {
                text: Some("Done.".into()),
                is_error: false,
                duration_ms: Some(4200),
                cost_usd: Some(0.031),
            })
        );
    }

    #[test]
    fn bookkeeping_messages_are_dropped() {
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"init"}"#,
                OutputFormat::ClaudeStreamJson
            ),
            None
        );
        assert_eq!(parse_line("   ", OutputFormat::ClaudeStreamJson), None);
    }

    #[test]
    fn non_json_output_is_preserved_rather_than_lost() {
        let event = parse_line(
            "warning: something happened",
            OutputFormat::ClaudeStreamJson,
        );
        assert_eq!(
            event,
            Some(AgentEvent::Raw {
                text: "warning: something happened".into()
            })
        );
    }

    #[test]
    fn plain_runners_pass_their_output_straight_through() {
        let line = r#"{"type":"assistant","message":{"content":[]}}"#;
        assert_eq!(
            parse_line(line, OutputFormat::Plain),
            Some(AgentEvent::Raw { text: line.into() })
        );
    }

    // --- Codex ---------------------------------------------------------------
    //
    // These lines were captured verbatim from `codex exec --json` (codex-cli
    // 0.149) rather than written from the documentation.

    #[test]
    fn a_codex_agent_message_becomes_text() {
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"I looked at README.md and it already says hello."}}"#;
        assert_eq!(
            parse_line(line, OutputFormat::CodexJsonl),
            Some(AgentEvent::Text {
                text: "I looked at README.md and it already says hello.".into()
            })
        );
    }

    #[test]
    fn a_codex_error_item_is_surfaced() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Model metadata for `qwen3:8b` not found."}}"#;
        assert_eq!(
            parse_line(line, OutputFormat::CodexJsonl),
            Some(AgentEvent::Error {
                message: "Model metadata for `qwen3:8b` not found.".into()
            })
        );
    }

    #[test]
    fn codex_session_bookkeeping_is_dropped() {
        for line in [
            r#"{"type":"thread.started","thread_id":"01a029"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":120,"output_tokens":14}}"#,
        ] {
            assert_eq!(parse_line(line, OutputFormat::CodexJsonl), None, "{line}");
        }
    }

    #[test]
    fn codex_started_items_do_not_double_up_with_completed_ones() {
        let started = r#"{"type":"item.started","item":{"id":"item_1","type":"agent_message","text":"partial"}}"#;
        assert_eq!(parse_line(started, OutputFormat::CodexJsonl), None);
    }

    #[test]
    fn codex_shell_commands_show_in_the_feed() {
        let line = r#"{"type":"item.completed","item":{"type":"command_execution","command":"cargo test","exit_code":0}}"#;
        assert_eq!(
            parse_line(line, OutputFormat::CodexJsonl),
            Some(AgentEvent::Tool {
                name: "Shell".into(),
                detail: Some("cargo test".into())
            })
        );
    }

    #[test]
    fn codex_file_changes_report_their_paths() {
        let line = r#"{"type":"item.completed","item":{"type":"file_change","changes":[{"path":"src/lib.rs","kind":"modify"},{"path":"README.md","kind":"add"}]}}"#;
        assert_eq!(
            parse_line(line, OutputFormat::CodexJsonl),
            Some(AgentEvent::Tool {
                name: "Edit".into(),
                detail: Some("src/lib.rs, README.md".into())
            })
        );
    }

    #[test]
    fn an_unknown_codex_item_is_shown_rather_than_lost() {
        // A later Codex may add item types; they should still appear.
        let line = r#"{"type":"item.completed","item":{"type":"something_new"}}"#;
        assert_eq!(
            parse_line(line, OutputFormat::CodexJsonl),
            Some(AgentEvent::Tool {
                name: "something_new".into(),
                detail: None
            })
        );
    }

    #[test]
    fn codex_reasoning_is_not_shown() {
        let line =
            r#"{"type":"item.completed","item":{"type":"reasoning","text":"thinking out loud"}}"#;
        assert_eq!(parse_line(line, OutputFormat::CodexJsonl), None);
    }

    #[test]
    fn a_codex_stream_error_is_surfaced() {
        let line = r#"{"type":"error","message":"Reconnecting... 1/5 (stream disconnected)"}"#;
        match parse_line(line, OutputFormat::CodexJsonl) {
            Some(AgentEvent::Error { message }) => assert!(message.contains("Reconnecting")),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn long_tool_details_are_truncated() {
        let long = "x".repeat(200);
        let line = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"{long}"}}}}]}}}}"#
        );
        let Some(AgentEvent::Tool { detail, .. }) =
            parse_line(&line, OutputFormat::ClaudeStreamJson)
        else {
            panic!("expected a tool event");
        };
        assert!(detail.unwrap().ends_with('…'));
    }
}
