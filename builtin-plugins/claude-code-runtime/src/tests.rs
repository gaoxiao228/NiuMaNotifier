use niuma_core::claude_code_managed_session::{
    update_registry, ManagedClaudeCodeSession, ManagedClaudeCodeSessionState,
};
use niuma_core::models::{CompletionReason, EventSessionScope, EventType, ToolKind};
use niuma_core::tool_session::{ToolSessionMessageRole, ToolSessionStatus};
use niuma_core::tool_session_rpc::{
    ProviderRpcRequest, SessionDetailParams, SessionDetailResult, SessionSnapshotParams,
    SessionSnapshotResult,
};
use std::fs::OpenOptions;
use std::io::Write;
use tempfile::tempdir;

use crate::claude::session_event_cursor::ClaudeSessionScanner;
use crate::claude::session_protocol::current::ClaudeJsonlParser;
use crate::claude::session_repository::ClaudeSessionRepository;
use crate::session_messages::parse_claude_message_line;
use crate::session_provider::ClaudeSessionProvider;

const SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";

#[test]
fn parser_emits_session_started_from_user_string_message() {
    let line = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:00.000Z","message":{"role":"user","content":"你好"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.tool, ToolKind::ClaudeCode);
    assert_eq!(event.session_id, SESSION_ID);
    assert_eq!(event.project_path, "/repo");
    assert_eq!(event.project_name, "repo");
    assert_eq!(event.event_type, EventType::SessionStarted);
    assert_eq!(event.summary, "你好");
}

#[test]
fn parser_treats_tool_use_text_as_activity_not_completion() {
    let line = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T15:00:55.967Z","message":{"id":"msg-tool-use-text","type":"message","role":"assistant","content":[{"type":"text","text":"让我进一步了解扫描器的具体实现，看看轮询到底做了什么。"}],"stop_reason":"tool_use"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::SessionActivity);
    assert_eq!(
        event.summary,
        "Claude Code 回复中，任务仍在继续：让我进一步了解扫描器的具体实现，看看轮询到底做了什么。"
    );
    assert_eq!(event.completion_reason, None);
}

#[test]
fn parser_treats_missing_stop_reason_text_as_activity() {
    let line = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-12T01:41:26.614Z","message":{"id":"legacy-text","type":"message","role":"assistant","content":[{"type":"text","text":"让我继续探索整体结构和部署文件。"}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::SessionActivity);
    assert_eq!(
        event.summary,
        "Claude Code 回复中，任务仍在继续：让我继续探索整体结构和部署文件。"
    );
    assert_eq!(event.completion_reason, None);
}

#[test]
fn parser_emits_completed_event_from_end_turn_assistant_text() {
    let line = r###"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T15:01:58.361Z","message":{"id":"msg-end-turn","type":"message","role":"assistant","content":[{"type":"text","text":"## 分析结论\n任务已经完成。"}],"stop_reason":"end_turn"}}"###;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.summary, "## 分析结论\n任务已经完成。");
    assert_eq!(
        event.content.as_deref(),
        Some("## 分析结论\n任务已经完成。")
    );
    assert_eq!(
        event.completion_reason,
        Some(niuma_core::models::CompletionReason::Normal)
    );
}

