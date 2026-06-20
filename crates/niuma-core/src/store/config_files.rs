use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::listener_config::ListenerConfig;
use crate::platform::locale::LanguagePreference;

#[derive(Clone, Debug)]
pub(super) struct ConfigFileStore {
    root: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppConfigFile {
    #[serde(default)]
    listener_config: ListenerConfig,
    #[serde(default = "default_language_preference")]
    language_preference: String,
    #[serde(default)]
    plugin_enabled_map: BTreeMap<String, bool>,
}

impl Default for AppConfigFile {
    fn default() -> Self {
        Self {
            listener_config: ListenerConfig::default(),
            language_preference: default_language_preference(),
            plugin_enabled_map: BTreeMap::new(),
        }
    }
}

impl ConfigFileStore {
    pub(super) fn new(db_path: &Path) -> Self {
        let root = db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self { root }
    }

    pub(super) fn listener_config(&self) -> Result<ListenerConfig, String> {
        Ok(self.read_app_config()?.listener_config)
    }

    pub(super) fn save_listener_config(&self, config: &ListenerConfig) -> Result<(), String> {
        let mut app_config = self.read_app_config()?;
        app_config.listener_config = config.clone();
        self.write_app_config(&app_config)
    }

    pub(super) fn language_preference(&self) -> Result<LanguagePreference, String> {
        let preference = self.read_app_config()?.language_preference;
        LanguagePreference::from_storage_id(&preference)
            .ok_or_else(|| format!("未知语言偏好：{preference}"))
    }

    pub(super) fn save_language_preference(
        &self,
        preference: LanguagePreference,
    ) -> Result<(), String> {
        let mut app_config = self.read_app_config()?;
        app_config.language_preference = preference.storage_id().to_string();
        self.write_app_config(&app_config)
    }

    pub(super) fn plugin_enabled_map(&self) -> Result<BTreeMap<String, bool>, String> {
        Ok(self.read_app_config()?.plugin_enabled_map)
    }

    pub(super) fn save_plugin_enabled_map(
        &self,
        map: &BTreeMap<String, bool>,
    ) -> Result<(), String> {
        let mut app_config = self.read_app_config()?;
        app_config.plugin_enabled_map = map.clone();
        self.write_app_config(&app_config)
    }

    pub(super) fn plugin_config(
        &self,
        plugin_id: &str,
    ) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
        let path = self.plugin_config_path(plugin_id);
        if !path.exists() {
            return Ok(None);
        }
        let value = read_json_file(&path)?;
        let Some(object) = value.as_object() else {
            return Err(format!("插件配置格式无效：{plugin_id}"));
        };
        Ok(Some(object.clone()))
    }

    pub(super) fn save_plugin_config(
        &self,
        plugin_id: &str,
        config: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        let path = self.plugin_config_path(plugin_id);
        write_json_file(&path, &serde_json::Value::Object(config.clone()))
    }

    pub(super) fn remove_plugin_config(&self, plugin_id: &str) -> Result<(), String> {
        let path = self.plugin_config_path(plugin_id);
        if path.exists() {
            fs::remove_file(&path).map_err(|error| format!("移除插件配置失败：{error}"))?;
        }
        Ok(())
    }

    fn app_config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    fn plugin_config_path(&self, plugin_id: &str) -> PathBuf {
        self.root
            .join("plugin-configs")
            .join(format!("{plugin_id}.json"))
    }

    fn read_app_config(&self) -> Result<AppConfigFile, String> {
        let path = self.app_config_path();
        if !path.exists() {
            return Ok(AppConfigFile::default());
        }
        serde_json::from_value(read_json_file(&path)?)
            .map_err(|error| format!("解析应用配置失败：{error}"))
    }

    fn write_app_config(&self, config: &AppConfigFile) -> Result<(), String> {
        let value =
            serde_json::to_value(config).map_err(|error| format!("序列化应用配置失败：{error}"))?;
        write_json_file(&self.app_config_path(), &value)
    }
}

fn default_language_preference() -> String {
    LanguagePreference::System.storage_id().to_string()
}

fn read_json_file(path: &Path) -> Result<serde_json::Value, String> {
    let content = fs::read_to_string(path).map_err(|error| format!("读取配置文件失败：{error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析配置文件失败：{error}"))
}

fn write_json_file(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败：{error}"))?;
    }
    let content = serde_json::to_string_pretty(value)
        .map_err(|error| format!("序列化配置文件失败：{error}"))?;
    fs::write(path, content).map_err(|error| format!("写入配置文件失败：{error}"))
}
