# Local Remote Settings And Browser Login Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a local settings panel for remote access configuration and browser-based account/device binding.

**Architecture:** Remote settings are stored in the host app config, while `device_token` and identity private key stay behind the credential-store boundary from the Local RemoteAgent foundation plan. The frontend renders a third settings panel and calls Tauri commands returning the existing `ApiResponse { code, message, data }` shape. Browser login opens the system browser, polls desktop-login completion, then persists non-sensitive config plus sensitive credentials separately.

**Tech Stack:** Rust, Tauri commands, `niuma_core::remote`, `reqwest`, TypeScript, existing `src/settingsView.ts`, existing `src/i18n.ts`, existing command response wrapper.

---

## Prerequisites

Implement this plan after `docs/superpowers/plans/2026-06-28-local-remote-agent-foundation-plan.md` has landed.

Required types from that milestone:

- `niuma_core::remote::config::RemoteConfig`
- `niuma_core::remote::credentials::{RemoteCredentialPayload, RemoteCredentialStore, RestrictedFileCredentialStore}`
- `niuma_core::remote::login_flow::{ApiEnvelope, DesktopLoginBindingResult, DesktopLoginStartRequest, DesktopLoginStartResponse}`
- `src-tauri/src/remote/login_flow.rs`

## Scope Check

This plan covers:

- Local remote settings persistence.
- Tauri commands for reading/saving remote settings.
- Tauri commands for starting browser login, polling login completion, and clearing local binding.
- Settings page panel and click/change handlers.
- i18n strings for all supported languages.
- Focused TypeScript and Rust tests.

This plan does not cover:

- `/ws/device` connection loop.
- Token revoked handling from WebSocket close codes.
- WebRTC signaling, relay, or E2EE RPC.
- External web console UI.
- Native Keychain/Credential Manager/Secret Service storage.

## API Contract Notes

Tauri commands must return `ApiResponse<serde_json::Value>`:

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

Business failures use non-zero `code` with `HTTP` not involved because these are Tauri IPC calls. Frontend must inspect `code` and throw `message` exactly like existing commands.

## File Structure

Create:

- `crates/niuma-core/src/remote/settings.rs` - serializable settings payload and store helpers.
- `src-tauri/src/remote/commands.rs` - remote settings and browser-login Tauri command helpers.
- `tests/remoteSettingsView.test.ts` - render coverage for the settings panel.

Modify:

- `crates/niuma-core/src/remote/mod.rs` - export `settings`.
- `crates/niuma-core/src/store/config_files.rs` - persist remote settings in the app config file.
- `crates/niuma-core/src/store.rs` - expose `remote_config` and `save_remote_config`.
- `crates/niuma-core/src/store/tests.rs` - persistence coverage.
- `src-tauri/src/remote/mod.rs` - export `commands`.
- `src-tauri/src/main.rs` - register Tauri commands.
- `src-tauri/src/commands.rs` - delegate remote commands or re-export command functions.
- `src/api.ts` - frontend API types and functions.
- `src/settingsView.ts` - remote settings panel render function and shell navigation.
- `src/main.ts` - load remote settings, wire save/login/poll/logout handlers.
- `src/i18n.ts` - add all remote settings translations for six languages.
- `src/styles.css` - remote settings panel layout.
- `tests/settingsViewRender.test.ts` - shell navigation coverage.
- `package.json` - add `test:remote-settings-view` and include it in `npm test`.

## Task 1: Core Remote Settings Persistence

**Files:**
- Create: `crates/niuma-core/src/remote/settings.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Modify: `crates/niuma-core/src/store/config_files.rs`
- Modify: `crates/niuma-core/src/store.rs`
- Test: `crates/niuma-core/src/store/tests.rs`

- [ ] **Step 1: Write failing store test**

Add this test to `crates/niuma-core/src/store/tests.rs`:

```rust
#[test]
fn remote_config_defaults_and_persists_without_device_token() {
    let root = test_data_dir("json_remote_config");
    let path = root.join("niuma.sqlite");
    let store = NiumaStore::new(path.clone());

    let default_config = store.remote_config().unwrap();
    assert_eq!(default_config.server_url, "https://remote.niuma.example");
    assert!(default_config.remote_access_enabled);
    assert!(default_config.remote_control_enabled);
    assert!(default_config.device.is_none());

    let mut saved = default_config;
    saved.server_url = "https://self-hosted.example.com".to_string();
    saved.remote_control_enabled = false;
    saved.user = Some(crate::remote::config::RemoteUserSummary {
        id: "user_1".to_string(),
        email: "user@example.com".to_string(),
        role: "owner".to_string(),
    });
    saved.device = Some(crate::remote::config::RemoteDeviceSummary {
        id: "dev_1".to_string(),
        name: "NiuMa MacBook".to_string(),
    });

    store.save_remote_config(&saved).unwrap();
    let reloaded = NiumaStore::new(path).remote_config().unwrap();

    assert_eq!(reloaded.server_url, "https://self-hosted.example.com");
    assert!(!reloaded.remote_control_enabled);
    assert_eq!(reloaded.user.unwrap().email, "user@example.com");
    assert_eq!(reloaded.device.unwrap().id, "dev_1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote_config_defaults_and_persists_without_device_token
```

Expected: FAIL because `remote_config` and `save_remote_config` do not exist.

- [ ] **Step 3: Add remote settings model**

Create `crates/niuma-core/src/remote/settings.rs`:

```rust
use crate::remote::config::RemoteConfig;

pub const DEFAULT_REMOTE_SERVER_URL: &str = "https://remote.niuma.example";

pub fn default_remote_config() -> RemoteConfig {
    RemoteConfig::default_for_server(DEFAULT_REMOTE_SERVER_URL)
}

pub fn normalize_server_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err("远程服务地址不能为空".to_string());
    }
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("远程服务地址必须以 http:// 或 https:// 开头".to_string());
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_server_url() {
        assert_eq!(
            normalize_server_url(" https://remote.example.com/ ").unwrap(),
            "https://remote.example.com"
        );
    }

    #[test]
    fn rejects_missing_scheme() {
        assert_eq!(
            normalize_server_url("remote.example.com").unwrap_err(),
            "远程服务地址必须以 http:// 或 https:// 开头"
        );
    }
}
```

Update `crates/niuma-core/src/remote/mod.rs`:

```rust
pub mod agent_state;
pub mod config;
pub mod credentials;
pub mod device_identity;
pub mod login_flow;
pub mod settings;
```

- [ ] **Step 4: Persist remote config in app config file**

Update `crates/niuma-core/src/store/config_files.rs` so `AppConfigFile` includes remote config:

```rust
use crate::remote::config::RemoteConfig;
use crate::remote::settings::default_remote_config;
```

Add the field:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AppConfigFile {
    #[serde(default = "default_language_preference")]
    language_preference: String,
    #[serde(default = "default_remote_config")]
    remote_config: RemoteConfig,
}
```

