use chrono::{TimeZone, Utc};

use crate::hook_payload::{
    claude_permission_request_from_payload, codex_permission_request_from_payload,
    HookPayloadParser, HookToolHint,
};
use crate::models::{EventType, ToolKind};

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
fn ignores_codex_session_start() {
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
    .unwrap();

    assert!(event.is_none());
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
fn codex_permission_request_id_is_stable_for_same_tool_input() {
    let payload = r#"{
        "hook_event_name": "PermissionRequest",
        "session_id": "s1",
        "turn_id": "t1",
        "cwd": "/Users/me/Code/NiuMaNotifier",
        "tool_name": "Bash",
        "tool_input": { "command": "cargo test" }
    }"#;

    let first = codex_permission_request_from_payload(payload.as_bytes()).unwrap();
    let second = codex_permission_request_from_payload(payload.as_bytes()).unwrap();

    assert_eq!(first.id, second.id);
    assert!(first.id.starts_with("codex:s1:t1:Bash:"));
    assert_eq!(first.command.as_deref(), Some("cargo test"));
    assert_eq!(first.project_name, "NiuMaNotifier");
}

#[test]
fn ignores_codex_stop() {
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
    .unwrap();

    assert!(event.is_none());
}

#[test]
fn ignores_claude_pre_tool_use_as_permission_source() {
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
    .unwrap();

    assert!(event.is_none());
}

#[test]
fn parses_claude_permission_request_command() {
    let payload = r#"{
        "hook_event_name": "PermissionRequest",
        "session_id": "claude-s1",
        "transcript_path": "/Users/me/.claude/projects/demo/claude-s1.jsonl",
        "cwd": "/Users/me/Code/App",
        "permission_mode": "default",
        "tool_name": "Bash",
        "toolUseID": "call_123",
        "tool_input": { "command": "cargo test", "description": "运行测试" }
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
    assert_eq!(event.summary, "Bash: cargo test");
    assert!(event
        .dedupe_key
        .contains("claude_code:claude-s1:permission_request:permission_request"));
}

#[test]
fn claude_permission_request_id_is_stable_for_same_tool_input() {
    let payload = r#"{
        "hook_event_name": "PermissionRequest",
        "session_id": "claude-s1",
        "cwd": "/Users/me/Code/App",
        "permission_mode": "default",
        "tool_name": "Bash",
        "toolUseID": "call_123",
        "tool_input": { "command": "cargo test", "description": "运行测试" }
    }"#;

    let first = claude_permission_request_from_payload(payload.as_bytes()).unwrap();
    let second = claude_permission_request_from_payload(payload.as_bytes()).unwrap();

    assert_eq!(first.id, second.id);
    assert!(first
        .id
        .starts_with("claude_code:claude-s1:permission_request:Bash:"));
    assert_eq!(first.command.as_deref(), Some("cargo test"));
    assert_eq!(first.description.as_deref(), Some("运行测试"));
    assert_eq!(first.tool_call_id.as_deref(), Some("call_123"));
    assert_eq!(first.project_name, "App");
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
