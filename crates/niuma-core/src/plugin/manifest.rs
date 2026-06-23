use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::models::ToolKind;

use super::{validation::parse_plugin_manifest, PluginConfigField};

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

fn tool_plugin_kind() -> PluginKind {
    PluginKind::Tool
}