Update `Default for AppConfigFile`:

```rust
impl Default for AppConfigFile {
    fn default() -> Self {
        Self {
            language_preference: default_language_preference(),
            remote_config: default_remote_config(),
        }
    }
}
```

Add methods inside `impl ConfigFiles`:

```rust
pub(super) fn remote_config(&self) -> Result<RemoteConfig, String> {
    Ok(self.read_app_config()?.remote_config)
}

pub(super) fn save_remote_config(&self, config: &RemoteConfig) -> Result<(), String> {
    let mut app_config = self.read_app_config()?;
    app_config.remote_config = config.clone();
    self.write_app_config(&app_config)
}
```

- [ ] **Step 5: Expose store methods**

Update `crates/niuma-core/src/store.rs`:

```rust
pub fn remote_config(&self) -> Result<crate::remote::config::RemoteConfig, String> {
    self.config_files().remote_config()
}

pub fn save_remote_config(&self, config: &crate::remote::config::RemoteConfig) -> Result<(), String> {
    self.config_files().save_remote_config(config)
}
```

- [ ] **Step 6: Run core tests**

Run:

```bash
cargo test -p niuma-core remote::settings remote_config_defaults_and_persists_without_device_token
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/niuma-core/src/remote/settings.rs crates/niuma-core/src/remote/mod.rs crates/niuma-core/src/store/config_files.rs crates/niuma-core/src/store.rs crates/niuma-core/src/store/tests.rs
git commit -m "feat: 新增远程设置持久化" -m "修改内容：新增远程设置模型、默认服务地址、服务地址规范化和 app config 持久化读写。" -m "修改原因：本机设置页需要保存远程访问开关、服务地址和绑定设备摘要，但不能保存 device token。"
```

## Task 2: Tauri Remote Settings And Login Commands

**Files:**
- Create: `src-tauri/src/remote/commands.rs`
- Modify: `src-tauri/src/remote/mod.rs`
- Modify: `src-tauri/src/remote/login_flow.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/remote/commands.rs`

- [ ] **Step 1: Write failing command helper tests**

Create `src-tauri/src/remote/commands.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::config::{RemoteConfig, RemoteDeviceSummary, RemoteUserSummary};

    #[test]
    fn remote_settings_payload_does_not_include_device_token() {
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.user = Some(RemoteUserSummary {
            id: "user_1".to_string(),
            email: "user@example.com".to_string(),
            role: "owner".to_string(),
        });
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "NiuMa MacBook".to_string(),
        });

        let payload = remote_settings_payload(config, true);
        assert_eq!(payload["server_url"], "https://remote.example.com");
        assert_eq!(payload["bound"], true);
        assert_eq!(payload["has_credential"], true);
        assert!(payload.get("device_token").is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::commands
```

Expected: FAIL because `remote_settings_payload` does not exist.

- [ ] **Step 3: Implement command helpers**

Replace `src-tauri/src/remote/commands.rs` with:

