//! Turning raw agent output into something worth watching.
//!
//! Claude Code's `--output-format stream-json` emits one JSON object per line.
//! Raw, it is unreadable; parsed, it becomes a live account of what the agent is
//! doing. Anything we do not recognise is passed through untouched rather than
//! dropped, so no output is ever silently lost.

use serde::Serialize;

/// A single item in a run's activity feed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Prose the agent produced.
    Text { text: String },
    /// The agent used a tool.
    Tool { name: String, detail: Option<String> },
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
/// `stream_json` should be true only for backends that emit Claude's stream-json.
/// Returns `None` for lines that carry no information (blank lines, protocol
/// bookkeeping such as `system`/`ping` messages).
pub fn parse_line(line: &str, stream_json: bool) -> Option<AgentEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if !stream_json {
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

    match value.get("type").and_then(|t| t.as_str()) {
        Some("assistant") => parse_assistant(&value),
        Some("result") => Some(parse_result(&value)),
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
        None => Some(AgentEvent::Raw {
            text: trimmed.to_string(),
        }),
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
            parse_line(line, true),
            Some(AgentEvent::Text {
                text: "Looking at the tests.".into()
            })
        );
    }

    #[test]
    fn tool_use_reports_the_file_it_touched() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/main.rs"}}]}}"#;
        assert_eq!(
            parse_line(line, true),
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
            parse_line(line, true),
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
        assert_eq!(parse_line(r#"{"type":"system","subtype":"init"}"#, true), None);
        assert_eq!(parse_line("   ", true), None);
    }

    #[test]
    fn non_json_output_is_preserved_rather_than_lost() {
        let event = parse_line("warning: something happened", true);
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
            parse_line(line, false),
            Some(AgentEvent::Raw { text: line.into() })
        );
    }

    #[test]
    fn long_tool_details_are_truncated() {
        let long = "x".repeat(200);
        let line = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"{long}"}}}}]}}}}"#
        );
        let Some(AgentEvent::Tool { detail, .. }) = parse_line(&line, true) else {
            panic!("expected a tool event");
        };
        assert!(detail.unwrap().ends_with('…'));
    }
}
