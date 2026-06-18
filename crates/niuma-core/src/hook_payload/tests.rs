use chrono::{TimeZone, Utc};

use crate::hook_payload::{HookPayloadParser, HookToolHint};
use crate::models::{CompletionReason, EventType, ToolKind};

#[test]
fn niuma_event_deserializes_without_optional_reason_fields() {
    let payload = r#"{
        "id":"event-old",
        "dedupe_key":"dedupe-old",
        "source":"test",
        "tool":"codex",
        "session_id":"s1",
        "project_path":"/tmp/demo",
        "project_name":"demo",
        "event_type":"assistant_message_completed",
        "severity":"info",
        "summary":"done",
        "payload_ref":null,
        "created_at":"1970-01-01T00:00:00Z"
    }"#;

    let event = serde_json::from_str::<crate::models::NiumaEvent>(payload).unwrap();

    assert_eq!(event.completion_reason, None);
    assert_eq!(event.failure_reason, None);
}

#[test]
fn parses_codex_session_start() {
    let payload = r#"{
        "hook_event_name": "SessionStart",
        "session_id": "s1",
        "cwd": "/Users/me/Code/NiuMaNotifier"
    }"#;

    let event = HookPayloadParser::parse(
        payload.as_bytes(),
        HookToolHint::Codex,
        Utc.timestamp_opt(1_000, 0).single().unwrap(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(event.tool, ToolKind::Codex);
    assert_eq!(event.session_id, "s1");
    assert_eq!(event.project_name, "NiuMaNotifier");
    assert_eq!(event.event_type, EventType::SessionStarted);
    assert_eq!(event.summary, "Codex session started");
}

#[test]
fn parses_codex_permission_request_command() {
    let payload = r#"{
        "hook_event_name": "PermissionRequest",
        "session_id": "s1",
        "turn_id": "t1",
        "cwd": "/Users/me/Code/NiuMaNotifier",
        "tool_name": "Bash",
        "tool_input": { "command": "cargo test" }
    }"#;

    let event = HookPayloadParser::parse(
        payload.as_bytes(),
        HookToolHint::Codex,
        Utc.timestamp_opt(1_000, 0).single().unwrap(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(event.event_type, EventType::ApprovalRequested);
    assert_eq!(event.severity, "urgent");
    assert_eq!(event.summary, "Bash: cargo test");
    assert!(event.dedupe_key.contains("codex:s1:t1:permission_request"));
}

#[test]
fn parses_codex_stop_as_completed() {
    let payload = r#"{
        "hook_event_name": "Stop",
        "session_id": "s1",
        "turn_id": "t2",
        "cwd": "/tmp/demo",
        "last_assistant_message": "已完成"
    }"#;

    let event = HookPayloadParser::parse(
        payload.as_bytes(),
        HookToolHint::Codex,
        Utc.timestamp_opt(1_000, 0).single().unwrap(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.completion_reason, Some(CompletionReason::Normal));
    assert_eq!(event.failure_reason, None);
    assert_eq!(event.summary, "已完成");
}

#[test]
fn parses_claude_pre_tool_use_as_approval() {
    let payload = r#"{
        "hook_event_name": "PreToolUse",
        "session_id": "claude-s1",
        "cwd": "/Users/me/Code/App",
        "tool_name": "Bash",
        "tool_input": { "description": "运行构建" }
    }"#;

    let event = HookPayloadParser::parse(
        payload.as_bytes(),
        HookToolHint::ClaudeCode,
        Utc.timestamp_opt(1_000, 0).single().unwrap(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(event.tool, ToolKind::ClaudeCode);
    assert_eq!(event.event_type, EventType::ApprovalRequested);
    assert_eq!(event.summary, "Bash: 运行构建");
}

#[test]
fn ignores_unknown_event() {
    let payload = r#"{
        "hook_event_name": "Other",
        "session_id": "s1"
    }"#;

    let event = HookPayloadParser::parse(
        payload.as_bytes(),
        HookToolHint::Codex,
        Utc.timestamp_opt(1_000, 0).single().unwrap(),
    )
    .unwrap();

    assert!(event.is_none());
}