```rust
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::remote::config::{RemoteConfig, RemoteDeviceSummary, RemoteUserSummary};
use niuma_core::remote::credentials::{RemoteCredentialPayload, RemoteCredentialStore, RestrictedFileCredentialStore};
use niuma_core::remote::login_flow::DesktopLoginBindingResult;
use niuma_core::remote::settings::normalize_server_url;
use niuma_core::store::NiumaStore;
use serde_json::json;
use std::path::PathBuf;

pub fn remote_settings_payload(config: RemoteConfig, has_credential: bool) -> serde_json::Value {
    json!({
        "server_url": config.server_url,
        "remote_access_enabled": config.remote_access_enabled,
        "remote_control_enabled": config.remote_control_enabled,
        "user": config.user,
        "device": config.device,
        "bound": config.device.is_some() && has_credential,
        "has_credential": has_credential,
        "last_connected_at": config.last_connected_at
    })
}

pub fn save_remote_settings_to_store(
    store: &NiumaStore,
    server_url: String,
    remote_access_enabled: bool,
    remote_control_enabled: bool,
) -> ApiResponse<serde_json::Value> {
    let server_url = match normalize_server_url(&server_url) {
        Ok(value) => value,
        Err(error) => return ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    };
    let mut config = match store.remote_config() {
        Ok(config) => config,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    if config.server_url != server_url {
        config.user = None;
        config.device = None;
        config.last_connected_at = None;
    }
    config.server_url = server_url;
    config.remote_access_enabled = remote_access_enabled;
    config.remote_control_enabled = remote_control_enabled;
    match store.save_remote_config(&config) {
        Ok(()) => ApiResponse::ok(json!({
            "saved": true,
            "settings": remote_settings_payload(config, false)
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

pub fn apply_remote_binding_result(
    store: &NiumaStore,
    credential_store: &dyn RemoteCredentialStore,
    server_url: &str,
    device_identity_private_key: String,
    result: DesktopLoginBindingResult,
) -> ApiResponse<serde_json::Value> {
    let credential = RemoteCredentialPayload {
        device_token: result.device_token,
        device_identity_private_key,
    };
    if let Err(error) = credential_store.save(server_url, &credential) {
        return ApiResponse::fail(ApiErrorCode::System, error.to_string());
    }
    let mut config = match store.remote_config() {
        Ok(config) => config,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    config.server_url = server_url.to_string();
    config.user = Some(RemoteUserSummary {
        id: result.user_id,
        email: result.user_email,
        role: result.user_role,
    });
    config.device = Some(RemoteDeviceSummary {
        id: result.device_id,
        name: result.device_name,
    });
    match store.save_remote_config(&config) {
        Ok(()) => ApiResponse::ok(json!({
            "completed": true,
            "settings": remote_settings_payload(config, true)
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

pub fn credential_store_for_data_dir(data_dir: PathBuf) -> RestrictedFileCredentialStore {
    RestrictedFileCredentialStore::new(data_dir.join("remote-credentials"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::config::{RemoteDeviceSummary, RemoteUserSummary};

    #[test]
    fn remote_settings_payload_does_not_include_device_token() {
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.user = Some(RemoteUserSummary {
            id: "user_1".to_string(),
            email: "user@example.com".to_string(),
            role: "owner".to_string(),
        });
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "NiuMa MacBook".to_string(),
        });

        let payload = remote_settings_payload(config, true);
        assert_eq!(payload["server_url"], "https://remote.example.com");
        assert_eq!(payload["bound"], true);
        assert_eq!(payload["has_credential"], true);
        assert!(payload.get("device_token").is_none());
    }
}
```

`device_identity_private_key` is generated locally by the desktop login session and is passed separately into `apply_remote_binding_result`. The remote server returns `device_token` and public account/device summaries only.

- [ ] **Step 4: Add Tauri command wrappers**

Update `src-tauri/src/commands.rs`:

```rust
#[tauri::command]
pub(crate) fn get_remote_settings(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    match runtime_state.store.remote_config() {
        Ok(config) => ApiResponse::ok(serde_json::json!({
            "settings": crate::remote::commands::remote_settings_payload(config, false)
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) fn save_remote_settings(
    runtime_state: tauri::State<'_, AppRuntimeState>,
    server_url: String,
    remote_access_enabled: bool,
    remote_control_enabled: bool,
) -> ApiResponse<serde_json::Value> {
    crate::remote::commands::save_remote_settings_to_store(
        &runtime_state.store,
        server_url,
        remote_access_enabled,
        remote_control_enabled,
    )
}

#[tauri::command]
pub(crate) fn clear_remote_binding(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    let mut config = match runtime_state.store.remote_config() {
        Ok(config) => config,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    config.user = None;
    config.device = None;
    config.last_connected_at = None;
    match runtime_state.store.save_remote_config(&config) {
        Ok(()) => ApiResponse::ok(serde_json::json!({
            "cleared": true,
            "settings": crate::remote::commands::remote_settings_payload(config, false)
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}
```

Add `mod remote;` in `src-tauri/src/main.rs` if the foundation task has not already added it.

Register commands in `src-tauri/src/main.rs`:

```rust
commands::get_remote_settings,
commands::save_remote_settings,
commands::clear_remote_binding,
```

Update `src-tauri/src/remote/mod.rs`:

```rust
pub mod agent;
pub mod commands;
pub mod device_socket;
pub mod login_flow;
```

- [ ] **Step 5: Add browser login session helpers**

Update `src-tauri/Cargo.toml` dependencies:

```toml
once_cell = "1"
open = "5"
```

Update `src-tauri/src/remote/login_flow.rs`:

```rust
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

pub async fn start_remote_login_session(config: &RemoteConfig) -> Result<RemoteLoginStarted, String> {
    let install_id = DeviceInstallId::generate();
    let device_fingerprint = derive_device_fingerprint(&config.server_url, &install_id);
    let device_identity_private_key = format!("local-dev-identity-{}", Uuid::new_v4());
    let device_identity_public_key = format!("public-{}", device_identity_private_key);
    let desktop_public_key = format!("desktop-login-{}", Uuid::new_v4());
    let request = DesktopLoginStartRequest::new(
        whoami_device_name(),
        device_fingerprint,
        desktop_public_key,
        device_identity_public_key,
    );
    let endpoint = format!("{}/api/v1/desktop-login/start", config.server_url.trim_end_matches('/'));
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
    let endpoint = format!("{}/api/v1/desktop-login/poll", server_url.trim_end_matches('/'));
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
```

This step uses a locally generated opaque identity secret string so settings/login work can persist credentials through the same boundary as the E2EE milestone. The E2EE RPC milestone owns the cryptographic key format; this settings milestone owns the storage and UI flow.

- [ ] **Step 6: Add browser login command contract**

Add command signatures to `src-tauri/src/commands.rs`. These commands delegate HTTP details to `src-tauri/src/remote/login_flow.rs`; that module owns request construction, browser opening, polling, and the in-memory login session private key.