#[test]
fn parser_ignores_end_turn_thinking_until_text_arrives() {
    // thinking 分片的 end_turn 不能抢先结束，同 message id 后续 text 才表示完成。
    let thinking = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T15:01:39.477Z","message":{"id":"msg-end-turn","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"Now summarize the result."}],"stop_reason":"end_turn"}}"#;
    let text = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T15:01:58.361Z","message":{"id":"msg-end-turn","type":"message","role":"assistant","content":[{"type":"text","text":"任务完成。"}],"stop_reason":"end_turn"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let first = parser.parse_line(thinking, "/tmp/session.jsonl").unwrap();
    let second = parser
        .parse_line(text, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(first.unwrap().event_type, EventType::SessionActivity);
    assert_eq!(second.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(second.summary, "任务完成。");
}

#[test]
fn parser_marks_stop_sequence_text_as_task_failed() {
    let line = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-12T07:41:26.361Z","message":{"id":"msg-stop-sequence","type":"message","role":"assistant","content":[{"type":"text","text":"API Error: Request rejected (429) · Service Unavailable"}],"stop_reason":"stop_sequence"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::TaskFailed);
    assert_eq!(event.summary, "Claude Code API error");
    assert_eq!(
        event.error_message.as_deref(),
        Some("API Error: Request rejected (429) · Service Unavailable")
    );
    assert_eq!(
        event.failure_reason,
        Some(niuma_core::models::FailureReason::ServerOverloaded)
    );
}

#[test]
fn parser_marks_system_api_error_as_task_failed() {
    let line = r#"{"type":"system","subtype":"api_error","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-12T07:41:00.961Z","error":{"message":"429 {\"error\":{\"message\":\"Service Unavailable\",\"type\":\"error\"},\"type\":\"error\"}","status":429,"formatted":"429 Service Unavailable"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::TaskFailed);
    assert_eq!(event.summary, "Claude Code API error");
    assert_eq!(
        event.error_message.as_deref(),
        Some("429 Service Unavailable")
    );
    assert_eq!(
        event.failure_reason,
        Some(niuma_core::models::FailureReason::ServerOverloaded)
    );
}

#[test]
fn parser_dedupes_api_error_and_stop_sequence_for_same_overloaded_failure() {
    let api_error = r#"{"type":"system","subtype":"api_error","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-12T07:41:00.961Z","error":{"message":"429 {\"error\":{\"message\":\"Service Unavailable\",\"type\":\"error\"},\"type\":\"error\"}","status":429,"formatted":"429 Service Unavailable"}}"#;
    let stop_sequence = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-12T07:41:26.361Z","message":{"id":"msg-stop-sequence","type":"message","role":"assistant","content":[{"type":"text","text":"API Error: Request rejected (429) · Service Unavailable"}],"stop_reason":"stop_sequence"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let first = parser
        .parse_line(api_error, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();
    let second = parser
        .parse_line(stop_sequence, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(first.event_type, EventType::TaskFailed);
    assert_eq!(second.event_type, EventType::TaskFailed);
    assert_eq!(first.dedupe_key, second.dedupe_key);
    assert_eq!(first.id, second.id);
}

#[test]
fn parser_keeps_unknown_terminal_failures_distinct_by_source() {
    let api_error = r#"{"type":"system","subtype":"api_error","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-12T07:41:00.961Z","error":{"message":"unexpected internal failure"}}"#;
    let stop_sequence = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-12T07:41:00.961Z","message":{"id":"msg-stop-sequence","type":"message","role":"assistant","content":[{"type":"text","text":"unexpected internal failure"}],"stop_reason":"stop_sequence"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let first = parser
        .parse_line(api_error, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();
    let second = parser
        .parse_line(stop_sequence, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(first.event_type, EventType::TaskFailed);
    assert_eq!(second.event_type, EventType::TaskFailed);
    assert_ne!(first.dedupe_key, second.dedupe_key);
}

#[test]
fn parser_marks_sidechain_assistant_completion_as_subagent_scope() {
    let line = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","isSidechain":true,"cwd":"/repo","timestamp":"2026-06-29T15:02:00.000Z","message":{"id":"msg-sidechain","type":"message","role":"assistant","content":[{"type":"text","text":"子任务完成。"}],"stop_reason":"end_turn"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.session_scope, Some(EventSessionScope::Subagent));
}

#[test]
fn parser_prioritizes_tool_use_when_text_and_tool_use_share_message() {
    let line = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T15:02:10.000Z","message":{"id":"msg-text-tool","type":"message","role":"assistant","content":[{"type":"text","text":"我会先运行命令。"},{"type":"tool_use","id":"toolu_shared_1","name":"Bash","input":{"command":"pwd"}}],"stop_reason":"tool_use"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::SessionActivity);
    assert_eq!(event.summary, "Claude Code 正在调用工具：Bash");
    assert_eq!(event.tool_call_id.as_deref(), Some("toolu_shared_1"));
}

#[test]
fn parser_tracks_pending_tool_until_tool_result() {
    let tool_use = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"sleep 1"}}]}}"#;
    let tool_result = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok","is_error":false}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let first = parser
        .parse_line(tool_use, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();
    let second = parser
        .parse_line(tool_result, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(first.event_type, EventType::SessionActivity);
    assert_eq!(first.summary, "Claude Code 正在调用工具：Bash");
    assert_eq!(second.event_type, EventType::SessionActivity);
    assert_eq!(second.summary, "Claude Code 工具执行完成：ok");
    assert_eq!(second.tool_call_id.as_deref(), Some("toolu_1"));
}

#[test]
fn parser_resolves_tool_result_to_matching_claude_approval_request_id() {
    let tool_use = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_webfetch","name":"WebFetch","input":{"prompt":"提取所有热搜话题标题和排名","url":"https://top.baidu.com/board?tab=realtime"}}]}}"#;
    let tool_result = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_webfetch","content":"User rejected tool use","is_error":true}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    parser.parse_line(tool_use, "/tmp/session.jsonl").unwrap();
    let event = parser
        .parse_line(tool_result, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.tool_call_id.as_deref(), Some("toolu_webfetch"));
    assert_eq!(
        event.attention_resolve_key.as_deref(),
        Some("approval:claude_code:11111111-1111-4111-8111-111111111111:permission_request:WebFetch:93e6a55efc47430b")
    );
}

#[test]
fn parser_marks_user_rejected_tool_use_as_interrupted_completion() {
    let tool_use = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-30T06:30:28.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_00_rejected","name":"WebFetch","input":{"url":"https://www.baidu.com","prompt":"获取今日头条"}}],"stop_reason":"tool_use"}}"#;
    let rejected = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-30T06:30:30.761Z","message":{"role":"user","content":[{"type":"tool_result","content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed.","is_error":true,"tool_use_id":"call_00_rejected"}]},"toolUseResult":"User rejected tool use"}"#;
    let mut parser = ClaudeJsonlParser::default();

    parser.parse_line(tool_use, "/tmp/session.jsonl").unwrap();
    let event = parser
        .parse_line(rejected, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.completion_reason, Some(CompletionReason::Interrupted));
    assert_eq!(event.tool_call_id.as_deref(), Some("call_00_rejected"));
    assert_eq!(event.summary, "Claude Code 已中断：用户拒绝了工具授权");
}

#[test]
fn parser_treats_tool_result_error_as_activity() {
    let line = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T15:00:29.898Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_grep","content":"<tool_use_error>Error: No such tool available: Grep. Grep is not available in this session — search file contents with `grep` via the Bash tool instead.</tool_use_error>","is_error":true}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::SessionActivity);
    assert_eq!(
        event.summary,
        "Claude Code 工具执行出错，任务可能仍在继续：<tool_use_error>Error: No such tool available: Grep. Grep is not available in this session — search file contents with `grep` via the Bash tool instead.</tool_use_error>"
    );
    assert_eq!(event.error_message, None);
    assert_eq!(event.failure_reason, None);
    assert_eq!(event.tool_call_id.as_deref(), Some("toolu_grep"));
}

#[test]
fn parser_keeps_tool_use_and_result_dedupe_keys_distinct_with_same_timestamp() {
    let tool_use = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T15:00:29.898Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_same","name":"Bash","input":{"command":"grep pattern file"}}]}}"#;
    let tool_result = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T15:00:29.898Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_same","content":"exit code 1","is_error":true}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let first = parser
        .parse_line(tool_use, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();
    let second = parser
        .parse_line(tool_result, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(first.event_type, EventType::SessionActivity);
    assert_eq!(second.event_type, EventType::SessionActivity);
    assert_ne!(first.dedupe_key, second.dedupe_key);
    assert_ne!(first.id, second.id);
    assert_eq!(second.tool_call_id.as_deref(), Some("toolu_same"));
}

#[test]
fn parser_ignores_tool_result_error_after_hook_permission_deny() {
    let attachment = r#"{"type":"attachment","attachment":{"type":"hook_permission_decision","decision":"deny","toolUseID":"toolu_denied","hookEvent":"PermissionRequest"},"timestamp":"2026-06-29T08:04:13.118Z","cwd":"/repo","sessionId":"11111111-1111-4111-8111-111111111111"}"#;
    let denied_result = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-29T08:04:13.119Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_denied","content":"用户在 NiuMa 中拒绝了本次权限请求","is_error":true}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    assert!(parser
        .parse_line(attachment, "/tmp/session.jsonl")
        .unwrap()
        .is_none());
    assert!(parser
        .parse_line(denied_result, "/tmp/session.jsonl")
        .unwrap()
        .is_none());
}

#[test]
fn detail_message_parser_maps_assistant_text_and_tool_rows() {
    let assistant = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"完成了"}]}}"#;
    let tool = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:06.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"pwd"}}]}}"#;

    let assistant_message = parse_claude_message_line(SESSION_ID, 0, assistant);
    let tool_message = parse_claude_message_line(SESSION_ID, 1, tool);

    assert_eq!(assistant_message.role, ToolSessionMessageRole::Assistant);
    assert_eq!(assistant_message.content, "完成了");
    assert_eq!(tool_message.role, ToolSessionMessageRole::ToolCall);
    assert_eq!(tool_message.content, "Bash");
}

