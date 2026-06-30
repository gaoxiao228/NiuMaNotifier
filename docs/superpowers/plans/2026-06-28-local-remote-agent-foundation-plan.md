# Local RemoteAgent Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the local NiumaNotifier RemoteAgent foundation: remote configuration, secure credential boundary, device identity, browser login binding, lifecycle state machine, and `/ws/device` connection heartbeat.

**Architecture:** RemoteAgent is a host-integrated Rust/Tauri module, not a plugin. Reusable models and platform-neutral logic live in `crates/niuma-core/src/remote/`; Tauri-specific browser opening, async agent tasks, and WebSocket networking live in `src-tauri/src/remote/`. This plan does not implement WebRTC, relay transport, or RPC method routing.

**Tech Stack:** Rust, Tauri shell, `reqwest`, `tokio-tungstenite`, `serde`, `sha2`, `rand`, existing `niuma_core::platform` paths, Rust unit tests.

---

## Scope Check

This plan covers:

- Remote config model and defaults.
- Device install ID and per-server device fingerprint.
- Credential storage trait and restricted-file fallback.
- Browser login binding start/poll flow.
- RemoteAgent lifecycle state machine.
- `/ws/device` connection request shape and heartbeat message generation.
- Token revoked cleanup behavior.

This plan does not cover:

- WebRTC signaling implementation.
- Relay transport implementation.
- E2EE RPC server execution.
- RemotePermissionGuard and RemoteAuditLog.
- Settings page UI and i18n.
- Platform-native Keychain/Credential Manager/Secret Service integrations. This plan defines the trait and a restricted-file fallback; native stores can be added behind the same trait.

## Architecture Constraints

- Do not put RemoteAgent in the plugin system.
- Do not store `device_token` in plugin directories.
- Platform-neutral paths and file-permission helpers belong in `niuma_core::platform`.
- RemoteAgent must not modify main state directly.
- Any future RPC method that changes state must reuse existing service boundaries and `StateMutationService`; this foundation plan does not invoke those methods.

## File Structure

Create:

- `crates/niuma-core/src/remote/mod.rs` - remote core module root.
- `crates/niuma-core/src/remote/config.rs` - remote config model.
- `crates/niuma-core/src/remote/credentials.rs` - credential trait, credential payload, restricted-file fallback.
- `crates/niuma-core/src/remote/device_identity.rs` - install ID and fingerprint derivation.
- `crates/niuma-core/src/remote/login_flow.rs` - desktop-login start/poll DTOs and decrypted binding result model.
- `crates/niuma-core/src/remote/agent_state.rs` - RemoteAgent lifecycle state machine.
- `src-tauri/src/remote/mod.rs`
- `src-tauri/src/remote/login_flow.rs` - Tauri browser open and HTTP binding flow shell.
- `src-tauri/src/remote/agent.rs` - RemoteAgent task orchestration shell.
- `src-tauri/src/remote/device_socket.rs` - `/ws/device` message builders and connection shell.

Modify:

- `crates/niuma-core/src/lib.rs` - export `remote` module.
- `src-tauri/src/main.rs` or `src-tauri/src/commands.rs` - wire remote module initialization only as a no-op-safe shell.
- `src-tauri/Cargo.toml` - add HTTP/WebSocket dependencies.
- `crates/niuma-core/Cargo.toml` - add `hex`, `rand`, `thiserror`; keep existing `serde`, `serde_json`, and `sha2` entries unchanged.

## Task 1: Remote Config Model

**Files:**
- Create: `crates/niuma-core/src/remote/config.rs`
- Create: `crates/niuma-core/src/remote/mod.rs`
- Modify: `crates/niuma-core/src/lib.rs`
- Test: `crates/niuma-core/src/remote/config.rs`

- [ ] **Step 1: Write failing config tests**

Create `crates/niuma-core/src/remote/config.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::config
```

Expected: FAIL because `RemoteConfig` and related types do not exist.

- [ ] **Step 3: Implement config model**

Replace `crates/niuma-core/src/remote/config.rs` with:

```rust
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
```

- [ ] **Step 4: Export module**

Create `crates/niuma-core/src/remote/mod.rs`:

```rust
pub mod config;
```

Update `crates/niuma-core/src/lib.rs`:

