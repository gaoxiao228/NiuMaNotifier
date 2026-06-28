use josekit::jwe::{self, ECDH_ES};
use josekit::jwk::{alg::ec::EcCurve, Jwk};
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::device_identity::{derive_device_fingerprint, DeviceInstallId};
use niuma_core::remote::login_flow::{
    ApiEnvelope, DesktopLoginBindingResult, DesktopLoginEncryptedResult, DesktopLoginStartRequest,
    DesktopLoginStartResponse,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

static LOGIN_SESSIONS: Lazy<Mutex<HashMap<String, RemoteLoginSessionSecret>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
struct RemoteLoginSessionSecret {
    device_identity_private_key: String,
    desktop_private_key_jwk: String,
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
    encrypted_result: DesktopLoginEncryptedResult,
}

#[derive(Debug, Clone)]
struct DesktopLoginKeyPair {
    desktop_private_key_jwk: String,
    desktop_public_key_jwk: String,
    device_identity_private_key_jwk: String,
    device_identity_public_key_jwk: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DesktopLoginDecryptedPayload {
    user: DesktopLoginDecryptedUser,
    device: DesktopLoginDecryptedDevice,
    device_token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DesktopLoginDecryptedUser {
    id: String,
    email: String,
    role: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DesktopLoginDecryptedDevice {
    id: String,
    name: String,
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
    let key_pair = create_desktop_login_key_pair()?;
    let request = DesktopLoginStartRequest::new(
        whoami_device_name(),
        device_fingerprint,
        key_pair.desktop_public_key_jwk.clone(),
        key_pair.device_identity_public_key_jwk.clone(),
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
                device_identity_private_key: key_pair.device_identity_private_key_jwk,
                desktop_private_key_jwk: key_pair.desktop_private_key_jwk,
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
        .json::<ApiEnvelope<serde_json::Value>>()
        .await
        .map_err(|error| format!("远程登录轮询响应解析失败：{error}"))?;
    if envelope.code == 240404 {
        return Ok(RemoteLoginPollOutcome {
            binding: None,
            device_identity_private_key: String::new(),
        });
    }
    let data = envelope
        .into_success_data()
        .map_err(|error| error.message)?;
    let data: DesktopLoginPollResponse = serde_json::from_value(data)
        .map_err(|error| format!("远程登录轮询数据解析失败：{error}"))?;
    let secret = LOGIN_SESSIONS
        .lock()
        .map_err(|_| "远程登录会话锁定失败".to_string())?
        .remove(request_id)
        .ok_or_else(|| "远程登录会话已失效，请重新登录".to_string())?;
    let binding =
        decrypt_desktop_login_result(&secret.desktop_private_key_jwk, &data.encrypted_result.jwe)?;
    Ok(RemoteLoginPollOutcome {
        binding: Some(binding),
        device_identity_private_key: secret.device_identity_private_key,
    })
}

fn create_desktop_login_key_pair() -> Result<DesktopLoginKeyPair, String> {
    // 桌面登录使用临时 ECDH 密钥接收服务端加密结果；设备身份密钥后续用于设备侧签名。
    let desktop_private_key = Jwk::generate_ec_key(EcCurve::P256)
        .map_err(|error| format!("生成桌面登录密钥失败：{error}"))?;
    let desktop_public_key = desktop_private_key
        .to_public_key()
        .map_err(|error| format!("导出桌面登录公钥失败：{error}"))?;
    let identity_private_key = Jwk::generate_ec_key(EcCurve::P256)
        .map_err(|error| format!("生成设备身份密钥失败：{error}"))?;
    let identity_public_key = identity_private_key
        .to_public_key()
        .map_err(|error| format!("导出设备身份公钥失败：{error}"))?;

    Ok(DesktopLoginKeyPair {
        desktop_private_key_jwk: serde_json::to_string(&desktop_private_key)
            .map_err(|error| format!("序列化桌面登录私钥失败：{error}"))?,
        desktop_public_key_jwk: serde_json::to_string(&desktop_public_key)
            .map_err(|error| format!("序列化桌面登录公钥失败：{error}"))?,
        device_identity_private_key_jwk: serde_json::to_string(&identity_private_key)
            .map_err(|error| format!("序列化设备身份私钥失败：{error}"))?,
        device_identity_public_key_jwk: serde_json::to_string(&identity_public_key)
            .map_err(|error| format!("序列化设备身份公钥失败：{error}"))?,
    })
}

fn decrypt_desktop_login_result(
    desktop_private_key_jwk: &str,
    jwe_compact: &str,
) -> Result<DesktopLoginBindingResult, String> {
    let private_key = Jwk::from_bytes(desktop_private_key_jwk)
        .map_err(|error| format!("读取桌面登录私钥失败：{error}"))?;
    let decrypter = ECDH_ES
        .decrypter_from_jwk(&private_key)
        .map_err(|error| format!("创建桌面登录解密器失败：{error}"))?;
    let (plaintext, _header) = jwe::deserialize_compact(jwe_compact, &decrypter)
        .map_err(|error| format!("解密桌面登录结果失败：{error}"))?;
    let payload: DesktopLoginDecryptedPayload = serde_json::from_slice(&plaintext)
        .map_err(|error| format!("解析桌面登录结果失败：{error}"))?;

    Ok(DesktopLoginBindingResult {
        user_id: payload.user.id,
        user_email: payload.user.email,
        user_role: payload.user.role,
        device_id: payload.device.id,
        device_name: payload.device.name,
        device_token: payload.device_token,
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

    #[test]
    fn generates_json_jwk_keys_for_desktop_login_start() {
        let key_pair = create_desktop_login_key_pair().expect("生成登录密钥");

        let desktop_public: serde_json::Value =
            serde_json::from_str(&key_pair.desktop_public_key_jwk).expect("桌面公钥必须是 JSON");
        assert_eq!(desktop_public["kty"], "EC");
        assert_eq!(desktop_public["crv"], "P-256");
        assert!(desktop_public.get("d").is_none());

        let identity_public: serde_json::Value =
            serde_json::from_str(&key_pair.device_identity_public_key_jwk)
                .expect("设备身份公钥必须是 JSON");
        assert_eq!(identity_public["kty"], "EC");
        assert_eq!(identity_public["crv"], "P-256");
    }

    #[test]
    fn decrypts_server_desktop_login_jwe_result() {
        let private_key_jwk = r#"{"kty":"EC","x":"xDrbIoDV8NPZxzia11MjMaGvU7XxDXGOCpYJBez7AwA","y":"iyM7Sd3NPGAFxLQdUs7XU2aGAC5GeRwjVm4YOFMl_Dg","crv":"P-256","d":"u1b5FpoSfAo0BCA2htKeetKPf2pjRyM-fdkKE2PrfoQ"}"#;
        let jwe = "eyJhbGciOiJFQ0RILUVTIiwiZW5jIjoiQTI1NkdDTSIsImVwayI6eyJ4IjoiTnVzQUJzQWhtQzBRcjZia0FxVzZ0RWtzT2lWM1VWcVFiVVN0WjZRWEpKSSIsImNydiI6IlAtMjU2Iiwia3R5IjoiRUMiLCJ5IjoiTjVWdkpDTi1GbVVjWUJkeE9xVWFLOEc1a2VYQlA5b3FYQTVkNUVqXy1CZyJ9fQ..cbU1h7UkdG5rGQ01.G4gFuq6l4fzPD-BSFO1OBLplUQg5aIIzEtnxZvYdH1ggEa3EdC85rs57k2dLuxNMIy57qPQX8D9uQPRPRhTfabbStiUc6_8h6HwiCZ_dg8O-UsX00Th1cyt_Ye2gjtfzDCMLNUZJXoUypWqJWnuRXFtq_-tngwu-uKZoigQtMf8Fd-yufVUKgwE.mGBZBgpOFUhKwxZnnPlWuw";

        let binding = decrypt_desktop_login_result(private_key_jwk, jwe).expect("解密绑定结果");

        assert_eq!(binding.user_id, "usr_1");
        assert_eq!(binding.device_id, "dev_1");
        assert_eq!(binding.device_token, "dvt_test");
    }
}
