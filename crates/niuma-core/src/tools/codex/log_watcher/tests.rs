use super::*;
use crate::tools::codex::log_protocol::{detect_log_protocol_family, CodexProtocolFamily};

#[test]
fn log_protocol_detector_recognizes_current_schema() {
    let columns = [
        "id",
        "ts",
        "ts_nanos",
        "level",
        "target",
        "feedback_log_body",
        "thread_id",
    ];

    assert_eq!(
        detect_log_protocol_family(columns),
        CodexProtocolFamily::Current
    );
}

#[test]
fn log_protocol_detector_marks_missing_columns_as_unsupported() {
    let columns = ["id", "ts", "target"];

    assert_eq!(
        detect_log_protocol_family(columns),
        CodexProtocolFamily::Unsupported
    );
}

#[test]
fn schema_probe_returns_false_for_missing_logs_table() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(temp.path()).unwrap();
    connection
        .execute_batch("CREATE TABLE unrelated (id INTEGER PRIMARY KEY);")
        .unwrap();
    drop(connection);

    assert!(!codex_log_schema_available(temp.path()).unwrap());
}

#[test]
fn schema_probe_returns_false_when_required_columns_are_missing() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(temp.path()).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL
            );
            "#,
        )
        .unwrap();
    drop(connection);

    assert!(!codex_log_schema_available(temp.path()).unwrap());
}

#[test]
fn schema_probe_returns_true_for_supported_logs_schema() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(temp.path()).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                ts_nanos INTEGER NOT NULL,
                level TEXT NOT NULL,
                target TEXT NOT NULL,
                feedback_log_body TEXT,
                thread_id TEXT
            );
            "#,
        )
        .unwrap();
    drop(connection);

    assert!(codex_log_schema_available(temp.path()).unwrap());
}

#[test]
fn parses_context_too_large_log_as_task_failed() {
    let row = CodexLogRow {
        id: 42,
        ts: 1_781_255_486,
        ts_nanos: 567_000_000,
        level: "INFO".to_string(),
        target: "codex_otel.log_only".to_string(),
        thread_id: None,
        feedback_log_body: Some(
            r#"event.name="codex.sse_event" event.kind=response.completed error.message={"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model. Please adjust your input and try again.","type":"invalid_request_error","code":"context_too_large"}} event.timestamp=2026-06-12T09:01:26.567Z conversation.id=019eb542-a886-72e0-86fd-e5730054991c app.version=0.139.0 model=gpt-5.5"#.to_string(),
        ),
    };

    let event = parse_codex_log_row(&row, "/Users/me/.codex/logs_2.sqlite")
        .expect("context_too_large 日志应该转换为事件");

    assert_eq!(event.event_type, crate::models::EventType::TaskFailed);
    assert_eq!(event.session_id, "019eb542-a886-72e0-86fd-e5730054991c");
    assert_eq!(event.severity, "urgent");
    assert_eq!(event.completion_reason, None);
    assert_eq!(
        event.failure_reason,
        Some(crate::models::FailureReason::ContextWindowExceeded)
    );
    assert!(event.summary.contains("context window"));
    assert_eq!(event.source, "codex-internal-log");
}

#[test]
fn parses_received_message_context_too_large_error_as_task_failed() {
    let row = CodexLogRow {
        id: 46,
        ts: 1_781_331_040,
        ts_nanos: 0,
        level: "TRACE".to_string(),
        target: "log".to_string(),
        thread_id: Some("019ebee7-f62e-79b1-9823-73a1aa652fb2".to_string()),
        feedback_log_body: Some(
            r#"Received message {"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model. Please adjust your input and try again.","type":"invalid_request_error","code":"context_too_large"}}"#.to_string(),
        ),
    };

    let event = parse_codex_log_row(&row, "/Users/me/.codex/logs_2.sqlite")
        .expect("Received message 中的 context_too_large 应该转换为事件");

    assert_eq!(event.event_type, crate::models::EventType::TaskFailed);
    assert_eq!(event.session_id, "019ebee7-f62e-79b1-9823-73a1aa652fb2");
    assert_eq!(
        event.failure_reason,
        Some(crate::models::FailureReason::ContextWindowExceeded)
    );
    assert_eq!(event.source, "codex-internal-log");
}

