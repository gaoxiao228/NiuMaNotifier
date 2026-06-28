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

pub struct DetailMessageSignature {
    pub role: ToolSessionMessageRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub is_structured_message: bool,
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
        .filter_map(|(line_index, line)| {
            (is_detail_message_line(line) && !is_injected_context_message_line(line))
                .then(|| parse_codex_message_line(session_id, *line_index, line))
        })
        .collect::<Vec<_>>();
    // API 约定详情消息倒序返回，最新一行排在最前。
    messages.reverse();
    messages
}

pub fn is_detail_message_line(line: &str) -> bool {
    let Ok(row) = serde_json::from_str::<CodexRow>(line) else {
        return false;
    };
    let item_type = row.payload.get("type").and_then(Value::as_str);
    is_user_visible_detail_row(&row.row_type, item_type, &row.payload)
}

pub fn detail_message_signature(line: &str) -> Option<DetailMessageSignature> {
    let Ok(row) = serde_json::from_str::<CodexRow>(line) else {
        return None;
    };
    let item_type = row.payload.get("type").and_then(Value::as_str);
    if !is_user_visible_detail_row(&row.row_type, item_type, &row.payload) {
        return None;
    }
    let role = role_for_row(&row.row_type, item_type, &row.payload);
    let content = content_for_row(&row.row_type, item_type, &row.payload);
    if content.is_empty() || is_injected_context_content(&content) {
        return None;
    }
    Some(DetailMessageSignature {
        role,
        content,
        created_at: parse_timestamp(row.timestamp.as_deref()).unwrap_or_else(epoch),
        is_structured_message: row.row_type == "response_item" && item_type == Some("message"),
    })
}

fn is_user_visible_detail_row(row_type: &str, item_type: Option<&str>, payload: &Value) -> bool {
    match row_type {
        "event_msg" => true,
        // 普通详情消息只承载用户可读对话，工具调用与输出保留给独立 trace/debug 视图。
        "response_item" => !is_tool_trace_response_item(item_type, payload),
        _ => false,
    }
}

