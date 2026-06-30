use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use chrono::{DateTime, Utc};
use niuma_core::hook_payload::claude_permission_request_id_from_tool_use;
use niuma_core::models::{
    CompletionReason, EventSessionScope, EventType, FailureReason, NiumaEvent, ToolKind,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Default)]
pub(crate) struct ClaudeJsonlParser {
    pending_tools: HashMap<String, PendingClaudeTool>,
    denied_hook_tool_uses: HashSet<String>,
}

#[derive(Clone)]
struct PendingClaudeTool {
    approval_resolve_key: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeRow {
    #[serde(rename = "type")]
    row_type: String,
    subtype: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    cwd: Option<String>,
    timestamp: Option<String>,
    #[serde(default)]
    message: Value,
    #[serde(default)]
    attachment: Value,
    #[serde(default)]
    error: Value,
    #[serde(rename = "toolUseResult", default)]
    tool_use_result: Value,
}

impl ClaudeRow {
    fn message_stop_reason(&self) -> Option<&str> {
        self.message
            .get("stop_reason")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
    }
}

struct ParsedClaudeEvent {
    event_type: EventType,
    summary: String,
    content: Option<String>,
    error_message: Option<String>,
    completion_reason: Option<CompletionReason>,
    failure_reason: Option<FailureReason>,
    tool_call_id: Option<String>,
    attention_resolve_key: Option<String>,
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
            session_scope: Some(if row.is_sidechain == Some(true) {
                EventSessionScope::Subagent
            } else {
                EventSessionScope::Main
            }),
            agent_nickname: None,
            agent_role: None,
            project_path,
            project_name,
            event_type: parsed.event_type,
            severity,
            summary: parsed.summary,
            content: parsed.content,
            error_message: parsed.error_message,
            attention_resolve_key: parsed.attention_resolve_key,
            completion_reason: parsed.completion_reason,
            failure_reason: parsed.failure_reason,
            tool_call_id: parsed.tool_call_id,
            payload_ref: Some(fallback_path.to_string()),
            interaction: None,
            created_at,
        }))
    }

    fn parse_event_shape(&mut self, row: &ClaudeRow) -> Option<ParsedClaudeEvent> {
        match row.row_type.as_str() {
            "attachment" => {
                self.record_hook_permission_decision(row);
                None
            }
            "user" => self.user_event(row),
            "assistant" => self.assistant_event(row),
            "system" if row.subtype.as_deref() == Some("api_error") => {
                Some(self.system_api_error_event(row))
            }
            "system" => None,
            _ => None,
        }
    }

    fn record_hook_permission_decision(&mut self, row: &ClaudeRow) {
        if row.attachment.get("type").and_then(Value::as_str) != Some("hook_permission_decision") {
            return;
        }
        if row.attachment.get("hookEvent").and_then(Value::as_str) != Some("PermissionRequest") {
            return;
        }
        if row.attachment.get("decision").and_then(Value::as_str) != Some("deny") {
            return;
        }
        if let Some(tool_use_id) = row
            .attachment
            .get("toolUseID")
            .and_then(Value::as_str)
            .and_then(trimmed_text)
        {
            // Claude 会把 Hook deny 继续写成 is_error=true 的 tool_result；该错误只是权限拒绝的回声。
            self.denied_hook_tool_uses.insert(tool_use_id);
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
                tool_call_id: None,
                attention_resolve_key: None,
                activity_key: row.timestamp.clone().unwrap_or_else(|| "user".to_string()),
            });
        }
        for item in content.as_array()? {
            if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let tool_use_id = item
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let pending_tool = self.pending_tools.remove(tool_use_id);
            let result = item
                .get("content")
                .and_then(text_from_value)
                .unwrap_or_else(|| "工具结果已返回".to_string());
            let is_error = item
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_error && self.denied_hook_tool_uses.remove(tool_use_id) {
                return None;
            }
            let (event_type, summary, error_message, failure_reason) = if is_error {
                if is_user_rejected_tool_use(row, &result) {
                    (
                        EventType::AssistantMessageCompleted,
                        "Claude Code 已中断：用户拒绝了工具授权".to_string(),
                        None,
                        None,
                    )
                } else {
                    (
                        EventType::SessionActivity,
                        format!("Claude Code 工具执行出错，任务可能仍在继续：{result}"),
                        None,
                        None,
                    )
                }
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
                completion_reason: if is_error && is_user_rejected_tool_use(row, &result) {
                    Some(CompletionReason::Interrupted)
                } else {
                    None
                },
                failure_reason,
                tool_call_id: trimmed_text(tool_use_id),
                attention_resolve_key: pending_tool.and_then(|tool| tool.approval_resolve_key),
                activity_key: tool_result_activity_key(row, tool_use_id),
            });
        }
        None
    }

    fn assistant_event(&mut self, row: &ClaudeRow) -> Option<ParsedClaudeEvent> {
        let content = row.message.get("content")?.as_array()?;
        if row.message_stop_reason() == Some("tool_use") {
            if let Some(tool_use) = content
                .iter()
                .find(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
            {
                return Some(self.assistant_tool_use_event(row, tool_use));
            }
        }
        for item in content {
            match item.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = item
                        .get("text")
                        .and_then(Value::as_str)
                        .and_then(trimmed_text)?;
                    return Some(match row.message_stop_reason() {
                        Some("end_turn") => ParsedClaudeEvent {
                            event_type: EventType::AssistantMessageCompleted,
                            summary: text.clone(),
                            content: Some(text),
                            error_message: None,
                            completion_reason: Some(CompletionReason::Normal),
                            failure_reason: None,
                            tool_call_id: None,
                            attention_resolve_key: None,
                            activity_key: activity_key(row, "assistant_text"),
                        },
                        Some("stop_sequence") => ParsedClaudeEvent {
                            event_type: EventType::TaskFailed,
                            summary: terminal_failure_summary(&text),
                            content: Some(text.clone()),
                            error_message: Some(text.clone()),
                            completion_reason: None,
                            failure_reason: Some(failure_reason_from_text(&text)),
                            tool_call_id: None,
                            attention_resolve_key: None,
                            activity_key: terminal_failure_activity_key(
                                row,
                                "stop_sequence",
                                &text,
                            ),
                        },
                        _ => ParsedClaudeEvent {
                            event_type: EventType::SessionActivity,
                            summary: format!("Claude Code 回复中，任务仍在继续：{text}"),
                            content: None,
                            error_message: None,
                            completion_reason: None,
                            failure_reason: None,
                            tool_call_id: None,
                            attention_resolve_key: None,
                            activity_key: activity_key(row, "assistant_text_activity"),
                        },
                    });
                }
                Some("tool_use") => {
                    return Some(self.assistant_tool_use_event(row, item));
                }
                Some("thinking") => {
                    return Some(ParsedClaudeEvent {
                        event_type: EventType::SessionActivity,
                        summary: "Claude Code 正在思考".to_string(),
                        content: None,
                        error_message: None,
                        completion_reason: None,
                        failure_reason: None,
                        tool_call_id: None,
                        attention_resolve_key: None,
                        activity_key: activity_key(row, "thinking"),
                    });
                }
                _ => {}
            }
        }
        None
    }

    fn assistant_tool_use_event(&mut self, row: &ClaudeRow, item: &Value) -> ParsedClaudeEvent {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("");
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("tool");
        if !id.is_empty() {
            self.pending_tools.insert(
                id.to_string(),
                PendingClaudeTool {
                    approval_resolve_key: claude_approval_resolve_key(row, name, item),
                },
            );
        }
        ParsedClaudeEvent {
            event_type: EventType::SessionActivity,
            summary: format!("Claude Code 正在调用工具：{name}"),
            content: None,
            error_message: None,
            completion_reason: None,
            failure_reason: None,
            tool_call_id: trimmed_text(id),
            attention_resolve_key: None,
            activity_key: activity_key(row, id),
        }
    }

    fn system_api_error_event(&mut self, row: &ClaudeRow) -> ParsedClaudeEvent {
        let error_message = row
            .error
            .get("formatted")
            .and_then(Value::as_str)
            .and_then(trimmed_text)
            .or_else(|| {
                row.error
                    .get("message")
                    .and_then(Value::as_str)
                    .and_then(trimmed_text)
            });
        let failure_reason = error_message
            .as_deref()
            .map(failure_reason_from_text)
            .unwrap_or(FailureReason::Unknown);

        ParsedClaudeEvent {
            event_type: EventType::TaskFailed,
            summary: "Claude Code API error".to_string(),
            content: None,
            activity_key: terminal_failure_activity_key(
                row,
                "api_error",
                error_message.as_deref().unwrap_or(""),
            ),
            error_message,
            completion_reason: None,
            failure_reason: Some(failure_reason),
            tool_call_id: None,
            attention_resolve_key: None,
        }
    }
}

