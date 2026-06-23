use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{CompletionReason, EventType, NiumaEvent, ToolKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookToolHint {
    Codex,
    ClaudeCode,
}

pub struct HookPayloadParser;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexPermissionRequest {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub tool_name: String,
    pub command: Option<String>,
    pub description: Option<String>,
    pub project_path: String,
    pub project_name: String,
}

impl HookPayloadParser {
    pub fn parse(
        data: &[u8],
        tool_hint: HookToolHint,
        now: DateTime<Utc>,
    ) -> Result<Option<NiumaEvent>, serde_json::Error> {
        let payload: Value = serde_json::from_slice(data)?;
        let event_name = string_field(&payload, "hook_event_name").unwrap_or_default();
        let tool = match tool_hint {
            HookToolHint::Codex => ToolKind::Codex,
            HookToolHint::ClaudeCode => ToolKind::ClaudeCode,
        };

        match (tool_hint, event_name.as_str()) {
            (HookToolHint::Codex, "PermissionRequest") => {
                let tool_name =
                    string_field(&payload, "tool_name").unwrap_or_else(|| "Tool".into());
                let command =
                    tool_input_summary(&payload).unwrap_or_else(|| "Codex 正在等待批准".into());
                let mut event = build_event(
                    &payload,
                    tool,
                    EventType::ApprovalRequested,
                    "urgent",
                    format!("{tool_name}: {command}"),
                    now,
                );
                if let Ok(request) = codex_permission_request_from_value(&payload) {
                    let approval_ref = format!("approval:{}", request.id);
                    event.payload_ref = Some(approval_ref.clone());
                    event.attention_resolve_key = Some(approval_ref);
                }
                Ok(Some(event))
            }
            (HookToolHint::ClaudeCode, "PreToolUse") => {
                let tool_name =
                    string_field(&payload, "tool_name").unwrap_or_else(|| "Tool".into());
                let command = tool_input_summary(&payload)
                    .unwrap_or_else(|| "Claude Code 正在等待批准".into());
                Ok(Some(build_event(
                    &payload,
                    tool,
                    EventType::ApprovalRequested,
                    "urgent",
                    format!("{tool_name}: {command}"),
                    now,
                )))
            }
            (HookToolHint::ClaudeCode, "Stop") => {
                let summary = string_field(&payload, "last_assistant_message")
                    .unwrap_or_else(|| "Claude Code 有新回复".into());
                Ok(Some(build_completed_event(
                    &payload,
                    tool,
                    truncate(&summary, 1_200),
                    now,
                )))
            }
            _ => Ok(None),
        }
    }
}

pub fn codex_permission_request_from_payload(
    data: &[u8],
) -> Result<CodexPermissionRequest, serde_json::Error> {
    let payload: Value = serde_json::from_slice(data)?;
    Ok(codex_permission_request_from_value(&payload)
        .unwrap_or_else(|_| codex_permission_request_from_value_lossy(&payload)))
}

fn codex_permission_request_from_value(payload: &Value) -> Result<CodexPermissionRequest, String> {
    let event_name = string_field(payload, "hook_event_name").unwrap_or_default();
    if event_name != "PermissionRequest" {
        return Err("不是 Codex PermissionRequest payload".to_string());
    }
    Ok(codex_permission_request_from_value_lossy(payload))
}

fn codex_permission_request_from_value_lossy(payload: &Value) -> CodexPermissionRequest {
    let session_id =
        string_field(payload, "session_id").unwrap_or_else(|| "unknown-session".into());
    let turn_id = string_field(payload, "turn_id").unwrap_or_else(|| "unknown-turn".into());
    let tool_name = string_field(payload, "tool_name").unwrap_or_else(|| "Tool".into());
    let project_path = string_field(payload, "cwd").unwrap_or_default();
    let project_name =
        project_name_from_path(&project_path).unwrap_or_else(|| "unknown-project".into());
    let tool_input = payload.get("tool_input").unwrap_or(&Value::Null);
    let tool_input_hash = stable_hash(&serde_json::to_string(tool_input).unwrap_or_default());
    // request id 必须跨 hook 重试稳定，避免同一次 Codex 授权请求创建多个待处理项。
    let id = format!(
        "codex:{}:{}:{}:{}",
        stable_id_part(&session_id),
        stable_id_part(&turn_id),
        stable_id_part(&tool_name),
        tool_input_hash
    );

    CodexPermissionRequest {
        id,
        session_id,
        turn_id,
        tool_name,
        command: tool_input_string(tool_input, "command")
            .or_else(|| tool_input_string(tool_input, "cmd")),
        description: tool_input_string(tool_input, "description"),
        project_path,
        project_name,
    }
}

fn build_completed_event(
    payload: &Value,
    tool: ToolKind,
    summary: String,
    now: DateTime<Utc>,
) -> NiumaEvent {
    let mut event = build_event(
        payload,
        tool,
        EventType::AssistantMessageCompleted,
        "info",
        summary,
        now,
    );
    event.completion_reason = Some(CompletionReason::Normal);
    event.content = Some(event.summary.clone());
    event
}

fn build_event(
    payload: &Value,
    tool: ToolKind,
    event_type: EventType,
    severity: &str,
    summary: String,
    now: DateTime<Utc>,
) -> NiumaEvent {
    let session_id =
        string_field(payload, "session_id").unwrap_or_else(|| "unknown-session".into());
    let turn_id = string_field(payload, "turn_id").unwrap_or_else(|| "unknown-turn".into());
    let project_path = string_field(payload, "cwd").unwrap_or_default();
    let project_name =
        project_name_from_path(&project_path).unwrap_or_else(|| "unknown-project".into());
    let event_key = event_type_key(&event_type);
    let tool_key = tool_key(&tool);
    let summary_hash = stable_hash(&summary);
    // dedupe_key 借鉴 OSXpush 的稳定 ID 思路，避免 hook 重试造成重复事件。
    let dedupe_key = format!("{tool_key}:{session_id}:{turn_id}:{event_key}:{summary_hash}");

    NiumaEvent {
        id: format!("event_{summary_hash}"),
        dedupe_key,
        source: "hook_helper".to_string(),
        tool,
        session_id,
        parent_session_id: None,
        normalized_session_id: None,
        session_scope: None,
        agent_nickname: None,
        agent_role: None,
        project_path,
        project_name,
        event_type,
        severity: severity.to_string(),
        summary,
        content: None,
        error_message: None,
        attention_resolve_key: None,
        completion_reason: None,
        failure_reason: None,
        payload_ref: None,
        created_at: now,
    }
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    match payload.get(key) {
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn tool_input_summary(payload: &Value) -> Option<String> {
    let tool_input = payload.get("tool_input")?;
    for key in ["command", "cmd", "description"] {
        if let Some(Value::String(value)) = tool_input.get(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(truncate(trimmed, 1_200));
            }
        }
    }
    None
}

fn tool_input_string(tool_input: &Value, key: &str) -> Option<String> {
    match tool_input.get(key) {
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn project_name_from_path(path: &str) -> Option<String> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn tool_key(tool: &ToolKind) -> &str {
    match tool {
        ToolKind::Codex => "codex",
        ToolKind::ClaudeCode => "claude_code",
        ToolKind::Custom(value) => value.as_str(),
    }
}

fn event_type_key(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::SessionStarted => "session_started",
        EventType::SessionIdled => "session_idled",
        EventType::ApprovalRequested => "permission_request",
        EventType::ApprovalResolved => "approval_resolved",
        EventType::ApprovalReturnedToCodex => "approval_returned_to_codex",
        EventType::InputRequested => "input_requested",
        EventType::TaskFailed => "task_failed",
        EventType::AssistantMessageCompleted => "assistant_message_completed",
        EventType::ManualDismissed => "manual_dismissed",
        EventType::SessionStaled => "session_staled",
        EventType::SessionActivity => "session_activity",
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let shortened = text.chars().take(limit).collect::<String>();
    format!("{shortened}\n...")
}

fn stable_hash(text: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:x}")
}

fn stable_id_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
