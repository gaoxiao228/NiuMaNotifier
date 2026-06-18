use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::response::Response;
use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::dashboard::DashboardService;
use niuma_core::main_state::MainStateService;
use niuma_core::models::NiumaEvent;
use niuma_core::notification_config::{NotificationConfigErrorKind, NotificationConfigService};
use serde::Deserialize;
use serde_json::json;

use crate::response::json_response;
use crate::state::AppState;

const RESET_CONFIRMATION: &str = "RESET_NIUMA_STATE";

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct EventsQuery {
    limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ResetStateRequest {
    confirm: String,
    #[allow(dead_code)]
    reason: Option<String>,
}

pub(crate) async fn get_main_state(State(state): State<AppState>) -> Response {
    match MainStateService::new(state.store).current_state(Utc::now()) {
        Ok(main_state) => json_response(200, ApiResponse::ok(json!({ "state": main_state }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_events(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> Response {
    match DashboardService::new(state.store).recent_events(query.limit.unwrap_or(50)) {
        Ok(events) => json_response(200, ApiResponse::ok(json!({ "list": events }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_sessions(State(state): State<AppState>) -> Response {
    match DashboardService::new(state.store).sessions() {
        Ok(sessions) => json_response(200, ApiResponse::ok(json!({ "list": sessions }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn post_event(State(state): State<AppState>, body: Bytes) -> Response {
    match serde_json::from_slice::<NiumaEvent>(&body) {
        Ok(event) => match state.mutation_service.append_events(vec![event]) {
            Ok(result) => json_response(
                200,
                ApiResponse::ok(json!({
                    "event_count": result.state.events.len(),
                    "session_count": result.state.sessions.len()
                })),
            ),
            Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
        },
        Err(error) => json_response(
            400,
            ApiResponse::fail(
                ApiErrorCode::ParameterFormat,
                format!("请求体无法解析：{error}"),
            ),
        ),
    }
}

pub(crate) async fn dismiss_blocker(State(state): State<AppState>) -> Response {
    match state.mutation_service.dismiss_active_blocker() {
        Ok(Some(result)) => json_response(
            200,
            ApiResponse::ok(json!({
                "dismissed": true,
                "dismissed_count": result.dismissed_count,
                "event": result.event
            })),
        ),
        Ok(None) => json_response(
            200,
            ApiResponse::ok(json!({
                "dismissed": false,
                "dismissed_count": 0
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn reset_state(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<ResetStateRequest>(&body) {
        Ok(request) => request,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterFormat,
                    format!("请求体无法解析：{error}"),
                ),
            )
        }
    };
    if request.confirm != RESET_CONFIRMATION {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                "confirm 必须为 RESET_NIUMA_STATE",
            ),
        );
    }
    let reset_at = Utc::now();
    match state.mutation_service.reset() {
        Ok(stored) => match MainStateService::new(state.store).current_state(reset_at) {
            Ok(main_state) => json_response(
                200,
                ApiResponse::ok(json!({
                    "reset": true,
                    "reset_at": reset_at,
                    "event_count": stored.events.len(),
                    "session_count": stored.sessions.len(),
                    "state": main_state
                })),
            ),
            Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
        },
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_notification_config(State(state): State<AppState>) -> Response {
    match NotificationConfigService::new(state.store).channels() {
        Ok(channels) => json_response(200, ApiResponse::ok(json!({ "channels": channels }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_listener_config(State(state): State<AppState>) -> Response {
    match state.store.listener_config() {
        Ok(config) => json_response(
            200,
            ApiResponse::ok(json!({
                "codex_listening_enabled": config.codex_listening_enabled
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
            )
        }
    };
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
    match state.mutation_service.set_codex_listening_enabled(enabled) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "codex_listening_enabled": result.config.codex_listening_enabled
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn save_notification_config(
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let value = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(value) => value,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterFormat,
                    format!("请求体无法解析：{error}"),
                ),
            )
        }
    };
    match NotificationConfigService::new(state.store).save_from_value(&value) {
        Ok(_) => json_response(200, ApiResponse::ok(json!({ "saved": true }))),
        Err(error) => match error.kind() {
            NotificationConfigErrorKind::BusinessValidation => json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, error.message()),
            ),
            NotificationConfigErrorKind::System => json_response(
                500,
                ApiResponse::fail(ApiErrorCode::System, error.message()),
            ),
        },
    }
}

pub(crate) async fn get_notification_records(State(state): State<AppState>) -> Response {
    match state.store.notification_records(20) {
        Ok(records) => json_response(200, ApiResponse::ok(json!({ "list": records }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}