fn claude_approval_resolve_key(row: &ClaudeRow, tool_name: &str, item: &Value) -> Option<String> {
    let session_id = row.session_id.as_deref()?.trim();
    if session_id.is_empty() {
        return None;
    }
    let tool_input = item.get("input").unwrap_or(&Value::Null);
    let request_id = claude_permission_request_id_from_tool_use(session_id, tool_name, tool_input);
    Some(format!("approval:{request_id}"))
}

fn is_user_rejected_tool_use(row: &ClaudeRow, result: &str) -> bool {
    row.tool_use_result.as_str() == Some("User rejected tool use")
        || result.contains("The user doesn't want to proceed with this tool use")
}

fn terminal_failure_summary(text: &str) -> String {
    if text
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("api error")
    {
        return "Claude Code API error".to_string();
    }
    format!("Claude Code 任务失败：{text}")
}

fn failure_reason_from_text(text: &str) -> FailureReason {
    let normalized = text.to_ascii_lowercase();
    if normalized.contains("429") || normalized.contains("service unavailable") {
        FailureReason::ServerOverloaded
    } else {
        FailureReason::Unknown
    }
}

fn terminal_failure_activity_key(row: &ClaudeRow, kind: &str, text: &str) -> String {
    match failure_reason_from_text(text) {
        FailureReason::ServerOverloaded => "terminal_failure:server_overloaded".to_string(),
        reason => {
            let timestamp = row
                .timestamp
                .as_deref()
                .and_then(trimmed_text)
                .unwrap_or_else(|| "no_timestamp".to_string());
            format!(
                "terminal_failure:{kind}:{timestamp}:{}:{:x}",
                failure_reason_key(&reason),
                stable_hash(text)
            )
        }
    }
}

