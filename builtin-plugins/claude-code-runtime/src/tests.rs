use niuma_core::models::{EventType, ToolKind};
use niuma_core::tool_session::ToolSessionMessageRole;

use crate::claude::session_protocol::current::ClaudeJsonlParser;
use crate::session_messages::parse_claude_message_line;

const SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";

#[test]
fn parser_emits_session_started_from_user_string_message() {
    let line = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:00.000Z","message":{"role":"user","content":"你好"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser.parse_line(line, "/tmp/session.jsonl").unwrap().unwrap();

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

    let event = parser.parse_line(line, "/tmp/session.jsonl").unwrap().unwrap();

    assert_eq!(event.event_type, EventType::AssistantMessageCompleted);
    assert_eq!(event.summary, "完成了");
    assert_eq!(event.content.as_deref(), Some("完成了"));
}

#[test]
fn parser_tracks_pending_tool_until_tool_result() {
    let tool_use = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"sleep 1"}}]}}"#;
    let tool_result = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok","is_error":false}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let first = parser.parse_line(tool_use, "/tmp/session.jsonl").unwrap().unwrap();
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

    let event = parser.parse_line(line, "/tmp/session.jsonl").unwrap().unwrap();

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
