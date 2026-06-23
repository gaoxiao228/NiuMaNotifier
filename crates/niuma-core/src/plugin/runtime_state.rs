use serde::{Deserialize, Serialize};

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
