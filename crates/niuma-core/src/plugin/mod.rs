use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::codex_hook::read_codex_hook_status;
use crate::config::codex_home;
use crate::listener_config::ListenerConfig;
use crate::models::ToolKind;

mod config;
mod enablement;
mod filesystem;
mod runtime_state;

pub use config::{
    merge_plugin_config_with_defaults, plugin_config_defaults, resolve_plugin_config,
    validate_plugin_config, PluginConfigField, PluginConfigFieldType,
};
pub use enablement::{
    default_non_tool_plugin_enabled, default_plugin_enabled, listener_config_after_plugin_removed,
    plugin_uses_listener_config, save_plugin_enabled_state,
};
pub use filesystem::{
    current_plugin_registry, default_user_plugin_dir, import_external_plugin_dir,
    remove_external_plugin,
};
pub use runtime_state::{PluginRuntimeState, PluginRuntimeStatus};

pub const CODEX_PLUGIN_COMMAND_ENV: &str = "NIUMA_CODEX_PLUGIN_COMMAND";
pub const BARK_PLUGIN_COMMAND_ENV: &str = "NIUMA_BARK_PLUGIN_COMMAND";
pub const NTFY_PLUGIN_COMMAND_ENV: &str = "NIUMA_NTFY_PLUGIN_COMMAND";
pub const BUILTIN_CODEX_PLUGIN_ID: &str = "builtin-codex";
pub const BUILTIN_BARK_PLUGIN_ID: &str = "builtin-bark";
pub const BUILTIN_NTFY_PLUGIN_ID: &str = "builtin-ntfy";

const CODEX_PLUGIN_COMMAND: &str = "niuma-codex-plugin";
const BARK_PLUGIN_COMMAND: &str = "niuma-plugin-bark";
const NTFY_PLUGIN_COMMAND: &str = "niuma-plugin-ntfy";
const BUILTIN_CODEX_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../../builtin-plugins/codex/plugin.json");
const BUILTIN_BARK_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../../builtin-plugins/bark/plugin.json");
const BUILTIN_NTFY_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../../builtin-plugins/ntfy/plugin.json");

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    EventWatcher,
    EventConsumer,
    ApprovalHandler,
    NotificationTest,
    StateConsumer,
    ToolSessionListProvider,
    ToolSessionDetailProvider,
    ToolSessionListReader,
    ToolSessionDetailReader,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Tool,
    Notification,
    StatusIndicator,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    Builtin,
    External,
}

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

    fn validate_provider_capability_uniqueness(
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
    if manifest.id != BUILTIN_CODEX_PLUGIN_ID {
        return Vec::new();
    }
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
    if manifest.kind != PluginKind::Tool {
        for capability in non_tool_forbidden_provider_capabilities(&manifest.capabilities) {
            return Err(format!(
                "非工具插件不能声明 {}：{}",
                plugin_capability_id(&capability),
                manifest.id
            ));
        }
    }
    if manifest
        .capabilities
        .contains(&PluginCapability::ToolSessionDetailProvider)
        && !manifest
            .capabilities
            .contains(&PluginCapability::ToolSessionListProvider)
    {
        return Err(format!(
            "tool_session_detail_provider 必须同时声明 tool_session_list_provider：{}",
            manifest.id
        ));
    }
    config::validate_config_schema(&manifest.id, &manifest.config_schema)
}

fn non_tool_forbidden_provider_capabilities(
    capabilities: &[PluginCapability],
) -> Vec<PluginCapability> {
    // 这些能力会代表具体 tool 上报或提供数据，必须绑定 tool_id 后才能安全路由。
    provider_capabilities(capabilities)
}

fn provider_capabilities(capabilities: &[PluginCapability]) -> Vec<PluginCapability> {
    [
        PluginCapability::EventWatcher,
        PluginCapability::ToolSessionListProvider,
        PluginCapability::ToolSessionDetailProvider,
    ]
    .into_iter()
    .filter(|capability| capabilities.contains(capability))
    .collect()
}