```rust
#[tauri::command]
pub(crate) async fn start_remote_login(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    let config = match runtime_state.store.remote_config() {
        Ok(config) => config,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    if !config.remote_access_enabled {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, "远程访问未启用");
    }

    match crate::remote::login_flow::start_remote_login_session(&config).await {
        Ok(started) => ApiResponse::ok(serde_json::json!({
            "started": true,
            "server_url": config.server_url,
            "request_id": started.request_id,
            "poll_token": started.poll_token,
            "login_url": started.login_url,
            "expires_in": started.expires_in
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

#[tauri::command]
pub(crate) async fn poll_remote_login(
    runtime_state: tauri::State<'_, AppRuntimeState>,
    request_id: String,
    poll_token: String,
) -> ApiResponse<serde_json::Value> {
    if request_id.trim().is_empty() || poll_token.trim().is_empty() {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, "登录轮询参数不能为空");
    }
    let config = match runtime_state.store.remote_config() {
        Ok(config) => config,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };

    let poll_result = match crate::remote::login_flow::poll_remote_login_session(
        &config.server_url,
        &request_id,
        &poll_token,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };

    if let Some(binding) = poll_result.binding {
        let credential_store = crate::remote::commands::credential_store_for_data_dir(
            NiumaStore::default_path()
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(std::env::temp_dir),
        );
        return crate::remote::commands::apply_remote_binding_result(
            &runtime_state.store,
            &credential_store,
            &config.server_url,
            poll_result.device_identity_private_key,
            binding,
        );
    }

    ApiResponse::ok(serde_json::json!({
        "completed": false,
        "settings": crate::remote::commands::remote_settings_payload(config, false)
    }))
}
```

The command behavior is:

```text
start_remote_login:
- POST {server_url}/api/v1/desktop-login/start
- request body uses DesktopLoginStartRequest
- parse ApiEnvelope<DesktopLoginStartResponse>
- open response.login_url in system browser
- return request_id, poll_token, login_url, expires_in

poll_remote_login:
- POST {server_url}/api/v1/desktop-login/poll
- request body contains request_id and poll_token
- pending response returns completed=false
- completed response returns completed=true and persists config plus credential
```

- [ ] **Step 7: Run Tauri command tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::commands
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/remote/commands.rs src-tauri/src/remote/mod.rs src-tauri/src/remote/login_flow.rs src-tauri/src/commands.rs src-tauri/src/main.rs
git commit -m "feat: 新增远程设置 Tauri 命令" -m "修改内容：新增远程设置读取、保存、清除绑定和浏览器登录命令契约。" -m "修改原因：设置页需要通过本机命令配置远程服务并启动账号设备绑定流程。"
```

## Task 3: Frontend API And Settings Panel Rendering

**Files:**
- Modify: `src/api.ts`
- Modify: `src/settingsView.ts`
- Create: `tests/remoteSettingsView.test.ts`
- Modify: `tests/settingsViewRender.test.ts`

- [ ] **Step 1: Write failing render tests**

Create `tests/remoteSettingsView.test.ts`:

```ts
import { renderRemoteSettingsPanel, renderSettingsShell } from '../src/settingsView'

const shell = renderSettingsShell({ language: 'zh-CN', activePanel: 'remote-access' })

if (!shell.includes('data-settings-panel="remote-access" aria-current="page"')) {
  throw new Error('远程访问面板选中时应标记当前导航项')
}

if (!shell.includes('id="settings-panel-remote-access" class="settings-panel remote-settings-panel"')) {
  throw new Error('设置页应渲染远程访问面板容器')
}

const html = renderRemoteSettingsPanel({
  language: 'zh-CN',
  settings: {
    server_url: 'https://remote.example.com',
    remote_access_enabled: true,
    remote_control_enabled: true,
    user: { id: 'user_1', email: 'user@example.com', role: 'owner' },
    device: { id: 'dev_1', name: 'NiuMa MacBook' },
    bound: true,
    has_credential: true,
    last_connected_at: null
  },
  busyAction: null,
  resultText: ''
})

if (!html.includes('value="https://remote.example.com"')) {
  throw new Error('远程设置应渲染服务端地址')
}

if (!html.includes('user@example.com') || !html.includes('NiuMa MacBook')) {
  throw new Error('已绑定状态应显示账号和设备摘要')
}