```rust
pub mod api_response;
pub mod approval;
pub mod approval_arbitration;
pub mod codex_hook;
pub mod codex_managed_control;
pub mod codex_managed_session;
pub mod config;
pub mod dashboard;
pub(crate) mod event_display;
pub mod hook_payload;
pub mod listener_config;
pub mod local_api_client;
pub mod main_state;
pub mod models;
pub mod notification_store;
pub mod platform;
pub mod plugin;
pub mod remote;
pub mod runtime_event;
pub mod state;
pub mod state_mutation;
pub mod store;
pub mod tool_metadata;
pub mod tool_session;
pub mod tool_session_rpc;
pub mod tools;
```

Keep the existing module order if the file has changed; the required new line is:

```rust
pub mod remote;
```

- [ ] **Step 5: Run config tests**

Run:

```bash
cargo test -p niuma-core remote::config
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-core/src/lib.rs crates/niuma-core/src/remote/mod.rs crates/niuma-core/src/remote/config.rs
git commit -m "feat: 新增远程配置模型" -m "修改内容：新增 RemoteConfig、远程账号摘要和设备摘要模型，并挂载 niuma_core::remote 模块。" -m "修改原因：本机 RemoteAgent 需要保存非敏感远程服务配置和绑定设备状态。"
```

## Task 2: Device Identity And Fingerprint

**Files:**
- Create: `crates/niuma-core/src/remote/device_identity.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Modify: `crates/niuma-core/Cargo.toml`
- Test: `crates/niuma-core/src/remote/device_identity.rs`

- [ ] **Step 1: Write failing identity tests**

Create `crates/niuma-core/src/remote/device_identity.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_different_fingerprints_for_different_server_origins() {
        let install_id = DeviceInstallId::from_bytes([7u8; 32]);
        let official = derive_device_fingerprint("https://remote.niuma.example", &install_id);
        let self_hosted = derive_device_fingerprint("https://remote.example.com", &install_id);

        assert_ne!(official, self_hosted);
        assert_eq!(official.len(), 64);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::device_identity
```

Expected: FAIL because `DeviceInstallId` and `derive_device_fingerprint` do not exist.

- [ ] **Step 3: Add dependencies**

Update `crates/niuma-core/Cargo.toml` dependencies without duplicating existing entries:

```toml
hex = "0.4"
rand = "0.8"
```

The existing `sha2 = "0.10"` dependency remains unchanged.

- [ ] **Step 4: Implement identity derivation**

Replace `crates/niuma-core/src/remote/device_identity.rs` with:

```rust
use rand::RngCore;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInstallId([u8; 32]);

impl DeviceInstallId {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn derive_device_fingerprint(server_origin: &str, install_id: &DeviceInstallId) -> String {
    let mut hasher = Sha256::new();
    // 加入固定上下文，避免同一个安装 ID 被其他用途的哈希结果复用。
    hasher.update(b"niuma-device-v1");
    hasher.update(server_origin.as_bytes());
    hasher.update(install_id.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_different_fingerprints_for_different_server_origins() {
        let install_id = DeviceInstallId::from_bytes([7u8; 32]);
        let official = derive_device_fingerprint("https://remote.niuma.example", &install_id);
        let self_hosted = derive_device_fingerprint("https://remote.example.com", &install_id);

        assert_ne!(official, self_hosted);
        assert_eq!(official.len(), 64);
    }
}
```

- [ ] **Step 5: Export module and run tests**

Update `crates/niuma-core/src/remote/mod.rs`:

```rust
pub mod config;
pub mod device_identity;
```

Run:

```bash
cargo test -p niuma-core remote::device_identity
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-core/Cargo.toml crates/niuma-core/src/remote/mod.rs crates/niuma-core/src/remote/device_identity.rs
git commit -m "feat: 新增远程设备身份派生" -m "修改内容：新增随机安装 ID 模型和按服务端 origin 派生 device fingerprint 的逻辑。" -m "修改原因：本机设备身份不能依赖硬件序列号，并且官方服务与自托管服务之间不能互相关联。"
```

## Task 3: Credential Store Boundary

**Files:**
- Create: `crates/niuma-core/src/remote/credentials.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Modify: `crates/niuma-core/Cargo.toml`
- Test: `crates/niuma-core/src/remote/credentials.rs`

- [ ] **Step 1: Write failing credential tests**

Create `crates/niuma-core/src/remote/credentials.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_payload_does_not_debug_print_token() {
        let payload = RemoteCredentialPayload {
            device_token: "secret-device-token".to_string(),
            device_identity_private_key: "secret-private-key".to_string(),
        };

        let debug = format!("{payload:?}");
        assert!(!debug.contains("secret-device-token"));
        assert!(!debug.contains("secret-private-key"));
    }

    #[test]
    fn restricted_file_store_saves_loads_and_clears_server_scoped_credential() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store = RestrictedFileCredentialStore::new(dir.path().to_path_buf());
        let payload = RemoteCredentialPayload {
            device_token: "device-token".to_string(),
            device_identity_private_key: "identity-private-key".to_string(),
        };

        store
            .save("https://remote.example.com", &payload)
            .expect("save credential");

        let loaded = store
            .load("https://remote.example.com")
            .expect("load credential");
        assert_eq!(loaded, payload);

        store
            .clear("https://remote.example.com")
            .expect("clear credential");
        assert!(matches!(
            store.load("https://remote.example.com"),
            Err(RemoteCredentialError::NotFound)
        ));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::credentials
```

Expected: FAIL because credential payload and store trait do not exist.

- [ ] **Step 3: Add dependency**

Update `crates/niuma-core/Cargo.toml` dependencies without duplicating existing entries:

```toml
thiserror = "1"
```

The existing `serde`, `serde_json`, and `tempfile` entries remain unchanged.

- [ ] **Step 4: Implement credential boundary**

Replace `crates/niuma-core/src/remote/credentials.rs` with:

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::fmt;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCredentialPayload {
    pub device_token: String,
    pub device_identity_private_key: String,
}

impl fmt::Debug for RemoteCredentialPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteCredentialPayload")
            .field("device_token", &"<redacted>")
            .field("device_identity_private_key", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum RemoteCredentialError {
    #[error("credential not found")]
    NotFound,
    #[error("credential io failed: {0}")]
    Io(String),
    #[error("credential serialization failed: {0}")]
    Serialization(String),
}

impl From<io::Error> for RemoteCredentialError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            Self::NotFound
        } else {
            Self::Io(error.to_string())
        }
    }
}

pub trait RemoteCredentialStore {
    fn load(&self, server_url: &str) -> Result<RemoteCredentialPayload, RemoteCredentialError>;
    fn save(&self, server_url: &str, payload: &RemoteCredentialPayload) -> Result<(), RemoteCredentialError>;
    fn clear(&self, server_url: &str) -> Result<(), RemoteCredentialError>;
}

#[derive(Debug, Clone)]
pub struct RestrictedFileCredentialStore {
    base_dir: PathBuf,
}

impl RestrictedFileCredentialStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn credential_path(&self, server_url: &str) -> PathBuf {
        let filename = server_url
            .replace("://", "_")
            .replace(['/', ':'], "_");
        self.base_dir.join(format!("{filename}.remote-credential.json"))
    }
}

