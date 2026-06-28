use std::time::Duration;

pub const TOKEN_REVOKED_CLOSE_CODE: u16 = 4003;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSocketCloseReason {
    TokenRevoked,
    ServerShutdown,
    NetworkError,
    ProtocolError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectBackoff {
    base: Duration,
    max: Duration,
}

impl ReconnectBackoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self { base, max }
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let multiplier = 2u32.saturating_pow(attempt.min(16));
        self.base.saturating_mul(multiplier).min(self.max)
    }
}

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self::new(Duration::from_secs(1), Duration::from_secs(60))
    }
}

pub fn device_socket_url(server_url: &str) -> Result<String, String> {
    let trimmed = server_url.trim().trim_end_matches('/');
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return Ok(format!("wss://{rest}/ws/device"));
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return Ok(format!("ws://{rest}/ws/device"));
    }
    Err("远程服务地址必须以 http:// 或 https:// 开头".to_string())
}

pub fn classify_device_socket_close(code: Option<u16>) -> DeviceSocketCloseReason {
    match code {
        Some(TOKEN_REVOKED_CLOSE_CODE) => DeviceSocketCloseReason::TokenRevoked,
        Some(1001) => DeviceSocketCloseReason::ServerShutdown,
        Some(1002) | Some(1003) | Some(1007) | Some(1008) => DeviceSocketCloseReason::ProtocolError,
        _ => DeviceSocketCloseReason::NetworkError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn derives_device_socket_url_from_server_url() {
        assert_eq!(
            device_socket_url("https://remote.example.com").unwrap(),
            "wss://remote.example.com/ws/device"
        );
        assert_eq!(
            device_socket_url("http://127.0.0.1:27880/").unwrap(),
            "ws://127.0.0.1:27880/ws/device"
        );
    }

    #[test]
    fn classifies_token_revoked_close_code() {
        assert_eq!(
            classify_device_socket_close(Some(4003)),
            DeviceSocketCloseReason::TokenRevoked
        );
    }

    #[test]
    fn retry_backoff_caps_at_sixty_seconds() {
        let policy = ReconnectBackoff::new(Duration::from_secs(1), Duration::from_secs(60));
        assert_eq!(policy.delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(8));
        assert_eq!(policy.delay_for_attempt(99), Duration::from_secs(60));
    }
}
