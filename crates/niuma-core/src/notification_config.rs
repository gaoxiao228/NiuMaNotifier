use serde_json::Value;

use crate::notification_store::{parse_notification_channel_configs, NotificationChannelConfig};
use crate::store::SqliteStateStore;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationConfigErrorKind {
    BusinessValidation,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationConfigError {
    kind: NotificationConfigErrorKind,
    message: String,
}

impl NotificationConfigError {
    pub fn business_validation(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationConfigErrorKind::BusinessValidation,
            message: message.into(),
        }
    }

    pub fn system(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationConfigErrorKind::System,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> NotificationConfigErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone)]
pub struct NotificationConfigService {
    store: SqliteStateStore,
}

impl NotificationConfigService {
    pub fn new(store: SqliteStateStore) -> Self {
        Self { store }
    }

    pub fn channels(&self) -> Result<Vec<NotificationChannelConfig>, String> {
        self.store.notification_channels()
    }

    pub fn save_from_value(
        &self,
        value: &Value,
    ) -> Result<Vec<NotificationChannelConfig>, NotificationConfigError> {
        // 统一复用配置解析规则，避免 API 与 Tauri command 各自维护校验逻辑。
        let channels = parse_notification_channel_configs(value)
            .map_err(NotificationConfigError::business_validation)?;
        self.store
            .save_notification_channels(channels.clone())
            .map_err(NotificationConfigError::system)?;
        Ok(channels)
    }
}