#[test]
fn ignores_high_demand_log_as_non_terminal_transient_error() {
    let row = CodexLogRow {
        id: 44,
        ts: 1_781_327_201,
        ts_nanos: 123_000_000,
        level: "INFO".to_string(),
        target: "codex_otel.log_only".to_string(),
        thread_id: None,
        feedback_log_body: Some(
            r#"event.name="codex.sse_event" event.kind=response.completed error.message=We're currently experiencing high demand, which may cause temporary errors. event.timestamp=2026-06-13T03:38:32.148Z conversation.id=019ebf0e-7ce8-7fa1-b6c2-2d552d96cc98 app.version=0.139.0 model=gpt-5.5"#.to_string(),
        ),
    };

    assert!(parse_codex_log_row(&row, "/Users/me/.codex/logs_2.sqlite").is_none());
}

#[test]
fn parses_turn_error_404_as_task_failed() {
    let row = CodexLogRow {
        id: 48,
        ts: 1_781_411_836,
        ts_nanos: 173_000_000,
        level: "INFO".to_string(),
        target: "codex_core::session::turn".to_string(),
        thread_id: Some("019ec46b-231b-7d02-86ae-2452d25b5a96".to_string()),
        feedback_log_body: Some(
            "session_loop{thread_id=019ec46b-231b-7d02-86ae-2452d25b5a96}:submission_dispatch{otel.name=\"op.dispatch.user_input\" submission.id=\"019ec46b-5789-75e1-8e18-727a75003a14\" codex.op=\"user_input\"}:turn{otel.name=\"session_task.turn\" thread.id=019ec46b-231b-7d02-86ae-2452d25b5a96 turn.id=019ec46b-5789-75e1-8e18-727a75003a14 model=gpt-5.5 codex.turn.reasoning_effort=medium}:run_turn: Turn error: unexpected status 404 Not Found: 404 page not found, url: http://tcyp.synology.me:38317/v2/responses".to_string(),
        ),
    };

    let event = parse_codex_log_row(&row, "/Users/me/.codex/logs_2.sqlite")
        .expect("Turn error 404 应该转换为失败事件");

    assert_eq!(event.event_type, crate::models::EventType::TaskFailed);
    assert_eq!(event.session_id, "019ec46b-231b-7d02-86ae-2452d25b5a96");
    assert_eq!(
        event.failure_reason,
        Some(crate::models::FailureReason::Fatal)
    );
    assert_eq!(event.summary, "Codex turn failed");
    assert_eq!(
        event.error_message.as_deref(),
        Some(
            "unexpected status 404 Not Found: 404 page not found, url: http://tcyp.synology.me:38317/v2/responses"
        )
    );
    assert!(event
        .dedupe_key
        .contains("019ec46b-5789-75e1-8e18-727a75003a14"));
}

#[test]
fn ignores_non_runtime_log_with_context_error_text() {
    let row = CodexLogRow {
        id: 43,
        ts: 1_781_255_486,
        ts_nanos: 0,
        level: "TRACE".to_string(),
        target: "log".to_string(),
        thread_id: Some("019eb542-a886-72e0-86fd-e5730054991c".to_string()),
        feedback_log_body: Some(
            r#"Received message {"type":"error","error":{"code":"context_too_large","type":"invalid_request_error"}}"#.to_string(),
        ),
    };

    assert!(parse_codex_log_row(&row, "/Users/me/.codex/logs_2.sqlite").is_none());
}

#[test]
fn ignores_codex_otel_assistant_text_with_context_error_example() {
    let row = CodexLogRow {
        id: 47,
        ts: 1_781_331_179,
        ts_nanos: 123_840_000,
        level: "INFO".to_string(),
        target: "codex_otel.log_only".to_string(),
        thread_id: Some("019ebee7-f62e-79b1-9823-73a1aa652fb2".to_string()),
        feedback_log_body: Some(
            r#"event.name="codex.tool_result" output=测试样例：error.message={"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model.","type":"invalid_request_error","code":"context_too_large"}} conversation.id=019eb542-a886-72e0-86fd-e5730054991c"#.to_string(),
        ),
    };

    assert!(parse_codex_log_row(&row, "/Users/me/.codex/logs_2.sqlite").is_none());
}

#[test]
fn ignores_codex_otel_tool_result_with_nested_sse_error_example() {
    let row = CodexLogRow {
        id: 49,
        ts: 1_781_331_180,
        ts_nanos: 456_000_000,
        level: "INFO".to_string(),
        target: "codex_otel.log_only".to_string(),
        thread_id: Some("019ebee7-f62e-79b1-9823-73a1aa652fb2".to_string()),
        feedback_log_body: Some(
            r#"event.name="codex.tool_result" tool.name="shell" output=测试输出包含样例：event.name="codex.sse_event" event.kind=response.completed error.message={"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model.","type":"invalid_request_error","code":"context_too_large"}} conversation.id=019eb542-a886-72e0-86fd-e5730054991c"#.to_string(),
        ),
    };

    assert!(parse_codex_log_row(&row, "/Users/me/.codex/logs_2.sqlite").is_none());
}

