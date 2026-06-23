use niuma_core::plugin::{resolve_plugin_config, PluginManifest, PluginRegistry, ToolPluginInfo};
use serde::Serialize;

use crate::state::AppState;

#[derive(Clone, Debug, Serialize)]
pub(super) struct ListenerToolView {
    id: String,
    plugin_id: String,
    display_name: String,
    enabled: bool,
    source: String,
    icon_url: Option<String>,
}

pub(super) fn trim_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn listener_tools(
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

pub(super) fn plugin_management_items(
    state: &AppState,
) -> Result<Vec<niuma_core::plugin::PluginManagementItem>, String> {
    let config = state.store.listener_config()?;
    let runtime_states = state.store.plugin_runtime_states()?;
    let plugin_enabled_map = state.store.plugin_enabled_map()?;
    Ok(plugin_registry(state).management_items(&config, &plugin_enabled_map, &runtime_states))
}

pub(super) fn plugin_registry(state: &AppState) -> PluginRegistry {
    PluginRegistry::with_builtin_plugins().discover_external_plugins(&state.plugin_dir)
}

pub(super) fn resolved_plugin_config(
    state: &AppState,
    plugin: &PluginManifest,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let stored_config = state.store.plugin_config(&plugin.id)?;
    Ok(resolve_plugin_config(plugin, stored_config))
}
