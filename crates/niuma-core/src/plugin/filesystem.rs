use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::listener_config::ListenerConfig;

use super::{
    PluginImportResult, PluginRegistry, PluginRemoveResult, PluginRuntimeState, PluginSource,
};

pub fn default_user_plugin_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("NiumaNotifier")
        .join("plugins")
}

pub fn current_plugin_registry() -> PluginRegistry {
    PluginRegistry::with_builtin_plugins().discover_external_plugins(&default_user_plugin_dir())
}

pub fn import_external_plugin_dir(
    source_dir: &Path,
    destination_root: &Path,
    config: &ListenerConfig,
    plugin_enabled_map: &BTreeMap<String, bool>,
    runtime_states: &HashMap<String, PluginRuntimeState>,
) -> Result<PluginImportResult, String> {
    let manifest_path = source_dir.join("plugin.json");
    let manifest = super::PluginManifest::from_path(&manifest_path)?;
    if PluginRegistry::with_builtin_plugins()
        .plugin_by_id(&manifest.id)
        .is_some()
    {
        return Err(format!("不能覆盖内置插件：{}", manifest.id));
    }
    if !manifest.supports_current_platform() {
        return Err(format!("插件不支持当前平台：{}", manifest.id));
    }

    let destination = destination_root.join(&manifest.id);
    if paths_refer_to_same_dir(source_dir, &destination) {
        return Err("插件已位于目标插件目录".to_string());
    }
    let registry_before_import =
        PluginRegistry::with_builtin_plugins().discover_external_plugins(destination_root);
    registry_before_import.validate_provider_capability_uniqueness(&manifest)?;
    fs::create_dir_all(destination_root).map_err(|error| format!("创建插件目录失败：{error}"))?;
    if destination.exists() {
        fs::remove_dir_all(&destination).map_err(|error| format!("替换旧插件失败：{error}"))?;
    }
    copy_dir_recursive(source_dir, &destination)?;

    let registry =
        PluginRegistry::with_builtin_plugins().discover_external_plugins(destination_root);
    let plugins = registry.management_items(config, plugin_enabled_map, runtime_states);
    let plugin = plugins
        .iter()
        .find(|plugin| plugin.id == manifest.id)
        .cloned()
        .ok_or_else(|| format!("插件导入后未被发现：{}", manifest.id))?;

    Ok(PluginImportResult {
        imported: true,
        plugin,
        plugins,
    })
}

pub fn remove_external_plugin(
    plugin_id: &str,
    destination_root: &Path,
    config: &ListenerConfig,
    plugin_enabled_map: &BTreeMap<String, bool>,
    runtime_states: &HashMap<String, PluginRuntimeState>,
) -> Result<PluginRemoveResult, String> {
    if PluginRegistry::with_builtin_plugins()
        .plugin_by_id(plugin_id)
        .is_some()
    {
        return Err(format!("不能移除内置插件：{plugin_id}"));
    }
    let registry =
        PluginRegistry::with_builtin_plugins().discover_external_plugins(destination_root);
    let Some(manifest) = registry.plugin_by_id(plugin_id) else {
        return Err(format!("未知插件：{plugin_id}"));
    };
    if manifest.source == PluginSource::Builtin {
        return Err(format!("不能移除内置插件：{plugin_id}"));
    }
    let plugin_dir = manifest
        .base_dir
        .as_ref()
        .ok_or_else(|| format!("插件缺少安装路径：{plugin_id}"))?;
    let destination_root = destination_root
        .canonicalize()
        .map_err(|error| format!("读取插件目录失败：{error}"))?;
    let plugin_dir = plugin_dir
        .canonicalize()
        .map_err(|error| format!("读取插件安装路径失败：{error}"))?;
    if !plugin_dir.starts_with(&destination_root) {
        return Err(format!("插件安装路径不在用户插件目录：{plugin_id}"));
    }

    fs::remove_dir_all(&plugin_dir).map_err(|error| format!("移除插件失败：{error}"))?;
    let registry =
        PluginRegistry::with_builtin_plugins().discover_external_plugins(&destination_root);
    Ok(PluginRemoveResult {
        removed: true,
        plugin_id: plugin_id.to_string(),
        plugins: registry.management_items(config, plugin_enabled_map, runtime_states),
    })
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| format!("创建插件目标目录失败：{error}"))?;
    for entry in fs::read_dir(source).map_err(|error| format!("读取插件目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取插件目录项失败：{error}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取插件目录项信息失败：{error}"))?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| format!("复制插件文件失败：{error}"))?;
        }
    }
    Ok(())
}

fn paths_refer_to_same_dir(left: &Path, right: &Path) -> bool {
    let Ok(left) = left.canonicalize() else {
        return false;
    };
    let Ok(right) = right.canonicalize() else {
        return false;
    };
    left == right
}
