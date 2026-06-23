use std::collections::BTreeMap;

use crate::listener_config::ListenerConfig;
use crate::state_mutation::StateMutationService;
use crate::store::NiumaStore;

use super::{PluginCapability, PluginManifest, PluginSource};

pub fn save_plugin_enabled_state(
    store: &NiumaStore,
    service: &StateMutationService,
    manifest: &PluginManifest,
    enabled: bool,
) -> Result<(), String> {
    if plugin_uses_listener_config(manifest) {
        let tool = manifest
            .tool_id
            .clone()
            .ok_or_else(|| format!("工具监听插件缺少 tool_id：{}", manifest.id))?;
        return service
            .set_tool_listening_enabled(tool, enabled)
            .map(|_| ());
    }
    let mut enabled_map = store.plugin_enabled_map()?;
    enabled_map.insert(manifest.id.clone(), enabled);
    store.save_plugin_enabled_map(&enabled_map)
}

pub fn listener_config_after_plugin_removed(
    store: &NiumaStore,
    service: &StateMutationService,
    manifest: &PluginManifest,
) -> Result<ListenerConfig, String> {
    if plugin_uses_listener_config(manifest) {
        let tool = manifest
            .tool_id
            .clone()
            .ok_or_else(|| format!("工具监听插件缺少 tool_id：{}", manifest.id))?;
        return service
            .set_tool_listening_enabled(tool, false)
            .map(|result| result.config);
    }
    // session provider 虽然有 tool_id，但不归工具监听开关管理，删除时只读取当前配置。
    store.listener_config()
}

pub(super) fn plugin_enabled(
    manifest: &PluginManifest,
    config: &ListenerConfig,
    plugin_enabled_map: &BTreeMap<String, bool>,
) -> bool {
    if plugin_uses_listener_config(manifest) {
        if let Some(tool) = &manifest.tool_id {
            return config.is_tool_enabled(tool);
        }
    }
    plugin_enabled_map
        .get(&manifest.id)
        .copied()
        .unwrap_or_else(|| default_plugin_enabled(manifest))
}

pub fn plugin_uses_listener_config(manifest: &PluginManifest) -> bool {
    // 只有 event_watcher 表示“工具监听开关”；session provider 虽然有 tool_id，也独立启用。
    manifest.tool_id.is_some()
        && manifest
            .capabilities
            .contains(&PluginCapability::EventWatcher)
}

pub fn default_plugin_enabled(manifest: &PluginManifest) -> bool {
    manifest.source == PluginSource::Builtin
}

pub fn default_non_tool_plugin_enabled(manifest: &PluginManifest) -> bool {
    // 保留给运行管理器使用；默认只自动开启内置插件，外置插件由导入流程写入显式状态。
    default_plugin_enabled(manifest)
}
