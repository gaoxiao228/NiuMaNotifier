use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_hook::{
    install_codex_hook, uninstall_codex_hook, CodexHookCommand, CodexHookStatus,
};
use niuma_core::config::codex_home;
use niuma_core::dashboard::DashboardService;
use niuma_core::main_state::MainStateService;
use niuma_core::models::ToolId;
use niuma_core::platform::locale::{
    active_language, active_language_preference, set_active_language_preference, LanguagePreference,
};
use niuma_core::plugin::{
    current_plugin_registry, default_user_plugin_dir, import_external_plugin_dir,
    remove_external_plugin, resolve_plugin_config, save_plugin_enabled_state,
    validate_plugin_config, PluginCapability, PluginKind, PluginManagementItem, PluginManifest,
    PluginRegistry, PluginRuntimeStatus, PluginSource, ToolPluginInfo, BUILTIN_CODEX_PLUGIN_ID,
};
use niuma_core::runtime_event::{
    PluginNotificationTestRequest, RuntimeEventBus, StateChangeReason,
};
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;
use serde_json::json;
use std::collections::BTreeMap;
use std::thread;
use std::time::{Duration, Instant};

const NOTIFICATION_TEST_TIMEOUT: Duration = Duration::from_secs(15);
const NOTIFICATION_TEST_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone)]
pub(crate) struct AppRuntimeState {
    pub(crate) store: NiumaStore,
    pub(crate) mutation_service: StateMutationService,
    pub(crate) runtime_events: RuntimeEventBus,
}

