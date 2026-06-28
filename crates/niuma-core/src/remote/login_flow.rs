use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiEnvelope<T> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

impl<T> ApiEnvelope<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            message: "ok".to_string(),
            data: Some(data),
        }
    }

    pub fn into_success_data(self) -> Result<T, RemoteApiEnvelopeError> {
        if self.code != 0 {
            return Err(RemoteApiEnvelopeError {
                code: self.code,
                message: self.message,
            });
        }

        self.data.ok_or(RemoteApiEnvelopeError {
            code: 900001,
            message: "服务端成功响应缺少 data".to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteApiEnvelopeError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceCapabilities {
    pub agent_protocol_version: u32,
    pub rpc_protocol_version: u32,
    pub supports_webrtc: bool,
    pub supports_relay: bool,
    pub supports_remote_control: bool,
}

impl Default for DeviceCapabilities {
    fn default() -> Self {
        Self {
            agent_protocol_version: 1,
            rpc_protocol_version: 1,
            supports_webrtc: true,
            supports_relay: true,
            supports_remote_control: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopLoginStartRequest {
    pub device_name: String,
    pub device_fingerprint: String,
    pub desktop_public_key: String,
    pub device_identity_public_key: String,
    pub capabilities: DeviceCapabilities,
}

impl DesktopLoginStartRequest {
    pub fn new(
        device_name: impl Into<String>,
        device_fingerprint: impl Into<String>,
        desktop_public_key: impl Into<String>,
        device_identity_public_key: impl Into<String>,
    ) -> Self {
        Self {
            device_name: device_name.into(),
            device_fingerprint: device_fingerprint.into(),
            desktop_public_key: desktop_public_key.into(),
            device_identity_public_key: device_identity_public_key.into(),
            capabilities: DeviceCapabilities::default(),
        }
    }

    pub fn supports_remote_control(&self) -> bool {
        self.capabilities.supports_remote_control
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopLoginStartResponse {
    pub request_id: String,
    pub poll_token: String,
    pub login_url: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopLoginBindingResult {
    pub user_id: String,
    pub user_email: String,
    pub user_role: String,
    pub device_id: String,
    pub device_name: String,
    pub device_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_request_contains_identity_public_key_and_capabilities() {
        let request = DesktopLoginStartRequest::new(
            "NiuMa MacBook",
            "f".repeat(64),
            "desktop-public-key",
            "{\"kty\":\"EC\"}",
        );

        assert_eq!(request.device_name, "NiuMa MacBook");
        assert!(request.supports_remote_control());
        assert_eq!(request.device_identity_public_key, "{\"kty\":\"EC\"}");
    }

    #[test]
    fn api_envelope_requires_success_code_before_data_is_used() {
        let envelope = ApiEnvelope::success(DesktopLoginStartResponse {
            request_id: "login_req_1".to_string(),
            poll_token: "poll_token_1".to_string(),
            login_url: "https://remote.example.com/desktop-login/login_req_1".to_string(),
            expires_in: 300,
        });

        let data = envelope.into_success_data().expect("success data");
        assert_eq!(data.request_id, "login_req_1");
    }
}
