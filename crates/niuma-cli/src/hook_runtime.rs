use std::io::Read;

use chrono::Utc;
use niuma_api::local_api_addr;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::hook_payload::{HookPayloadParser, HookToolHint};
use niuma_core::local_api_client::submit_event_to_local_api;
use niuma_core::models::{NiumaEvent, ToolKind};
use niuma_core::store::SqliteStateStore;
use serde_json::json;

pub fn run_codex_hook() -> ApiResponse<serde_json::Value> {
    run_hook(HookToolHint::Codex)
}

pub fn run_hook(tool_hint: HookToolHint) -> ApiResponse<serde_json::Value> {
    let mut input = Vec::new();
    if let Err(error) = std::io::stdin().read_to_end(&mut input) {
        return ApiResponse::fail(ApiErrorCode::System, format!("读取 stdin 失败：{error}"));
    }
    if input.is_empty() {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, "hook payload 不能为空");
    }

    match HookPayloadParser::parse(&input, tool_hint, Utc::now()) {
        Ok(Some(event)) => {
            let store = SqliteStateStore::new(SqliteStateStore::default_path());
            submit_parsed_event(&store, &local_api_addr(), event)
        }
        Ok(None) => ApiResponse::ok(json!({ "ignored": true })),
        Err(error) => ApiResponse::fail(
            ApiErrorCode::ParameterFormat,
            format!("JSON 解析失败：{error}"),
        ),
    }
}

fn submit_parsed_event(
    store: &SqliteStateStore,
    local_api_addr: &str,
    event: NiumaEvent,
) -> ApiResponse<serde_json::Value> {
    match store.listener_config() {
        Ok(config) if !config.is_tool_enabled(&event.tool) => {
            return ApiResponse::ok(json!({
                "event": event,
                "submitted": false,
                "reason": tool_listening_disabled_reason(&event.tool)
            }));
        }
        Ok(_) => {}
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    }

    match submit_event_to_local_api(local_api_addr, &event) {
        Ok(api_body) => ApiResponse::ok(json!({
            "event": event,
            "submitted": true,
            "local_api": api_body
        })),
        Err(error) => ApiResponse::ok(json!({
            "event": event,
            "submitted": false,
            "submit_error": error
        })),
    }
}

fn tool_listening_disabled_reason(tool: &ToolKind) -> &'static str {
    match tool {
        ToolKind::Codex => "codex_listening_disabled",
        ToolKind::ClaudeCode => "claude_code_listening_disabled",
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use niuma_core::listener_config::ListenerConfig;
    use niuma_core::models::{EventType, NiumaEvent, ToolKind};
    use niuma_core::store::SqliteStateStore;

    use super::*;

    #[test]
    fn submit_parsed_codex_event_is_skipped_when_listener_disabled() {
        let store = SqliteStateStore::new(test_sqlite_path("codex_listener_disabled"));
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: false,
                ..ListenerConfig::default()
            })
            .unwrap();

        let response = submit_parsed_event(&store, "127.0.0.1:9", sample_codex_event());

        assert_eq!(response.code, 0);
        assert_eq!(response.data["submitted"], false);
        assert_eq!(response.data["reason"], "codex_listening_disabled");
    }

    #[test]
    fn submit_parsed_claude_code_event_is_skipped_when_listener_disabled() {
        let store = SqliteStateStore::new(test_sqlite_path("claude_listener_disabled"));
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: true,
                claude_code_listening_enabled: false,
            })
            .unwrap();

        let response = submit_parsed_event(
            &store,
            "127.0.0.1:9",
            sample_tool_event(ToolKind::ClaudeCode),
        );

        assert_eq!(response.code, 0);
        assert_eq!(response.data["submitted"], false);
        assert_eq!(response.data["reason"], "claude_code_listening_disabled");
    }

    fn sample_codex_event() -> NiumaEvent {
        sample_tool_event(ToolKind::Codex)
    }

    fn sample_tool_event(tool: ToolKind) -> NiumaEvent {
        NiumaEvent {
            id: "event-hook-disabled".to_string(),
            dedupe_key: "dedupe-hook-disabled".to_string(),
            source: "test".to_string(),
            tool,
            session_id: "session-hook".to_string(),
            project_path: "/tmp/hook".to_string(),
            project_name: "hook".to_string(),
            event_type: EventType::SessionStarted,
            severity: "info".to_string(),
            summary: "Hook test".to_string(),
            content: None,
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            created_at: Utc.timestamp_opt(1_000, 0).single().unwrap(),
        }
    }

    fn test_sqlite_path(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "niuma-cli-hook-{name}-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }
}
