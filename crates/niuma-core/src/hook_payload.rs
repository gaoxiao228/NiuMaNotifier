use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{CompletionReason, EventType, NiumaEvent, ToolKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookToolHint {
    Codex,
    ClaudeCode,
}

pub struct HookPayloadParser;

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
            (HookToolHint::Codex, "SessionStart") => Ok(Some(build_event(
                &payload,
                tool,
                EventType::SessionStarted,
                "info",
                "Codex session started".to_string(),
                now,
            ))),
            (HookToolHint::Codex, "PermissionRequest") => {
                let tool_name =
                    string_field(&payload, "tool_name").unwrap_or_else(|| "Tool".into());
                let command =
                    tool_input_summary(&payload).unwrap_or_else(|| "Codex 正在等待批准".into());
                Ok(Some(build_event(
                    &payload,
                    tool,
                    EventType::ApprovalRequested,
                    "urgent",
                    format!("{tool_name}: {command}"),
                    now,
                )))
            }
            (HookToolHint::Codex, "Stop") => {
                let summary = string_field(&payload, "last_assistant_message")
                    .unwrap_or_else(|| "Codex 有新回复".into());
                Ok(Some(build_completed_event(
                    &payload,
                    tool,
                    truncate(&summary, 1_200),
                    now,
                )))
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

fn project_name_from_path(path: &str) -> Option<String> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn tool_key(tool: &ToolKind) -> &'static str {
    match tool {
        ToolKind::Codex => "codex",
        ToolKind::ClaudeCode => "claude_code",
    }
}

fn event_type_key(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::SessionStarted => "session_started",
        EventType::SessionIdled => "session_idled",
        EventType::ApprovalRequested => "permission_request",
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

#[cfg(test)]
mod tests;