if (html.includes('device_token')) {
  throw new Error('远程设置页面不能渲染 device_token')
}
```

Update `tests/settingsViewRender.test.ts`:

```ts
if (!shell.includes('data-settings-panel="remote-access"')) {
  throw new Error('设置页左侧应包含远程访问入口')
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
tsc --target ES2022 --module commonjs --moduleResolution node --lib ES2022,DOM --skipLibCheck --strict --esModuleInterop --outDir /tmp/niuma-remote-settings-view-test tests/remoteSettingsView.test.ts && node /tmp/niuma-remote-settings-view-test/tests/remoteSettingsView.test.js
```

Expected: FAIL because remote settings render functions and panel type do not exist.

- [ ] **Step 3: Add frontend API types**

Update `src/api.ts`:

```ts
export type RemoteSettingsPayload = {
  server_url: string
  remote_access_enabled: boolean
  remote_control_enabled: boolean
  user: RemoteUserSummary | null
  device: RemoteDeviceSummary | null
  bound: boolean
  has_credential: boolean
  last_connected_at: string | null
}

export type RemoteUserSummary = {
  id: string
  email: string
  role: string
}

export type RemoteDeviceSummary = {
  id: string
  name: string
}

export type RemoteLoginStartResult = {
  started: boolean
  server_url: string
  request_id: string
  poll_token: string
  login_url: string
  expires_in: number
}

export type RemoteLoginPollResult = {
  completed: boolean
  settings: RemoteSettingsPayload
}

export async function getRemoteSettings() {
  const response = await invoke<ApiResponse<{ settings: RemoteSettingsPayload }>>('get_remote_settings')
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data.settings
}

export async function saveRemoteSettings(settings: Pick<RemoteSettingsPayload, 'server_url' | 'remote_access_enabled' | 'remote_control_enabled'>) {
  const response = await invoke<ApiResponse<{ saved: boolean; settings: RemoteSettingsPayload }>>(
    'save_remote_settings',
    {
      serverUrl: settings.server_url,
      remoteAccessEnabled: settings.remote_access_enabled,
      remoteControlEnabled: settings.remote_control_enabled
    }
  )
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data.settings
}

export async function startRemoteLogin() {
  const response = await invoke<ApiResponse<RemoteLoginStartResult>>('start_remote_login')
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data
}

export async function pollRemoteLogin(requestId: string, pollToken: string) {
  const response = await invoke<ApiResponse<RemoteLoginPollResult>>('poll_remote_login', {
    requestId,
    pollToken
  })
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data
}

export async function clearRemoteBinding() {
  const response = await invoke<ApiResponse<{ cleared: boolean; settings: RemoteSettingsPayload }>>(
    'clear_remote_binding'
  )
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data.settings
}
```

- [ ] **Step 4: Render remote settings panel**

Update `src/settingsView.ts` imports:

```ts
import type { PluginConfigField, PluginManagementItem, RemoteSettingsPayload } from './api'
```

Update panel type:

```ts
export type SettingsPanel = 'plugins' | 'notification-history' | 'remote-access'
```

Add render options:

```ts
export type RemoteSettingsRenderOptions = {
  language: LanguageCode
  settings: RemoteSettingsPayload | null
  busyAction: 'save' | 'login' | 'logout' | null
  resultText: string
}
```

Update `renderSettingsShell` to add the third nav item and panel container:

```ts
const remoteAccessActive = activePanel === 'remote-access'
```

Add nav button after plugin management:

```ts
<button class="settings-nav-item ${remoteAccessActive ? 'active' : ''}" type="button" data-settings-panel="remote-access" ${
  remoteAccessActive ? 'aria-current="page"' : ''
}>${escapeHtml(t.remoteAccess)}</button>
```

Add panel container before notification history:

```ts
<div id="settings-panel-remote-access" class="settings-panel remote-settings-panel" ${
  remoteAccessActive ? '' : 'hidden'
}>
  <div id="remote-settings-panel"></div>
</div>
```

Add function:

```ts
export function renderRemoteSettingsPanel(options: RemoteSettingsRenderOptions) {
  const t = translations[options.language]
  const settings = options.settings
  const saveBusy = options.busyAction === 'save'
  const loginBusy = options.busyAction === 'login'
  const logoutBusy = options.busyAction === 'logout'
  const serverUrl = settings?.server_url ?? ''
  const bound = settings?.bound === true
  const account = settings?.user?.email ?? t.remoteNotLoggedIn
  const device = settings?.device?.name ?? t.remoteNoBoundDevice
  return `
    <div class="settings-heading">
      <div>
        <h2>${escapeHtml(t.remoteAccess)}</h2>
        <p>${escapeHtml(t.remoteAccessDescription)}</p>
      </div>
      <button id="remote-login" type="button" ${loginBusy ? 'disabled' : ''}>${escapeHtml(
        loginBusy ? t.remoteLoginOpening : t.remoteLogin
      )}</button>
    </div>
    <form id="remote-settings-form" class="remote-settings-form">
      <label class="plugin-config-field" for="remote-server-url">
        <span>${escapeHtml(t.remoteServerUrl)}</span>
        <input id="remote-server-url" name="server_url" type="url" value="${escapeHtml(serverUrl)}">
      </label>
      <label class="plugin-enable-toggle remote-toggle">
        <span>${escapeHtml(t.remoteAccessEnabled)}</span>
        <input id="remote-access-enabled" name="remote_access_enabled" type="checkbox" ${
          settings?.remote_access_enabled !== false ? 'checked' : ''
        }>
      </label>
      <label class="plugin-enable-toggle remote-toggle">
        <span>${escapeHtml(t.remoteControlEnabled)}</span>
        <input id="remote-control-enabled" name="remote_control_enabled" type="checkbox" ${
          settings?.remote_control_enabled !== false ? 'checked' : ''
        }>
      </label>
      <button id="remote-settings-save" type="submit" ${saveBusy ? 'disabled' : ''}>${escapeHtml(
        saveBusy ? t.saving : t.save
      )}</button>
    </form>
    <dl class="remote-binding-summary">
      <dt>${escapeHtml(t.remoteBindingStatus)}</dt>
      <dd>${escapeHtml(bound ? t.remoteBound : t.remoteUnbound)}</dd>
      <dt>${escapeHtml(t.remoteAccount)}</dt>
      <dd>${escapeHtml(account)}</dd>
      <dt>${escapeHtml(t.remoteDevice)}</dt>
      <dd>${escapeHtml(device)}</dd>
    </dl>
    <div class="remote-actions">
      <button id="remote-logout" type="button" ${!bound || logoutBusy ? 'disabled' : ''}>${escapeHtml(
        logoutBusy ? t.remoteLoggingOut : t.remoteLogout
      )}</button>
    </div>
    <p id="remote-settings-result" class="settings-result">${escapeHtml(options.resultText)}</p>
  `
}
```

- [ ] **Step 5: Run render tests**

Run:

```bash
tsc --target ES2022 --module commonjs --moduleResolution node --lib ES2022,DOM --skipLibCheck --strict --esModuleInterop --outDir /tmp/niuma-remote-settings-view-test tests/remoteSettingsView.test.ts && node /tmp/niuma-remote-settings-view-test/tests/remoteSettingsView.test.js
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/api.ts src/settingsView.ts tests/remoteSettingsView.test.ts tests/settingsViewRender.test.ts
git commit -m "feat: 新增远程访问设置面板渲染" -m "修改内容：新增远程设置前端 API 类型、设置页远程访问面板和渲染测试。" -m "修改原因：用户需要在本机设置页配置远程服务并查看账号设备绑定状态。"
```

## Task 4: Frontend State, Events, And Login Polling

**Files:**
- Modify: `src/main.ts`
- Test: `tests/remoteSettingsView.test.ts`

- [ ] **Step 1: Add remote settings state**

Update `src/main.ts` imports:

```ts
import {
  clearRemoteBinding,
  getRemoteSettings,
  pollRemoteLogin,
  saveRemoteSettings,
  startRemoteLogin,
  type RemoteSettingsPayload
} from './api'
import { renderRemoteSettingsPanel } from './settingsView'
```

Add state near existing settings state:

```ts
let remoteSettings: RemoteSettingsPayload | null = null
let remoteSettingsBusyAction: 'save' | 'login' | 'logout' | null = null
let remoteSettingsResultText = ''
let remoteLoginPollTimer: number | null = null
```

- [ ] **Step 2: Render and load remote settings**

Add functions:

```ts
function renderRemoteSettings() {
  const element = document.querySelector<HTMLElement>('#remote-settings-panel')
  if (!element) {
    return
  }
  element.innerHTML = renderRemoteSettingsPanel({
    language: currentLanguage,
    settings: remoteSettings,
    busyAction: remoteSettingsBusyAction,
    resultText: remoteSettingsResultText
  })
}

async function loadRemoteSettings() {
  try {
    remoteSettings = await getRemoteSettings()
    remoteSettingsResultText = ''
  } catch (error) {
    remoteSettingsResultText = error instanceof Error ? error.message : translations[currentLanguage].error
  }
  renderRemoteSettings()
}
```

Call `renderRemoteSettings()` from `renderSettings()` after `renderSettingsNotificationHistory()`.

Call `loadRemoteSettings()` when opening settings or switching to the remote panel:

```ts
if (activeSettingsPanel === 'remote-access' && !remoteSettings) {
  void loadRemoteSettings()
}
```

- [ ] **Step 3: Add save/login/logout handlers**

Add to the existing `settingsViewEl?.addEventListener('submit', ...)` handler:

```ts
if ((event.target as HTMLElement | null)?.id === 'remote-settings-form') {
  event.preventDefault()
  const form = event.target as HTMLFormElement
  const formData = new FormData(form)
  remoteSettingsBusyAction = 'save'
  renderRemoteSettings()
  saveRemoteSettings({
    server_url: String(formData.get('server_url') ?? ''),
    remote_access_enabled: formData.get('remote_access_enabled') === 'on',
    remote_control_enabled: formData.get('remote_control_enabled') === 'on'
  })
    .then((settings) => {
      remoteSettings = settings
      remoteSettingsResultText = translations[currentLanguage].saved
    })
    .catch((error) => {
      remoteSettingsResultText = error instanceof Error ? error.message : translations[currentLanguage].error
    })
    .finally(() => {
      remoteSettingsBusyAction = null
      renderRemoteSettings()
    })
}
```

Add to the existing settings click handler:

```ts
if (target?.id === 'remote-login') {
  remoteSettingsBusyAction = 'login'
  remoteSettingsResultText = translations[currentLanguage].remoteLoginOpening
  renderRemoteSettings()
  startRemoteLogin()
    .then((result) => {
      remoteSettingsResultText = translations[currentLanguage].remoteLoginWaiting
      startRemoteLoginPolling(result.request_id, result.poll_token)
    })
    .catch((error) => {
      remoteSettingsResultText = error instanceof Error ? error.message : translations[currentLanguage].error
    })
    .finally(() => {
      remoteSettingsBusyAction = null
      renderRemoteSettings()
    })
}

if (target?.id === 'remote-logout') {
  remoteSettingsBusyAction = 'logout'
  renderRemoteSettings()
  clearRemoteBinding()
    .then((settings) => {
      remoteSettings = settings
      remoteSettingsResultText = translations[currentLanguage].remoteLogoutSuccess
    })
    .catch((error) => {
      remoteSettingsResultText = error instanceof Error ? error.message : translations[currentLanguage].error
    })
    .finally(() => {
      remoteSettingsBusyAction = null
      renderRemoteSettings()
    })
}
```

Add polling helper:

```ts
function startRemoteLoginPolling(requestId: string, pollToken: string) {
  if (remoteLoginPollTimer !== null) {
    window.clearInterval(remoteLoginPollTimer)
  }
  remoteLoginPollTimer = window.setInterval(() => {
    pollRemoteLogin(requestId, pollToken)
      .then((result) => {
        if (!result.completed) {
          return
        }
        if (remoteLoginPollTimer !== null) {
          window.clearInterval(remoteLoginPollTimer)
          remoteLoginPollTimer = null
        }
        remoteSettings = result.settings
        remoteSettingsResultText = translations[currentLanguage].remoteLoginSuccess
        renderRemoteSettings()
      })
      .catch((error) => {
        if (remoteLoginPollTimer !== null) {
          window.clearInterval(remoteLoginPollTimer)
          remoteLoginPollTimer = null
        }
        remoteSettingsResultText = error instanceof Error ? error.message : translations[currentLanguage].error
        renderRemoteSettings()
      })
  }, 1500)
}
```

- [ ] **Step 4: Update settings panel switch validation**

Update settings panel click logic:

```ts
if (
  settingsPanel === 'plugins' ||
  settingsPanel === 'notification-history' ||
  settingsPanel === 'remote-access'
) {
  if (settingsPanel === activeSettingsPanel) {
    return
  }
  activeSettingsPanel = settingsPanel
  renderSettings()
  if (activeSettingsPanel === 'notification-history' && !notificationRecordsLoaded) {
    void loadNotificationRecords()
  }
  if (activeSettingsPanel === 'remote-access' && !remoteSettings) {
    void loadRemoteSettings()
  }
}
```

- [ ] **Step 5: Run TypeScript build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/main.ts
git commit -m "feat: 接入远程设置页交互" -m "修改内容：新增远程设置加载、保存、浏览器登录轮询和清除绑定交互。" -m "修改原因：用户需要从本机设置页直接完成远程访问配置和账号设备绑定。"
```

## Task 5: I18n And Styling

**Files:**
- Modify: `src/i18n.ts`
- Modify: `src/styles.css`
- Modify: `package.json`

- [ ] **Step 1: Add translation fields**

Update `Translation` in `src/i18n.ts`:

```ts
remoteAccess: string
remoteAccessDescription: string
remoteServerUrl: string
remoteAccessEnabled: string
remoteControlEnabled: string
remoteLogin: string
remoteLoginOpening: string
remoteLoginWaiting: string
remoteLoginSuccess: string
remoteLogout: string
remoteLoggingOut: string
remoteLogoutSuccess: string
remoteBindingStatus: string
remoteBound: string
remoteUnbound: string
remoteAccount: string
remoteDevice: string
remoteNotLoggedIn: string
remoteNoBoundDevice: string
```

Add values for every language:

```ts
// zh-CN
remoteAccess: '远程访问',
remoteAccessDescription: '配置外网服务端、账号绑定和本机远程控制开关。',
remoteServerUrl: '远程服务地址',
remoteAccessEnabled: '启用远程访问',
remoteControlEnabled: '允许远程控制',
remoteLogin: '登录并绑定',
remoteLoginOpening: '正在打开浏览器...',
remoteLoginWaiting: '浏览器登录完成后会自动绑定本机设备',
remoteLoginSuccess: '已完成登录绑定',
remoteLogout: '解除绑定',
remoteLoggingOut: '正在解除绑定...',
remoteLogoutSuccess: '已解除本机绑定',
remoteBindingStatus: '绑定状态',
remoteBound: '已绑定',
remoteUnbound: '未绑定',
remoteAccount: '账号',
remoteDevice: '设备',
remoteNotLoggedIn: '未登录',
remoteNoBoundDevice: '未绑定设备',

// zh-TW
remoteAccess: '遠端存取',
remoteAccessDescription: '設定外網服務端、帳號綁定和本機遠端控制開關。',
remoteServerUrl: '遠端服務位址',
remoteAccessEnabled: '啟用遠端存取',
remoteControlEnabled: '允許遠端控制',
remoteLogin: '登入並綁定',
remoteLoginOpening: '正在開啟瀏覽器...',
remoteLoginWaiting: '瀏覽器登入完成後會自動綁定本機裝置',
remoteLoginSuccess: '已完成登入綁定',
remoteLogout: '解除綁定',
remoteLoggingOut: '正在解除綁定...',
remoteLogoutSuccess: '已解除本機綁定',
remoteBindingStatus: '綁定狀態',
remoteBound: '已綁定',
remoteUnbound: '未綁定',
remoteAccount: '帳號',
remoteDevice: '裝置',
remoteNotLoggedIn: '未登入',
remoteNoBoundDevice: '未綁定裝置',

// en
remoteAccess: 'Remote access',
remoteAccessDescription: 'Configure the public server, account binding, and local remote-control switches.',
remoteServerUrl: 'Remote server URL',
remoteAccessEnabled: 'Enable remote access',
remoteControlEnabled: 'Allow remote control',
remoteLogin: 'Sign in and bind',
remoteLoginOpening: 'Opening browser...',
remoteLoginWaiting: 'This device will bind automatically after browser sign-in finishes.',
remoteLoginSuccess: 'Sign-in binding completed',
remoteLogout: 'Unbind',
remoteLoggingOut: 'Unbinding...',
remoteLogoutSuccess: 'Local binding removed',
remoteBindingStatus: 'Binding status',
remoteBound: 'Bound',
remoteUnbound: 'Not bound',
remoteAccount: 'Account',
remoteDevice: 'Device',
remoteNotLoggedIn: 'Not signed in',
remoteNoBoundDevice: 'No bound device',

// ja
remoteAccess: 'リモートアクセス',
remoteAccessDescription: '公開サーバー、アカウント連携、ローカルのリモート操作設定を構成します。',
remoteServerUrl: 'リモートサーバー URL',
remoteAccessEnabled: 'リモートアクセスを有効化',
remoteControlEnabled: 'リモート操作を許可',
remoteLogin: 'ログインして連携',
remoteLoginOpening: 'ブラウザーを開いています...',
remoteLoginWaiting: 'ブラウザーでのログイン完了後、このデバイスは自動的に連携されます。',
remoteLoginSuccess: 'ログイン連携が完了しました',
remoteLogout: '連携解除',
remoteLoggingOut: '連携解除中...',
remoteLogoutSuccess: 'ローカル連携を解除しました',
remoteBindingStatus: '連携状態',
remoteBound: '連携済み',
remoteUnbound: '未連携',
remoteAccount: 'アカウント',
remoteDevice: 'デバイス',
remoteNotLoggedIn: '未ログイン',
remoteNoBoundDevice: '連携デバイスなし',

// ko
remoteAccess: '원격 액세스',
remoteAccessDescription: '공개 서버, 계정 바인딩, 로컬 원격 제어 스위치를 설정합니다.',
remoteServerUrl: '원격 서버 URL',
remoteAccessEnabled: '원격 액세스 사용',
remoteControlEnabled: '원격 제어 허용',
remoteLogin: '로그인 및 바인딩',
remoteLoginOpening: '브라우저를 여는 중...',
remoteLoginWaiting: '브라우저 로그인이 끝나면 이 기기가 자동으로 바인딩됩니다.',
remoteLoginSuccess: '로그인 바인딩 완료',
remoteLogout: '바인딩 해제',
remoteLoggingOut: '바인딩 해제 중...',
remoteLogoutSuccess: '로컬 바인딩이 해제되었습니다',
remoteBindingStatus: '바인딩 상태',
remoteBound: '바인딩됨',
remoteUnbound: '바인딩 안 됨',
remoteAccount: '계정',
remoteDevice: '기기',
remoteNotLoggedIn: '로그인 안 됨',
remoteNoBoundDevice: '바인딩된 기기 없음',

// de
remoteAccess: 'Remotezugriff',
remoteAccessDescription: 'Konfiguriert öffentlichen Server, Kontobindung und lokale Fernsteuerung.',
remoteServerUrl: 'Remote-Server-URL',
remoteAccessEnabled: 'Remotezugriff aktivieren',
remoteControlEnabled: 'Fernsteuerung erlauben',
remoteLogin: 'Anmelden und binden',
remoteLoginOpening: 'Browser wird geöffnet...',
remoteLoginWaiting: 'Nach der Browser-Anmeldung wird dieses Gerät automatisch gebunden.',
remoteLoginSuccess: 'Anmeldebindung abgeschlossen',
remoteLogout: 'Bindung lösen',
remoteLoggingOut: 'Bindung wird gelöst...',
remoteLogoutSuccess: 'Lokale Bindung entfernt',
remoteBindingStatus: 'Bindungsstatus',
remoteBound: 'Gebunden',
remoteUnbound: 'Nicht gebunden',
remoteAccount: 'Konto',
remoteDevice: 'Gerät',
remoteNotLoggedIn: 'Nicht angemeldet',
remoteNoBoundDevice: 'Kein gebundenes Gerät',
```

- [ ] **Step 2: Add CSS**

Update `src/styles.css`:

```css
.remote-settings-panel {
  min-width: 0;
}

.remote-settings-form {
  display: grid;
  gap: 14px;
  max-width: 560px;
}

.remote-toggle {
  justify-content: space-between;
  max-width: 560px;
}

.remote-binding-summary {
  display: grid;
  grid-template-columns: max-content minmax(0, 1fr);
  gap: 10px 16px;
  margin: 22px 0 0;
  max-width: 560px;
}

.remote-binding-summary dt {
  color: var(--text-muted);
}

.remote-binding-summary dd {
  margin: 0;
  min-width: 0;
  overflow-wrap: anywhere;
}

.remote-actions {
  display: flex;
  gap: 10px;
  margin-top: 18px;
}
```

- [ ] **Step 3: Add npm test script**

Update `package.json` scripts:

```json
"test:remote-settings-view": "tsc --target ES2022 --module commonjs --moduleResolution node --lib ES2022,DOM --skipLibCheck --strict --esModuleInterop --outDir /tmp/niuma-remote-settings-view-test tests/remoteSettingsView.test.ts && node /tmp/niuma-remote-settings-view-test/tests/remoteSettingsView.test.js"
```

Add `&& npm run test:remote-settings-view` to the existing `test` script.

- [ ] **Step 4: Run frontend tests**

Run:

```bash
npm run test:settings-view && npm run test:remote-settings-view
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/i18n.ts src/styles.css package.json
git commit -m "feat: 补充远程设置国际化和样式" -m "修改内容：补齐远程访问设置的六语言文案、面板样式和前端测试脚本。" -m "修改原因：项目界面文案必须支持所有已支持语言，远程设置面板也需要稳定布局验证。"
```

## Task 6: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-5.

- [ ] **Step 1: Run core tests**

Run:

```bash
cargo test -p niuma-core remote::settings remote_config_defaults_and_persists_without_device_token
```

Expected: PASS.

- [ ] **Step 2: Run Tauri remote command tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::commands
```

Expected: PASS.

- [ ] **Step 3: Run focused frontend tests**

Run:

```bash
npm run test:settings-view && npm run test:remote-settings-view
```

Expected: PASS.

- [ ] **Step 4: Run full frontend build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 5: Verify token is not rendered or stored in app config**

Run:

```bash
rg -n "device_token" src crates/niuma-core/src/store src-tauri/src/commands.rs src-tauri/src/remote
```

Expected: `device_token` appears only in remote credential handling and login binding result mapping, not in `src/settingsView.ts`, `src/main.ts`, `src/api.ts`, or app config persistence.

- [ ] **Step 6: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

## Self-Review

Spec coverage in this plan:

- Local settings page opens browser login: covered by Tasks 2 and 4.
- First login has no extra local confirmation popup: covered by the direct `remote-login` click flow.
- Account system remains on remote server: local settings only stores summaries and credentials.
- Self-hosting support: covered by editable `server_url`.
- RemoteAgent is not pluginized: all files are under core remote, Tauri remote, and settings UI.
- `device_token` is not rendered or stored in app config: covered by credential boundary and Task 6 scan.
- i18n requirement: covered by Task 5 for six languages.

Known follow-up plans:

- Implement RemoteAgent `/ws/device` connection loop and token revoked cleanup.
- Add online/offline status event wiring from RemoteAgent into settings display.
- Implement external web console device list and direct/relay connection controls.