#[test]
fn repository_builds_snapshot_from_claude_project_files() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("projects").join("-repo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("11111111-1111-4111-8111-111111111111.jsonl"),
        r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:00.000Z","message":{"role":"user","content":"你好"}}"#,
    )
    .unwrap();
    let mut repository = ClaudeSessionRepository::new(temp.path().to_path_buf());

    let sessions = repository.refresh_snapshot().unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(
        sessions[0].id,
        "claude_code:11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(sessions[0].tool, ToolKind::ClaudeCode);
    assert_eq!(
        sessions[0].session_id,
        "11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(sessions[0].project_path, "/repo");
    assert_eq!(sessions[0].project_name, "repo");
    assert_eq!(sessions[0].status, ToolSessionStatus::Active);
    assert_eq!(
        sessions[0].first_user_message_preview.as_deref(),
        Some("你好")
    );
}

#[test]
fn provider_returns_snapshot_and_detail_for_claude_code_tool() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("projects").join("-repo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("11111111-1111-4111-8111-111111111111.jsonl"),
        concat!(
            r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:00.000Z","message":{"role":"user","content":"你好"}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"完成"}]}}"#,
            "\n"
        ),
    )
    .unwrap();
    let mut provider = ClaudeSessionProvider::with_claude_home(temp.path().to_path_buf());

    let snapshot_request = ProviderRpcRequest::new(
        "1",
        "session_snapshot",
        SessionSnapshotParams {
            tool: ToolKind::ClaudeCode,
        },
    )
    .unwrap();
    let snapshot = provider.handle_request(snapshot_request);
    assert!(snapshot.error.is_none());
    let snapshot_result = snapshot.result_as::<SessionSnapshotResult>().unwrap();
    assert_eq!(snapshot_result.sessions.len(), 1);

    let detail_request = ProviderRpcRequest::new(
        "2",
        "session_detail",
        SessionDetailParams {
            tool: ToolKind::ClaudeCode,
            session_id: "11111111-1111-4111-8111-111111111111".to_string(),
            limit: 10,
            cursor: None,
        },
    )
    .unwrap();
    let detail = provider.handle_request(detail_request);
    assert!(detail.error.is_none());
    let detail_result = detail.result_as::<SessionDetailResult>().unwrap();
    assert_eq!(detail_result.detail.messages.len(), 2);
    assert_eq!(
        detail_result.detail.messages[0].role,
        ToolSessionMessageRole::Assistant
    );
    assert_eq!(detail_result.detail.messages[0].content, "完成");
    assert_eq!(
        detail_result.detail.messages[1].role,
        ToolSessionMessageRole::User
    );
    assert_eq!(detail_result.detail.messages[1].content, "你好");
}

