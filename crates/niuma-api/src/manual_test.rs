use axum::body::Bytes;
use axum::extract::State;
use axum::response::Response;
use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::config;
use niuma_core::models::{EventType, NiumaEvent, ToolKind};
use serde::Deserialize;
use serde_json::json;

use crate::response::json_response;
use crate::state::AppState;

#[derive(Clone, Debug, Deserialize)]
struct ManualTestScenarioRequest {
    scenario: String,
    sessions: Vec<ManualTestSessionRequest>,
}

#[derive(Clone, Debug, Deserialize)]
struct ManualTestSessionRequest {
    session_id: String,
    tool: String,
    project_name: String,
    project_path: String,
    status: String,
    summary: String,
}

pub(crate) async fn manual_test_scenario(State(state): State<AppState>, body: Bytes) -> Response {
    if !manual_test_enabled() {
        return json_response(
            404,
            ApiResponse::fail(ApiErrorCode::RouteNotFound, "接口不存在"),
        );
    }

    let request = match serde_json::from_slice::<ManualTestScenarioRequest>(&body) {
        Ok(request) => request,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterFormat,
                    format!("请求体无法解析：{error}"),
                ),
            );
        }
    };

    if request.sessions.is_empty() {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, "至少启用一个 session"),
        );
    }

    let events = match manual_test_events(request) {
        Ok(events) => events,
        Err(message) => {
            return json_response(
                400,
                ApiResponse::fail(ApiErrorCode::ParameterFormat, message),
            );
        }
    };

    match state.mutation_service.append_events(events) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "event_count": result.state.events.len(),
                "session_count": result.state.runtime_states.len()
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

fn manual_test_enabled() -> bool {
    config::manual_test_enabled(cfg!(debug_assertions))
}

fn manual_test_events(request: ManualTestScenarioRequest) -> Result<Vec<NiumaEvent>, String> {
    let now = Utc::now();
    let timestamp = now.timestamp_millis();
    let scenario = non_empty_or(request.scenario, "manual");
    request
        .sessions
        .into_iter()
        .enumerate()
        .map(|(index, session)| {
            let tool = parse_manual_tool(&session.tool)?;
            let event_type = event_type_from_status(&session.status)?;
            let session_id = non_empty_or(session.session_id, format!("manual-{index}"));
            let project_name = non_empty_or(session.project_name, format!("Manual Test {index}"));
            let project_path = non_empty_or(
                session.project_path,
                format!("/manual-test/{scenario}/{session_id}"),
            );
            let summary = non_empty_or(session.summary, format!("Manual test {scenario}"));
            Ok(NiumaEvent {
                id: format!("event_manual_test_{timestamp}_{index}_{session_id}"),
                dedupe_key: format!(
                    "manual_test:{scenario}:{session_id}:{}:{timestamp}:{index}",
                    session.status
                ),
                source: "manual_test".to_string(),
                tool,
                session_id,
                project_path,
                project_name,
                event_type,
                severity: severity_from_status(&session.status).to_string(),
                summary: summary.clone(),
                content: content_from_status(&session.status, &summary),
                error_message: error_message_from_status(&session.status, &summary),
                attention_resolve_key: None,
                completion_reason: None,
                failure_reason: None,
                payload_ref: None,
                created_at: now + chrono::Duration::milliseconds(index as i64),
            })
        })
        .collect()
}

fn parse_manual_tool(value: &str) -> Result<ToolKind, String> {
    match value.trim().to_lowercase().as_str() {
        "codex" => Ok(ToolKind::Codex),
        "claude_code" | "claude-code" | "claude" => Ok(ToolKind::ClaudeCode),
        other => Err(format!("tool 参数不合法：{other}")),
    }
}

fn event_type_from_status(value: &str) -> Result<EventType, String> {
    match value.trim().to_lowercase().as_str() {
        "running" => Ok(EventType::SessionStarted),
        "waiting_approval" => Ok(EventType::ApprovalRequested),
        "waiting_input" => Ok(EventType::InputRequested),
        "error" => Ok(EventType::TaskFailed),
        "completed" => Ok(EventType::AssistantMessageCompleted),
        "idle" => Ok(EventType::SessionIdled),
        other => Err(format!("status 参数不合法：{other}")),
    }
}

fn severity_from_status(value: &str) -> &'static str {
    match value {
        "waiting_approval" | "waiting_input" | "error" => "urgent",
        _ => "info",
    }
}

fn content_from_status(status: &str, summary: &str) -> Option<String> {
    matches!(status, "waiting_approval" | "waiting_input" | "completed")
        .then(|| summary.to_string())
}

fn error_message_from_status(status: &str, summary: &str) -> Option<String> {
    (status == "error").then(|| summary.to_string())
}

fn non_empty_or(value: String, fallback: impl Into<String>) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.into()
    } else {
        trimmed.to_string()
    }
}
