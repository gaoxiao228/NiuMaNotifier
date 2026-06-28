use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::device_identity::{derive_device_fingerprint, DeviceInstallId};
use niuma_core::remote::login_flow::{
    ApiEnvelope, DesktopLoginBindingResult, DesktopLoginStartRequest, DesktopLoginStartResponse,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

static LOGIN_SESSIONS: Lazy<Mutex<HashMap<String, RemoteLoginSessionSecret>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
struct RemoteLoginSessionSecret {
    device_identity_private_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteLoginStarted {
    pub request_id: String,
    pub poll_token: String,
    pub login_url: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone)]
pub struct RemoteLoginPollOutcome {
    pub binding: Option<DesktopLoginBindingResult>,
    pub device_identity_private_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopLoginPollRequest {
    request_id: String,
    poll_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopLoginPollResponse {
    completed: bool,
    binding: Option<DesktopLoginBindingResult>,
}

pub trait BrowserOpener {
    fn open_url(&self, url: &str) -> Result<(), String>;
}

pub struct RemoteLoginFlow;

impl RemoteLoginFlow {
    pub fn build_start_request(
        device_name: &str,
        device_fingerprint: String,
        desktop_public_key: String,
        device_identity_public_key: String,
    ) -> DesktopLoginStartRequest {
        DesktopLoginStartRequest::new(
            device_name,
            device_fingerprint,
            desktop_public_key,
            device_identity_public_key,
        )
    }

    pub fn open_login_url(
        opener: &dyn BrowserOpener,
        response: &DesktopLoginStartResponse,
    ) -> Result<(), String> {
        opener.open_url(&response.login_url)
    }

    pub fn apply_binding_result(result: DesktopLoginBindingResult) -> DesktopLoginBindingResult {
        result
    }
}

pub async fn start_remote_login_session(
    config: &RemoteConfig,
) -> Result<RemoteLoginStarted, String> {
    let install_id = DeviceInstallId::generate();
    let device_fingerprint = derive_device_fingerprint(&config.server_url, &install_id);
    let device_identity_private_key = format!("local-dev-identity-{}", Uuid::new_v4());
    let device_identity_public_key = format!("public-{device_identity_private_key}");
    let desktop_public_key = format!("desktop-login-{}", Uuid::new_v4());
    let request = DesktopLoginStartRequest::new(
        whoami_device_name(),
        device_fingerprint,
        desktop_public_key,
        device_identity_public_key,
    );
    let endpoint = format!(
        "{}/api/v1/desktop-login/start",
        config.server_url.trim_end_matches('/')
    );
    let response = reqwest::Client::new()
        .post(endpoint)
        .json(&request)
        .send()
        .await
        .map_err(|error| format!("远程登录请求失败：{error}"))?;
    let envelope = response
        .json::<ApiEnvelope<DesktopLoginStartResponse>>()
        .await
        .map_err(|error| format!("远程登录响应解析失败：{error}"))?;
    let data = envelope
        .into_success_data()
        .map_err(|error| error.message)?;
    open::that(&data.login_url).map_err(|error| format!("打开浏览器失败：{error}"))?;
    LOGIN_SESSIONS
        .lock()
        .map_err(|_| "远程登录会话锁定失败".to_string())?
        .insert(
            data.request_id.clone(),
            RemoteLoginSessionSecret {
                device_identity_private_key,
            },
        );
    Ok(RemoteLoginStarted {
        request_id: data.request_id,
        poll_token: data.poll_token,
        login_url: data.login_url,
        expires_in: data.expires_in,
    })
}

pub async fn poll_remote_login_session(
    server_url: &str,
    request_id: &str,
    poll_token: &str,
) -> Result<RemoteLoginPollOutcome, String> {
    let endpoint = format!(
        "{}/api/v1/desktop-login/poll",
        server_url.trim_end_matches('/')
    );
    let response = reqwest::Client::new()
        .post(endpoint)
        .json(&DesktopLoginPollRequest {
            request_id: request_id.to_string(),
            poll_token: poll_token.to_string(),
        })
        .send()
        .await
        .map_err(|error| format!("远程登录轮询失败：{error}"))?;
    let envelope = response
        .json::<ApiEnvelope<DesktopLoginPollResponse>>()
        .await
        .map_err(|error| format!("远程登录轮询响应解析失败：{error}"))?;
    let data = envelope
        .into_success_data()
        .map_err(|error| error.message)?;
    if !data.completed {
        return Ok(RemoteLoginPollOutcome {
            binding: None,
            device_identity_private_key: String::new(),
        });
    }
    let secret = LOGIN_SESSIONS
        .lock()
        .map_err(|_| "远程登录会话锁定失败".to_string())?
        .remove(request_id)
        .ok_or_else(|| "远程登录会话已失效，请重新登录".to_string())?;
    Ok(RemoteLoginPollOutcome {
        binding: data.binding,
        device_identity_private_key: secret.device_identity_private_key,
    })
}

fn whoami_device_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "NiuMa Device".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBrowserOpener;

    impl BrowserOpener for TestBrowserOpener {
        fn open_url(&self, url: &str) -> Result<(), String> {
            assert_eq!(url, "https://remote.example.com/login");
            Ok(())
        }
    }

    #[test]
    fn builds_start_request_and_opens_login_url() {
        let request = RemoteLoginFlow::build_start_request(
            "NiuMa MacBook",
            "f".repeat(64),
            "desktop-public-key".to_string(),
            "{\"kty\":\"EC\"}".to_string(),
        );
        assert!(request.supports_remote_control());

        let response = DesktopLoginStartResponse {
            request_id: "login_req_1".to_string(),
            poll_token: "poll_token_1".to_string(),
            login_url: "https://remote.example.com/login".to_string(),
            expires_in: 300,
        };
        RemoteLoginFlow::open_login_url(&TestBrowserOpener, &response).unwrap();
    }

    #[test]
    fn returns_binding_result_without_persisting_credentials() {
        let result = DesktopLoginBindingResult {
            user_id: "user_1".to_string(),
            user_email: "user@example.com".to_string(),
            user_role: "owner".to_string(),
            device_id: "dev_1".to_string(),
            device_name: "NiuMa MacBook".to_string(),
            device_token: "device-token".to_string(),
        };

        assert_eq!(
            RemoteLoginFlow::apply_binding_result(result).device_id,
            "dev_1"
        );
    }
}