#[test]
fn scanner_reads_appended_tool_use_before_tool_result() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("session.jsonl");
    std::fs::write(&file, "").unwrap();
    let repository = ClaudeSessionRepository::new(temp.path().to_path_buf());
    let mut scanner = ClaudeSessionScanner::with_repository(repository);

    {
        let mut handle = OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(
            handle,
            r#"{{"type":"assistant","sessionId":"44444444-4444-4444-8444-444444444444","cwd":"/repo","timestamp":"2026-06-28T01:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"toolu_1","name":"Bash","input":{{"command":"sleep 10"}}}}]}}}}"#
        )
        .unwrap();
    }
    let first = scanner.scan_file(&file).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].summary, "Claude Code 正在调用工具：Bash");

    {
        let mut handle = OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(
            handle,
            r#"{{"type":"user","sessionId":"44444444-4444-4444-8444-444444444444","cwd":"/repo","timestamp":"2026-06-28T01:00:02.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_1","content":"done","is_error":false}}]}}}}"#
        )
        .unwrap();
    }
    let second = scanner.scan_file(&file).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].summary, "Claude Code 工具执行完成：done");
}

#[test]
fn repository_attaches_control_channel_for_managed_claude_session() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("projects").join("-repo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("11111111-1111-4111-8111-111111111111.jsonl"),
        r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:00.000Z","message":{"role":"user","content":"你好"}}"#,
    )
    .unwrap();
    let registry_path = temp.path().join("managed").join("claude-code.json");
    update_registry(&registry_path, |registry| {
        registry.upsert(ManagedClaudeCodeSession {
            wrapper_session_id: "niuma_claude_1".to_string(),
            state: ManagedClaudeCodeSessionState::Started,
            cwd: "/repo".to_string(),
            pid: Some(123),
            control_socket: None,
            started_at: chrono::Utc::now(),
            claude_session_id: Some("11111111-1111-4111-8111-111111111111".to_string()),
            transcript_path: None,
            bound_at: None,
            binding_failure_reason: None,
        });
    })
    .unwrap();
    let mut repository = ClaudeSessionRepository::with_managed_registry_path(
        temp.path().to_path_buf(),
        registry_path,
    );

    let sessions = repository.refresh_snapshot().unwrap();

    let control = sessions[0].control.as_ref().unwrap();
    assert_eq!(
        control.channels[0].id,
        "niuma_claude_managed:niuma_claude_1"
    );
    assert!(!control.resumable);
    assert!(control.preferred_channel_id.is_none());
    assert_eq!(control.channels[0].provider, "niuma_claude");
    assert!(!control.channels[0].available);
    assert!(control.channels[0].actions.is_empty());
    assert!(control.channels[0].capabilities.is_empty());
}
