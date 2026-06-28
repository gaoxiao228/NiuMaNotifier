use niuma_core::claude_code_managed_session::{
    update_registry, ManagedClaudeCodeSession, ManagedClaudeCodeSessionState,
};
use niuma_core::models::{EventType, ToolKind};
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
fn parser_emits_completed_event_from_assistant_text() {
    let line = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"完成了"}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.summary, "完成了");
    assert_eq!(event.content.as_deref(), Some("完成了"));
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
}

#[test]
fn parser_marks_failed_tool_result_as_task_failed() {
    let line = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:04.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"exit code 1","is_error":true}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser
        .parse_line(line, "/tmp/session.jsonl")
        .unwrap()
        .unwrap();

    assert_eq!(event.event_type, EventType::TaskFailed);
    assert_eq!(event.summary, "Claude Code 工具执行失败：exit code 1");
    assert_eq!(event.error_message.as_deref(), Some("exit code 1"));
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
        control.preferred_channel_id.as_deref(),
        Some("niuma_claude_managed:niuma_claude_1")
    );
    assert!(control.resumable);
    assert_eq!(control.channels[0].provider, "niuma_claude");
    assert!(control.channels[0]
        .actions
        .iter()
        .any(|action| action.action_type == "send_instruction"));
}
