use super::*;
use crate::codex::session_protocol::{detect_session_protocol_family, CodexProtocolFamily};
use niuma_core::models::{CompletionReason, EventType, FailureReason};
use std::io::Write;

#[test]
fn protocol_detector_recognizes_current_session_meta_shape() {
    let family = detect_session_protocol_family(
        r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
    )
    .unwrap();

    assert_eq!(family, CodexProtocolFamily::Current);
}

#[test]
fn protocol_detector_recognizes_current_event_message_shape() {
    let family = detect_session_protocol_family(
        r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#,
    )
    .unwrap();

    assert_eq!(family, CodexProtocolFamily::Current);
}

#[test]
fn protocol_detector_marks_unknown_session_row_as_unsupported() {
    let family = detect_session_protocol_family(
        r#"{"type":"future_msg","payload":{"kind":"started","session":"s1"}}"#,
    )
    .unwrap();

    assert_eq!(family, CodexProtocolFamily::Unsupported);
}

fn json_line(row_type: &str, payload: serde_json::Value) -> String {
    serde_json::json!({
        "timestamp": "1970-01-01T00:00:00Z",
        "type": row_type,
        "payload": payload
    })
    .to_string()
}

#[test]
fn parses_task_started_into_running_event() {
    let mut parser = CodexJsonlParser::default();
    let meta = r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#;
    let row = r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#;

    assert!(parser.parse_line(meta, "rollout.jsonl").unwrap().is_none());
    let event = parser.parse_line(row, "rollout.jsonl").unwrap().unwrap();

    assert_eq!(event.session_id, "session-123");
    assert_eq!(event.project_path, "/tmp/demo");
    assert_eq!(event.project_name, "demo");
    assert_eq!(event.event_type, EventType::SessionStarted);
    assert_eq!(
        event.dedupe_key,
        "codex_file:session-123:turn-1:task_started"
    );
}

#[test]
fn ignores_task_complete_without_agent_message() {
    let mut parser = CodexJsonlParser::default();
    let line = json_line(
        "event_msg",
        serde_json::json!({
            "type": "task_complete",
            "last_agent_message": null
        }),
    );

    let event = parser.parse_line(&line, "/tmp/session.jsonl").unwrap();

    assert!(
        event.is_none(),
        "没有 assistant 输出的 task_complete 不应生成完成事件"
    );
}

#[test]
fn task_complete_content_uses_last_agent_message() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"最终回答正文"}}"#,
            "/tmp/session.jsonl",
        )
        .unwrap()
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}"#,
            "/tmp/session.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.content.as_deref(), Some("最终回答正文"));
    assert_eq!(event.summary, "最终回答正文");
}

#[test]
fn task_complete_uses_payload_last_agent_message() {
    let mut parser = CodexJsonlParser::default();
    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1","last_agent_message":"payload 中的最终回答"}}"#,
            "/tmp/session.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.content.as_deref(), Some("payload 中的最终回答"));
    assert_eq!(event.completion_reason, Some(CompletionReason::Normal));
}

#[test]
fn user_response_item_message_is_not_cached_as_assistant_message() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"你好"}]}}"#,
            "/tmp/session.jsonl",
        )
        .unwrap()
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1","last_agent_message":null}}"#,
            "/tmp/session.jsonl",
        )
        .unwrap();

    assert!(event.is_none(), "用户消息不能被当成 assistant 完成正文");
}

#[test]
fn assistant_response_item_message_is_cached_for_task_complete() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"你好，我是回答"}]}}"#,
            "/tmp/session.jsonl",
        )
        .unwrap()
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1","last_agent_message":null}}"#,
            "/tmp/session.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.content.as_deref(), Some("你好，我是回答"));
}

#[test]
fn task_complete_uses_latest_agent_message_as_summary() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();
    parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#,
            "rollout.jsonl",
        )
        .unwrap();
    parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"已完成：修复通知历史展示。"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.completion_reason, Some(CompletionReason::Normal));
    assert_eq!(event.summary, "已完成：修复通知历史展示。");
}

