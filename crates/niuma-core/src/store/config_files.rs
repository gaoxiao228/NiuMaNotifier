use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::listener_config::ListenerConfig;
use crate::platform::locale::LanguagePreference;
use crate::remote::config::RemoteConfig;
use crate::remote::device_identity::DeviceInstallId;
use crate::remote::settings::default_remote_config;

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
    #[serde(default = "default_remote_config")]
    remote_config: RemoteConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RemoteDeviceInstallIdFile {
    version: u32,
    install_id: String,
}

enum RemoteDeviceInstallIdCreateError {
    AlreadyExists,
    Other(String),
}

impl Default for AppConfigFile {
    fn default() -> Self {
        Self {
            listener_config: ListenerConfig::default(),
            language_preference: default_language_preference(),
            plugin_enabled_map: BTreeMap::new(),
            remote_config: default_remote_config(),
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

    pub(super) fn remote_config(&self) -> Result<RemoteConfig, String> {
        Ok(self.read_app_config()?.remote_config)
    }

    pub(super) fn save_remote_config(&self, config: &RemoteConfig) -> Result<(), String> {
        let mut app_config = self.read_app_config()?;
        app_config.remote_config = config.clone();
        self.write_app_config(&app_config)
    }

    pub(super) fn remote_device_install_id(&self) -> Result<DeviceInstallId, String> {
        let path = self.remote_device_install_id_path();
        match create_remote_device_install_id_file(&path) {
            Ok(install_id) => Ok(install_id),
            Err(RemoteDeviceInstallIdCreateError::AlreadyExists) => {
                read_remote_device_install_id_with_retry(&path)
            }
            Err(RemoteDeviceInstallIdCreateError::Other(error)) => Err(error),
        }
    }

    fn app_config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    fn remote_device_install_id_path(&self) -> PathBuf {
        self.root.join("remote-device-install-id.json")
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

fn create_remote_device_install_id_file(
    path: &Path,
) -> Result<DeviceInstallId, RemoteDeviceInstallIdCreateError> {
    let install_id = DeviceInstallId::generate();
    let file = RemoteDeviceInstallIdFile {
        version: 1,
        install_id: install_id.to_hex(),
    };
    let value = serde_json::to_value(file).map_err(|error| {
        RemoteDeviceInstallIdCreateError::Other(format!("序列化远程设备安装 ID 文件失败：{error}"))
    })?;
    let content = serde_json::to_string_pretty(&value).map_err(|error| {
        RemoteDeviceInstallIdCreateError::Other(format!("序列化远程设备安装 ID 文件失败：{error}"))
    })?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            RemoteDeviceInstallIdCreateError::Other(format!("创建配置目录失败：{error}"))
        })?;
    }

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == ErrorKind::AlreadyExists {
                RemoteDeviceInstallIdCreateError::AlreadyExists
            } else {
                RemoteDeviceInstallIdCreateError::Other(format!(
                    "创建远程设备安装 ID 文件失败：{error}"
                ))
            }
        })?;

    // create_new 保证只有一个首次创建者；其他并发调用会读取这个创建者写入的 ID。
    file.write_all(content.as_bytes()).map_err(|error| {
        RemoteDeviceInstallIdCreateError::Other(format!("写入远程设备安装 ID 文件失败：{error}"))
    })?;
    Ok(install_id)
}

fn read_remote_device_install_id_with_retry(path: &Path) -> Result<DeviceInstallId, String> {
    let mut last_error = None;
    for attempt in 0..20 {
        match read_remote_device_install_id_file(path) {
            Ok(install_id) => return Ok(install_id),
            Err(error) => {
                last_error = Some(error);
                if attempt < 19 {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "读取远程设备安装 ID 文件失败".to_string()))
}

fn read_remote_device_install_id_file(path: &Path) -> Result<DeviceInstallId, String> {
    let file: RemoteDeviceInstallIdFile = serde_json::from_value(read_json_file(path)?)
        .map_err(|error| format!("解析远程设备安装 ID 文件失败：{error}"))?;
    if file.version != 1 {
        return Err(format!("远程设备安装 ID 文件版本不支持：{}", file.version));
    }
    DeviceInstallId::from_hex(&file.install_id)
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
