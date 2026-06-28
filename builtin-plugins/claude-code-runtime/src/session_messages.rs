use chrono::{DateTime, Utc};
use niuma_core::tool_session::{ToolSessionMessage, ToolSessionMessageRole};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct ClaudeRow {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    row_type: String,
    #[serde(default)]
    message: Value,
}

pub(crate) fn parse_claude_message_line(
    session_id: &str,
    line_index: usize,
    line: &str,
) -> ToolSessionMessage {
    let id = format!("claude_code:{session_id}:{line_index:020}");
    let Ok(row) = serde_json::from_str::<ClaudeRow>(line) else {
        return ToolSessionMessage {
            id,
            role: ToolSessionMessageRole::Unknown,
            content: String::new(),
            created_at: epoch(),
            metadata: json!({"reason": "invalid_json"}),
        };
    };
    let (role, content, content_type) = role_content_for_row(&row);
    ToolSessionMessage {
        id,
        role,
        content,
        created_at: parse_timestamp(row.timestamp.as_deref()).unwrap_or_else(epoch),
        metadata: json!({
            "raw_type": row.row_type,
            "content_type": content_type,
        }),
    }
}

pub(crate) fn is_detail_message_line(line: &str) -> bool {
    let Ok(row) = serde_json::from_str::<ClaudeRow>(line) else {
        return false;
    };
    let (_role, content, content_type) = role_content_for_row(&row);
    content_type != "unmapped" && !content.trim().is_empty()
}

fn role_content_for_row(row: &ClaudeRow) -> (ToolSessionMessageRole, String, &'static str) {
    let content = row.message.get("content").unwrap_or(&Value::Null);
    if row.row_type == "user" {
        if let Some(text) = content.as_str().and_then(trimmed_text) {
            return (ToolSessionMessageRole::User, text, "text");
        }
        if let Some((text, is_error)) = first_tool_result(content) {
            let content_type = if is_error {
                "tool_result_error"
            } else {
                "tool_result"
            };
            return (ToolSessionMessageRole::ToolResult, text, content_type);
        }
    }
    if row.row_type == "assistant" {
        if let Some((role, text, content_type)) = first_assistant_item(content) {
            return (role, text, content_type);
        }
    }
    (ToolSessionMessageRole::Event, String::new(), "unmapped")
}

fn first_assistant_item(content: &Value) -> Option<(ToolSessionMessageRole, String, &'static str)> {
    for item in content.as_array()? {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = item
                    .get("text")
                    .and_then(Value::as_str)
                    .and_then(trimmed_text)?;
                return Some((ToolSessionMessageRole::Assistant, text, "text"));
            }
            Some("thinking") => {
                return Some((
                    ToolSessionMessageRole::Assistant,
                    "Claude Code 正在思考".to_string(),
                    "thinking",
                ));
            }
            Some("tool_use") => {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("tool");
                return Some((
                    ToolSessionMessageRole::ToolCall,
                    name.to_string(),
                    "tool_use",
                ));
            }
            _ => {}
        }
    }
    None
}

fn first_tool_result(content: &Value) -> Option<(String, bool)> {
    for item in content.as_array()? {
        if item.get("type").and_then(Value::as_str) == Some("tool_result") {
            let text = item
                .get("content")
                .and_then(text_from_value)
                .unwrap_or_else(|| "工具结果已返回".to_string());
            let is_error = item
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            return Some((text, is_error));
        }
    }
    None
}

fn text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => trimmed_text(text),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .and_then(trimmed_text)
                        .or_else(|| {
                            item.get("text")
                                .and_then(Value::as_str)
                                .and_then(trimmed_text)
                        })
                        .or_else(|| {
                            item.get("content")
                                .and_then(Value::as_str)
                                .and_then(trimmed_text)
                        })
                })
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from(std::time::SystemTime::UNIX_EPOCH)
}

fn trimmed_text(value: &str) -> Option<String> {
    let text = value.trim();
    (!text.is_empty()).then(|| text.to_string())
}