#[test]
fn ignores_non_error_text_with_high_demand_phrase() {
    let row = CodexLogRow {
        id: 45,
        ts: 1_781_327_201,
        ts_nanos: 0,
        level: "TRACE".to_string(),
        target: "codex_api::endpoint::responses_websocket".to_string(),
        thread_id: Some("019ebf0e-7ce8-7fa1-b6c2-2d552d96cc98".to_string()),
        feedback_log_body: Some(
            r#"websocket request text includes We're currently experiencing high demand, which may cause temporary errors."#.to_string(),
        ),
    };

    assert!(parse_codex_log_row(&row, "/Users/me/.codex/logs_2.sqlite").is_none());
}

#[test]
fn scanner_reads_new_error_rows_once() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(temp.path()).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                ts_nanos INTEGER NOT NULL,
                level TEXT NOT NULL,
                target TEXT NOT NULL,
                feedback_log_body TEXT,
                module_path TEXT,
                file TEXT,
                line INTEGER,
                thread_id TEXT,
                process_uuid TEXT,
                estimated_bytes INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body, thread_id)
            VALUES (
                1781255486,
                567000000,
                'INFO',
                'codex_otel.trace_safe',
                'event.name="codex.sse_event" error.message={"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model.","type":"invalid_request_error","code":"context_too_large"}} conversation.id=019eb542-a886-72e0-86fd-e5730054991c',
                NULL
            );
            "#,
        )
        .unwrap();
    drop(connection);
    let mut scanner = CodexLogScanner::default();

    let first = scanner.scan_file(temp.path()).unwrap();
    let second = scanner.scan_file(temp.path()).unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].event_type, crate::models::EventType::TaskFailed);
    assert!(second.is_empty());
}

#[test]
fn scanner_reads_received_message_context_too_large_error_rows_once() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(temp.path()).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                ts_nanos INTEGER NOT NULL,
                level TEXT NOT NULL,
                target TEXT NOT NULL,
                feedback_log_body TEXT,
                module_path TEXT,
                file TEXT,
                line INTEGER,
                thread_id TEXT,
                process_uuid TEXT,
                estimated_bytes INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body, thread_id)
            VALUES (
                1781331040,
                0,
                'TRACE',
                'log',
                'Received message {"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model. Please adjust your input and try again.","type":"invalid_request_error","code":"context_too_large"}}',
                '019ebee7-f62e-79b1-9823-73a1aa652fb2'
            );
            "#,
        )
        .unwrap();
    drop(connection);
    let mut scanner = CodexLogScanner::default();

    let first = scanner.scan_file(temp.path()).unwrap();
    let second = scanner.scan_file(temp.path()).unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].event_type, crate::models::EventType::TaskFailed);
    assert_eq!(
        first[0].failure_reason,
        Some(crate::models::FailureReason::ContextWindowExceeded)
    );
    assert!(second.is_empty());
}

#[test]
fn scanner_ignores_high_demand_error_rows() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(temp.path()).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                ts_nanos INTEGER NOT NULL,
                level TEXT NOT NULL,
                target TEXT NOT NULL,
                feedback_log_body TEXT,
                module_path TEXT,
                file TEXT,
                line INTEGER,
                thread_id TEXT,
                process_uuid TEXT,
                estimated_bytes INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body, thread_id)
            VALUES (
                1781327201,
                123000000,
                'INFO',
                'codex_core::session::turn',
                'session_loop{thread_id=019ebf0e-7ce8-7fa1-b6c2-2d552d96cc98}:run_turn: Turn error: We''re currently experiencing high demand, which may cause temporary errors.',
                '019ebf0e-7ce8-7fa1-b6c2-2d552d96cc98'
            );
            "#,
        )
        .unwrap();
    drop(connection);
    let mut scanner = CodexLogScanner::default();

    let first = scanner.scan_file(temp.path()).unwrap();
    let second = scanner.scan_file(temp.path()).unwrap();

    assert!(first.is_empty());
    assert!(second.is_empty());
}