impl RemoteCredentialStore for RestrictedFileCredentialStore {
    fn load(&self, server_url: &str) -> Result<RemoteCredentialPayload, RemoteCredentialError> {
        let path = self.credential_path(server_url);
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(|error| RemoteCredentialError::Serialization(error.to_string()))
    }

    fn save(&self, server_url: &str, payload: &RemoteCredentialPayload) -> Result<(), RemoteCredentialError> {
        fs::create_dir_all(&self.base_dir)?;
        let path = self.credential_path(server_url);
        let bytes = serde_json::to_vec(payload)
            .map_err(|error| RemoteCredentialError::Serialization(error.to_string()))?;
        fs::write(&path, bytes)?;
        restrict_file_to_current_user(&path)?;
        Ok(())
    }

    fn clear(&self, server_url: &str) -> Result<(), RemoteCredentialError> {
        let path = self.credential_path(server_url);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(RemoteCredentialError::Io(error.to_string())),
        }
    }
}

#[cfg(unix)]
fn restrict_file_to_current_user(path: &std::path::Path) -> Result<(), RemoteCredentialError> {
    use std::os::unix::fs::PermissionsExt;

    // 受限文件 fallback 只允许当前用户读写，避免 device token 被其他本机用户读取。
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_file_to_current_user(_path: &std::path::Path) -> Result<(), RemoteCredentialError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_payload_does_not_debug_print_token() {
        let payload = RemoteCredentialPayload {
            device_token: "secret-device-token".to_string(),
            device_identity_private_key: "secret-private-key".to_string(),
        };

        let debug = format!("{payload:?}");
        assert!(!debug.contains("secret-device-token"));
        assert!(!debug.contains("secret-private-key"));
    }

    #[test]
    fn credential_path_is_server_scoped() {
        let store = RestrictedFileCredentialStore::new(PathBuf::from("/tmp/niuma"));
        assert_ne!(
            store.credential_path("https://remote.niuma.example"),
            store.credential_path("https://remote.example.com")
        );
    }

    #[test]
    fn restricted_file_store_saves_loads_and_clears_server_scoped_credential() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store = RestrictedFileCredentialStore::new(dir.path().to_path_buf());
        let payload = RemoteCredentialPayload {
            device_token: "device-token".to_string(),
            device_identity_private_key: "identity-private-key".to_string(),
        };

        store
            .save("https://remote.example.com", &payload)
            .expect("save credential");

        let loaded = store
            .load("https://remote.example.com")
            .expect("load credential");
        assert_eq!(loaded, payload);

        store
            .clear("https://remote.example.com")
            .expect("clear credential");
        assert!(matches!(
            store.load("https://remote.example.com"),
            Err(RemoteCredentialError::NotFound)
        ));
    }
}
```

- [ ] **Step 5: Export module and run tests**

Update `crates/niuma-core/src/remote/mod.rs`:

```rust
pub mod config;
pub mod credentials;
pub mod device_identity;
```

Run:

```bash
cargo test -p niuma-core remote::credentials
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-core/Cargo.toml crates/niuma-core/src/remote/mod.rs crates/niuma-core/src/remote/credentials.rs
git commit -m "feat: 新增远程凭据存储边界" -m "修改内容：新增远程凭据 payload、凭据存储 trait 和按服务端隔离的受限文件存储路径。" -m "修改原因：RemoteAgent 需要保存 device token 和设备身份私钥，同时避免泄露到日志或插件目录。"
```

## Task 4: Desktop Login Binding DTOs

**Files:**
- Create: `crates/niuma-core/src/remote/login_flow.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Test: `crates/niuma-core/src/remote/login_flow.rs`