#[test]
fn task_started_clears_previous_agent_message_summary() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();
    parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#,
            "rollout.jsonl",
        )
        .unwrap();
    parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"上一轮完成内容"}}"#,
            "rollout.jsonl",
        )
        .unwrap();
    parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-2"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-2"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    assert!(event.is_none(), "新任务不能串用上一轮 assistant 消息");
}

#[test]
fn parses_interrupted_turn_aborted_as_completed() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-1","reason":"interrupted"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.severity, "info");
    assert_eq!(event.completion_reason, Some(CompletionReason::Interrupted));
    assert_eq!(event.failure_reason, None);
}

#[test]
fn parses_thread_rolled_back_as_completed() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"thread_rolled_back","num_turns":1}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.severity, "info");
    assert_eq!(event.completion_reason, Some(CompletionReason::RolledBack));
    assert_eq!(event.failure_reason, None);
}

#[test]
fn parses_known_abort_reason_as_error() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-1","reason":"request_timeout"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::TaskFailed);
    assert_eq!(event.severity, "urgent");
    assert_eq!(event.completion_reason, None);
    assert_eq!(event.failure_reason, Some(FailureReason::Timeout));
}

#[test]
fn parses_context_window_abort_as_task_failed_reason() {
    let mut parser = CodexJsonlParser::default();
    let line = json_line(
        "event_msg",
        serde_json::json!({
            "type": "turn_aborted",
            "reason": "context_window_exceeded"
        }),
    );

    let event = parser
        .parse_line(&line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::TaskFailed);
    assert_eq!(event.completion_reason, None);
    assert_eq!(
        event.failure_reason,
        Some(FailureReason::ContextWindowExceeded)
    );
}

#[test]
fn parses_event_message_activity_as_internal_session_activity() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"timestamp":"2026-06-12T01:33:06.170Z","type":"event_msg","payload":{"type":"agent_message","message":"working"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::SessionActivity);
    assert_eq!(
        event.dedupe_key,
        "codex_file:session-123:2026-06-12T01:33:06.170Z:agent_message"
    );
    assert_eq!(
        event.created_at,
        chrono::DateTime::parse_from_rfc3339("2026-06-12T01:33:06.170Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    );
}

#[test]
fn parses_response_item_tool_call_as_internal_session_activity() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::SessionActivity);
    assert_eq!(
        event.dedupe_key,
        "codex_file:session-123:call-1:function_call"
    );
}

#[test]
fn parses_request_user_input_function_call_as_input_requested() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"timestamp":"2026-06-14T02:28:04.241Z","type":"response_item","payload":{"type":"function_call","name":"request_user_input","arguments":"{\"questions\":[{\"id\":\"app_form\",\"header\":\"形态\",\"question\":\"这个红绿灯程序你更希望主要以什么形态运行？\",\"options\":[{\"label\":\"托盘常驻 (Recommended)\",\"description\":\"跨平台常驻后台，托盘图标/小窗口显示状态，最适合长期监控。\"}]}]}","call_id":"call-input-1"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::InputRequested);
    assert_eq!(event.severity, "urgent");
    assert_eq!(
        event.summary,
        "Codex 等待输入：这个红绿灯程序你更希望主要以什么形态运行？"
    );
    assert_eq!(
        event.content.as_deref(),
        Some(
            "这个红绿灯程序你更希望主要以什么形态运行？\n\n1. 托盘常驻 (Recommended)\n跨平台常驻后台，托盘图标/小窗口显示状态，最适合长期监控。"
        )
    );
    assert_eq!(event.attention_resolve_key, None);
    assert_eq!(
        event.dedupe_key,
        "codex_file:session-123:call-input-1:function_call"
    );
}

#[test]
fn parses_plan_item_completed_as_input_requested() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r##"{"timestamp":"2026-06-14T07:13:24.848Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"turn-plan-1","item":{"type":"Plan","id":"plan-1","text":"# 红绿灯悬浮模块规划\n\n**Summary**\n- 新增一个独立 Tauri 悬浮小窗口。"}}}"##,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::InputRequested);
    assert_eq!(event.severity, "urgent");
    assert_eq!(event.summary, "Codex 等待确认：Implement this plan?");
    assert_eq!(
        event.content.as_deref(),
        Some(
            "Implement this plan?\n\n# 红绿灯悬浮模块规划\n\n**Summary**\n- 新增一个独立 Tauri 悬浮小窗口。\n\n1. Yes, implement this plan\n2. Yes, clear context and implement\n3. No, stay in Plan mode"
        )
    );
    assert_eq!(
        event.dedupe_key,
        "codex_file:session-123:turn-plan-1:item_completed"
    );
}