fn failure_reason_key(reason: &FailureReason) -> &'static str {
    match reason {
        FailureReason::ServerOverloaded => "server_overloaded",
        FailureReason::Timeout => "timeout",
        FailureReason::ContextWindowExceeded => "context_window_exceeded",
        FailureReason::UsageLimitReached => "usage_limit_reached",
        FailureReason::PolicyBlocked => "policy_blocked",
        FailureReason::ResponseStreamFailed => "response_stream_failed",
        FailureReason::ConnectionFailed => "connection_failed",
        FailureReason::QuotaExceeded => "quota_exceeded",
        FailureReason::InternalServerError => "internal_server_error",
        FailureReason::RetryLimit => "retry_limit",
        FailureReason::SandboxError => "sandbox_error",
        FailureReason::Fatal => "fatal",
        FailureReason::Unknown => "unknown",
    }
}

fn tool_result_activity_key(row: &ClaudeRow, tool_use_id: &str) -> String {
    let id = tool_use_id.trim();
    match row.timestamp.as_deref().and_then(trimmed_text) {
        Some(timestamp) if !id.is_empty() => format!("{timestamp}:tool_result:{id}"),
        Some(timestamp) => format!("{timestamp}:tool_result"),
        None if !id.is_empty() => format!("tool_result:{id}"),
        None => "tool_result".to_string(),
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

fn trimmed_text(value: &str) -> Option<String> {
    let text = value.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn stable_hash(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