- [ ] **Step 1: Write failing login DTO tests**

Create `crates/niuma-core/src/remote/login_flow.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::login_flow
```

Expected: FAIL because DTOs do not exist.

- [ ] **Step 3: Implement login DTOs**

Replace `crates/niuma-core/src/remote/login_flow.rs` with:

```rust
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
```

- [ ] **Step 4: Export module and run tests**

Update `crates/niuma-core/src/remote/mod.rs`:

```rust
pub mod config;
pub mod credentials;
pub mod device_identity;
pub mod login_flow;
```

Run:

```bash
cargo test -p niuma-core remote::login_flow
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/niuma-core/src/remote/mod.rs crates/niuma-core/src/remote/login_flow.rs
git commit -m "feat: 新增远程浏览器绑定模型" -m "修改内容：新增 desktop-login start 请求、响应、统一 API envelope 和绑定结果 DTO。" -m "修改原因：本机设置页点击登录后需要通过浏览器完成账号登录和设备绑定，并按服务端统一响应结构解析结果。"
```

## Task 5: RemoteAgent Lifecycle State Machine

**Files:**
- Create: `crates/niuma-core/src/remote/agent_state.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Test: `crates/niuma-core/src/remote/agent_state.rs`

- [ ] **Step 1: Write failing lifecycle tests**

Create `crates/niuma-core/src/remote/agent_state.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_goes_disabled_when_remote_access_off() {
        let state = RemoteAgentState::startup(false, true);
        assert_eq!(state, RemoteAgentState::Disabled);
    }

    #[test]
    fn startup_goes_not_configured_without_device_token() {
        let state = RemoteAgentState::startup(true, false);
        assert_eq!(state, RemoteAgentState::NotConfigured);
    }

    #[test]
    fn token_revoked_stops_reconnect() {
        let policy = ReconnectPolicy::for_state(RemoteAgentState::TokenRevoked);
        assert!(!policy.should_reconnect);
        assert!(policy.clear_credentials);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::agent_state
```

Expected: FAIL because lifecycle types do not exist.

- [ ] **Step 3: Implement lifecycle state machine**

Replace `crates/niuma-core/src/remote/agent_state.rs` with:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteAgentState {
    Disabled,
    NotConfigured,
    Binding,
    Connecting,
    Online,
    Reconnecting,
    TokenRevoked,
    ServerUnreachable,
    Error,
}

impl RemoteAgentState {
    pub fn startup(remote_access_enabled: bool, has_device_token: bool) -> Self {
        if !remote_access_enabled {
            return Self::Disabled;
        }
        if !has_device_token {
            return Self::NotConfigured;
        }
        Self::Connecting
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    pub should_reconnect: bool,
    pub clear_credentials: bool,
}

impl ReconnectPolicy {
    pub fn for_state(state: RemoteAgentState) -> Self {
        match state {
            RemoteAgentState::TokenRevoked => Self {
                should_reconnect: false,
                clear_credentials: true,
            },
            RemoteAgentState::Disabled | RemoteAgentState::NotConfigured => Self {
                should_reconnect: false,
                clear_credentials: false,
            },
            _ => Self {
                should_reconnect: true,
                clear_credentials: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_goes_disabled_when_remote_access_off() {
        let state = RemoteAgentState::startup(false, true);
        assert_eq!(state, RemoteAgentState::Disabled);
    }

    #[test]
    fn startup_goes_not_configured_without_device_token() {
        let state = RemoteAgentState::startup(true, false);
        assert_eq!(state, RemoteAgentState::NotConfigured);
    }

    #[test]
    fn token_revoked_stops_reconnect() {
        let policy = ReconnectPolicy::for_state(RemoteAgentState::TokenRevoked);
        assert!(!policy.should_reconnect);
        assert!(policy.clear_credentials);
    }
}
```

- [ ] **Step 4: Export module and run tests**

Update `crates/niuma-core/src/remote/mod.rs`:

```rust
pub mod agent_state;
pub mod config;
pub mod credentials;
pub mod device_identity;
pub mod login_flow;
```

Run:

```bash
cargo test -p niuma-core remote::agent_state
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/niuma-core/src/remote/mod.rs crates/niuma-core/src/remote/agent_state.rs
git commit -m "feat: 新增 RemoteAgent 生命周期状态机" -m "修改内容：新增 RemoteAgent 状态枚举、启动状态判断和重连策略。" -m "修改原因：本机远程访问需要稳定表达 disabled、not_configured、connecting、online 和 token_revoked 等状态。"
```

## Task 6: Tauri Remote Login And Device Socket Shell

**Files:**
- Create: `src-tauri/src/remote/mod.rs`
- Create: `src-tauri/src/remote/login_flow.rs`
- Create: `src-tauri/src/remote/device_socket.rs`
- Create: `src-tauri/src/remote/agent.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/remote/device_socket.rs`

- [ ] **Step 1: Write failing device socket message tests**

Create `src-tauri/src/remote/device_socket.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_device_hello_message() {
        let message = device_hello_message("dev_1");
        assert_eq!(message["type"], "device.hello");
        assert_eq!(message["data"]["device_id"], "dev_1");
        assert_eq!(message["data"]["agent_protocol_version"], 1);
    }

    #[test]
    fn builds_heartbeat_message() {
        let message = device_heartbeat_message();
        assert_eq!(message["type"], "device.heartbeat");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket
```

Expected: FAIL because message builders do not exist.

- [ ] **Step 3: Add dependencies**

Update `src-tauri/Cargo.toml` dependencies without duplicating existing entries:

```toml
futures-util = "0.3"
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
url = "2"
uuid = { version = "1", features = ["v4"] }
```

The existing `serde` and `serde_json` entries remain unchanged.

- [ ] **Step 4: Implement remote module shell**

Create `src-tauri/src/remote/mod.rs`:

```rust
pub mod agent;
pub mod device_socket;
pub mod login_flow;
```

Create `src-tauri/src/remote/device_socket.rs`:

```rust
use serde_json::{json, Value};
use uuid::Uuid;

pub fn device_hello_message(device_id: &str) -> Value {
    json!({
        "version": 1,
        "type": "device.hello",
        "id": format!("msg_{}", Uuid::new_v4()),
        "data": {
            "device_id": device_id,
            "agent_protocol_version": 1,
            "rpc_protocol_version": 1,
            "capabilities": {
                "supports_webrtc": true,
                "supports_relay": true,
                "supports_remote_control": true
            }
        }
    })
}

pub fn device_heartbeat_message() -> Value {
    json!({
        "version": 1,
        "type": "device.heartbeat",
        "id": format!("msg_{}", Uuid::new_v4()),
        "data": {}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_device_hello_message() {
        let message = device_hello_message("dev_1");
        assert_eq!(message["type"], "device.hello");
        assert_eq!(message["data"]["device_id"], "dev_1");
        assert_eq!(message["data"]["agent_protocol_version"], 1);
    }

    #[test]
    fn builds_heartbeat_message() {
        let message = device_heartbeat_message();
        assert_eq!(message["type"], "device.heartbeat");
    }
}
```

Create `src-tauri/src/remote/login_flow.rs`:

```rust
use niuma_core::remote::login_flow::{DesktopLoginBindingResult, DesktopLoginStartRequest, DesktopLoginStartResponse};

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

    pub fn open_login_url(opener: &dyn BrowserOpener, response: &DesktopLoginStartResponse) -> Result<(), String> {
        opener.open_url(&response.login_url)
    }

    pub fn apply_binding_result(result: DesktopLoginBindingResult) -> DesktopLoginBindingResult {
        result
    }
}
```

Create `src-tauri/src/remote/agent.rs`:

```rust
use niuma_core::remote::agent_state::RemoteAgentState;
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::credentials::RemoteCredentialPayload;

pub struct RemoteAgent;

impl RemoteAgent {
    pub fn startup_state(config: &RemoteConfig, credentials: Option<&RemoteCredentialPayload>) -> RemoteAgentState {
        RemoteAgentState::startup(config.remote_access_enabled, credentials.is_some())
    }
}
```

- [ ] **Step 5: Wire module in Tauri main**

Update `src-tauri/src/main.rs` or the existing module declaration area:

```rust
mod remote;
```

Do not start network connections from module initialization in this task. Agent startup should remain a callable shell until settings and lifecycle wiring are implemented.

- [ ] **Step 6: Run Tauri remote tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/main.rs src-tauri/src/remote/mod.rs src-tauri/src/remote/login_flow.rs src-tauri/src/remote/device_socket.rs src-tauri/src/remote/agent.rs
git commit -m "feat: 新增本机 RemoteAgent 壳层" -m "修改内容：新增 Tauri remote 模块、浏览器绑定流程壳层、设备 WebSocket 消息构造和 RemoteAgent 启动状态判断。" -m "修改原因：本机需要以内置模块方式连接远程服务端，而不是通过插件系统承载远程访问。"
```

## Task 7: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-6.

- [ ] **Step 1: Run core remote tests**

Run:

```bash
cargo test -p niuma-core remote::
```

Expected: PASS.

- [ ] **Step 2: Run Tauri remote shell tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::
```

Expected: PASS.

- [ ] **Step 3: Verify RemoteAgent is not placed in plugin system**

Run:

```bash
rg -n "RemoteAgent|remote::|device_token" builtin-plugins examples/plugins src-tauri/src/tools crates/niuma-core/src/remote src-tauri/src/remote
```

Expected: `RemoteAgent`, `remote::`, and `device_token` appear only in `crates/niuma-core/src/remote` and `src-tauri/src/remote`, not in plugin manifests or plugin runtime directories.

- [ ] **Step 4: Verify no direct main-state mutation**

Run:

```bash
rg -n "StateMutationService|NiumaStore|StoredState|RuntimeEventBus" crates/niuma-core/src/remote src-tauri/src/remote
```

Expected: no output in this foundation milestone. RemoteAgent should not write main state before RPC router and permission plans define the service boundary.

- [ ] **Step 5: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

- [ ] **Step 6: Record milestone result**

Add this note to the implementation issue or PR description:

```text
Local RemoteAgent foundation complete:
- Remote config model
- Device install ID and fingerprint derivation
- Credential storage boundary
- Desktop-login binding DTOs
- RemoteAgent lifecycle state machine
- Tauri remote module shell
- /ws/device hello and heartbeat message builders

Verification:
- cargo test -p niuma-core remote::
- cargo test --manifest-path src-tauri/Cargo.toml remote::
- rg plugin boundary scan
- rg main-state mutation scan
```

Do not mark local remote control complete after this milestone. WebRTC signaling, relay transport, E2EE RPC server execution, RemotePermissionGuard, RemoteAuditLog, settings UI, and i18n remain separate milestones.

## Self-Review

Spec coverage in this plan:

- RemoteAgent is host-integrated, not pluginized: covered by architecture constraints and Task 7 scan.
- Local remote config and credential boundary: covered by Tasks 1 and 3.
- Device install ID and fingerprint derivation: covered by Task 2.
- Browser login binding data contract: covered by Task 4.
- RemoteAgent lifecycle state machine: covered by Task 5.
- `/ws/device` hello and heartbeat shape: covered by Task 6.
- Main state mutation boundary: covered by Task 7 scan.

Known follow-up plans:

- Implement settings page remote access controls and i18n strings.
- Implement browser login HTTP start/poll execution and credential save/clear commands.
- Implement WebSocket connection loop with backoff and token revoked cleanup.
- Implement WebRTC/relay transport selection.
- Implement RemoteRpcRouter, RemotePermissionGuard, and RemoteAuditLog.