#[test]
fn plan_confirmation_task_complete_does_not_clear_waiting_input() {
    use niuma_core::listener_config::ListenerConfig;
    use niuma_core::main_state::{MainStateService, MainStateStatus};
    use niuma_core::store::SqliteStateStore;

    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let input = parser
        .parse_line(
            r##"{"timestamp":"2026-06-14T07:13:24.848Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"turn-plan-1","item":{"type":"Plan","id":"plan-1","text":"# 红绿灯悬浮模块规划"}}}"##,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();
    let plan_message = parser
        .parse_line(
            r##"{"timestamp":"2026-06-14T07:13:25.154Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"<proposed_plan>\n# 红绿灯悬浮模块规划\n</proposed_plan>"}],"phase":"final_answer"}}"##,
            "rollout.jsonl",
        )
        .unwrap();
    let token_count = parser
        .parse_line(
            r#"{"timestamp":"2026-06-14T07:13:25.208Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":100}}}}"#,
            "rollout.jsonl",
        )
        .unwrap();
    let complete = parser
        .parse_line(
            r#"{"timestamp":"2026-06-14T07:13:25.262Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan-1","last_agent_message":null}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    assert_eq!(input.event_type, EventType::InputRequested);
    assert!(
        plan_message.is_none(),
        "Plan Mode 的 assistant message 只是计划正文，不能作为运行中活动清掉等待确认状态"
    );
    assert!(
        token_count.is_none(),
        "Plan Mode 的 token_count 是计划输出后的遥测，不能作为运行中活动清掉等待确认状态"
    );
    assert!(
        complete.is_none(),
        "Plan Mode 的 task_complete 只表示模型输出结束，不能清掉 Implement this plan? 确认菜单"
    );

    let temp = tempfile::tempdir().unwrap();
    let store = SqliteStateStore::new(temp.path().join("state.sqlite"));
    store
        .save_listener_config(&ListenerConfig {
            codex_listening_enabled: true,
            ..ListenerConfig::default()
        })
        .unwrap();
    store.append_event(input).unwrap();
    let state = MainStateService::new(store)
        .current_state(chrono::TimeZone::timestamp_opt(&chrono::Utc, 1_000, 0).unwrap())
        .unwrap();

    assert_eq!(state.status, MainStateStatus::WaitingInput);
}

#[test]
fn ignores_non_plan_item_completed() {
    let mut parser = CodexJsonlParser::default();
    let event = parser
        .parse_line(
            r#"{"timestamp":"2026-06-14T07:13:24.848Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"Todo","id":"todo-1","text":"任务完成"}}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    assert!(event.is_none());
}

#[test]
fn ignores_plan_item_completed_without_text() {
    let mut parser = CodexJsonlParser::default();
    let event = parser
        .parse_line(
            r#"{"timestamp":"2026-06-14T07:13:24.848Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"Plan","id":"plan-1","text":"   "}}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    assert!(event.is_none());
}

#[test]
fn parses_escalated_function_call_as_approval_and_resolves_on_matching_output() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let approval = parser
        .parse_line(
            r#"{"timestamp":"2026-06-12T12:34:20.946Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"/bin/echo niuma-permission-probe\",\"sandbox_permissions\":\"require_escalated\",\"justification\":\"是否允许执行这个无副作用的 echo 命令？\"}","call_id":"call-approval-1"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(approval.event_type, EventType::ApprovalRequested);
    assert_eq!(approval.severity, "urgent");
    assert_eq!(
        approval.summary,
        "exec_command: 是否允许执行这个无副作用的 echo 命令？"
    );
    assert_eq!(
        approval.attention_resolve_key.as_deref(),
        Some("codex_permission:session-123:call-approval-1")
    );

    let resolved = parser
        .parse_line(
            r#"{"timestamp":"2026-06-12T12:34:27.341Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-approval-1","output":"Process exited with code 0"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(resolved.event_type, EventType::SessionActivity);
    assert_eq!(
        resolved.attention_resolve_key.as_deref(),
        Some("codex_permission:session-123:call-approval-1")
    );
    assert_ne!(approval.id, resolved.id);
}

#[test]
fn resolves_permission_when_output_is_seen_without_cached_approval() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let resolved = parser
        .parse_line(
            r#"{"timestamp":"2026-06-12T12:34:27.341Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-approval-1","output":"Process exited with code 0"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(resolved.event_type, EventType::SessionActivity);
    assert_eq!(
        resolved.attention_resolve_key.as_deref(),
        Some("codex_permission:session-123:call-approval-1")
    );
}

#[test]
fn event_id_is_stable_unique_and_dedupe_key_format_is_unchanged() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let started = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();
    let completed = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1","last_agent_message":"完成"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_ne!(started.id, completed.id);
    assert_eq!(
        started.dedupe_key,
        "codex_file:session-123:turn-1:task_started"
    );
    assert_eq!(
        completed.dedupe_key,
        "codex_file:session-123:turn-1:task_complete"
    );
}

#[test]
fn fallback_session_id_uses_full_path_for_same_basename() {
    let row = r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#;
    let first = CodexJsonlParser::default()
        .parse_line(row, "/tmp/one/rollout.jsonl")
        .unwrap()
        .unwrap();
    let second = CodexJsonlParser::default()
        .parse_line(row, "/tmp/two/rollout.jsonl")
        .unwrap()
        .unwrap();

    assert_ne!(first.session_id, second.session_id);
    assert!(first.session_id.starts_with("fallback-rollout-"));
    assert!(second.session_id.starts_with("fallback-rollout-"));
}

#[test]
fn fallback_session_id_extracts_uuid_from_rollout_filename() {
    let row = r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#;
    let event = CodexJsonlParser::default()
        .parse_line(
            row,
            "/tmp/rollout-2026-06-11T13-58-25-019eb542-a886-72e0-86fd-e5730054991c.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.session_id, "019eb542-a886-72e0-86fd-e5730054991c");
}

#[test]
fn incomplete_session_meta_keeps_existing_parser_state() {
    let mut parser = CodexJsonlParser::default();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"id":"session-123","cwd":"/tmp/demo"}}"#,
            "rollout.jsonl",
        )
        .unwrap();
    parser
        .parse_line(
            r#"{"type":"session_meta","payload":{"cwd":""}}"#,
            "rollout.jsonl",
        )
        .unwrap();

    let event = parser
        .parse_line(
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#,
            "rollout.jsonl",
        )
        .unwrap()
        .unwrap();

    assert_eq!(event.session_id, "session-123");
    assert_eq!(event.project_path, "/tmp/demo");
}

#[test]
fn invalid_json_returns_error() {
    let error = CodexJsonlParser::default()
        .parse_line("{not-json", "rollout.jsonl")
        .unwrap_err();

    assert!(error.contains("解析 Codex JSONL 失败"));
}

#[test]
fn scanner_reads_only_new_complete_lines() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-123\",\"cwd\":\"/tmp/demo\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}\n"
        ),
    )
    .unwrap();

    let mut scanner = CodexSessionScanner::default();
    let first = scanner.scan_file(&path).unwrap();
    let second = scanner.scan_file(&path).unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 0);
}

