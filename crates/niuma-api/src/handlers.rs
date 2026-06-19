use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::response::Response;
use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::dashboard::DashboardService;
use niuma_core::main_state::MainStateService;
use niuma_core::models::{EventType, NiumaEvent, ToolId};
use niuma_core::notification_store::{NotificationRecordStatus, PluginNotificationResult};
use niuma_core::plugin::{
    import_external_plugin_dir, remove_external_plugin, resolve_plugin_config,
    validate_plugin_config, PluginKind, PluginManifest, PluginRegistry, PluginSource,
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

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginEnabledRequest {
    plugin_id: String,
    enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginConfigQuery {
    plugin_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginConfigSaveRequest {
    plugin_id: String,
    config: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginNotificationResultRequest {
    plugin_id: String,
    event_id: String,
    status: String,
    title: Option<String>,
    body: Option<String>,
    reason: Option<String>,
    error_message: Option<String>,
    sent_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginNotificationTestResultRequest {
    plugin_id: String,
    test_id: String,
    status: String,
    title: Option<String>,
    body: Option<String>,
    error_message: Option<String>,
    sent_at: Option<String>,
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

pub(crate) async fn post_plugin_notification_result(
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<PluginNotificationResultRequest>(&body) {
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
    match save_plugin_notification_result(&state, request) {
        Ok(record_id) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "record_id": record_id
            })),
        ),
        Err(PluginNotificationResultError::Business(message)) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
        ),
        Err(PluginNotificationResultError::System(message)) => {
            json_response(500, ApiResponse::fail(ApiErrorCode::System, message))
        }
    }
}

pub(crate) async fn post_plugin_notification_test_result(
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<PluginNotificationTestResultRequest>(&body) {
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
    match save_plugin_notification_test_result(&state, request) {
        Ok(record_id) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "record_id": record_id
            })),
        ),
        Err(PluginNotificationResultError::Business(message)) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
        ),
        Err(PluginNotificationResultError::System(message)) => {
            json_response(500, ApiResponse::fail(ApiErrorCode::System, message))
        }
    }
}

