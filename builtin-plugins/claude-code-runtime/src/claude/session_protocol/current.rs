use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use chrono::{DateTime, Utc};
use niuma_core::models::{
    CompletionReason, EventSessionScope, EventType, FailureReason, NiumaEvent, ToolKind,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Default)]
pub(crate) struct ClaudeJsonlParser {
    pending_tools: HashMap<String, String>,
}

#[derive(Deserialize)]
struct ClaudeRow {
    #[serde(rename = "type")]
    row_type: String,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    #[serde(default)]
    message: Value,
}

struct ParsedClaudeEvent {
    event_type: EventType,
    summary: String,
    content: Option<String>,
    error_message: Option<String>,
    completion_reason: Option<CompletionReason>,
    failure_reason: Option<FailureReason>,
    activity_key: String,
}

impl ClaudeJsonlParser {
    pub(crate) fn parse_line(
        &mut self,
        line: &str,
        fallback_path: &str,
    ) -> Result<Option<NiumaEvent>, String> {
        let row: ClaudeRow = serde_json::from_str(line)
            .map_err(|error| format!("解析 Claude Code JSONL 失败：{error}"))?;
        let Some(session_id) = row
            .session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };
        let Some(parsed) = self.parse_event_shape(&row) else {
            return Ok(None);
        };

        let created_at = parse_timestamp(row.timestamp.as_deref()).unwrap_or_else(Utc::now);
        let project_path = row.cwd.clone().unwrap_or_default();
        let project_name = project_name_for_path(&project_path);
        let dedupe_key = format!(
            "claude_code_file:{session_id}:{}:{}",
            parsed.activity_key,
            event_type_key(&parsed.event_type)
        );
        // 事件 ID 从去重键派生，避免同一毫秒内多行事件生成相同 ID。
        let event_id = format!("event_claude_code_file_{}", stable_hash(&dedupe_key));
        let severity = severity_for_event_type(&parsed.event_type).to_string();

        Ok(Some(NiumaEvent {
            id: event_id,
            dedupe_key,
            source: "claude-code-session-file".to_string(),
            tool: ToolKind::ClaudeCode,
            session_id: session_id.to_string(),
            parent_session_id: None,
            normalized_session_id: Some(session_id.to_string()),
            session_scope: Some(EventSessionScope::Main),
            agent_nickname: None,
            agent_role: None,
            project_path,
            project_name,
            event_type: parsed.event_type,
            severity,
            summary: parsed.summary,
            content: parsed.content,
            error_message: parsed.error_message,
            attention_resolve_key: None,
            completion_reason: parsed.completion_reason,
            failure_reason: parsed.failure_reason,
            payload_ref: Some(fallback_path.to_string()),
            interaction: None,
            created_at,
        }))
    }

    fn parse_event_shape(&mut self, row: &ClaudeRow) -> Option<ParsedClaudeEvent> {
        match row.row_type.as_str() {
            "user" => self.user_event(row),
            "assistant" => self.assistant_event(row),
            _ => None,
        }
    }

    fn user_event(&mut self, row: &ClaudeRow) -> Option<ParsedClaudeEvent> {
        let content = row.message.get("content")?;
        if let Some(text) = content.as_str().and_then(trimmed_text) {
            return Some(ParsedClaudeEvent {
                event_type: EventType::SessionStarted,
                summary: text.clone(),
                content: Some(text),
                error_message: None,
                completion_reason: None,
                failure_reason: None,
                activity_key: row.timestamp.clone().unwrap_or_else(|| "user".to_string()),
            });
        }
        for item in content.as_array()? {
            if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let tool_use_id = item.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
            self.pending_tools.remove(tool_use_id);
            let result = item
                .get("content")
                .and_then(text_from_value)
                .unwrap_or_else(|| "工具结果已返回".to_string());
            let is_error = item
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let (event_type, summary, error_message, failure_reason) = if is_error {
                (
                    EventType::TaskFailed,
                    format!("Claude Code 工具执行失败：{result}"),
                    Some(result.clone()),
                    Some(FailureReason::Unknown),
                )
            } else {
                (
                    EventType::SessionActivity,
                    format!("Claude Code 工具执行完成：{result}"),
                    None,
                    None,
                )
            };
            return Some(ParsedClaudeEvent {
                event_type,
                summary,
                content: None,
                error_message,
                completion_reason: None,
                failure_reason,
                activity_key: activity_key(row, tool_use_id),
            });
        }
        None
    }

    fn assistant_event(&mut self, row: &ClaudeRow) -> Option<ParsedClaudeEvent> {
        for item in row.message.get("content")?.as_array()? {
            match item.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = item.get("text").and_then(Value::as_str).and_then(trimmed_text)?;
                    return Some(ParsedClaudeEvent {
                        event_type: EventType::AssistantMessageCompleted,
                        summary: text.clone(),
                        content: Some(text),
                        error_message: None,
                        completion_reason: Some(CompletionReason::Normal),
                        failure_reason: None,
                        activity_key: activity_key(row, "assistant_text"),
                    });
                }
                Some("tool_use") => {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("tool");
                    if !id.is_empty() {
                        self.pending_tools.insert(id.to_string(), name.to_string());
                    }
                    return Some(ParsedClaudeEvent {
                        event_type: EventType::SessionActivity,
                        summary: format!("Claude Code 正在调用工具：{name}"),
                        content: None,
                        error_message: None,
                        completion_reason: None,
                        failure_reason: None,
                        activity_key: activity_key(row, id),
                    });
                }
                Some("thinking") => {
                    return Some(ParsedClaudeEvent {
                        event_type: EventType::SessionActivity,
                        summary: "Claude Code 正在思考".to_string(),
                        content: None,
                        error_message: None,
                        completion_reason: None,
                        failure_reason: None,
                        activity_key: activity_key(row, "thinking"),
                    });
                }
                _ => {}
            }
        }
        None
    }
}

fn activity_key(row: &ClaudeRow, fallback: &str) -> String {
    row.timestamp
        .clone()
        .or_else(|| (!fallback.is_empty()).then(|| fallback.to_string()))
        .unwrap_or_else(|| "event".to_string())
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn project_name_for_path(path: &str) -> String {
    path.rsplit('/')
        .find(|value| !value.is_empty())
        .unwrap_or("Claude Code")
        .to_string()
}

fn event_type_key(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::SessionStarted => "session_started",
        EventType::TaskFailed => "task_failed",
        EventType::AssistantMessageCompleted => "assistant_message_completed",
        EventType::SessionActivity => "session_activity",
        _ => "event",
    }
}

fn severity_for_event_type(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::TaskFailed => "error",
        EventType::AssistantMessageCompleted => "info",
        EventType::SessionStarted | EventType::SessionActivity => "info",
        _ => "info",
    }
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
                        .or_else(|| item.get("text").and_then(Value::as_str).and_then(trimmed_text))
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

fn trimmed_text(value: &str) -> Option<String> {
    let text = value.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn stable_hash(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