#[test]
fn scanner_reads_turn_error_rows_once() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(temp.path()).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                ts_nanos INTEGER NOT NULL,
                level TEXT NOT NULL,
                target TEXT NOT NULL,
                feedback_log_body TEXT,
                module_path TEXT,
                file TEXT,
                line INTEGER,
                thread_id TEXT,
                process_uuid TEXT,
                estimated_bytes INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body, thread_id)
            VALUES (
                1781411836,
                173000000,
                'INFO',
                'codex_core::session::turn',
                'session_loop{thread_id=019ec46b-231b-7d02-86ae-2452d25b5a96}:turn{turn.id=019ec46b-5789-75e1-8e18-727a75003a14}:run_turn: Turn error: unexpected status 404 Not Found: 404 page not found, url: http://tcyp.synology.me:38317/v2/responses',
                '019ec46b-231b-7d02-86ae-2452d25b5a96'
            );
            "#,
        )
        .unwrap();
    drop(connection);
    let mut scanner = CodexLogScanner::default();

    let first = scanner.scan_file(temp.path()).unwrap();
    let second = scanner.scan_file(temp.path()).unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].event_type, crate::models::EventType::TaskFailed);
    assert_eq!(first[0].summary, "Codex turn failed");
    assert!(second.is_empty());
}

#[test]
fn same_runtime_error_rows_share_dedupe_key_within_short_window() {
    let session_id = "019ebee7-f62e-79b1-9823-73a1aa652fb2";
    let first = CodexLogRow {
        id: 50,
        ts: 1_781_336_311,
        ts_nanos: 957_668_000,
        level: "INFO".to_string(),
        target: "codex_otel.log_only".to_string(),
        thread_id: None,
        feedback_log_body: Some(format!(
            r#"event.name="codex.sse_event" error.message={{"type":"error","status":400,"error":{{"message":"Your input exceeds the context window of this model.","type":"invalid_request_error","code":"context_too_large"}}}} conversation.id={session_id}"#
        )),
    };
    let second = CodexLogRow {
        id: 51,
        ts: 1_781_336_311,
        ts_nanos: 961_347_000,
        level: "INFO".to_string(),
        target: "codex_core::session::turn".to_string(),
        thread_id: Some(session_id.to_string()),
        feedback_log_body: Some(
            r#"session_loop{thread_id=019ebee7-f62e-79b1-9823-73a1aa652fb2}:run_turn: Turn error: {"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model.","type":"invalid_request_error","code":"context_too_large"}}"#
                .to_string(),
        ),
    };

    let first_event = parse_codex_log_row(&first, "/Users/me/.codex/logs_2.sqlite")
        .expect("第一条 context_too_large 应转换为失败事件");
    let second_event = parse_codex_log_row(&second, "/Users/me/.codex/logs_2.sqlite")
        .expect("第二条 context_too_large 应转换为失败事件");

    assert_eq!(first_event.id, second_event.id);
    assert_eq!(first_event.dedupe_key, second_event.dedupe_key);
    assert_eq!(first_event.failure_reason, second_event.failure_reason);
}

#[test]
fn scanner_advances_past_irrelevant_rows_and_deduplicates_dual_otel_rows() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(temp.path()).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                ts_nanos INTEGER NOT NULL,
                level TEXT NOT NULL,
                target TEXT NOT NULL,
                feedback_log_body TEXT,
                module_path TEXT,
                file TEXT,
                line INTEGER,
                thread_id TEXT,
                process_uuid TEXT,
                estimated_bytes INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body, thread_id)
            VALUES (1781255485, 0, 'TRACE', 'log', 'Received message containing context_too_large text', NULL);
            "#,
        )
        .unwrap();
    let mut scanner = CodexLogScanner::default();

    assert!(scanner.scan_file(temp.path()).unwrap().is_empty());

    connection
        .execute_batch(
            r#"
            INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body, thread_id)
            VALUES
            (
                1781255486,
                567000000,
                'INFO',
                'codex_otel.log_only',
                'event.name="codex.sse_event" error.message={"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model.","type":"invalid_request_error","code":"context_too_large"}} conversation.id=019eb542-a886-72e0-86fd-e5730054991c',
                NULL
            ),
            (
                1781255486,
                567000000,
                'INFO',
                'codex_otel.trace_safe',
                'event.name="codex.sse_event" error.message={"type":"error","status":400,"error":{"message":"Your input exceeds the context window of this model.","type":"invalid_request_error","code":"context_too_large"}} conversation.id=019eb542-a886-72e0-86fd-e5730054991c',
                NULL
            );
            "#,
        )
        .unwrap();

    let events = scanner.scan_file(temp.path()).unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].dedupe_key, events[1].dedupe_key);
    assert!(scanner.scan_file(temp.path()).unwrap().is_empty());
}
