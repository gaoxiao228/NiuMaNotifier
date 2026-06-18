use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::listener_config::ListenerConfig;
use crate::models::ToolKind;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    EventWatcher,
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
    pub tool_id: ToolKind,
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
    #[serde(default = "external_source")]
    pub source: PluginSource,
    #[serde(skip)]
    pub base_dir: Option<PathBuf>,
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
    pub tool_id: ToolKind,
    pub display_name: String,
    pub version: String,
    pub source: PluginSource,
    pub enabled: bool,
    pub runtime_status: PluginRuntimeStatus,
    pub last_error: Option<String>,
    pub icon_url: Option<String>,
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
        // 同 id 后注册覆盖先注册，方便用户插件在后续版本覆盖内置实现。
        self.manifests.retain(|item| item.id != manifest.id);
        self.manifests.push(manifest);
    }

    pub fn manifests(&self) -> &[PluginManifest] {
        &self.manifests
    }

    pub fn plugin_for_tool(&self, tool: &ToolKind) -> Option<&PluginManifest> {
        self.manifests.iter().find(|item| &item.tool_id == tool)
    }

    pub fn plugin_by_id(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.manifests.iter().find(|item| item.id == plugin_id)
    }

    pub fn tools(&self) -> Vec<ToolPluginInfo> {
        self.manifests.iter().map(ToolPluginInfo::from).collect()
    }

    pub fn management_items(
        &self,
        config: &ListenerConfig,
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
                    tool_id: manifest.tool_id.clone(),
                    display_name: manifest.display_name.clone(),
                    version: manifest.version.clone(),
                    source: manifest.source.clone(),
                    enabled: config.is_tool_enabled(&manifest.tool_id),
                    runtime_status: runtime.status,
                    last_error: runtime.last_error,
                    icon_url: manifest.icon_url.clone(),
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
        let mut manifest: PluginManifest = serde_json::from_str(&content)
            .map_err(|error| format!("解析插件 manifest 失败：{error}"))?;
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

impl From<&PluginManifest> for ToolPluginInfo {
    fn from(manifest: &PluginManifest) -> Self {
        Self {
            id: manifest.id.clone(),
            tool_id: manifest.tool_id.clone(),
            display_name: manifest.display_name.clone(),
            version: manifest.version.clone(),
            source: manifest.source.clone(),
            icon_url: manifest.icon_url.clone(),
            capabilities: manifest.capabilities.clone(),
        }
    }
}

pub fn builtin_codex_manifest() -> PluginManifest {
    PluginManifest {
        id: "builtin-codex".to_string(),
        tool_id: ToolKind::Codex,
        display_name: "Codex".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        command: None,
        args: Vec::new(),
        env: BTreeMap::new(),
        platforms: vec![current_platform_id().to_string()],
        capabilities: vec![PluginCapability::EventWatcher],
        icon_url: None,
        source: PluginSource::Builtin,
        base_dir: None,
    }
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

pub fn import_external_plugin_dir(
    source_dir: &Path,
    destination_root: &Path,
    config: &ListenerConfig,
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
    let plugins = registry.management_items(config, runtime_states);
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
        plugins: registry.management_items(config, runtime_states),
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

fn external_source() -> PluginSource {
    PluginSource::External
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

        assert_eq!(manifest.tool_id, ToolId::Custom("cursor".to_string()));
        assert_eq!(manifest.source, PluginSource::External);
        assert!(manifest.supports_current_platform());
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
        assert_eq!(plugin.tool_id, ToolKind::Codex);
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

        assert_eq!(plugin.tool_id, ToolKind::Custom("demo_tool".to_string()));
        assert_eq!(plugin.source, PluginSource::External);
        assert_eq!(plugin.base_dir.as_deref(), Some(plugin_dir.as_path()));
        assert_eq!(registry.tools()[0].display_name, "Demo Tool");
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
            &HashMap::new(),
        )
        .unwrap();

        let result = remove_external_plugin(
            "niuma-plugin-demo",
            destination.path(),
            &ListenerConfig::default(),
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
            &HashMap::new(),
        )
        .unwrap_err();

        assert!(error.contains("不能移除内置插件"));
    }

    #[test]
    fn unsupported_platform_manifest_is_filtered() {
        let manifest = PluginManifest {
            id: "future".to_string(),
            tool_id: ToolKind::Custom("future".to_string()),
            display_name: "Future".to_string(),
            version: "0.1.0".to_string(),
            command: Some("./future".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: vec!["unsupported-os".to_string()],
            capabilities: vec![PluginCapability::EventWatcher],
            icon_url: None,
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
}