#[test]
fn scanner_prime_file_to_end_skips_existing_content_and_reads_future_lines() {
    let path = std::env::temp_dir().join(format!("niuma-codex-prime-{}.jsonl", std::process::id()));
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(
        file,
        r#"{{"type":"session_meta","payload":{{"id":"session-123","cwd":"/tmp/demo"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"old"}}}}"#
    )
    .unwrap();

    let mut scanner = CodexSessionScanner::default();
    scanner.prime_file_to_end(&path).unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"new"}}}}"#
    )
    .unwrap();
    drop(file);

    let events = scanner.scan_file(&path).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, EventType::SessionStarted);
    assert_eq!(
        events[0].dedupe_key,
        "codex_file:session-123:new:task_started"
    );
    assert_eq!(events[0].project_path, "/tmp/demo");
    let _ = std::fs::remove_file(path);
}

#[test]
fn scanner_tail_scan_reads_only_tail_window_and_primes_future_offset() {
    let path = std::env::temp_dir().join(format!("niuma-codex-tail-{}.jsonl", std::process::id()));
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(
        file,
        r#"{{"type":"session_meta","payload":{{"id":"session-123","cwd":"/tmp/demo"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"old"}}}}"#
    )
    .unwrap();
    writeln!(file, "{}", "x".repeat(2048)).unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"tail"}}}}"#
    )
    .unwrap();

    let mut scanner = CodexSessionScanner::default();
    let tail_events = scanner.scan_file_tail(&path, 512).unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"future"}}}}"#
    )
    .unwrap();
    drop(file);
    let future_events = scanner.scan_file(&path).unwrap();

    assert_eq!(tail_events.len(), 1);
    assert!(tail_events[0].dedupe_key.ends_with(":tail:task_started"));
    assert_eq!(future_events.len(), 1);
    assert!(future_events[0]
        .dedupe_key
        .ends_with(":future:task_started"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn scanner_keeps_offset_before_incomplete_line() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-123\",\"cwd\":\"/tmp/demo\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}"
        ),
    )
    .unwrap();

    let mut scanner = CodexSessionScanner::default();
    let first = scanner.scan_file(&path).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    let second = scanner.scan_file(&path).unwrap();

    assert_eq!(first.len(), 0);
    assert_eq!(second.len(), 1);
}

