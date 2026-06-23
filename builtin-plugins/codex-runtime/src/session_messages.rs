use chrono::{DateTime, TimeZone, Utc};
use niuma_core::tool_session::{ToolSessionMessage, ToolSessionMessageRole};
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct CodexRow {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    row_type: String,
    #[serde(default)]
    payload: Value,
}

// 将单行 Codex JSONL 归一成统一会话消息；这里刻意只保留最小 metadata，避免泄露 raw payload。
pub fn parse_codex_message_line(
    session_id: &str,
    line_index: usize,
    line: &str,
) -> ToolSessionMessage {
    let id = format!("codex:{session_id}:{line_index:020}");
    let Ok(row) = serde_json::from_str::<CodexRow>(line) else {
        return ToolSessionMessage {
            id,
            role: ToolSessionMessageRole::Unknown,
            content: String::new(),
            created_at: epoch(),
            metadata: metadata_with_reason(None, None, "invalid_json"),
        };
    };

    let item_type = row.payload.get("type").and_then(Value::as_str);
    let role = role_for_row(&row.row_type, item_type, &row.payload);
    let content = content_for_row(&row.row_type, item_type, &row.payload);
    let metadata = if content.is_empty() {
        metadata_with_reason(
            Some(row.row_type.as_str()),
            item_type,
            "content_unavailable",
        )
    } else {
        metadata(Some(row.row_type.as_str()), item_type)
    };

    ToolSessionMessage {
        id,
        role,
        content,
        created_at: parse_timestamp(row.timestamp.as_deref()).unwrap_or_else(epoch),
        metadata,
    }
}

pub fn parse_codex_messages_newest_first(
    session_id: &str,
    indexed_lines: &[(usize, String)],
) -> Vec<ToolSessionMessage> {
    let mut messages = indexed_lines
        .iter()
        .map(|(line_index, line)| parse_codex_message_line(session_id, *line_index, line))
        .collect::<Vec<_>>();
    // API 约定详情消息倒序返回，最新一行排在最前。
    messages.reverse();
    messages
}

pub fn is_detail_message_line(line: &str) -> bool {
    let Ok(row) = serde_json::from_str::<CodexRow>(line) else {
        return false;
    };
    matches!(row.row_type.as_str(), "event_msg" | "response_item")
}

fn role_for_row(
    row_type: &str,
    item_type: Option<&str>,
    payload: &Value,
) -> ToolSessionMessageRole {
    match row_type {
        "response_item" => match item_type {
            Some("message") => role_for_message_payload(payload),
            Some(
                "function_call"
                | "custom_tool_call"
                | "web_search_call"
                | "tool_search_call"
                | "image_generation_call",
            ) => ToolSessionMessageRole::ToolCall,
            Some("function_call_output" | "custom_tool_call_output" | "tool_search_output") => {
                ToolSessionMessageRole::ToolResult
            }
            Some("reasoning") => ToolSessionMessageRole::Assistant,
            Some(_) => ToolSessionMessageRole::Event,
            None => ToolSessionMessageRole::Unknown,
        },
        "event_msg" => match item_type {
            Some("agent_message") | Some("task_complete") => ToolSessionMessageRole::Assistant,
            Some("user_message") | Some("user_input") => ToolSessionMessageRole::User,
            Some("system_message") => ToolSessionMessageRole::System,
            Some(_) => ToolSessionMessageRole::Event,
            None => ToolSessionMessageRole::Unknown,
        },
        "session_meta" => ToolSessionMessageRole::Event,
        _ => ToolSessionMessageRole::Unknown,
    }
}

fn role_for_message_payload(payload: &Value) -> ToolSessionMessageRole {
    match payload.get("role").and_then(Value::as_str) {
        Some("user") => ToolSessionMessageRole::User,
        Some("assistant") => ToolSessionMessageRole::Assistant,
        Some("system") => ToolSessionMessageRole::System,
        Some("tool") => ToolSessionMessageRole::ToolResult,
        Some(_) => ToolSessionMessageRole::Unknown,
        None => ToolSessionMessageRole::Unknown,
    }
}

fn content_for_row(row_type: &str, item_type: Option<&str>, payload: &Value) -> String {
    if row_type == "event_msg" && item_type == Some("task_complete") {
        return payload
            .get("last_agent_message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
    }
    if row_type == "event_msg" && item_type == Some("item_completed") {
        return payload
            .get("item")
            .and_then(|item| text_from_value(item.get("text").unwrap_or(&Value::Null)))
            .unwrap_or_default();
    }
    if matches!(
        item_type,
        Some("function_call" | "custom_tool_call" | "web_search_call" | "tool_search_call")
    ) {
        // 工具调用参数结构变化较频繁，详情里只展示稳定的调用名，不把完整 arguments 当正文透出。
        return payload
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
    }

    payload
        .get("message")
        .and_then(text_from_value)
        .or_else(|| payload.get("text").and_then(text_from_value))
        .or_else(|| payload.get("output").and_then(text_from_value))
        .or_else(|| payload.get("content").and_then(text_from_value))
        .or_else(|| payload.get("summary").and_then(text_from_value))
        .unwrap_or_default()
}

fn text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => trimmed_text(text),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| {
                    item.get("text")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("content").and_then(Value::as_str))
                        .and_then(trimmed_text)
                })
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

fn trimmed_text(value: &str) -> Option<String> {
    let text = value.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn epoch() -> DateTime<Utc> {
    Utc.timestamp_opt(0, 0)
        .single()
        .expect("Unix epoch timestamp must be valid")
}

fn metadata(row_type: Option<&str>, item_type: Option<&str>) -> Value {
    Value::Object(base_metadata(row_type, item_type))
}

fn metadata_with_reason(
    row_type: Option<&str>,
    item_type: Option<&str>,
    reason: &'static str,
) -> Value {
    let mut map = base_metadata(row_type, item_type);
    map.insert("content_extract_reason".to_string(), json!(reason));
    Value::Object(map)
}

fn base_metadata(row_type: Option<&str>, item_type: Option<&str>) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("source".to_string(), json!("codex-session-jsonl"));
    if let Some(row_type) = row_type {
        map.insert("codex_row_type".to_string(), json!(row_type));
    }
    if let Some(item_type) = item_type {
        map.insert("codex_item_type".to_string(), json!(item_type));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::tool_session::ToolSessionMessageRole;

    #[test]
    fn codex_messages_are_returned_newest_first_without_raw_payload() {
        let lines = vec![
            (
                0,
                r#"{"timestamp":"2026-06-22T01:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"用户问题"}],"secret":"不能泄露"}}"#
                    .to_string(),
            ),
            (
                1,
                r#"{"timestamp":"2026-06-22T01:00:01Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"助手回答"}],"raw_line":"不能泄露"}}"#
                    .to_string(),
            ),
        ];

        let messages = parse_codex_messages_newest_first("session-1", &lines);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, ToolSessionMessageRole::Assistant);
        assert_eq!(messages[0].content, "助手回答");
        assert_eq!(messages[1].role, ToolSessionMessageRole::User);
        assert_eq!(messages[1].content, "用户问题");
        let encoded = serde_json::to_string(&messages).unwrap();
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("raw_line"));
        assert!(!encoded.contains("不能泄露"));
        assert!(encoded.contains("codex_row_type"));
        assert!(encoded.contains("codex_item_type"));
    }
}
