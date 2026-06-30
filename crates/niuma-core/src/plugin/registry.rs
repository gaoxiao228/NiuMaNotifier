use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::claude_hook::read_claude_hook_status;
use crate::codex_hook::read_codex_hook_status;
use crate::config::codex_home;
use crate::listener_config::ListenerConfig;
use crate::models::ToolKind;

use super::{
    builtin_bark_manifest, builtin_claude_code_manifest, builtin_codex_manifest,
    builtin_ntfy_manifest, enablement,
    validation::{plugin_capability_id, provider_capabilities},
    PluginCapability, PluginConfigField, PluginKind, PluginManifest, PluginRuntimeState,
    PluginRuntimeStatus, PluginSource, BUILTIN_CLAUDE_CODE_PLUGIN_ID, BUILTIN_CODEX_PLUGIN_ID,
};

// 管理动作只描述插件管理界面上的受控操作，不等同于插件运行时 capability。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginManagementActionKind {
    Primary,
    Secondary,
    Danger,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginManagementActionStatusLevel {
    Neutral,
    Ok,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginManagementAction {
    pub id: String,
    pub label: String,
    pub description: String,
    pub kind: PluginManagementActionKind,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_label: Option<String>,
    pub status_level: PluginManagementActionStatusLevel,
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
    // 由宿主后端生成，避免外部 manifest 直接声明任意本机操作。
    pub management_actions: Vec<PluginManagementAction>,
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
        registry.register(builtin_claude_code_manifest());
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
                            if let Err(error) =
                                self.validate_provider_capability_uniqueness(&manifest)
                            {
                                eprintln!(
                                    "NiumaNotifier external plugin manifest ignored {}: {error}",
                                    manifest_path.display()
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
        if let Err(error) = self.validate_provider_capability_uniqueness(&manifest) {
            panic!("内置或直接注册插件 manifest 无效：{error}");
        }
        // 同 id 后注册覆盖先注册，方便外部插件导入新版时替换旧 manifest。
        self.manifests.retain(|item| item.id != manifest.id);
        self.manifests.push(manifest);
    }

    pub fn validate_provider_capability_uniqueness(
        &self,
        manifest: &PluginManifest,
    ) -> Result<(), String> {
        let Some(tool_id) = manifest.tool_id.as_ref() else {
            return Ok(());
        };
        for capability in provider_capabilities(&manifest.capabilities) {
            if let Some(existing) = self.manifests.iter().find(|item| {
                item.id != manifest.id
                    && item.tool_id.as_ref() == Some(tool_id)
                    && item.capabilities.contains(&capability)
            }) {
                return Err(format!(
                    "同一工具的 provider capability 只能由一个插件声明：tool_id={}，capability={}，已有插件={}，新插件={}",
                    tool_id.as_str(),
                    plugin_capability_id(&capability),
                    existing.id,
                    manifest.id
                ));
            }
        }
        Ok(())
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
                    enabled: enablement::plugin_enabled(manifest, config, plugin_enabled_map),
                    runtime_status: runtime.status,
                    last_error: runtime.last_error,
                    icon_url: manifest.icon_url.clone(),
                    capabilities: manifest.capabilities.clone(),
                    management_actions: management_actions_for_manifest(manifest),
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

fn management_actions_for_manifest(manifest: &PluginManifest) -> Vec<PluginManagementAction> {
    match manifest.id.as_str() {
        BUILTIN_CODEX_PLUGIN_ID => codex_hook_management_actions(),
        BUILTIN_CLAUDE_CODE_PLUGIN_ID => claude_hook_management_actions(),
        _ => Vec::new(),
    }
}

fn codex_hook_management_actions() -> Vec<PluginManagementAction> {
    // 插件管理只展示 Niuma 可稳定检测的安装状态；Codex 信任是用户操作提示，不参与状态判断。
    let action = match read_codex_hook_status(&codex_home()) {
        Ok(status) if status.installed => PluginManagementAction {
            id: "codex_hook_uninstall".to_string(),
            label: "移除 Hook".to_string(),
            description: "Hook 已安装到 Codex 配置。首次使用前，请在 Codex 的 /hooks 中信任 Niuma Hook。"
                .to_string(),
            kind: PluginManagementActionKind::Danger,
            enabled: true,
            status_label: Some("Hook 已安装".to_string()),
            status_level: PluginManagementActionStatusLevel::Ok,
        },
        Ok(_) => PluginManagementAction {
            id: "codex_hook_install".to_string(),
            label: "安装 Hook".to_string(),
            description:
                "接收 Codex 权限请求并回传允许/拒绝结果。安装后需在 Codex 中执行 /hooks 信任，信任后才能拦截授权请求。"
                    .to_string(),
            kind: PluginManagementActionKind::Primary,
            enabled: true,
            status_label: Some("Hook 未安装".to_string()),
            status_level: PluginManagementActionStatusLevel::Neutral,
        },
        Err(error) => PluginManagementAction {
            id: "codex_hook_install".to_string(),
            label: "安装 Hook".to_string(),
            description:
                "接收 Codex 权限请求并回传允许/拒绝结果。安装后需在 Codex 中执行 /hooks 信任，信任后才能拦截授权请求。"
                    .to_string(),
            kind: PluginManagementActionKind::Primary,
            enabled: true,
            status_label: Some(format!("Hook 状态读取失败：{error}")),
            status_level: PluginManagementActionStatusLevel::Error,
        },
    };
    vec![action]
}

fn claude_hook_management_actions() -> Vec<PluginManagementAction> {
    let action = match read_claude_hook_status(&crate::config::claude_config_dir()) {
        Ok(status) if status.installed => PluginManagementAction {
            id: "claude_hook_uninstall".to_string(),
            label: "移除 Hook".to_string(),
            description: "Hook 已安装到 Claude Code 配置，Niuma 将代处理权限请求。".to_string(),
            kind: PluginManagementActionKind::Danger,
            enabled: true,
            status_label: Some("Hook 已安装".to_string()),
            status_level: PluginManagementActionStatusLevel::Ok,
        },
        Ok(_) => PluginManagementAction {
            id: "claude_hook_install".to_string(),
            label: "安装 Hook".to_string(),
            description: "接收 Claude Code 权限请求并回传允许/拒绝结果。".to_string(),
            kind: PluginManagementActionKind::Primary,
            enabled: true,
            status_label: Some("Hook 未安装".to_string()),
            status_level: PluginManagementActionStatusLevel::Neutral,
        },
        Err(error) => PluginManagementAction {
            id: "claude_hook_install".to_string(),
            label: "安装 Hook".to_string(),
            description: "接收 Claude Code 权限请求并回传允许/拒绝结果。".to_string(),
            kind: PluginManagementActionKind::Primary,
            enabled: true,
            status_label: Some(format!("Hook 状态读取失败：{error}")),
            status_level: PluginManagementActionStatusLevel::Error,
        },
    };
    vec![action]
}

impl ToolPluginInfo {
    fn try_from_manifest(manifest: &PluginManifest) -> Option<Self> {
        let tool_id = manifest.tool_id.clone()?;
        if !manifest
            .capabilities
            .contains(&PluginCapability::EventWatcher)
        {
            return None;
        }
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
