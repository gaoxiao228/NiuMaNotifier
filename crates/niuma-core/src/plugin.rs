use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    Stopped,
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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn registry_contains_builtin_codex_plugin() {
        let registry = PluginRegistry::with_builtin_plugins();
        let plugin = registry.plugin_for_tool(&ToolKind::Codex).unwrap();

        assert_eq!(plugin.id, "builtin-codex");
        assert_eq!(plugin.tool_id, ToolKind::Codex);
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
}
