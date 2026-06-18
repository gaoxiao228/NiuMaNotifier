use serde::Serialize;

use crate::models::{EventType, NiumaEvent, SessionStatus};

// 主状态 detail 和通知正文共用这里的展示字段规则，避免 content/summary fallback 分叉。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EventDisplayDetail {
    pub event_id: String,
    pub event_type: String,
    pub severity: String,
    pub summary: String,
    pub content: Option<String>,
    pub error_message: Option<String>,
    pub payload_ref: Option<String>,
    pub completion_reason: Option<String>,
    pub failure_reason: Option<String>,
}

pub(crate) fn detail_from_event(event: &NiumaEvent) -> EventDisplayDetail {
    EventDisplayDetail {
        event_id: event.id.clone(),
        event_type: event_type_name(&event.event_type).to_string(),
        severity: event.severity.clone(),
        summary: truncate(&event.summary, 200),
        content: fallback_content_for_event(event),
        error_message: fallback_error_for_event(event),
        payload_ref: event.payload_ref.clone(),
        completion_reason: event.completion_reason.as_ref().map(enum_to_snake_string),
        failure_reason: event.failure_reason.as_ref().map(enum_to_snake_string),
    }
}

pub(crate) fn fallback_content_for_status(
    status: &SessionStatus,
    content: Option<&str>,
    summary: &str,
) -> Option<String> {
    match status {
        SessionStatus::WaitingApproval | SessionStatus::WaitingInput | SessionStatus::Completed => {
            Some(truncate(
                content
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(summary),
                8000,
            ))
        }
        SessionStatus::Running => content
            .filter(|value| !value.trim().is_empty())
            .map(|value| truncate(value, 8000)),
        _ => None,
    }
}

pub(crate) fn fallback_error_for_status(
    status: &SessionStatus,
    error_message: Option<&str>,
    summary: &str,
) -> Option<String> {
    if *status == SessionStatus::Error {
        return Some(truncate(
            error_message
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(summary),
            4000,
        ));
    }
    None
}

pub(crate) fn status_for_event_type(event_type: &EventType) -> SessionStatus {
    match event_type {
        EventType::SessionStarted | EventType::SessionActivity => SessionStatus::Running,
        EventType::ApprovalRequested => SessionStatus::WaitingApproval,
        EventType::InputRequested => SessionStatus::WaitingInput,
        EventType::TaskFailed => SessionStatus::Error,
        EventType::AssistantMessageCompleted | EventType::ManualDismissed => {
            SessionStatus::Completed
        }
        EventType::SessionIdled => SessionStatus::Idle,
        EventType::SessionStaled => SessionStatus::Stale,
    }
}

pub(crate) fn event_type_name(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::SessionStarted => "session_started",
        EventType::SessionIdled => "session_idled",
        EventType::ApprovalRequested => "approval_requested",
        EventType::InputRequested => "input_requested",
        EventType::TaskFailed => "task_failed",
        EventType::AssistantMessageCompleted => "assistant_message_completed",
        EventType::ManualDismissed => "manual_dismissed",
        EventType::SessionStaled => "session_staled",
        EventType::SessionActivity => "session_activity",
    }
}

pub(crate) fn event_type_name_for_status(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::WaitingApproval => "approval_requested",
        SessionStatus::WaitingInput => "input_requested",
        SessionStatus::Running => "session_activity",
        SessionStatus::Completed => "assistant_message_completed",
        SessionStatus::Error => "task_failed",
        SessionStatus::Idle => "session_idled",
        SessionStatus::Stale => "session_staled",
    }
}

pub(crate) fn status_summary(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::WaitingApproval => "waiting approval",
        SessionStatus::WaitingInput => "waiting input",
        SessionStatus::Running => "running",
        SessionStatus::Completed => "completed",
        SessionStatus::Error => "error",
        SessionStatus::Idle => "idle",
        SessionStatus::Stale => "stale",
    }
}

fn fallback_content_for_event(event: &NiumaEvent) -> Option<String> {
    let fallback_status = status_for_event_type(&event.event_type);
    fallback_content_for_status(&fallback_status, event.content.as_deref(), &event.summary)
}

fn fallback_error_for_event(event: &NiumaEvent) -> Option<String> {
    let fallback_status = status_for_event_type(&event.event_type);
    fallback_error_for_status(
        &fallback_status,
        event.error_message.as_deref(),
        &event.summary,
    )
}

fn enum_to_snake_string<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_default()
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
