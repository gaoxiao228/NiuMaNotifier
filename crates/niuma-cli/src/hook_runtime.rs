use std::io::Read;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use niuma_api::local_api_addr;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::hook_payload::{
    codex_permission_request_from_payload, CodexPermissionRequest, HookPayloadParser, HookToolHint,
};
use niuma_core::local_api_client::{get_local_api, post_local_api, submit_event_to_local_api};
use niuma_core::models::{ApprovalDecisionKind, ApprovalStatus, NiumaEvent, ToolKind};
use niuma_core::store::NiumaStore;
use serde_json::json;

const CODEX_APPROVAL_PROXY_TIMEOUT: Duration = Duration::from_secs(600);
const CODEX_APPROVAL_POLL_INTERVAL: Duration = Duration::from_millis(500);
const CODEX_APPROVAL_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

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

    if tool_hint == HookToolHint::Codex && is_hook_event(&input, "PermissionRequest") {
        let store = NiumaStore::new(NiumaStore::default_path());
        return run_codex_permission_proxy(
            &store,
            &local_api_addr(),
            &input,
            CODEX_APPROVAL_PROXY_TIMEOUT,
            CODEX_APPROVAL_POLL_INTERVAL,
        );
    }

    match HookPayloadParser::parse(&input, tool_hint, Utc::now()) {
        Ok(Some(event)) => {
            let store = NiumaStore::new(NiumaStore::default_path());
            submit_parsed_event(&store, &local_api_addr(), event)
        }
        Ok(None) => ApiResponse::ok(json!({ "ignored": true })),
        Err(error) => ApiResponse::fail(
            ApiErrorCode::ParameterFormat,
            format!("JSON 解析失败：{error}"),
        ),
    }
}

fn run_codex_permission_proxy(
    store: &NiumaStore,
    local_api_addr: &str,
    input: &[u8],
    timeout: Duration,
    poll_interval: Duration,
) -> ApiResponse<serde_json::Value> {
    let request = match codex_permission_request_from_payload(input) {
        Ok(request) => request,
        Err(error) => {
            return ApiResponse::fail(
                ApiErrorCode::ParameterFormat,
                format!("JSON 解析失败：{error}"),
            );
        }
    };
    match store.listener_config() {
        Ok(config) if !config.is_tool_enabled(&ToolKind::Codex) => {
            return ApiResponse::ok(json!({
                "request_id": request.id,
                "submitted": false,
                "reason": tool_listening_disabled_reason(&ToolKind::Codex)
            }));
        }
        Ok(_) => {}
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    }

    let create_result = match create_approval_request(local_api_addr, &request, timeout) {
        Ok(value) => value,
        Err(error) => return ApiResponse::fail(ApiErrorCode::ServiceUnavailable, error),
    };
    if approval_create_returned_to_codex(&create_result) {
        return ApiResponse::ok(json!({
            "request_id": request.id,
            "returned_to_codex": true,
            "reason": "already_fallback_to_codex"
        }));
    }

    let deadline = std::time::Instant::now() + timeout;
    let mut last_heartbeat = std::time::Instant::now();
    while std::time::Instant::now() < deadline {
        if last_heartbeat.elapsed() >= CODEX_APPROVAL_HEARTBEAT_INTERVAL {
            if let Err(error) = heartbeat_approval_proxy(local_api_addr, &request.id) {
                eprintln!(
                    "NiumaNotifier approval heartbeat failed request_id={}: {}",
                    request.id, error
                );
            }
            last_heartbeat = std::time::Instant::now();
        }
        match approval_decision(local_api_addr, &request.id) {
            Ok(Some(ApprovalDecisionKind::Allow)) => {
                print_codex_decision(ApprovalDecisionKind::Allow, None);
                return ApiResponse::ok(json!({
                    "request_id": request.id,
                    "decision": "allow",
                    "submitted": true
                }));
            }
            Ok(Some(ApprovalDecisionKind::Deny)) => {
                print_codex_decision(
                    ApprovalDecisionKind::Deny,
                    Some("用户在 NiuMa 中拒绝了本次权限请求".to_string()),
                );
                return ApiResponse::ok(json!({
                    "request_id": request.id,
                    "decision": "deny",
                    "submitted": true
                }));
            }
            Ok(None) => thread::sleep(poll_interval),
            Err(error) => return ApiResponse::fail(ApiErrorCode::ServiceUnavailable, error),
        }
    }

    match return_approval_to_codex(local_api_addr, &request.id) {
        Ok(value) => ApiResponse::ok(json!({
            "request_id": request.id,
            "returned_to_codex": true,
            "local_api": value
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::ServiceUnavailable, error),
    }
}

fn submit_parsed_event(
    store: &NiumaStore,
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

pub(crate) fn codex_permission_decision_stdout(
    decision: ApprovalDecisionKind,
    message: Option<String>,
) -> serde_json::Value {
    let mut decision_value = json!({
        "behavior": match decision {
            ApprovalDecisionKind::Allow => "allow",
            ApprovalDecisionKind::Deny => "deny",
        }
    });
    if let (ApprovalDecisionKind::Deny, Some(message)) = (decision, message) {
        decision_value["message"] = json!(message);
    }
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": decision_value
        }
    })
}

fn print_codex_decision(decision: ApprovalDecisionKind, message: Option<String>) {
    let value = codex_permission_decision_stdout(decision, message);
    println!(
        "{}",
        serde_json::to_string(&value).expect("Codex hook 输出必须可序列化")
    );
}