fn is_tool_trace_response_item(item_type: Option<&str>, payload: &Value) -> bool {
    match item_type {
        Some(
            "function_call"
            | "custom_tool_call"
            | "web_search_call"
            | "tool_search_call"
            | "image_generation_call"
            | "function_call_output"
            | "custom_tool_call_output"
            | "tool_search_output",
        ) => true,
        Some("message") => payload.get("role").and_then(Value::as_str) == Some("tool"),
        _ => false,
    }
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

fn is_injected_context_message_line(line: &str) -> bool {
    let Ok(row) = serde_json::from_str::<CodexRow>(line) else {
        return false;
    };
    if !matches!(row.row_type.as_str(), "event_msg" | "response_item") {
        return false;
    }
    let item_type = row.payload.get("type").and_then(Value::as_str);
    let content = content_for_row(&row.row_type, item_type, &row.payload);
    is_injected_context_content(&content)
}

fn is_injected_context_content(content: &str) -> bool {
    let content = content.trim_start();
    // Codex 会把注入上下文和中断标记作为 role=user 写入 JSONL；这些不是用户真实提问。
    (content.starts_with("# AGENTS.md instructions")
        && content.contains("<INSTRUCTIONS>")
        && content.contains("</INSTRUCTIONS>"))
        || (content.starts_with("<environment_context>")
            && content.contains("</environment_context>"))
        || (content.starts_with("<turn_aborted>") && content.contains("</turn_aborted>"))
        || (content.starts_with("<subagent_notification>")
            && content.contains("</subagent_notification>"))
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
        assert_eq!(messages[0].id, "codex:session-1:00000000000000000001");
        assert_eq!(messages[0].role, ToolSessionMessageRole::Assistant);
        assert_eq!(messages[0].content, "助手回答");
        assert_eq!(
            messages[0].created_at.to_rfc3339(),
            "2026-06-22T01:00:01+00:00"
        );
        assert_eq!(messages[1].id, "codex:session-1:00000000000000000000");
        assert_eq!(messages[1].role, ToolSessionMessageRole::User);
        assert_eq!(messages[1].content, "用户问题");
        assert_eq!(
            messages[1].created_at.to_rfc3339(),
            "2026-06-22T01:00:00+00:00"
        );
        let encoded = serde_json::to_string(&messages).unwrap();
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("raw_line"));
        assert!(!encoded.contains("不能泄露"));
        assert!(encoded.contains("codex_row_type"));
        assert!(encoded.contains("codex_item_type"));
    }

    #[test]
    fn codex_messages_skip_injected_agents_context() {
        let lines = vec![
            (
                0,
                r##"{"timestamp":"2026-06-22T01:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions\n\n<INSTRUCTIONS>\n# Global Rules\n\n- 始终使用简体中文与我交流\n</INSTRUCTIONS>\n<environment_context>\n  <cwd>/tmp/demo</cwd>\n</environment_context>"}]}}"##
                    .to_string(),
            ),
            (
                1,
                r#"{"timestamp":"2026-06-22T01:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"真实用户问题"}]}}"#
                    .to_string(),
            ),
        ];

        let messages = parse_codex_messages_newest_first("session-1", &lines);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, ToolSessionMessageRole::User);
        assert_eq!(messages[0].content, "真实用户问题");
    }

    #[test]
    fn codex_messages_skip_turn_aborted_context() {
        let lines = vec![
            (
                0,
                r#"{"timestamp":"2026-06-22T01:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<turn_aborted>\nThe user interrupted the previous turn on purpose. Any running unified exec processes may still be running in the background.\n</turn_aborted>"}]}}"#
                    .to_string(),
            ),
            (
                1,
                r#"{"timestamp":"2026-06-22T01:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"真实用户问题"}]}}"#
                    .to_string(),
            ),
        ];

        let messages = parse_codex_messages_newest_first("session-1", &lines);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, ToolSessionMessageRole::User);
        assert_eq!(messages[0].content, "真实用户问题");
    }

    #[test]
    fn codex_messages_skip_subagent_notification_context() {
        let lines = vec![
            (
                0,
                r#"{"timestamp":"2026-06-22T01:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<subagent_notification>\n{\"agent_path\":\"agent-1\",\"status\":{\"completed\":\"审查完成\"}}\n</subagent_notification>"}]}}"#
                    .to_string(),
            ),
            (
                1,
                r#"{"timestamp":"2026-06-22T01:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"真实用户问题"}]}}"#
                    .to_string(),
            ),
        ];

        let messages = parse_codex_messages_newest_first("session-1", &lines);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, ToolSessionMessageRole::User);
        assert_eq!(messages[0].content, "真实用户问题");
    }

    #[test]
    fn codex_messages_skip_tool_calls_and_outputs_in_detail_messages() {
        let lines = vec![
            (
                0,
                r#"{"timestamp":"2026-06-22T01:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"请执行一个命令"}]}}"#
                    .to_string(),
            ),
            (
                1,
                r#"{"timestamp":"2026-06-22T01:00:01Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":"{\"cmd\":\"echo hello\"}"}}"#
                    .to_string(),
            ),
            (
                2,
                r#"{"timestamp":"2026-06-22T01:00:02Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"Chunk ID: f97341\nWall time: 0.0000 seconds\nProcess exited with code 0\nOriginal token count: 0\nOutput:\n"}}"#
                    .to_string(),
            ),
            (
                3,
                r#"{"timestamp":"2026-06-22T01:00:03Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"命令执行完成"}]}}"#
                    .to_string(),
            ),
        ];

        let messages = parse_codex_messages_newest_first("session-1", &lines);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, ToolSessionMessageRole::Assistant);
        assert_eq!(messages[0].content, "命令执行完成");
        assert_eq!(messages[1].role, ToolSessionMessageRole::User);
        assert_eq!(messages[1].content, "请执行一个命令");
        let joined = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!joined.contains("exec_command"));
        assert!(!joined.contains("Chunk ID: f97341"));
        assert!(!joined.contains("Wall time"));
        assert!(!joined.contains("Original token count"));
    }
}
