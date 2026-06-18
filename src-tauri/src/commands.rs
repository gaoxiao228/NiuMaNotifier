use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::dashboard::DashboardService;
use niuma_core::main_state::MainStateService;
use niuma_core::models::ToolId;
use niuma_core::notification_config::{NotificationConfigErrorKind, NotificationConfigService};
use niuma_core::platform::locale::{
    active_language, active_language_preference, set_active_language_preference, LanguagePreference,
};
use niuma_core::plugin::{default_user_plugin_dir, PluginRegistry, ToolPluginInfo};
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::SqliteStateStore;
use serde_json::json;

#[derive(Clone)]
pub(crate) struct AppRuntimeState {
    pub(crate) mutation_service: StateMutationService,
}

#[tauri::command]
pub(crate) fn get_main_state() -> ApiResponse<serde_json::Value> {
    match MainStateService::new(default_store()).current_state(Utc::now()) {
        Ok(state) => ApiResponse::ok(json!({ "state": state })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn get_recent_events() -> ApiResponse<serde_json::Value> {
    match dashboard_service().recent_events(10) {
        Ok(events) => ApiResponse::ok(json!({ "list": events })),
        Err(error) => ApiResponse::ok(json!({
            "list": [],
            "warning": error
        })),
    }
}

#[tauri::command]
pub(crate) fn get_sessions() -> ApiResponse<serde_json::Value> {
    match dashboard_service().sessions() {
        Ok(sessions) => ApiResponse::ok(json!({ "list": sessions })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn get_local_api_url() -> ApiResponse<serde_json::Value> {
    ApiResponse::ok(json!({
        "url": format!("http://{}", niuma_api::local_api_addr())
    }))
}

#[tauri::command]
pub(crate) fn get_active_language() -> ApiResponse<serde_json::Value> {
    if let Err(error) = restore_language_preference_from_store() {
        return ApiResponse::fail(ApiErrorCode::System, error);
    }
    ApiResponse::ok(json!({
        "language": active_language().storage_id(),
        "preference": active_language_preference().storage_id()
    }))
}

#[tauri::command]
pub(crate) fn save_language_preference(language: String) -> ApiResponse<serde_json::Value> {
    let Some(preference) = LanguagePreference::from_storage_id(&language) else {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, "语言不受支持");
    };
    let store = default_store();
    match store.save_language_preference(preference) {
        Ok(()) => {
            set_active_language_preference(preference);
            ApiResponse::ok(json!({
                "saved": true,
                "language": active_language().storage_id(),
                "preference": active_language_preference().storage_id()
            }))
        }
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn get_listener_config() -> ApiResponse<serde_json::Value> {
    get_listener_config_from_store(default_store())
}

#[tauri::command]
pub(crate) fn save_listener_config(
    runtime_state: tauri::State<'_, AppRuntimeState>,
    codex_listening_enabled: bool,
) -> ApiResponse<serde_json::Value> {
    save_listener_config_with_service(&runtime_state.mutation_service, codex_listening_enabled)
}

#[tauri::command]
pub(crate) fn get_notification_config() -> ApiResponse<serde_json::Value> {
    match NotificationConfigService::new(default_store()).channels() {
        Ok(channels) => ApiResponse::ok(json!({ "channels": channels })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn save_notification_config(
    channels: Vec<serde_json::Value>,
) -> ApiResponse<serde_json::Value> {
    // Tauri 调用形态保持为 invoke('save_notification_config', { channels })，
    // 内部再转换成 Local API 共用的 {"channels": [...]} 结构做统一校验。
    let value = json!({ "channels": channels });
    match NotificationConfigService::new(default_store()).save_from_value(&value) {
        Ok(_) => ApiResponse::ok(json!({ "saved": true })),
        Err(error) => match error.kind() {
            NotificationConfigErrorKind::BusinessValidation => {
                ApiResponse::fail(ApiErrorCode::BusinessValidation, error.message())
            }
            NotificationConfigErrorKind::System => {
                ApiResponse::fail(ApiErrorCode::System, error.message())
            }
        },
    }
}

#[tauri::command]
pub(crate) fn get_notification_records() -> ApiResponse<serde_json::Value> {
    match default_store().notification_records(20) {
        Ok(records) => ApiResponse::ok(json!({ "list": records })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn send_test_notification(channel: String) -> ApiResponse<serde_json::Value> {
    match crate::notification_runtime::send_test_notification(
        default_store(),
        channel,
        crate::notification_runtime::UreqNotificationSender::default(),
    ) {
        Ok(result) => ApiResponse::ok(json!(result)),
        Err(error) => ApiResponse::fail(error.api_error_code(), error.message()),
    }
}

#[tauri::command]
pub(crate) fn dismiss_active_blocker(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    match runtime_state.mutation_service.dismiss_active_blocker() {
        Ok(Some(result)) => ApiResponse::ok(json!({
            "dismissed": true,
            "dismissed_count": result.dismissed_count,
            "event": result.event
        })),
        Ok(None) => ApiResponse::ok(json!({
            "dismissed": false,
            "dismissed_count": 0
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn get_listener_config_from_store(store: SqliteStateStore) -> ApiResponse<serde_json::Value> {
    match store.listener_config() {
        Ok(config) => ApiResponse::ok(json!({
            "codex_listening_enabled": config.is_tool_enabled(&ToolId::Codex),
            "tool_listening_enabled": config.tool_enabled_map(),
            "tools": listener_tools(&config)
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn save_listener_config_with_service(
    service: &StateMutationService,
    codex_listening_enabled: bool,
) -> ApiResponse<serde_json::Value> {
    match service.set_codex_listening_enabled(codex_listening_enabled) {
        Ok(result) => ApiResponse::ok(json!({
            "saved": true,
            "codex_listening_enabled": result.config.is_tool_enabled(&ToolId::Codex),
            "tool_listening_enabled": result.config.tool_enabled_map(),
            "tools": listener_tools(&result.config)
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn listener_tools(config: &niuma_core::listener_config::ListenerConfig) -> Vec<serde_json::Value> {
    let registry = PluginRegistry::with_builtin_plugins()
        .discover_external_plugins(&default_user_plugin_dir());
    registry
        .tools()
        .iter()
        .map(|plugin| listener_tool(plugin, config))
        .collect()
}

fn listener_tool(
    plugin: &ToolPluginInfo,
    config: &niuma_core::listener_config::ListenerConfig,
) -> serde_json::Value {
    json!({
        "id": plugin.tool_id.as_str(),
        "plugin_id": plugin.id,
        "display_name": plugin.display_name,
        "enabled": config.is_tool_enabled(&plugin.tool_id),
        "source": format!("{:?}", plugin.source).to_lowercase(),
        "icon_url": plugin.icon_url
    })
}

fn dashboard_service() -> DashboardService {
    DashboardService::new(default_store())
}

pub(crate) fn restore_language_preference_from_store() -> Result<(), String> {
    let preference = default_store().language_preference()?;
    set_active_language_preference(preference);
    Ok(())
}

fn default_store() -> SqliteStateStore {
    SqliteStateStore::new(SqliteStateStore::default_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::runtime_event::RuntimeEventBus;
    use std::path::PathBuf;

    #[test]
    fn listener_config_helpers_default_disabled_and_save_enabled() {
        let store = SqliteStateStore::new(test_sqlite_path("listener_config_helpers"));
        let service = StateMutationService::new(store.clone(), RuntimeEventBus::new());

        let default_config = get_listener_config_from_store(store.clone());
        let save = save_listener_config_with_service(&service, true);
        let reloaded = get_listener_config_from_store(store);

        assert_eq!(default_config.code, 0);
        assert_eq!(default_config.data["codex_listening_enabled"], false);
        assert_eq!(save.code, 0);
        assert_eq!(save.data["saved"], true);
        assert_eq!(save.data["codex_listening_enabled"], true);
        assert_eq!(reloaded.data["codex_listening_enabled"], true);
    }

    fn test_sqlite_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "niuma-tauri-commands-{name}-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }
}