fn create_approval_request(
    local_api_addr: &str,
    request: &CodexPermissionRequest,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let body = json!({
        "request_id": request.id,
        "tool": "codex",
        "session_id": request.session_id,
        "turn_id": request.turn_id,
        "tool_name": request.tool_name,
        "command": request.command,
        "description": request.description,
        "project_path": request.project_path,
        "project_name": request.project_name,
        "timeout_seconds": timeout.as_secs()
    });
    post_json_envelope(local_api_addr, "/api/v1/approval-requests", body)
}

fn approval_decision(
    local_api_addr: &str,
    request_id: &str,
) -> Result<Option<ApprovalDecisionKind>, String> {
    let value = get_json_envelope(
        local_api_addr,
        &format!("/api/v1/approval-decisions?request_id={request_id}"),
    )?;
    let status = approval_status_from_value(&value["status"])?;
    match status {
        ApprovalStatus::Allowed => Ok(Some(ApprovalDecisionKind::Allow)),
        ApprovalStatus::Denied => Ok(Some(ApprovalDecisionKind::Deny)),
        ApprovalStatus::Pending | ApprovalStatus::ReturnedToCodex => Ok(None),
    }
}

fn return_approval_to_codex(
    local_api_addr: &str,
    request_id: &str,
) -> Result<serde_json::Value, String> {
    post_json_envelope(
        local_api_addr,
        "/api/v1/approval-requests/return",
        json!({
            "request_id": request_id,
            "returned_by": "hook-helper",
            "reason": "10 分钟内未处理，请回到 Codex 中操作"
        }),
    )
}

fn heartbeat_approval_proxy(
    local_api_addr: &str,
    request_id: &str,
) -> Result<serde_json::Value, String> {
    post_json_envelope(
        local_api_addr,
        "/api/v1/approval-requests/heartbeat",
        approval_heartbeat_body(request_id),
    )
}

fn approval_heartbeat_body(request_id: &str) -> serde_json::Value {
    json!({
        "request_id": request_id,
        "source": "hook-helper"
    })
}

fn get_json_envelope(local_api_addr: &str, path: &str) -> Result<serde_json::Value, String> {
    let body = get_local_api(local_api_addr, path)?;
    api_data_from_body(&body)
}

fn post_json_envelope(
    local_api_addr: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let body = post_local_api(local_api_addr, path, Some(&body.to_string()))?;
    api_data_from_body(&body)
}

fn api_data_from_body(body: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| format!("Local API 响应无法解析：{error}"))?;
    if value["code"].as_i64() == Some(0) {
        return Ok(value["data"].clone());
    }
    Err(value["message"]
        .as_str()
        .unwrap_or("Local API 返回失败")
        .to_string())
}

fn approval_status_from_value(value: &serde_json::Value) -> Result<ApprovalStatus, String> {
    match value.as_str().unwrap_or_default() {
        "pending" => Ok(ApprovalStatus::Pending),
        "allowed" => Ok(ApprovalStatus::Allowed),
        "denied" => Ok(ApprovalStatus::Denied),
        "returned_to_codex" => Ok(ApprovalStatus::ReturnedToCodex),
        other => Err(format!("未知授权状态：{other}")),
    }
}

fn approval_create_returned_to_codex(value: &serde_json::Value) -> bool {
    value.get("accepted").and_then(serde_json::Value::as_bool) == Some(false)
        && value.get("hook_action").and_then(serde_json::Value::as_str) == Some("return_to_codex")
}

fn is_hook_event(input: &[u8], expected: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(input)
        .ok()
        .and_then(|value| {
            value
                .get("hook_event_name")
                .and_then(serde_json::Value::as_str)
                .map(|event_name| event_name == expected)
        })
        .unwrap_or(false)
}

fn tool_listening_disabled_reason(tool: &ToolKind) -> String {
    match tool {
        ToolKind::Codex => "codex_listening_disabled".to_string(),
        ToolKind::ClaudeCode => "claude_code_listening_disabled".to_string(),
        ToolKind::Custom(value) => format!("{value}_listening_disabled"),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use niuma_core::listener_config::ListenerConfig;
    use niuma_core::models::{EventType, NiumaEvent, ToolKind};
    use niuma_core::store::NiumaStore;

    use super::*;

    #[test]
    fn codex_allow_decision_stdout_matches_permission_request_schema() {
        let value = codex_permission_decision_stdout(ApprovalDecisionKind::Allow, None);

        assert_eq!(
            value,
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "allow" }
                }
            })
        );
    }

    #[test]
    fn codex_deny_decision_stdout_includes_message() {
        let value = codex_permission_decision_stdout(
            ApprovalDecisionKind::Deny,
            Some("用户拒绝".to_string()),
        );

        assert_eq!(value["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(
            value["hookSpecificOutput"]["decision"]["message"],
            "用户拒绝"
        );
    }

    #[test]
    fn approval_create_response_return_to_codex_is_detected() {
        let value = serde_json::json!({
            "request_id": "approval-1",
            "accepted": false,
            "ownership": "watcher_fallback",
            "hook_action": "return_to_codex",
            "status": "already_fallback"
        });

        assert!(approval_create_returned_to_codex(&value));
    }

    #[test]
    fn approval_heartbeat_body_contains_request_id_and_source() {
        let body = approval_heartbeat_body("approval-1");

        assert_eq!(body["request_id"], "approval-1");
        assert_eq!(body["source"], "hook-helper");
    }

    #[test]
    fn submit_parsed_codex_event_is_skipped_when_listener_disabled() {
        let store = NiumaStore::new(test_sqlite_path("codex_listener_disabled"));
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
        let store = NiumaStore::new(test_sqlite_path("claude_listener_disabled"));
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: true,
                claude_code_listening_enabled: false,
                ..ListenerConfig::default()
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
        let dir = std::env::temp_dir().join(format!(
            "niuma-cli-hook-{name}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("niuma.sqlite")
    }
}