#[tauri::command]
pub(crate) fn get_main_state(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    match MainStateService::new(runtime_state.store.clone()).current_state(Utc::now()) {
        Ok(state) => ApiResponse::ok(json!({ "state": state })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn get_recent_events(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    match DashboardService::new(runtime_state.store.clone()).recent_events(10) {
        Ok(events) => ApiResponse::ok(json!({ "list": events })),
        Err(error) => ApiResponse::ok(json!({
            "list": [],
            "warning": error
        })),
    }
}

#[tauri::command]
pub(crate) fn get_sessions(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    match DashboardService::new(runtime_state.store.clone()).sessions() {
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
    tool_listening_enabled: Option<BTreeMap<String, bool>>,
) -> ApiResponse<serde_json::Value> {
    save_listener_config_with_service(
        &runtime_state.mutation_service,
        codex_listening_enabled,
        tool_listening_enabled,
    )
}

#[tauri::command]
pub(crate) fn get_plugins(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    match plugin_management_items_from_store(&runtime_state.store) {
        Ok(plugins) => ApiResponse::ok(json!({ "list": plugins })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn select_and_import_plugin_dir(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    let Some(path) = rfd::FileDialog::new().pick_folder() else {
        return ApiResponse::ok(json!({
            "imported": false,
            "cancelled": true,
            "plugins": plugin_management_items_from_store(&runtime_state.store).unwrap_or_default()
        }));
    };
    let response =
        import_plugin_dir_from_path(&runtime_state.store, &runtime_state.mutation_service, path);
    if response.code == 0 {
        runtime_state
            .runtime_events
            .publish_state_changed(StateChangeReason::ListenerConfigChanged);
    }
    response
}

#[tauri::command]
pub(crate) fn remove_plugin(
    runtime_state: tauri::State<'_, AppRuntimeState>,
    plugin_id: String,
) -> ApiResponse<serde_json::Value> {
    remove_plugin_by_id(
        &runtime_state.store,
        &runtime_state.mutation_service,
        &runtime_state.runtime_events,
        &plugin_id,
    )
}

#[tauri::command]
pub(crate) fn set_plugin_enabled(
    runtime_state: tauri::State<'_, AppRuntimeState>,
    plugin_id: String,
    enabled: bool,
) -> ApiResponse<serde_json::Value> {
    set_plugin_enabled_by_id(
        &runtime_state.store,
        &runtime_state.mutation_service,
        &runtime_state.runtime_events,
        &plugin_id,
        enabled,
    )
}

#[tauri::command]
pub(crate) fn run_plugin_action(
    runtime_state: tauri::State<'_, AppRuntimeState>,
    plugin_id: String,
    action_id: String,
) -> ApiResponse<serde_json::Value> {
    run_plugin_action_by_id(&runtime_state.store, &plugin_id, &action_id)
}

#[tauri::command]
pub(crate) fn get_plugin_config(plugin_id: String) -> ApiResponse<serde_json::Value> {
    get_plugin_config_by_id(&plugin_id)
}

fn run_plugin_action_by_id(
    store: &NiumaStore,
    plugin_id: &str,
    action_id: &str,
) -> ApiResponse<serde_json::Value> {
    let registry = current_plugin_registry();
    let Some(plugin) = registry.plugin_by_id(plugin_id) else {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("未知插件：{plugin_id}"),
        );
    };
    if plugin.id != BUILTIN_CODEX_PLUGIN_ID {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("插件 {plugin_id} 不支持管理动作"),
        );
    }
    // 桌面端 fallback 与 Local API 使用同一组受控动作，避免插件自定义命令直连系统配置。
    let action_result = match action_id {
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
        _ => Err(format!("未知插件动作：{plugin_id} / {action_id}")),
    };
    let (message, status) = match action_result {
        Ok(result) => result,
        Err(error) if action_id != "codex_hook_install" && action_id != "codex_hook_uninstall" => {
            return ApiResponse::fail(ApiErrorCode::BusinessValidation, error);
        }
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    match plugin_management_items_from_store(store) {
        Ok(plugins) => ApiResponse::ok(json!({
            "plugin_id": plugin_id,
            "action_id": action_id,
            "message": message,
            "status": codex_hook_status_json(status),
            "plugins": plugins
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
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
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("Cargo.toml")
}

#[tauri::command]
pub(crate) fn save_plugin_config(
    runtime_state: tauri::State<'_, AppRuntimeState>,
    plugin_id: String,
    config: serde_json::Value,
) -> ApiResponse<serde_json::Value> {
    save_plugin_config_by_id(&runtime_state.runtime_events, &plugin_id, config)
}

#[tauri::command]
pub(crate) fn get_notification_records() -> ApiResponse<serde_json::Value> {
    match default_store().notification_history_records(20) {
        Ok(records) => ApiResponse::ok(json!({ "list": records })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn send_test_notification(
    runtime_state: tauri::State<'_, AppRuntimeState>,
    plugin_id: String,
) -> ApiResponse<serde_json::Value> {
    request_plugin_test_notification(&runtime_state.runtime_events, &plugin_id)
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

fn get_listener_config_from_store(store: NiumaStore) -> ApiResponse<serde_json::Value> {
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
    tool_listening_enabled: Option<BTreeMap<String, bool>>,
) -> ApiResponse<serde_json::Value> {
    let result = if let Some(tool_map) = tool_listening_enabled {
        let mut config = match default_store().listener_config() {
            Ok(config) => config,
            Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
        };
        // Tauri 回退路径也支持动态工具表，避免外部插件开关只能依赖 Local API。
        for (tool_id, enabled) in tool_map {
            config = config.with_tool_enabled(&ToolId::from_id(tool_id), enabled);
        }
        service.set_listener_config(config)
    } else {
        service.set_codex_listening_enabled(codex_listening_enabled)
    };

    match result {
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

fn plugin_management_items_from_store(
    store: &NiumaStore,
) -> Result<Vec<PluginManagementItem>, String> {
    let config = store.listener_config()?;
    let runtime_states = store.plugin_runtime_states()?;
    let plugin_enabled_map = store.plugin_enabled_map()?;
    Ok(current_plugin_registry().management_items(&config, &plugin_enabled_map, &runtime_states))
}

fn request_plugin_test_notification(
    runtime_events: &RuntimeEventBus,
    plugin_id: &str,
) -> ApiResponse<serde_json::Value> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, "plugin_id 不能为空");
    }
    let store = default_store();
    let registry = current_plugin_registry();
    let Some(plugin) = registry.plugin_by_id(plugin_id).cloned() else {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("未知插件：{plugin_id}"),
        );
    };
    if plugin.kind != PluginKind::Notification {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("插件 {plugin_id} 不是通知插件"),
        );
    }
    if !plugin
        .capabilities
        .contains(&PluginCapability::NotificationTest)
    {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("通知插件 {plugin_id} 不支持测试通知"),
        );
    }
    if let Err(error) = validate_notification_test_plugin(&store, &plugin) {
        return error;
    }

    let now = Utc::now();
    let test_id = format!(
        "manual-test:{}:{}",
        plugin_id,
        now.timestamp_nanos_opt()
            .unwrap_or_else(|| now.timestamp_micros())
    );
    let request = PluginNotificationTestRequest {
        test_id: test_id.clone(),
        plugin_id: plugin_id.to_string(),
        title: "NiuMa 测试通知".to_string(),
        body: "如果你收到这条消息，说明通知插件配置正常。".to_string(),
        created_at: now,
    };
    runtime_events.publish_plugin_notification_test(request);

    match wait_for_plugin_notification_test_result(&store, plugin_id, &test_id) {
        Ok(result) => match result.status {
            niuma_core::notification_store::NotificationRecordStatus::Sent => {
                ApiResponse::ok(json!({
                    "sent": true,
                    "plugin_id": plugin_id,
                    "test_id": test_id,
                    "record_id": result.id
                }))
            }
            niuma_core::notification_store::NotificationRecordStatus::Failed => ApiResponse::fail(
                ApiErrorCode::ServiceUnavailable,
                result
                    .error_message
                    .unwrap_or_else(|| "测试通知发送失败".to_string()),
            ),
            _ => ApiResponse::fail(ApiErrorCode::ServiceUnavailable, "测试通知结果状态无效"),
        },
        Err(error) => ApiResponse::fail(ApiErrorCode::ServiceUnavailable, error),
    }
}

fn validate_notification_test_plugin(
    store: &NiumaStore,
    plugin: &PluginManifest,
) -> Result<(), ApiResponse<serde_json::Value>> {
    let enabled = store
        .plugin_enabled_map()
        .map_err(|error| ApiResponse::fail(ApiErrorCode::System, error))?
        .get(&plugin.id)
        .copied()
        .unwrap_or(false);
    if !enabled {
        return Err(ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("通知插件 {} 未启用", plugin.id),
        ));
    }
    let config = resolved_plugin_config(store, plugin)
        .map_err(|error| ApiResponse::fail(ApiErrorCode::System, error))?;
    validate_plugin_config(plugin, &config)
        .map_err(|error| ApiResponse::fail(ApiErrorCode::BusinessValidation, error))?;
    let runtime_states = store
        .plugin_runtime_states()
        .map_err(|error| ApiResponse::fail(ApiErrorCode::System, error))?;
    let status = runtime_states.get(&plugin.id).map(|state| &state.status);
    if !matches!(
        status,
        Some(PluginRuntimeStatus::Running) | Some(PluginRuntimeStatus::Starting)
    ) {
        return Err(ApiResponse::fail(
            ApiErrorCode::ServiceUnavailable,
            format!("通知插件 {} 未运行", plugin.id),
        ));
    }
    Ok(())
}

fn wait_for_plugin_notification_test_result(
    store: &NiumaStore,
    plugin_id: &str,
    test_id: &str,
) -> Result<niuma_core::notification_store::PluginNotificationResult, String> {
    let deadline = Instant::now() + NOTIFICATION_TEST_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(result) = store.plugin_notification_result(plugin_id, test_id)? {
            return Ok(result);
        }
        thread::sleep(NOTIFICATION_TEST_POLL_INTERVAL);
    }
    Err("测试通知等待插件回写结果超时".to_string())
}

fn import_plugin_dir_from_path(
    store: &NiumaStore,
    service: &StateMutationService,
    path: std::path::PathBuf,
) -> ApiResponse<serde_json::Value> {
    let config = match store.listener_config() {
        Ok(config) => config,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    let runtime_states = match store.plugin_runtime_states() {
        Ok(states) => states,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    let plugin_enabled_map = match store.plugin_enabled_map() {
        Ok(map) => map,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    match import_external_plugin_dir(
        &path,
        &default_user_plugin_dir(),
        &config,
        &plugin_enabled_map,
        &runtime_states,
    ) {
        Ok(mut result) => {
            let registry = current_plugin_registry();
            let Some(manifest) = registry.plugin_by_id(&result.plugin.id).cloned() else {
                return ApiResponse::fail(
                    ApiErrorCode::System,
                    format!("插件导入后未被发现：{}", result.plugin.id),
                );
            };
            if let Err(error) = save_plugin_enabled_state(store, service, &manifest, true) {
                return ApiResponse::fail(ApiErrorCode::System, error);
            }
            match plugin_management_items_from_store(store) {
                Ok(plugins) => {
                    if let Some(plugin) = plugins.iter().find(|item| item.id == result.plugin.id) {
                        result.plugin = plugin.clone();
                    }
                    result.plugins = plugins;
                    ApiResponse::ok(json!(result))
                }
                Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
            }
        }
        Err(error) => ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    }
}

fn remove_plugin_by_id(
    store: &NiumaStore,
    service: &StateMutationService,
    runtime_events: &RuntimeEventBus,
    plugin_id: &str,
) -> ApiResponse<serde_json::Value> {
    if PluginRegistry::with_builtin_plugins()
        .plugin_by_id(plugin_id)
        .is_some()
    {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("不能移除内置插件：{plugin_id}"),
        );
    }
    let registry = current_plugin_registry();
    let Some(plugin) = registry.plugin_by_id(plugin_id).cloned() else {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("未知插件：{plugin_id}"),
        );
    };
    if plugin.source == PluginSource::Builtin {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("不能移除内置插件：{plugin_id}"),
        );
    }
    let config = match plugin.tool_id.clone() {
        Some(tool) => match service.set_tool_listening_enabled(tool, false) {
            Ok(result) => result.config,
            Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
        },
        None => match store.listener_config() {
            Ok(config) => config,
            Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
        },
    };
    if let Err(error) = store.remove_plugin_runtime_state(plugin_id) {
        return ApiResponse::fail(ApiErrorCode::System, error);
    }
    if let Err(error) = store.remove_plugin_config(plugin_id) {
        return ApiResponse::fail(ApiErrorCode::System, error);
    }
    let runtime_states = match store.plugin_runtime_states() {
        Ok(states) => states,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    let plugin_enabled_map = match store.plugin_enabled_map() {
        Ok(map) => map,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    match remove_external_plugin(
        plugin_id,
        &default_user_plugin_dir(),
        &config,
        &plugin_enabled_map,
        &runtime_states,
    ) {
        Ok(result) => {
            runtime_events.publish_state_changed(StateChangeReason::ListenerConfigChanged);
            ApiResponse::ok(json!(result))
        }
        Err(error) => ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    }
}

fn get_plugin_config_by_id(plugin_id: &str) -> ApiResponse<serde_json::Value> {
    let store = default_store();
    let registry = current_plugin_registry();
    let Some(plugin) = registry.plugin_by_id(plugin_id).cloned() else {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("未知插件：{plugin_id}"),
        );
    };
    match resolved_plugin_config(&store, &plugin) {
        Ok(config) => ApiResponse::ok(json!({
            "plugin_id": plugin_id,
            "config": config,
            "config_schema": plugin.config_schema
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn save_plugin_config_by_id(
    runtime_events: &RuntimeEventBus,
    plugin_id: &str,
    config: serde_json::Value,
) -> ApiResponse<serde_json::Value> {
    let store = default_store();
    let registry = current_plugin_registry();
    let Some(plugin) = registry.plugin_by_id(plugin_id).cloned() else {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("未知插件：{plugin_id}"),
        );
    };
    let Some(config) = config.as_object().cloned() else {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, "config 必须是对象");
    };
    if let Err(error) = validate_plugin_config(&plugin, &config) {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, error);
    }
    if let Err(error) = store.save_plugin_config(&plugin.id, &config) {
        return ApiResponse::fail(ApiErrorCode::System, error);
    }
    runtime_events.publish_state_changed(StateChangeReason::PluginConfigChanged);
    match resolved_plugin_config(&store, &plugin) {
        Ok(saved_config) => ApiResponse::ok(json!({
            "saved": true,
            "plugin_id": plugin_id,
            "config": saved_config,
            "config_schema": plugin.config_schema
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn resolved_plugin_config(
    store: &NiumaStore,
    plugin: &PluginManifest,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let stored_config = store.plugin_config(&plugin.id)?;
    Ok(resolve_plugin_config(plugin, stored_config))
}

fn set_plugin_enabled_by_id(
    store: &NiumaStore,
    service: &StateMutationService,
    runtime_events: &RuntimeEventBus,
    plugin_id: &str,
    enabled: bool,
) -> ApiResponse<serde_json::Value> {
    let registry = current_plugin_registry();
    let Some(plugin) = registry.plugin_by_id(plugin_id).cloned() else {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            format!("未知插件：{plugin_id}"),
        );
    };

    if let Some(tool) = plugin.tool_id {
        if let Err(error) = service.set_tool_listening_enabled(tool, enabled) {
            return ApiResponse::fail(ApiErrorCode::System, error);
        }
    } else {
        let mut enabled_map = match store.plugin_enabled_map() {
            Ok(map) => map,
            Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
        };
        enabled_map.insert(plugin.id.clone(), enabled);
        if let Err(error) = store.save_plugin_enabled_map(&enabled_map) {
            return ApiResponse::fail(ApiErrorCode::System, error);
        }
        runtime_events.publish_state_changed(StateChangeReason::PluginConfigChanged);
    }

    match plugin_management_items_from_store(store) {
        Ok(plugins) => ApiResponse::ok(json!({
            "saved": true,
            "plugin_id": plugin_id,
            "enabled": enabled,
            "plugins": plugins
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
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

pub(crate) fn restore_language_preference_from_store() -> Result<(), String> {
    let preference = default_store().language_preference()?;
    set_active_language_preference(preference);
    Ok(())
}

fn default_store() -> NiumaStore {
    NiumaStore::new(NiumaStore::default_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::runtime_event::RuntimeEventBus;
    use std::path::PathBuf;

    #[test]
    fn listener_config_helpers_default_enabled_and_save_disabled() {
        let store = NiumaStore::new(test_sqlite_path("listener_config_helpers"));
        let service = StateMutationService::new(store.clone(), RuntimeEventBus::new());

        let default_config = get_listener_config_from_store(store.clone());
        let save = save_listener_config_with_service(&service, false, None);
        let reloaded = get_listener_config_from_store(store);

        assert_eq!(default_config.code, 0);
        assert_eq!(default_config.data["codex_listening_enabled"], true);
        assert_eq!(save.code, 0);
        assert_eq!(save.data["saved"], true);
        assert_eq!(save.data["codex_listening_enabled"], false);
        assert_eq!(reloaded.data["codex_listening_enabled"], false);
    }

    #[test]
    fn plugin_management_items_from_store_reads_shared_runtime_state() {
        let store = NiumaStore::new(test_sqlite_path("plugin_management_runtime_state"));
        store
            .save_plugin_runtime_state(
                "builtin-codex",
                niuma_core::plugin::PluginRuntimeState::running(),
            )
            .unwrap();

        let plugins = plugin_management_items_from_store(&store).unwrap();
        let codex = plugins
            .iter()
            .find(|plugin| plugin.id == "builtin-codex")
            .unwrap();

        assert_eq!(codex.runtime_status, PluginRuntimeStatus::Running);
    }

    fn test_sqlite_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niuma-tauri-commands-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("niuma.sqlite")
    }
}
