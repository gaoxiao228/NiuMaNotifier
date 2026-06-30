use axum::body::Bytes;
use axum::extract::State;
use axum::response::Response;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::models::NiumaEvent;
use serde::Deserialize;
use serde_json::json;

use super::{approval, shared};
use crate::response::json_response;
use crate::state::AppState;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginEventsRequest {
    plugin_id: String,
    events: Vec<NiumaEvent>,
}

pub(crate) async fn post_event(State(state): State<AppState>, body: Bytes) -> Response {
    match serde_json::from_slice::<NiumaEvent>(&body) {
        Ok(event) => {
            if approval::is_codex_watcher_approval(&event) {
                return approval::handle_watcher_approval_event(state, event).await;
            }
            approval::cancel_codex_watcher_approval_if_resolved(&state, &event);
            match approval::resolve_claude_watcher_approval_if_tool_continued(&state, &event) {
                Ok(Some(resolved)) => append_events_response(&state, vec![resolved, event]),
                Ok(None) => append_events_response(&state, vec![event]),
                Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
            }
        }
        Err(error) => json_response(
            400,
            ApiResponse::fail(
                ApiErrorCode::ParameterFormat,
                format!("请求体无法解析：{error}"),
            ),
        ),
    }
}

pub(crate) async fn post_plugin_events(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginEventsRequest>(&body) {
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
    let registry = shared::plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id) else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };
    let Some(plugin_tool) = plugin.tool_id.as_ref() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("插件 {} 不能上报工具事件", plugin.id),
            ),
        );
    };
    if let Some(event) = request
        .events
        .iter()
        .find(|event| &event.tool != plugin_tool)
    {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!(
                    "插件 {} 只能上报 {} 事件，收到 {}",
                    plugin.id,
                    plugin_tool.as_str(),
                    event.tool.as_str()
                ),
            ),
        );
    }

    let mut immediate_events = Vec::new();
    let mut delayed_count = 0usize;
    let mut suppressed_count = 0usize;
    for event in request.events {
        if approval::is_codex_watcher_approval(&event) {
            match approval::arbitrate_watcher_approval_event(&state, event) {
                approval::WatcherApprovalApiOutcome::Apply(event) => immediate_events.push(event),
                approval::WatcherApprovalApiOutcome::Delayed { .. } => delayed_count += 1,
                approval::WatcherApprovalApiOutcome::Suppressed { .. } => suppressed_count += 1,
            }
        } else {
            approval::cancel_codex_watcher_approval_if_resolved(&state, &event);
            match approval::resolve_claude_watcher_approval_if_tool_continued(&state, &event) {
                Ok(Some(resolved)) => {
                    immediate_events.push(resolved);
                    immediate_events.push(event.clone());
                }
                Ok(None) => {}
                Err(error) => {
                    return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
                }
            }
            if !immediate_events
                .iter()
                .any(|item| item.id == event.id && item.dedupe_key == event.dedupe_key)
            {
                immediate_events.push(event);
            }
        }
    }

    if immediate_events.is_empty() {
        return json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": plugin.id,
                "event_count": 0,
                "applied_count": 0,
                "session_count": 0,
                "delayed_count": delayed_count,
                "suppressed_count": suppressed_count
            })),
        );
    }

    match state.mutation_service.append_events(immediate_events) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": plugin.id,
                "event_count": result.state.events.len(),
                "applied_count": result.applied_events.len(),
                "session_count": result.state.runtime_states.len(),
                "delayed_count": delayed_count,
                "suppressed_count": suppressed_count
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

fn append_events_response(state: &AppState, events: Vec<NiumaEvent>) -> Response {
    match state.mutation_service.append_events(events) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "accepted": true,
                "delayed": false,
                "applied": true,
                "event_count": result.state.events.len(),
                "session_count": result.state.runtime_states.len()
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}