#[test]
fn scanner_does_not_commit_parser_or_offset_after_parse_error() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-a\",\"cwd\":\"/tmp/a\"}}\n",
    )
    .unwrap();

    let mut scanner = CodexSessionScanner::default();
    assert_eq!(scanner.scan_file(&path).unwrap().len(), 0);

    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-b\",\"cwd\":\"/tmp/b\"}}\n",
                "{not-json}\n",
            )
            .as_bytes(),
        )
        .unwrap();

    assert!(scanner.scan_file(&path).is_err());

    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-a\",\"cwd\":\"/tmp/a\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}\n",
        ),
    )
    .unwrap();
    let events = scanner.scan_file(&path).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "session-a");
    assert_eq!(events[0].project_path, "/tmp/a");
}

#[test]
fn scanner_resets_when_file_is_truncated_and_rewritten() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-a\",\"cwd\":\"/tmp/a-long-project\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-a\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-a\",\"last_agent_message\":\"完成\"}}\n",
        ),
    )
    .unwrap();

    let mut scanner = CodexSessionScanner::default();
    assert_eq!(scanner.scan_file(&path).unwrap().len(), 2);

    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"b\",\"cwd\":\"/b\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-b\"}}\n",
        ),
    )
    .unwrap();
    let events = scanner.scan_file(&path).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "b");
    assert_eq!(events[0].project_path, "/b");
}

#[test]
fn scanner_reads_crlf_jsonl_lines() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-crlf\",\"cwd\":\"/tmp/crlf\"}}\r\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}\r\n",
        ),
    )
    .unwrap();

    let mut scanner = CodexSessionScanner::default();
    let events = scanner.scan_file(&path).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "session-crlf");
    assert_eq!(events[0].project_path, "/tmp/crlf");
}

#[test]
fn codex_session_dirs_returns_today_and_yesterday() {
    let now = chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 6, 11, 12, 0, 0)
        .single()
        .unwrap();
    let dirs = codex_session_dirs(std::path::Path::new("/tmp/codex-home"), now);

    assert_eq!(
        dirs,
        vec![
            std::path::PathBuf::from("/tmp/codex-home/sessions/2026/06/11"),
            std::path::PathBuf::from("/tmp/codex-home/sessions/2026/06/10")
        ]
    );
}

#[test]
fn codex_session_dirs_handles_year_boundary() {
    let now = chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 1, 1, 0, 0, 0)
        .single()
        .unwrap();
    let dirs = codex_session_dirs(std::path::Path::new("/tmp/codex-home"), now);

    assert_eq!(
        dirs,
        vec![
            std::path::PathBuf::from("/tmp/codex-home/sessions/2026/01/01"),
            std::path::PathBuf::from("/tmp/codex-home/sessions/2025/12/31")
        ]
    );
}
