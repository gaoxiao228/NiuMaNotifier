use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::response::Response;
use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::dashboard::DashboardService;
use niuma_core::main_state::MainStateService;
use niuma_core::models::{NiumaEvent, ToolId};
use niuma_core::notification_config::{NotificationConfigErrorKind, NotificationConfigService};
use niuma_core::plugin::{
    import_external_plugin_dir, remove_external_plugin, PluginRegistry, PluginSource,
    ToolPluginInfo,
};
use niuma_core::runtime_event::StateChangeReason;
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginEventsRequest {
    plugin_id: String,
    events: Vec<NiumaEvent>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginImportRequest {
    source_dir: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginRemoveRequest {
    plugin_id: String,
}

#[derive(Clone, Debug, Serialize)]
struct ListenerToolView {
    id: String,
    plugin_id: String,
    display_name: String,
    enabled: bool,
    source: String,
    icon_url: Option<String>,
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
            )
        }
    };
    let registry = plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id) else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };
    if let Some(event) = request
        .events
        .iter()
        .find(|event| event.tool != plugin.tool_id)
    {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!(
                    "插件 {} 只能上报 {} 事件，收到 {}",
                    plugin.id,
                    plugin.tool_id.as_str(),
                    event.tool.as_str()
                ),
            ),
        );
    }

    match state.mutation_service.append_events(request.events) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": plugin.id,
                "event_count": result.state.events.len(),
                "applied_count": result.applied_events.len(),
                "session_count": result.state.sessions.len()
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_plugins(State(state): State<AppState>) -> Response {
    match plugin_management_items(&state) {
        Ok(plugins) => json_response(200, ApiResponse::ok(json!({ "list": plugins }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn import_plugin(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginImportRequest>(&body) {
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
    let config = match state.store.listener_config() {
        Ok(config) => config,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    let runtime_states = match state.store.plugin_runtime_states() {
        Ok(states) => states,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };

    match import_external_plugin_dir(
        std::path::Path::new(&request.source_dir),
        &state.plugin_dir,
        &config,
        &runtime_states,
    ) {
        Ok(result) => {
            state
                .runtime_events
                .publish_state_changed(StateChangeReason::ListenerConfigChanged);
            json_response(200, ApiResponse::ok(json!(result)))
        }
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

pub(crate) async fn remove_plugin(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginRemoveRequest>(&body) {
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
    let registry = plugin_registry(&state);
    if PluginRegistry::with_builtin_plugins()
        .plugin_by_id(&request.plugin_id)
        .is_some()
    {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("不能移除内置插件：{}", request.plugin_id),
            ),
        );
    }
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };
    if plugin.source == PluginSource::Builtin {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("不能移除内置插件：{}", request.plugin_id),
            ),
        );
    }

    let config = match state
        .mutation_service
        .set_tool_listening_enabled(plugin.tool_id.clone(), false)
    {
        Ok(result) => result.config,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    if let Err(error) = state.store.remove_plugin_runtime_state(&plugin.id) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }
    let runtime_states = match state.store.plugin_runtime_states() {
        Ok(states) => states,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };

    match remove_external_plugin(
        &request.plugin_id,
        &state.plugin_dir,
        &config,
        &runtime_states,
    ) {
        Ok(result) => {
            state
                .runtime_events
                .publish_state_changed(StateChangeReason::ListenerConfigChanged);
            json_response(200, ApiResponse::ok(json!(result)))
        }
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
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
                "codex_listening_enabled": config.is_tool_enabled(&ToolId::Codex),
                "tool_listening_enabled": config.tool_enabled_map(),
                "tools": listener_tools(&plugin_registry(&state).tools(), &config)
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
                "tools": listener_tools(&plugin_registry(&state).tools(), &result.config)
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

fn listener_tools(
    plugins: &[ToolPluginInfo],
    config: &niuma_core::listener_config::ListenerConfig,
) -> Vec<ListenerToolView> {
    plugins
        .iter()
        .map(|plugin| ListenerToolView {
            id: plugin.tool_id.as_str().to_string(),
            plugin_id: plugin.id.clone(),
            display_name: plugin.display_name.clone(),
            enabled: config.is_tool_enabled(&plugin.tool_id),
            source: format!("{:?}", plugin.source).to_lowercase(),
            icon_url: plugin.icon_url.clone(),
        })
        .collect()
}

fn plugin_management_items(
    state: &AppState,
) -> Result<Vec<niuma_core::plugin::PluginManagementItem>, String> {
    let config = state.store.listener_config()?;
    let runtime_states = state.store.plugin_runtime_states()?;
    Ok(plugin_registry(state).management_items(&config, &runtime_states))
}

fn plugin_registry(state: &AppState) -> PluginRegistry {
    PluginRegistry::with_builtin_plugins().discover_external_plugins(&state.plugin_dir)
}
