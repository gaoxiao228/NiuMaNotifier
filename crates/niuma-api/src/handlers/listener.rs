use axum::body::Bytes;
use axum::extract::State;
use axum::response::Response;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::models::ToolId;
use serde_json::json;

use super::shared;
use crate::response::json_response;
use crate::state::AppState;

pub(crate) async fn get_listener_config(State(state): State<AppState>) -> Response {
    match state.store.listener_config() {
        Ok(config) => json_response(
            200,
            ApiResponse::ok(json!({
                "codex_listening_enabled": config.is_tool_enabled(&ToolId::Codex),
                "tool_listening_enabled": config.tool_enabled_map(),
                "tools": shared::listener_tools(&shared::plugin_registry(&state).tools(), &config)
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn save_listener_config(State(state): State<AppState>, body: Bytes) -> Response {
    let value = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(value) => value,
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
    let mut config = match state.store.listener_config() {
        Ok(config) => config,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    if let Some(map) = value
        .get("tool_listening_enabled")
        .and_then(serde_json::Value::as_object)
    {
        for (tool_id, enabled_value) in map {
            let Some(enabled) = enabled_value.as_bool() else {
                return json_response(
                    200,
                    ApiResponse::fail(
                        ApiErrorCode::BusinessValidation,
                        "tool_listening_enabled 的值必须是布尔值",
                    ),
                );
            };
            config = config.with_tool_enabled(&ToolId::from_id(tool_id.clone()), enabled);
        }
    } else {
        let Some(enabled_value) = value.get("codex_listening_enabled") else {
            return json_response(
                200,
                ApiResponse::fail(
                    ApiErrorCode::BusinessValidation,
                    "codex_listening_enabled 不能为空",
                ),
            );
        };
        let Some(enabled) = enabled_value.as_bool() else {
            return json_response(
                200,
                ApiResponse::fail(
                    ApiErrorCode::BusinessValidation,
                    "codex_listening_enabled 必须是布尔值",
                ),
            );
        };
        config = config.with_tool_enabled(&ToolId::Codex, enabled);
    }
    let enabled = config.is_tool_enabled(&ToolId::Codex);
    match state.mutation_service.set_listener_config(config.clone()) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "codex_listening_enabled": enabled,
                "tool_listening_enabled": result.config.tool_enabled_map(),
                "tools": shared::listener_tools(&shared::plugin_registry(&state).tools(), &result.config)
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}
