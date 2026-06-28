use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteUserSummary {
    pub id: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteDeviceSummary {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteConfig {
    pub server_url: String,
    pub user: Option<RemoteUserSummary>,
    pub device: Option<RemoteDeviceSummary>,
    pub remote_access_enabled: bool,
    pub remote_control_enabled: bool,
    pub last_connected_at: Option<String>,
}

impl RemoteConfig {
    pub fn default_for_server(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            user: None,
            device: None,
            remote_access_enabled: true,
            remote_control_enabled: true,
            last_connected_at: None,
        }
    }

    pub fn has_bound_device(&self) -> bool {
        self.device.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_remote_config_is_enabled_after_login_policy_ready() {
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        assert_eq!(config.server_url, "https://remote.example.com");
        assert!(config.remote_access_enabled);
        assert!(config.remote_control_enabled);
        assert!(config.user.is_none());
        assert!(config.device.is_none());
    }

    #[test]
    fn detects_configured_device() {
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        assert!(!config.has_bound_device());
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "NiuMa MacBook".to_string(),
        });
        assert!(config.has_bound_device());
    }
}