fn plugin_capability_id(capability: &PluginCapability) -> &'static str {
    match capability {
        PluginCapability::EventWatcher => "event_watcher",
        PluginCapability::EventConsumer => "event_consumer",
        PluginCapability::ApprovalHandler => "approval_handler",
        PluginCapability::NotificationTest => "notification_test",
        PluginCapability::StateConsumer => "state_consumer",
        PluginCapability::ToolSessionListProvider => "tool_session_list_provider",
        PluginCapability::ToolSessionDetailProvider => "tool_session_detail_provider",
        PluginCapability::ToolSessionListReader => "tool_session_list_reader",
        PluginCapability::ToolSessionDetailReader => "tool_session_detail_reader",
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_hook::{
        codex_config_file, codex_hooks_file, install_codex_hook, CodexHookCommand,
    };
    use crate::listener_config::ListenerConfig;
    use crate::models::ToolId;
    use crate::runtime_event::RuntimeEventBus;
    use crate::state_mutation::StateMutationService;
    use crate::store::NiumaStore;
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
    fn parses_notification_approval_handler_capability() {
        let manifest = parse_plugin_manifest(
            r#"{
                "id": "approval-menu",
                "kind": "notification",
                "display_name": "Approval Menu",
                "version": "0.1.0",
                "command": "approval-menu",
                "capabilities": ["event_consumer", "approval_handler"]
            }"#,
        )
        .unwrap();

        assert_eq!(manifest.kind, PluginKind::Notification);
        assert_eq!(manifest.tool_id, None);
        assert_eq!(
            manifest.capabilities,
            vec![
                PluginCapability::EventConsumer,
                PluginCapability::ApprovalHandler
            ]
        );
    }

    #[test]
    fn parses_status_indicator_state_consumer_without_tool_id() {
        let manifest = parse_plugin_manifest(
            r#"{
                "id": "status-indicator-demo",
                "kind": "status_indicator",
                "display_name": "Status Indicator Demo",
                "version": "0.1.0",
                "command": "node",
                "args": ["./bin/status-indicator-demo.mjs"],
                "capabilities": ["state_consumer"]
            }"#,
        )
        .unwrap();

        assert_eq!(manifest.kind, PluginKind::StatusIndicator);
        assert_eq!(manifest.tool_id, None);
        assert_eq!(manifest.capabilities, vec![PluginCapability::StateConsumer]);
    }

    #[test]
    fn parses_tool_session_provider_and_reader_capabilities() {
        let manifest = parse_plugin_manifest(
            r#"{
                "id": "codex-session-provider",
                "kind": "tool",
                "tool_id": "codex",
                "display_name": "Codex Session Provider",
                "version": "0.1.0",
                "command": "node",
                "capabilities": [
                    "tool_session_list_provider",
                    "tool_session_detail_provider",
                    "tool_session_list_reader",
                    "tool_session_detail_reader"
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(
            manifest.capabilities,
            vec![
                PluginCapability::ToolSessionListProvider,
                PluginCapability::ToolSessionDetailProvider,
                PluginCapability::ToolSessionListReader,
                PluginCapability::ToolSessionDetailReader,
            ]
        );
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
    fn rejects_tool_session_detail_provider_without_list_provider() {
        let error = parse_plugin_manifest(
            r#"{
                "id": "broken-detail-provider",
                "kind": "tool",
                "tool_id": "codex",
                "display_name": "Broken Detail Provider",
                "version": "0.1.0",
                "command": "broken",
                "capabilities": ["tool_session_detail_provider"]
            }"#,
        )
        .unwrap_err();

        assert!(
            error.contains("tool_session_detail_provider 必须同时声明 tool_session_list_provider")
        );
    }

    #[test]
    fn rejects_tool_session_provider_on_non_tool_plugin() {
        let error = parse_plugin_manifest(
            r#"{
                "id": "broken-notification-provider",
                "kind": "notification",
                "display_name": "Broken Provider",
                "version": "0.1.0",
                "command": "broken",
                "capabilities": ["tool_session_list_provider"]
            }"#,
        )
        .unwrap_err();

        assert!(error.contains("非工具插件不能声明 tool_session_list_provider"));
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
        assert_eq!(plugin.icon_url.as_deref(), Some("/assets/codex-icon.png"));
        assert_eq!(
            plugin.capabilities,
            vec![
                PluginCapability::EventWatcher,
                PluginCapability::ToolSessionListProvider,
                PluginCapability::ToolSessionDetailProvider,
            ]
        );
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
        assert_eq!(plugin.icon_url.as_deref(), Some("/assets/bark-icon.png"));
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
        assert_eq!(plugin.icon_url.as_deref(), Some("/assets/ntfy-logo.svg"));
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
    #[should_panic(expected = "同一工具的 provider capability 只能由一个插件声明")]
    fn registry_rejects_duplicate_provider_capability_for_same_tool() {
        let mut registry = PluginRegistry::new();
        registry.register(
            parse_plugin_manifest(
                r#"{
                    "id": "codex-list-provider-a",
                    "kind": "tool",
                    "tool_id": "codex",
                    "display_name": "Codex List Provider A",
                    "version": "0.1.0",
                    "command": "provider-a",
                    "capabilities": ["tool_session_list_provider"]
                }"#,
            )
            .unwrap(),
        );

        registry.register(
            parse_plugin_manifest(
                r#"{
                    "id": "codex-list-provider-b",
                    "kind": "tool",
                    "tool_id": "codex",
                    "display_name": "Codex List Provider B",
                    "version": "0.1.0",
                    "command": "provider-b",
                    "capabilities": ["tool_session_list_provider"]
                }"#,
            )
            .unwrap(),
        );
    }

    #[test]
    fn registry_skips_external_plugin_with_duplicate_builtin_provider_capability() {
        let temp = tempfile::tempdir().unwrap();
        let plugin_dir = temp.path().join("external-codex-session-provider");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            r#"{
                "id": "external-codex-session-provider",
                "kind": "tool",
                "tool_id": "codex",
                "display_name": "External Codex Session Provider",
                "version": "0.1.0",
                "command": "node",
                "platforms": ["macos", "windows", "linux"],
                "capabilities": ["tool_session_list_provider"]
            }"#,
        )
        .unwrap();

        let registry =
            PluginRegistry::with_builtin_plugins().discover_external_plugins(temp.path());

        assert!(registry
            .plugin_by_id("external-codex-session-provider")
            .is_none());
        assert!(registry.plugin_by_id(BUILTIN_CODEX_PLUGIN_ID).is_some());
    }

    #[test]
    fn registry_tools_excludes_non_tool_consumers() {
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
        registry.register(
            parse_plugin_manifest(
                r#"{
                "id": "status-indicator-demo",
                "kind": "status_indicator",
                "display_name": "Status Indicator Demo",
                "version": "0.1.0",
                "command": "node",
                "capabilities": ["state_consumer"]
            }"#,
            )
            .unwrap(),
        );

        assert!(registry.tools().is_empty());
    }

    #[test]
    fn management_items_enable_non_tool_plugins_by_default_until_explicitly_disabled() {
        let mut registry = PluginRegistry::new();
        registry.register(
            parse_plugin_manifest(
                r#"{
                "id": "builtin-bark",
                "kind": "notification",
                "display_name": "Bark",
                "version": "0.1.0",
                "command": "niuma-plugin-bark",
                "capabilities": ["event_consumer"],
                "source": "builtin"
            }"#,
            )
            .unwrap(),
        );
        registry.register(
            parse_plugin_manifest(
                r#"{
                "id": "status-indicator-demo",
                "kind": "status_indicator",
                "display_name": "Status Indicator Demo",
                "version": "0.1.0",
                "command": "node",
                "capabilities": ["state_consumer"]
            }"#,
            )
            .unwrap(),
        );
        let enabled = BTreeMap::from([("status-indicator-demo".to_string(), false)]);

        let items =
            registry.management_items(&ListenerConfig::default(), &enabled, &HashMap::new());

        assert_eq!(items[0].kind, PluginKind::Notification);
        assert_eq!(items[0].tool_id, None);
        assert!(items[0].enabled);
        assert_eq!(items[1].kind, PluginKind::StatusIndicator);
        assert_eq!(items[1].tool_id, None);
        assert!(!items[1].enabled);
    }

    #[test]
    fn external_session_provider_enable_state_uses_plugin_map_without_changing_listener_config() {
        let store = NiumaStore::new(test_sqlite_path("session_provider_enabled_map"));
        store
            .save_listener_config(
                &ListenerConfig::default().with_tool_enabled(&ToolKind::Codex, true),
            )
            .unwrap();
        let service = StateMutationService::new(store.clone(), RuntimeEventBus::new());
        let manifest = parse_plugin_manifest(
            r#"{
                "id": "external-demo-session-provider",
                "kind": "tool",
                "tool_id": "demo_tool",
                "display_name": "External Demo Session Provider",
                "version": "0.1.0",
                "command": "node",
                "capabilities": ["tool_session_list_provider", "tool_session_detail_provider"]
            }"#,
        )
        .unwrap();

        // session provider 有 tool_id，但没有 event_watcher，启用状态必须独立于工具监听配置。
        save_plugin_enabled_state(&store, &service, &manifest, false).unwrap();

        assert!(store
            .listener_config()
            .unwrap()
            .is_tool_enabled(&ToolKind::Codex));
        assert_eq!(
            store
                .plugin_enabled_map()
                .unwrap()
                .get("external-demo-session-provider"),
            Some(&false)
        );
    }

    #[test]
    fn builtin_codex_provider_follows_listener_config_after_merge() {
        let registry = PluginRegistry::with_builtin_plugins();
        let items = registry.management_items(
            &ListenerConfig::default().with_tool_enabled(&ToolKind::Codex, false),
            &BTreeMap::new(),
            &HashMap::new(),
        );
        let codex = items
            .iter()
            .find(|plugin| plugin.id == BUILTIN_CODEX_PLUGIN_ID)
            .unwrap();

        // 合并后 provider 能力跟随 Codex listener，关闭监听也不再保留 session 列表。
        assert!(!codex.enabled);
    }

    #[test]
    fn plugin_remove_session_provider_listener_config_keeps_tool_enabled() {
        let store = NiumaStore::new(test_sqlite_path("remove_session_provider_listener_config"));
        store
            .save_listener_config(
                &ListenerConfig::default()
                    .with_tool_enabled(&ToolKind::Custom("demo_tool".to_string()), true),
            )
            .unwrap();
        let service = StateMutationService::new(store.clone(), RuntimeEventBus::new());
        let manifest = parse_plugin_manifest(
            r#"{
                "id": "demo-session-provider",
                "kind": "tool",
                "tool_id": "demo_tool",
                "display_name": "Session Provider",
                "version": "0.1.0",
                "command": "node",
                "capabilities": ["tool_session_list_provider", "tool_session_detail_provider"]
            }"#,
        )
        .unwrap();

        let config = listener_config_after_plugin_removed(&store, &service, &manifest).unwrap();

        // session provider 的 tool_id 只用于会话数据归属，不应触发工具监听关闭。
        assert!(config.is_tool_enabled(&ToolKind::Custom("demo_tool".to_string())));
        assert!(store
            .listener_config()
            .unwrap()
            .is_tool_enabled(&ToolKind::Custom("demo_tool".to_string())));
    }

    #[test]
    fn plugin_remove_event_watcher_listener_config_disables_tool() {
        let store = NiumaStore::new(test_sqlite_path("remove_event_watcher_listener_config"));
        store
            .save_listener_config(
                &ListenerConfig::default()
                    .with_tool_enabled(&ToolKind::Custom("demo_tool".to_string()), true),
            )
            .unwrap();
        let service = StateMutationService::new(store.clone(), RuntimeEventBus::new());
        let manifest = parse_plugin_manifest(
            r#"{
                "id": "demo-event-watcher",
                "kind": "tool",
                "tool_id": "demo_tool",
                "display_name": "Event Watcher",
                "version": "0.1.0",
                "command": "node",
                "capabilities": ["event_watcher"]
            }"#,
        )
        .unwrap();

        let config = listener_config_after_plugin_removed(&store, &service, &manifest).unwrap();

        // event_watcher 才代表工具监听开关，删除时继续沿用原先的关闭逻辑。
        assert!(!config.is_tool_enabled(&ToolKind::Custom("demo_tool".to_string())));
        assert!(!store
            .listener_config()
            .unwrap()
            .is_tool_enabled(&ToolKind::Custom("demo_tool".to_string())));
    }

    #[test]
    fn builtin_codex_management_item_declares_install_hook_action_when_uninstalled() {
        let _guard = env_lock().lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("CODEX_HOME", temp.path());
        let registry = PluginRegistry::with_builtin_plugins();

        let items = registry.management_items(
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        );

        let codex = items
            .iter()
            .find(|plugin| plugin.id == "builtin-codex")
            .unwrap();
        let action_ids = codex
            .management_actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(action_ids, vec!["codex_hook_install"]);
        assert_eq!(codex.management_actions[0].label, "安装 Hook");
        assert_eq!(
            codex.management_actions[0].status_label.as_deref(),
            Some("Hook 未安装")
        );

        let bark = items
            .iter()
            .find(|plugin| plugin.id == "builtin-bark")
            .unwrap();
        assert!(bark.management_actions.is_empty());

        restore_codex_home(previous_codex_home);
    }

    #[test]
    fn builtin_codex_management_item_declares_remove_hook_action_when_installed() {
        let _guard = env_lock().lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        std::env::set_var("CODEX_HOME", temp.path());

        install_codex_hook(temp.path(), CodexHookCommand::Installed).unwrap();
        let registry = PluginRegistry::with_builtin_plugins();
        let items = registry.management_items(
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        );

        let codex = items
            .iter()
            .find(|plugin| plugin.id == "builtin-codex")
            .unwrap();
        assert_eq!(codex.management_actions.len(), 1);
        let remove_action = &codex.management_actions[0];

        assert_eq!(remove_action.id, "codex_hook_uninstall");
        assert_eq!(remove_action.label, "移除 Hook");
        assert_eq!(remove_action.status_label.as_deref(), Some("Hook 已安装"));
        assert_eq!(
            remove_action.status_level,
            PluginManagementActionStatusLevel::Ok
        );
        assert_eq!(
            remove_action.description,
            "Hook 已安装到 Codex 配置。首次使用前，请在 Codex 的 /hooks 中信任 Niuma Hook。"
        );
        assert!(codex_config_file(temp.path()).exists() || codex_hooks_file(temp.path()).exists());

        restore_codex_home(previous_codex_home);
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
    fn import_rejects_duplicate_provider_before_copying_destination() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        write_session_provider_plugin(source.path(), "external-codex-session-provider", "codex");

        let error = import_external_plugin_dir(
            source.path(),
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap_err();

        // 与内置 Codex session provider 冲突时，不能留下半导入目录。
        assert!(error.contains("同一工具的 provider capability 只能由一个插件声明"));
        assert!(!destination
            .path()
            .join("external-codex-session-provider")
            .exists());
    }

    #[test]
    fn import_replaces_same_external_provider_without_self_conflict() {
        let source = tempfile::tempdir().unwrap();
        let replacement = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        write_session_provider_plugin(source.path(), "demo-session-provider", "demo_tool");
        write_session_provider_plugin(replacement.path(), "demo-session-provider", "demo_tool");
        std::fs::write(replacement.path().join("replacement.txt"), "new").unwrap();

        import_external_plugin_dir(
            source.path(),
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        let result = import_external_plugin_dir(
            replacement.path(),
            destination.path(),
            &ListenerConfig::default(),
            &BTreeMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        // 同 id 替换自身时允许 provider capability 不变，但仍执行目录替换。
        assert!(result.imported);
        assert!(destination
            .path()
            .join("demo-session-provider/replacement.txt")
            .exists());
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

    fn write_session_provider_plugin(dir: &Path, id: &str, tool_id: &str) {
        std::fs::write(
            dir.join("plugin.json"),
            format!(
                r#"{{
                    "id": "{id}",
                    "kind": "tool",
                    "tool_id": "{tool_id}",
                    "display_name": "Session Provider",
                    "version": "0.1.0",
                    "command": "node",
                    "platforms": ["macos", "windows", "linux"],
                    "capabilities": ["tool_session_list_provider", "tool_session_detail_provider"]
                }}"#
            ),
        )
        .unwrap();
    }

    fn test_sqlite_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niuma-notifier-plugin-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("niuma.sqlite")
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn restore_codex_home(previous_codex_home: Option<String>) {
        if let Some(value) = previous_codex_home {
            std::env::set_var("CODEX_HOME", value);
        } else {
            std::env::remove_var("CODEX_HOME");
        }
    }

    fn builtin_codex_plugin_manifest_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."))
            .join("builtin-plugins/codex/plugin.json")
    }
}
