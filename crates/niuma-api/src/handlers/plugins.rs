use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::response::Response;
use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_hook::{
    install_codex_hook, uninstall_codex_hook, CodexHookCommand, CodexHookStatus,
};
use niuma_core::config::codex_home;
use niuma_core::models::EventType;
use niuma_core::notification_store::{NotificationRecordStatus, PluginNotificationResult};
use niuma_core::plugin::{
    import_external_plugin_dir, listener_config_after_plugin_removed, plugin_uses_listener_config,
    remove_external_plugin, save_plugin_enabled_state, validate_plugin_config, PluginKind,
    PluginRegistry, PluginSource, BUILTIN_CODEX_PLUGIN_ID,
};
use niuma_core::runtime_event::StateChangeReason;
use serde::Deserialize;
use serde_json::json;

use super::shared;
use crate::response::json_response;
use crate::state::AppState;

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
pub(crate) struct PluginActionRequest {
    plugin_id: String,
    action_id: String,
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
            );
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
            );
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
    match shared::plugin_management_items(&state) {
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

    let registry = shared::plugin_registry(state);
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
            ));
        }
    };
    let record_id = plugin_notification_record_id(plugin_id, event_id);
    let result = PluginNotificationResult {
        id: record_id.clone(),
        plugin_id: plugin_id.to_string(),
        event_id: event_id.to_string(),
        event_type: event.event_type,
        status,
        title: shared::trim_optional_string(request.title),
        body: shared::trim_optional_string(request.body),
        reason: shared::trim_optional_string(request.reason),
        error_message: shared::trim_optional_string(request.error_message),
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

    let registry = shared::plugin_registry(state);
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
            ));
        }
    };
    let record_id = plugin_notification_test_record_id(plugin_id, test_id);
    let result = PluginNotificationResult {
        id: record_id.clone(),
        plugin_id: plugin_id.to_string(),
        event_id: test_id.to_string(),
        event_type: EventType::SessionActivity,
        status,
        title: shared::trim_optional_string(request.title),
        body: shared::trim_optional_string(request.body),
        reason: Some("manual_test".to_string()),
        error_message: shared::trim_optional_string(request.error_message),
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
            );
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
        Ok(mut result) => {
            let registry = shared::plugin_registry(&state);
            let Some(manifest) = registry.plugin_by_id(&result.plugin.id).cloned() else {
                return json_response(
                    500,
                    ApiResponse::fail(
                        ApiErrorCode::System,
                        format!("插件导入后未被发现：{}", result.plugin.id),
                    ),
                );
            };
            if let Err(error) =
                save_plugin_enabled_state(&state.store, &state.mutation_service, &manifest, true)
            {
                return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
            }
            let plugins = match shared::plugin_management_items(&state) {
                Ok(plugins) => plugins,
                Err(error) => {
                    return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
                }
            };
            if let Some(plugin) = plugins.iter().find(|item| item.id == result.plugin.id) {
                result.plugin = plugin.clone();
            }
            result.plugins = plugins;
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
            );
        }
    };
    let registry = shared::plugin_registry(&state);
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

    let config = match listener_config_after_plugin_removed(
        &state.store,
        &state.mutation_service,
        &plugin,
    ) {
        Ok(config) => config,
        Err(error) => {
            return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
        }
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
            );
        }
    };
    let registry = shared::plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };

    let uses_listener_config = plugin_uses_listener_config(&plugin);
    if let Err(error) = save_plugin_enabled_state(
        &state.store,
        &state.mutation_service,
        &plugin,
        request.enabled,
    ) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }
    if !uses_listener_config {
        // 非 event_watcher 插件的启用状态不会触发 listener 变更，需要显式刷新运行管理。
        state
            .runtime_events
            .publish_state_changed(StateChangeReason::PluginConfigChanged);
    }

    match shared::plugin_management_items(&state) {
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

pub(crate) async fn run_plugin_action(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginActionRequest>(&body) {
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
    if plugin.id != BUILTIN_CODEX_PLUGIN_ID {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("插件 {} 不支持管理动作", request.plugin_id),
            ),
        );
    }

    // 插件管理动作走后端 allowlist，避免外部插件通过 manifest 注入任意本机命令。
    let action_result = match request.action_id.as_str() {
        "codex_hook_install" => {
            install_codex_hook(&codex_home(), codex_hook_command_mode()).map(|status| {
                (
                    "Hook 已安装，请在 Codex 中执行 /hooks 信任 Niuma Hook",
                    status,
                )
            })
        }
        "codex_hook_uninstall" => {
            uninstall_codex_hook(&codex_home()).map(|status| ("Hook 已移除", status))
        }
        _ => Err(format!(
            "未知插件动作：{} / {}",
            request.plugin_id, request.action_id
        )),
    };
    let (message, status) = match action_result {
        Ok(result) => result,
        Err(error)
            if request.action_id != "codex_hook_install"
                && request.action_id != "codex_hook_uninstall" =>
        {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
            );
        }
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    match shared::plugin_management_items(&state) {
        Ok(plugins) => json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": request.plugin_id,
                "action_id": request.action_id,
                "message": message,
                "status": codex_hook_status_json(status),
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
    let registry = shared::plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&query.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", query.plugin_id),
            ),
        );
    };
    match shared::resolved_plugin_config(&state, &plugin) {
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

fn codex_hook_status_json(status: CodexHookStatus) -> serde_json::Value {
    json!(status)
}

fn codex_hook_command_mode() -> CodexHookCommand {
    if niuma_core::platform::executable::command_on_path("niuma") {
        CodexHookCommand::Installed
    } else {
        CodexHookCommand::Dev {
            manifest_path: repo_manifest_path(),
        }
    }
}

fn repo_manifest_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("Cargo.toml")
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
            );
        }
    };
    let registry = shared::plugin_registry(&state);
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
    match shared::resolved_plugin_config(&state, &plugin) {
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