pub(crate) async fn get_plugins(State(state): State<AppState>) -> Response {
    match plugin_management_items(&state) {
        Ok(plugins) => json_response(200, ApiResponse::ok(json!({ "list": plugins }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

enum PluginNotificationResultError {
    Business(String),
    System(String),
}

fn save_plugin_notification_result(
    state: &AppState,
    request: PluginNotificationResultRequest,
) -> Result<String, PluginNotificationResultError> {
    let plugin_id = request.plugin_id.trim();
    if plugin_id.is_empty() {
        return Err(PluginNotificationResultError::Business(
            "plugin_id 不能为空".to_string(),
        ));
    }
    let event_id = request.event_id.trim();
    if event_id.is_empty() {
        return Err(PluginNotificationResultError::Business(
            "event_id 不能为空".to_string(),
        ));
    }

    let registry = plugin_registry(state);
    let Some(plugin) = registry.plugin_by_id(plugin_id) else {
        return Err(PluginNotificationResultError::Business(format!(
            "未知插件：{plugin_id}"
        )));
    };
    if plugin.kind != PluginKind::Notification {
        return Err(PluginNotificationResultError::Business(format!(
            "插件 {plugin_id} 不是通知插件"
        )));
    }

    let event = state
        .store
        .public_event_by_id(event_id)
        .map_err(PluginNotificationResultError::System)?
        .ok_or_else(|| {
            PluginNotificationResultError::Business(format!("事件不存在：{event_id}"))
        })?;
    let status = parse_plugin_notification_status(&request.status)?;
    let sent_at = match (status.clone(), request.sent_at.as_deref()) {
        (NotificationRecordStatus::Sent, Some(value)) => {
            Some(parse_rfc3339_time(value, "sent_at")?)
        }
        (NotificationRecordStatus::Sent, None) => Some(Utc::now()),
        (NotificationRecordStatus::Failed, _) => None,
        _ => {
            return Err(PluginNotificationResultError::Business(
                "status 仅支持 sent 或 failed".to_string(),
            ))
        }
    };
    let record_id = plugin_notification_record_id(plugin_id, event_id);
    let result = PluginNotificationResult {
        id: record_id.clone(),
        plugin_id: plugin_id.to_string(),
        event_id: event_id.to_string(),
        event_type: event.event_type,
        status,
        title: trim_optional_string(request.title),
        body: trim_optional_string(request.body),
        reason: trim_optional_string(request.reason),
        error_message: trim_optional_string(request.error_message),
        created_at: Utc::now(),
        sent_at,
    };
    state
        .store
        .save_plugin_notification_result(&result)
        .map_err(PluginNotificationResultError::System)?;
    Ok(record_id)
}

fn save_plugin_notification_test_result(
    state: &AppState,
    request: PluginNotificationTestResultRequest,
) -> Result<String, PluginNotificationResultError> {
    let plugin_id = request.plugin_id.trim();
    if plugin_id.is_empty() {
        return Err(PluginNotificationResultError::Business(
            "plugin_id 不能为空".to_string(),
        ));
    }
    let test_id = request.test_id.trim();
    if test_id.is_empty() {
        return Err(PluginNotificationResultError::Business(
            "test_id 不能为空".to_string(),
        ));
    }

    let registry = plugin_registry(state);
    let Some(plugin) = registry.plugin_by_id(plugin_id) else {
        return Err(PluginNotificationResultError::Business(format!(
            "未知插件：{plugin_id}"
        )));
    };
    if plugin.kind != PluginKind::Notification {
        return Err(PluginNotificationResultError::Business(format!(
            "插件 {plugin_id} 不是通知插件"
        )));
    }

    let status = parse_plugin_notification_status(&request.status)?;
    let sent_at = match (status.clone(), request.sent_at.as_deref()) {
        (NotificationRecordStatus::Sent, Some(value)) => {
            Some(parse_rfc3339_time(value, "sent_at")?)
        }
        (NotificationRecordStatus::Sent, None) => Some(Utc::now()),
        (NotificationRecordStatus::Failed, _) => None,
        _ => {
            return Err(PluginNotificationResultError::Business(
                "status 仅支持 sent 或 failed".to_string(),
            ))
        }
    };
    let record_id = plugin_notification_test_record_id(plugin_id, test_id);
    let result = PluginNotificationResult {
        id: record_id.clone(),
        plugin_id: plugin_id.to_string(),
        event_id: test_id.to_string(),
        event_type: EventType::SessionActivity,
        status,
        title: trim_optional_string(request.title),
        body: trim_optional_string(request.body),
        reason: Some("manual_test".to_string()),
        error_message: trim_optional_string(request.error_message),
        created_at: Utc::now(),
        sent_at,
    };
    state
        .store
        .save_plugin_notification_result(&result)
        .map_err(PluginNotificationResultError::System)?;
    Ok(record_id)
}

fn parse_plugin_notification_status(
    value: &str,
) -> Result<NotificationRecordStatus, PluginNotificationResultError> {
    match value.trim() {
        "sent" => Ok(NotificationRecordStatus::Sent),
        "failed" => Ok(NotificationRecordStatus::Failed),
        _ => Err(PluginNotificationResultError::Business(
            "status 仅支持 sent 或 failed".to_string(),
        )),
    }
}

fn parse_rfc3339_time(
    value: &str,
    field: &str,
) -> Result<chrono::DateTime<Utc>, PluginNotificationResultError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| PluginNotificationResultError::Business(format!("{field} 格式无效")))
}

fn trim_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn plugin_notification_record_id(plugin_id: &str, event_id: &str) -> String {
    format!("plugin_notification:{plugin_id}:{event_id}")
}

fn plugin_notification_test_record_id(plugin_id: &str, test_id: &str) -> String {
    format!("plugin_notification_test:{plugin_id}:{test_id}")
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
    let plugin_enabled_map = match state.store.plugin_enabled_map() {
        Ok(map) => map,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };

    match import_external_plugin_dir(
        std::path::Path::new(&request.source_dir),
        &state.plugin_dir,
        &config,
        &plugin_enabled_map,
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

    let config = match plugin.tool_id.clone() {
        Some(tool) => match state
            .mutation_service
            .set_tool_listening_enabled(tool, false)
        {
            Ok(result) => result.config,
            Err(error) => {
                return json_response(500, ApiResponse::fail(ApiErrorCode::System, error))
            }
        },
        None => match state.store.listener_config() {
            Ok(config) => config,
            Err(error) => {
                return json_response(500, ApiResponse::fail(ApiErrorCode::System, error))
            }
        },
    };
    if let Err(error) = state.store.remove_plugin_runtime_state(&plugin.id) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }
    let runtime_states = match state.store.plugin_runtime_states() {
        Ok(states) => states,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    let plugin_enabled_map = match state.store.plugin_enabled_map() {
        Ok(map) => map,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };

    match remove_external_plugin(
        &request.plugin_id,
        &state.plugin_dir,
        &config,
        &plugin_enabled_map,
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

pub(crate) async fn set_plugin_enabled(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginEnabledRequest>(&body) {
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
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };

    if let Some(tool) = plugin.tool_id {
        if let Err(error) = state
            .mutation_service
            .set_tool_listening_enabled(tool, request.enabled)
        {
            return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
        }
    } else {
        let mut enabled_map = match state.store.plugin_enabled_map() {
            Ok(map) => map,
            Err(error) => {
                return json_response(500, ApiResponse::fail(ApiErrorCode::System, error))
            }
        };
        enabled_map.insert(plugin.id.clone(), request.enabled);
        if let Err(error) = state.store.save_plugin_enabled_map(&enabled_map) {
            return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
        }
        state
            .runtime_events
            .publish_state_changed(StateChangeReason::PluginConfigChanged);
    }

    match plugin_management_items(&state) {
        Ok(plugins) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "plugin_id": request.plugin_id,
                "enabled": request.enabled,
                "plugins": plugins
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_plugin_config(
    State(state): State<AppState>,
    Query(query): Query<PluginConfigQuery>,
) -> Response {
    let registry = plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&query.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", query.plugin_id),
            ),
        );
    };
    match resolved_plugin_config(&state, &plugin) {
        Ok(config) => json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": query.plugin_id,
                "config": config,
                "config_schema": plugin.config_schema
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn save_plugin_config(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginConfigSaveRequest>(&body) {
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
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };
    let Some(config) = request.config.as_object().cloned() else {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, "config 必须是对象"),
        );
    };
    if let Err(error) = validate_plugin_config(&plugin, &config) {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        );
    }
    if let Err(error) = state.store.save_plugin_config(&plugin.id, &config) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }
    state
        .runtime_events
        .publish_state_changed(StateChangeReason::PluginConfigChanged);
    match resolved_plugin_config(&state, &plugin) {
        Ok(saved_config) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "plugin_id": request.plugin_id,
                "config": saved_config,
                "config_schema": plugin.config_schema
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
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

pub(crate) async fn get_notification_records(State(state): State<AppState>) -> Response {
    match state.store.notification_history_records(20) {
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
    let plugin_enabled_map = state.store.plugin_enabled_map()?;
    Ok(plugin_registry(state).management_items(&config, &plugin_enabled_map, &runtime_states))
}

fn plugin_registry(state: &AppState) -> PluginRegistry {
    PluginRegistry::with_builtin_plugins().discover_external_plugins(&state.plugin_dir)
}

fn resolved_plugin_config(
    state: &AppState,
    plugin: &PluginManifest,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let stored_config = state.store.plugin_config(&plugin.id)?;
    Ok(resolve_plugin_config(plugin, stored_config))
}
