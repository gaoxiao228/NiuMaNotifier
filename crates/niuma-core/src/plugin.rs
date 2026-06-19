use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::listener_config::ListenerConfig;
use crate::models::ToolKind;

pub const CODEX_PLUGIN_COMMAND_ENV: &str = "NIUMA_CODEX_PLUGIN_COMMAND";
pub const BARK_PLUGIN_COMMAND_ENV: &str = "NIUMA_BARK_PLUGIN_COMMAND";
pub const NTFY_PLUGIN_COMMAND_ENV: &str = "NIUMA_NTFY_PLUGIN_COMMAND";
pub const BUILTIN_BARK_PLUGIN_ID: &str = "builtin-bark";
pub const BUILTIN_NTFY_PLUGIN_ID: &str = "builtin-ntfy";

const CODEX_PLUGIN_COMMAND: &str = "niuma-codex-plugin";
const BARK_PLUGIN_COMMAND: &str = "niuma-plugin-bark";
const NTFY_PLUGIN_COMMAND: &str = "niuma-plugin-ntfy";
const BUILTIN_CODEX_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../builtin-plugins/codex/plugin.json");
const BUILTIN_BARK_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../builtin-plugins/bark/plugin.json");
const BUILTIN_NTFY_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../builtin-plugins/ntfy/plugin.json");

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    EventWatcher,
    EventConsumer,
    NotificationTest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Tool,
    Notification,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    Builtin,
    External,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntimeStatus {
    Starting,
    Stopped,
    Stopping,
    Running,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    #[serde(default = "tool_plugin_kind")]
    pub kind: PluginKind,
    #[serde(default)]
    pub tool_id: Option<ToolKind>,
    pub display_name: String,
    pub version: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub config_schema: Vec<PluginConfigField>,
    #[serde(default = "external_source")]
    pub source: PluginSource,
    #[serde(skip)]
    pub base_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginConfigFieldType {
    String,
    Secret,
    Url,
    Number,
    Boolean,
    Select,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginConfigField {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: PluginConfigFieldType,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolPluginInfo {
    pub id: String,
    pub tool_id: ToolKind,
    pub display_name: String,
    pub version: String,
    pub source: PluginSource,
    pub icon_url: Option<String>,
    pub capabilities: Vec<PluginCapability>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginRuntimeState {
    pub status: PluginRuntimeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl PluginRuntimeState {
    pub fn starting() -> Self {
        Self {
            status: PluginRuntimeStatus::Starting,
            last_error: None,
        }
    }

    pub fn stopped() -> Self {
        Self {
            status: PluginRuntimeStatus::Stopped,
            last_error: None,
        }
    }

    pub fn stopping() -> Self {
        Self {
            status: PluginRuntimeStatus::Stopping,
            last_error: None,
        }
    }

    pub fn running() -> Self {
        Self {
            status: PluginRuntimeStatus::Running,
            last_error: None,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            status: PluginRuntimeStatus::Failed,
            last_error: Some(error.into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PluginManagementItem {
    pub id: String,
    pub kind: PluginKind,
    pub tool_id: Option<ToolKind>,
    pub display_name: String,
    pub version: String,
    pub source: PluginSource,
    pub enabled: bool,
    pub runtime_status: PluginRuntimeStatus,
    pub last_error: Option<String>,
    pub icon_url: Option<String>,
    pub capabilities: Vec<PluginCapability>,
    pub config_schema: Vec<PluginConfigField>,
    pub install_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PluginImportResult {
    pub imported: bool,
    pub plugin: PluginManagementItem,
    pub plugins: Vec<PluginManagementItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PluginRemoveResult {
    pub removed: bool,
    pub plugin_id: String,
    pub plugins: Vec<PluginManagementItem>,
}

#[derive(Clone, Debug, Default)]
pub struct PluginRegistry {
    manifests: Vec<PluginManifest>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtin_plugins() -> Self {
        let mut registry = Self::new();
        registry.register(builtin_codex_manifest());
        registry.register(builtin_bark_manifest());
        registry.register(builtin_ntfy_manifest());
        registry
    }

    pub fn discover_external_plugins(mut self, root: &Path) -> Self {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let manifest_path = entry.path().join("plugin.json");
                match PluginManifest::from_path(&manifest_path) {
                    Ok(mut manifest) => {
                        manifest.source = PluginSource::External;
                        if manifest.supports_current_platform() {
                            if self
                                .plugin_by_id(&manifest.id)
                                .is_some_and(|item| item.source == PluginSource::Builtin)
                            {
                                eprintln!(
                                    "NiumaNotifier external plugin manifest ignored {}: 不能覆盖内置插件 {}",
                                    manifest_path.display(),
                                    manifest.id
                                );
                                continue;
                            }
                            self.register(manifest);
                        }
                    }
                    Err(error) if manifest_path.exists() => {
                        eprintln!(
                            "NiumaNotifier plugin manifest ignored {}: {error}",
                            manifest_path.display()
                        );
                    }
                    Err(_) => {}
                }
            }
        }
        self
    }

    pub fn register(&mut self, manifest: PluginManifest) {
        // 同 id 后注册覆盖先注册，方便外部插件导入新版时替换旧 manifest。
        self.manifests.retain(|item| item.id != manifest.id);
        self.manifests.push(manifest);
    }

    pub fn manifests(&self) -> &[PluginManifest] {
        &self.manifests
    }

    pub fn plugin_for_tool(&self, tool: &ToolKind) -> Option<&PluginManifest> {
        self.manifests
            .iter()
            .find(|item| item.tool_id.as_ref() == Some(tool))
    }

    pub fn plugin_by_id(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.manifests.iter().find(|item| item.id == plugin_id)
    }

    pub fn tools(&self) -> Vec<ToolPluginInfo> {
        self.manifests
            .iter()
            .filter_map(ToolPluginInfo::try_from_manifest)
            .collect()
    }

    pub fn management_items(
        &self,
        config: &ListenerConfig,
        plugin_enabled_map: &BTreeMap<String, bool>,
        runtime_states: &HashMap<String, PluginRuntimeState>,
    ) -> Vec<PluginManagementItem> {
        self.manifests
            .iter()
            .map(|manifest| {
                let runtime = runtime_states
                    .get(&manifest.id)
                    .cloned()
                    .unwrap_or_else(PluginRuntimeState::stopped);
                PluginManagementItem {
                    id: manifest.id.clone(),
                    kind: manifest.kind.clone(),
                    tool_id: manifest.tool_id.clone(),
                    display_name: manifest.display_name.clone(),
                    version: manifest.version.clone(),
                    source: manifest.source.clone(),
                    enabled: plugin_enabled(manifest, config, plugin_enabled_map),
                    runtime_status: runtime.status,
                    last_error: runtime.last_error,
                    icon_url: manifest.icon_url.clone(),
                    capabilities: manifest.capabilities.clone(),
                    config_schema: manifest.config_schema.clone(),
                    install_path: manifest
                        .base_dir
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string()),
                }
            })
            .collect()
    }
}

impl PluginManifest {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let content =
            fs::read_to_string(path).map_err(|error| format!("读取插件 manifest 失败：{error}"))?;
        let mut manifest = parse_plugin_manifest(&content)?;
        manifest.base_dir = path.parent().map(Path::to_path_buf);
        Ok(manifest)
    }

    pub fn supports_current_platform(&self) -> bool {
        self.platforms.is_empty()
            || self
                .platforms
                .iter()
                .any(|platform| platform == current_platform_id())
    }
}

impl ToolPluginInfo {
    fn try_from_manifest(manifest: &PluginManifest) -> Option<Self> {
        let tool_id = manifest.tool_id.clone()?;
        Some(Self {
            id: manifest.id.clone(),
            tool_id,
            display_name: manifest.display_name.clone(),
            version: manifest.version.clone(),
            source: manifest.source.clone(),
            icon_url: manifest.icon_url.clone(),
            capabilities: manifest.capabilities.clone(),
        })
    }
}

pub fn builtin_codex_manifest() -> PluginManifest {
    let mut manifest = parse_plugin_manifest(BUILTIN_CODEX_PLUGIN_MANIFEST_JSON)
        .expect("内置 Codex 插件 manifest 必须是有效 plugin.json");
    manifest.source = PluginSource::Builtin;
    manifest.command = Some(builtin_codex_plugin_command(manifest.command));
    // 内置插件的 binary 路径由桌面端/环境变量解析，不使用源码目录作为运行目录。
    manifest.base_dir = None;
    manifest
}

pub fn builtin_bark_manifest() -> PluginManifest {
    let mut manifest = parse_plugin_manifest(BUILTIN_BARK_PLUGIN_MANIFEST_JSON)
        .expect("内置 Bark 插件 manifest 必须是有效 plugin.json");
    manifest.source = PluginSource::Builtin;
    manifest.command = Some(builtin_plugin_command(
        BARK_PLUGIN_COMMAND_ENV,
        manifest.command,
        BARK_PLUGIN_COMMAND,
    ));
    manifest.base_dir = None;
    manifest
}

pub fn builtin_ntfy_manifest() -> PluginManifest {
    let mut manifest = parse_plugin_manifest(BUILTIN_NTFY_PLUGIN_MANIFEST_JSON)
        .expect("内置 ntfy 插件 manifest 必须是有效 plugin.json");
    manifest.source = PluginSource::Builtin;
    manifest.command = Some(builtin_plugin_command(
        NTFY_PLUGIN_COMMAND_ENV,
        manifest.command,
        NTFY_PLUGIN_COMMAND,
    ));
    manifest.base_dir = None;
    manifest
}

pub fn default_user_plugin_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("NiumaNotifier")
        .join("plugins")
}

pub fn current_plugin_registry() -> PluginRegistry {
    PluginRegistry::with_builtin_plugins().discover_external_plugins(&default_user_plugin_dir())
}

pub fn plugin_config_defaults(
    manifest: &PluginManifest,
) -> serde_json::Map<String, serde_json::Value> {
    manifest
        .config_schema
        .iter()
        .filter(|field| !field.default.is_null())
        .map(|field| (field.key.clone(), field.default.clone()))
        .collect()
}

pub fn merge_plugin_config_with_defaults(
    manifest: &PluginManifest,
    config: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut merged = plugin_config_defaults(manifest);
    for (key, value) in config {
        merged.insert(key, value);
    }
    merged
}

pub fn resolve_plugin_config(
    manifest: &PluginManifest,
    stored_config: Option<serde_json::Map<String, serde_json::Value>>,
) -> serde_json::Map<String, serde_json::Value> {
    let config = stored_config.unwrap_or_default();
    merge_plugin_config_with_defaults(manifest, config)
}

pub fn validate_plugin_config(
    manifest: &PluginManifest,
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    for field in &manifest.config_schema {
        let Some(value) = config.get(&field.key) else {
            if field.required {
                return Err(format!("{} 不能为空", field.label));
            }
            continue;
        };
        if !plugin_config_value_matches(field, value) {
            return Err(format!("{} 类型无效", field.label));
        }
        if field.required && plugin_config_value_is_empty(value) {
            return Err(format!("{} 不能为空", field.label));
        }
    }
    Ok(())
}

pub fn import_external_plugin_dir(
    source_dir: &Path,
    destination_root: &Path,
    config: &ListenerConfig,
    plugin_enabled_map: &BTreeMap<String, bool>,
    runtime_states: &HashMap<String, PluginRuntimeState>,
) -> Result<PluginImportResult, String> {
    let manifest_path = source_dir.join("plugin.json");
    let manifest = PluginManifest::from_path(&manifest_path)?;
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

fn current_platform_id() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

fn parse_plugin_manifest(content: &str) -> Result<PluginManifest, String> {
    let manifest: PluginManifest = serde_json::from_str(content)
        .map_err(|error| format!("解析插件 manifest 失败：{error}"))?;
    validate_plugin_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_plugin_manifest(manifest: &PluginManifest) -> Result<(), String> {
    if manifest.kind == PluginKind::Tool && manifest.tool_id.is_none() {
        return Err(format!("工具插件缺少 tool_id：{}", manifest.id));
    }
    if manifest.kind != PluginKind::Tool
        && manifest
            .capabilities
            .contains(&PluginCapability::EventWatcher)
    {
        return Err(format!("非工具插件不能声明 event_watcher：{}", manifest.id));
    }
    let mut keys = std::collections::BTreeSet::new();
    for field in &manifest.config_schema {
        if field.key.trim().is_empty() {
            return Err(format!("插件配置项 key 不能为空：{}", manifest.id));
        }
        if field.label.trim().is_empty() {
            return Err(format!("插件配置项 label 不能为空：{}", manifest.id));
        }
        if !keys.insert(field.key.clone()) {
            return Err(format!(
                "插件配置项 key 重复：{}:{}",
                manifest.id, field.key
            ));
        }
    }
    Ok(())
}

fn plugin_config_value_matches(field: &PluginConfigField, value: &Value) -> bool {
    match field.field_type {
        PluginConfigFieldType::String
        | PluginConfigFieldType::Secret
        | PluginConfigFieldType::Url => value.is_string(),
        PluginConfigFieldType::Number => value.is_number(),
        PluginConfigFieldType::Boolean => value.is_boolean(),
        PluginConfigFieldType::Select => {
            let Some(value) = value.as_str() else {
                return false;
            };
            field.options.is_empty() || field.options.iter().any(|option| option == value)
        }
    }
}

fn plugin_config_value_is_empty(value: &Value) -> bool {
    match value {
        Value::String(text) => text.trim().is_empty(),
        Value::Null => true,
        _ => false,
    }
}

fn plugin_enabled(
    manifest: &PluginManifest,
    config: &ListenerConfig,
    plugin_enabled_map: &BTreeMap<String, bool>,
) -> bool {
    if let Some(tool) = &manifest.tool_id {
        return config.is_tool_enabled(tool);
    }
    plugin_enabled_map
        .get(&manifest.id)
        .copied()
        .unwrap_or(false)
}

fn builtin_codex_plugin_command(default_command: Option<String>) -> String {
    // 桌面端或打包脚本可覆盖命令路径；默认值来自内置 plugin.json，最后回退到普通裸命令。
    builtin_plugin_command(
        CODEX_PLUGIN_COMMAND_ENV,
        default_command,
        CODEX_PLUGIN_COMMAND,
    )
}

fn builtin_plugin_command(
    env_key: &str,
    default_command: Option<String>,
    fallback_command: &str,
) -> String {
    std::env::var(env_key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| default_command.filter(|value| !value.trim().is_empty()))
        .unwrap_or_else(|| fallback_command.to_string())
}

fn external_source() -> PluginSource {
    PluginSource::External
}

fn tool_plugin_kind() -> PluginKind {
    PluginKind::Tool
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listener_config::ListenerConfig;
    use crate::models::ToolId;
    use std::sync::{Mutex, OnceLock};

    #[test]
    fn parses_manifest_with_string_tool_id() {
        let manifest: PluginManifest = serde_json::from_str(
            r#"{
                "id": "cursor-plugin",
                "tool_id": "cursor",
                "display_name": "Cursor",
                "version": "0.1.0",
                "command": "./bin/cursor-plugin",
                "capabilities": ["event_watcher"]
            }"#,
        )
        .unwrap();

        assert_eq!(manifest.kind, PluginKind::Tool);
        assert_eq!(manifest.tool_id, Some(ToolId::Custom("cursor".to_string())));
        assert_eq!(manifest.source, PluginSource::External);
        assert!(manifest.supports_current_platform());
    }

    #[test]
    fn parses_notification_event_consumer_without_tool_id() {
        let manifest = parse_plugin_manifest(
            r#"{
                "id": "builtin-bark",
                "kind": "notification",
                "display_name": "Bark",
                "version": "0.1.0",
                "command": "niuma-plugin-bark",
                "capabilities": ["event_consumer"]
            }"#,
        )
        .unwrap();

        assert_eq!(manifest.kind, PluginKind::Notification);
        assert_eq!(manifest.tool_id, None);
        assert_eq!(manifest.capabilities, vec![PluginCapability::EventConsumer]);
    }

    #[test]
    fn rejects_tool_plugin_without_tool_id() {
        let error = parse_plugin_manifest(
            r#"{
                "id": "broken-tool",
                "kind": "tool",
                "display_name": "Broken",
                "version": "0.1.0",
                "command": "broken",
                "capabilities": ["event_watcher"]
            }"#,
        )
        .unwrap_err();

        assert!(error.contains("工具插件缺少 tool_id"));
    }

    #[test]
    fn rejects_event_watcher_on_notification_plugin() {
        let error = parse_plugin_manifest(
            r#"{
                "id": "broken-notification",
                "kind": "notification",
                "display_name": "Broken",
                "version": "0.1.0",
                "command": "broken",
                "capabilities": ["event_watcher"]
            }"#,
        )
        .unwrap_err();

        assert!(error.contains("非工具插件不能声明 event_watcher"));
    }

    #[test]
    fn runtime_status_serializes_transition_states() {
        assert_eq!(
            serde_json::to_string(&PluginRuntimeStatus::Starting).unwrap(),
            r#""starting""#
        );
        assert_eq!(
            serde_json::to_string(&PluginRuntimeStatus::Stopping).unwrap(),
            r#""stopping""#
        );
        assert_eq!(
            serde_json::from_str::<PluginRuntimeStatus>(r#""starting""#).unwrap(),
            PluginRuntimeStatus::Starting
        );
        assert_eq!(
            serde_json::from_str::<PluginRuntimeStatus>(r#""stopping""#).unwrap(),
            PluginRuntimeStatus::Stopping
        );
    }

    #[test]
    fn registry_contains_builtin_codex_plugin() {
        let registry = PluginRegistry::with_builtin_plugins();
        let plugin = registry.plugin_for_tool(&ToolKind::Codex).unwrap();

        assert_eq!(plugin.id, "builtin-codex");
        assert_eq!(plugin.kind, PluginKind::Tool);
        assert_eq!(plugin.tool_id, Some(ToolKind::Codex));
    }

    #[test]
    fn registry_contains_builtin_bark_notification_plugin() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var(BARK_PLUGIN_COMMAND_ENV);
        let registry = PluginRegistry::with_builtin_plugins();
        let plugin = registry.plugin_by_id("builtin-bark").unwrap();

        assert_eq!(plugin.kind, PluginKind::Notification);
        assert_eq!(plugin.tool_id, None);
        assert_eq!(
            plugin.capabilities,
            vec![
                PluginCapability::EventConsumer,
                PluginCapability::NotificationTest
            ]
        );
    }

    #[test]
    fn registry_contains_builtin_ntfy_notification_plugin() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var(NTFY_PLUGIN_COMMAND_ENV);
        let registry = PluginRegistry::with_builtin_plugins();
        let plugin = registry.plugin_by_id("builtin-ntfy").unwrap();

        assert_eq!(plugin.kind, PluginKind::Notification);
        assert_eq!(plugin.tool_id, None);
        assert_eq!(
            plugin.capabilities,
            vec![
                PluginCapability::EventConsumer,
                PluginCapability::NotificationTest
            ]
        );
        assert_eq!(plugin.config_schema[0].key, "topic");
    }

    #[test]
    fn builtin_bark_manifest_uses_command_override_from_env() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var(BARK_PLUGIN_COMMAND_ENV, "/tmp/niuma-plugin-bark-test");

        let manifest = builtin_bark_manifest();

        std::env::remove_var(BARK_PLUGIN_COMMAND_ENV);
        assert_eq!(
            manifest.command.as_deref(),
            Some("/tmp/niuma-plugin-bark-test")
        );
        assert!(manifest.args.is_empty());
    }

    #[test]
    fn builtin_ntfy_manifest_uses_command_override_from_env() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var(NTFY_PLUGIN_COMMAND_ENV, "/tmp/niuma-plugin-ntfy-test");

        let manifest = builtin_ntfy_manifest();

        std::env::remove_var(NTFY_PLUGIN_COMMAND_ENV);
        assert_eq!(
            manifest.command.as_deref(),
            Some("/tmp/niuma-plugin-ntfy-test")
        );
        assert!(manifest.args.is_empty());
    }

    #[test]
    fn builtin_codex_manifest_runs_as_independent_plugin_process() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var(CODEX_PLUGIN_COMMAND_ENV);

        let manifest = builtin_codex_manifest();

        assert_eq!(manifest.source, PluginSource::Builtin);
        assert!(manifest
            .command
            .as_deref()
            .is_some_and(|value| !value.is_empty()));
        assert_eq!(manifest.args, Vec::<String>::new());
        assert_ne!(
            manifest.command.as_deref(),
            std::env::current_exe()
                .ok()
                .as_deref()
                .and_then(Path::to_str)
        );
    }

    #[test]
    fn builtin_codex_manifest_is_backed_by_builtin_plugin_json() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var(CODEX_PLUGIN_COMMAND_ENV);

        let manifest_path = builtin_codex_plugin_manifest_path();
        assert!(
            manifest_path.exists(),
            "内置 Codex 插件应该有 plugin.json: {}",
            manifest_path.display()
        );

        let file_manifest = PluginManifest::from_path(&manifest_path).unwrap();
        let manifest = builtin_codex_manifest();

        assert_eq!(manifest.id, file_manifest.id);
        assert_eq!(manifest.kind, PluginKind::Tool);
        assert_eq!(manifest.tool_id, file_manifest.tool_id);
        assert_eq!(manifest.display_name, file_manifest.display_name);
        assert_eq!(manifest.version, file_manifest.version);
        assert_eq!(manifest.command, file_manifest.command);
        assert_eq!(manifest.args, file_manifest.args);
        assert_eq!(manifest.platforms, file_manifest.platforms);
        assert_eq!(manifest.capabilities, file_manifest.capabilities);
        assert_eq!(manifest.source, PluginSource::Builtin);
        assert_eq!(manifest.base_dir, None);
    }

    #[test]
    fn builtin_codex_manifest_uses_command_override_from_env() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var(CODEX_PLUGIN_COMMAND_ENV, "/tmp/niuma-codex-plugin-test");

        let manifest = builtin_codex_manifest();

        std::env::remove_var(CODEX_PLUGIN_COMMAND_ENV);
        assert_eq!(
            manifest.command.as_deref(),
            Some("/tmp/niuma-codex-plugin-test")
        );
        assert!(manifest.args.is_empty());
    }

    #[test]
    fn external_manifest_cannot_override_builtin_codex_plugin() {
        let temp = tempfile::tempdir().unwrap();
        let plugin_dir = temp.path().join("builtin-codex");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_demo_plugin(&plugin_dir, "builtin-codex");

        let registry =
            PluginRegistry::with_builtin_plugins().discover_external_plugins(temp.path());
        let plugin = registry.plugin_by_id("builtin-codex").unwrap();

        assert_eq!(plugin.source, PluginSource::Builtin);
        assert_eq!(plugin.tool_id, Some(ToolKind::Codex));
    }

    #[test]
    fn registry_discovers_external_plugin_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let plugin_dir = temp.path().join("niuma-plugin-demo");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            r#"{
                "id": "niuma-plugin-demo",
                "tool_id": "demo_tool",
                "display_name": "Demo Tool",
                "version": "0.1.0",
                "command": "node",
                "args": ["./bin/niuma-plugin-demo.mjs"],
                "platforms": ["macos", "windows", "linux"],
                "capabilities": ["event_watcher"]
            }"#,
        )
        .unwrap();

        let registry = PluginRegistry::new().discover_external_plugins(temp.path());
        let plugin = registry.plugin_by_id("niuma-plugin-demo").unwrap();

        assert_eq!(
            plugin.tool_id,
            Some(ToolKind::Custom("demo_tool".to_string()))
        );
        assert_eq!(plugin.source, PluginSource::External);
        assert_eq!(plugin.base_dir.as_deref(), Some(plugin_dir.as_path()));
        assert_eq!(registry.tools()[0].display_name, "Demo Tool");
    }

    #[test]
    fn registry_tools_excludes_notification_event_consumers() {
        let mut registry = PluginRegistry::new();
        registry.register(
            parse_plugin_manifest(
                r#"{
                "id": "builtin-bark",
                "kind": "notification",
                "display_name": "Bark",
                "version": "0.1.0",
                "command": "niuma-plugin-bark",
                "capabilities": ["event_consumer"]
            }"#,
            )
            .unwrap(),
        );

        assert!(registry.tools().is_empty());
    }

    #[test]
    fn management_items_read_notification_enabled_from_plugin_map() {
        let mut registry = PluginRegistry::new();
        registry.register(
            parse_plugin_manifest(
                r#"{
                "id": "builtin-bark",
                "kind": "notification",
                "display_name": "Bark",
                "version": "0.1.0",
                "command": "niuma-plugin-bark",
                "capabilities": ["event_consumer"]
            }"#,
            )
            .unwrap(),
        );
        let enabled = BTreeMap::from([("builtin-bark".to_string(), true)]);

        let items =
            registry.management_items(&ListenerConfig::default(), &enabled, &HashMap::new());

        assert_eq!(items[0].kind, PluginKind::Notification);
        assert_eq!(items[0].tool_id, None);
        assert!(items[0].enabled);
    }

    #[test]
    fn imports_external_plugin_dir_into_destination_root() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        write_demo_plugin(source.path(), "niuma-plugin-demo");
        std::fs::create_dir_all(source.path().join("bin")).unwrap();
        std::fs::write(source.path().join("bin/demo.js"), "console.log('demo')").unwrap();

        let result = import_external_plugin_dir(
            source.path(),
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        assert!(result.imported);
        assert_eq!(result.plugin.id, "niuma-plugin-demo");
        assert!(destination
            .path()
            .join("niuma-plugin-demo/bin/demo.js")
            .exists());
    }

    #[test]
    fn removes_external_plugin_dir_from_destination_root() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        write_demo_plugin(source.path(), "niuma-plugin-demo");
        import_external_plugin_dir(
            source.path(),
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        let result = remove_external_plugin(
            "niuma-plugin-demo",
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        assert!(result.removed);
        assert_eq!(result.plugin_id, "niuma-plugin-demo");
        assert!(!destination.path().join("niuma-plugin-demo").exists());
        assert!(result
            .plugins
            .iter()
            .all(|plugin| plugin.id != "niuma-plugin-demo"));
    }

    #[test]
    fn import_rejects_missing_manifest() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();

        let error = import_external_plugin_dir(
            source.path(),
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap_err();

        assert!(error.contains("读取插件 manifest 失败"));
    }

    #[test]
    fn import_rejects_builtin_plugin_id() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        write_demo_plugin(source.path(), "builtin-codex");

        let error = import_external_plugin_dir(
            source.path(),
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap_err();

        assert!(error.contains("不能覆盖内置插件"));
    }

    #[test]
    fn remove_rejects_builtin_plugin_id() {
        let destination = tempfile::tempdir().unwrap();

        let error = remove_external_plugin(
            "builtin-codex",
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap_err();

        assert!(error.contains("不能移除内置插件"));
    }

    #[test]
    fn unsupported_platform_manifest_is_filtered() {
        let manifest = PluginManifest {
            id: "future".to_string(),
            kind: PluginKind::Tool,
            tool_id: Some(ToolKind::Custom("future".to_string())),
            display_name: "Future".to_string(),
            version: "0.1.0".to_string(),
            command: Some("./future".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: vec!["unsupported-os".to_string()],
            capabilities: vec![PluginCapability::EventWatcher],
            icon_url: None,
            config_schema: Vec::new(),
            source: PluginSource::External,
            base_dir: None,
        };

        assert!(!manifest.supports_current_platform());
    }

    fn write_demo_plugin(dir: &Path, id: &str) {
        std::fs::write(
            dir.join("plugin.json"),
            format!(
                r#"{{
                    "id": "{id}",
                    "tool_id": "demo_tool",
                    "display_name": "Demo Tool",
                    "version": "0.1.0",
                    "command": "node",
                    "args": ["./bin/demo.js"],
                    "platforms": ["macos", "windows", "linux"],
                    "capabilities": ["event_watcher"]
                }}"#
            ),
        )
        .unwrap();
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn builtin_codex_plugin_manifest_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."))
            .join("builtin-plugins/codex/plugin.json")
    }
}
