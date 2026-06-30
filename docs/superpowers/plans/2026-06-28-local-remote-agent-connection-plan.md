# Local RemoteAgent Device Connection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the local RemoteAgent `/ws/device` connection loop with device-token authentication, heartbeat, reconnect backoff, and token-revoked cleanup.

**Architecture:** Pure state and retry policy live in `niuma_core::remote`; Tauri-specific WebSocket networking and background task startup live in `src-tauri/src/remote`. The agent reads remote config plus credential store, connects only when enabled and bound, sends `device.hello` and periodic `device.heartbeat`, classifies close reasons, and clears credentials on token revocation. This milestone does not process signaling, relay, WebRTC, or E2EE RPC payloads.

**Tech Stack:** Rust, Tauri background service, `tokio`, `tokio-tungstenite`, `futures-util`, `serde_json`, existing `RemoteConfig`, existing credential store, Rust unit tests.

---

## Prerequisites

Implement after these plans:

- `docs/superpowers/plans/2026-06-28-local-remote-agent-foundation-plan.md`
- `docs/superpowers/plans/2026-06-28-local-remote-settings-login-plan.md`
- Server-side counterpart: `docs/superpowers/plans/2026-06-28-remote-server-device-presence-plan.md`

Required local pieces:

- `RemoteConfig`
- `RemoteCredentialStore`
- `RemoteAgentState`
- `ReconnectPolicy`
- `device_hello_message`
- `device_heartbeat_message`

## Scope Check

This plan covers:

- `wss://` / `ws://` device socket URL derivation from configured server URL.
- `Authorization: Device <device_token>` upgrade header.
- Device hello and heartbeat send loop.
- WebSocket close classification for token revoked and retryable network failures.
- Exponential backoff policy with cap.
- Agent startup from Tauri background services.
- Credential cleanup on token revoked.
- Local status snapshot for settings UI.

This plan does not cover:

- `/ws/client` signaling.
- WebRTC offer/answer/ICE handling.
- Relay fallback frames.
- E2EE RPC session establishment.
- Remote audit log.
- Main-state mutation or tool-control execution.

## Protocol Notes

The local RemoteAgent connects to:

```text
{server_url}/ws/device
```

Scheme mapping:

```text
https://remote.example.com -> wss://remote.example.com/ws/device
http://127.0.0.1:27880 -> ws://127.0.0.1:27880/ws/device
```

Upgrade header:

```text
Authorization: Device <device_token>
```

Messages:

```json
{
  "version": 1,
  "type": "device.hello",
  "id": "msg_001",
  "data": {
    "device_id": "dev_1",
    "agent_protocol_version": 1,
    "rpc_protocol_version": 1,
    "capabilities": {
      "supports_webrtc": true,
      "supports_relay": true,
      "supports_remote_control": true
    }
  }
}
```

```json
{
  "version": 1,
  "type": "device.heartbeat",
  "id": "msg_002",
  "data": {}
}
```

Server close code `4003` means token revoked. The local agent must clear credentials, clear bound device summary, enter `TokenRevoked`, and stop reconnecting.

## File Structure

Create:

- `crates/niuma-core/src/remote/connection_policy.rs` - URL derivation, close classification, retry backoff.
- `src-tauri/src/remote/status.rs` - in-process RemoteAgent status snapshot.

Modify:

- `crates/niuma-core/src/remote/mod.rs` - export `connection_policy`.
- `src-tauri/src/remote/device_socket.rs` - real WebSocket connect/send loop helpers.
- `src-tauri/src/remote/agent.rs` - agent runner, lifecycle, credential cleanup.
- `src-tauri/src/remote/mod.rs` - export `status`.
- `src-tauri/src/background.rs` - start RemoteAgent background task after Local API startup.
- `src-tauri/src/commands.rs` - expose `get_remote_agent_status`.
- `src-tauri/src/main.rs` - register the status command.
- `src/api.ts` - add remote agent status type and API helper.
- `src/settingsView.ts` - render status summary in remote settings panel.
- `src/main.ts` - refresh status when remote settings panel is visible.
- `src/i18n.ts` - add status labels for six languages.

## Task 1: Core Connection Policy

**Files:**
- Create: `crates/niuma-core/src/remote/connection_policy.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Test: `crates/niuma-core/src/remote/connection_policy.rs`

- [ ] **Step 1: Write failing policy tests**

Create `crates/niuma-core/src/remote/connection_policy.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::connection_policy
```

Expected: FAIL because `connection_policy` is not exported and functions do not exist.

- [ ] **Step 3: Implement connection policy**

Replace `crates/niuma-core/src/remote/connection_policy.rs` with:

```rust
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
    if trimmed.starts_with("https://") {
        return Ok(format!("wss://{}/ws/device", &trimmed["https://".len()..]));
    }
    if trimmed.starts_with("http://") {
        return Ok(format!("ws://{}/ws/device", &trimmed["http://".len()..]));
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
```

- [ ] **Step 4: Export module and run tests**

Update `crates/niuma-core/src/remote/mod.rs`:

```rust
pub mod agent_state;
pub mod config;
pub mod connection_policy;
pub mod credentials;
pub mod device_identity;
pub mod login_flow;
pub mod settings;
```

Run:

```bash
cargo test -p niuma-core remote::connection_policy
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/niuma-core/src/remote/connection_policy.rs crates/niuma-core/src/remote/mod.rs
git commit -m "feat: 新增远程设备连接策略" -m "修改内容：新增 /ws/device URL 派生、WebSocket 关闭原因分类和指数退避策略。" -m "修改原因：本机 RemoteAgent 需要稳定处理连接地址、断线重连和 token 吊销关闭码。"
```

## Task 2: Device Socket Networking Helpers

**Files:**
- Modify: `src-tauri/src/remote/device_socket.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/remote/device_socket.rs`

- [ ] **Step 1: Write failing helper tests**

Add tests to `src-tauri/src/remote/device_socket.rs`:

```rust
#[cfg(test)]
mod connection_tests {
    use super::*;

    #[test]
    fn builds_device_authorization_header() {
        assert_eq!(
            device_authorization_header("dvt_secret"),
            "Device dvt_secret"
        );
    }

    #[test]
    fn token_is_not_embedded_in_url() {
        let request = DeviceSocketConnectRequest {
            server_url: "https://remote.example.com".to_string(),
            device_id: "dev_1".to_string(),
            device_token: "dvt_secret".to_string(),
            heartbeat_interval_seconds: 20,
        };

        assert_eq!(request.socket_url().unwrap(), "wss://remote.example.com/ws/device");
        assert!(!request.socket_url().unwrap().contains("dvt_secret"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket
```

Expected: FAIL because `DeviceSocketConnectRequest` and `device_authorization_header` do not exist.

- [ ] **Step 3: Confirm dependencies**

Update `src-tauri/Cargo.toml` dependencies without duplicating existing entries:

```toml
futures-util = "0.3"
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
http = "1"
```

The foundation plan may already add `futures-util` and `tokio-tungstenite`; keep one entry for each dependency.

- [ ] **Step 4: Implement connect request and real socket runner**

Update `src-tauri/src/remote/device_socket.rs`:

```rust
use futures_util::{SinkExt, StreamExt};
use http::Request;
use niuma_core::remote::connection_policy::{
    classify_device_socket_close, device_socket_url, DeviceSocketCloseReason,
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DeviceSocketConnectRequest {
    pub server_url: String,
    pub device_id: String,
    pub device_token: String,
    pub heartbeat_interval_seconds: u64,
}

impl DeviceSocketConnectRequest {
    pub fn socket_url(&self) -> Result<String, String> {
        device_socket_url(&self.server_url)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceSocketRunResult {
    Closed(DeviceSocketCloseReason),
    Failed(String),
}

pub fn device_authorization_header(device_token: &str) -> String {
    format!("Device {device_token}")
}

pub async fn run_device_socket_once(request: DeviceSocketConnectRequest) -> DeviceSocketRunResult {
    let socket_url = match request.socket_url() {
        Ok(value) => value,
        Err(error) => return DeviceSocketRunResult::Failed(error),
    };
    let upgrade_request = match Request::builder()
        .uri(&socket_url)
        .header("Authorization", device_authorization_header(&request.device_token))
        .body(())
    {
        Ok(value) => value,
        Err(error) => return DeviceSocketRunResult::Failed(format!("构造远程连接请求失败：{error}")),
    };
    let (stream, _) = match connect_async(upgrade_request).await {
        Ok(value) => value,
        Err(error) => return DeviceSocketRunResult::Failed(format!("远程设备连接失败：{error}")),
    };
    let (mut writer, mut reader) = stream.split();
    if let Err(error) = writer
        .send(Message::Text(device_hello_message(&request.device_id).to_string()))
        .await
    {
        return DeviceSocketRunResult::Failed(format!("发送远程 hello 失败：{error}"));
    }

    let mut heartbeat = time::interval(Duration::from_secs(request.heartbeat_interval_seconds));
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if let Err(error) = writer
                    .send(Message::Text(device_heartbeat_message().to_string()))
                    .await
                {
                    return DeviceSocketRunResult::Failed(format!("发送远程 heartbeat 失败：{error}"));
                }
            }
            next = reader.next() => {
                match next {
                    Some(Ok(Message::Close(frame))) => {
                        return DeviceSocketRunResult::Closed(classify_device_socket_close(
                            frame.map(|value| value.code.into())
                        ));
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if let Err(error) = writer.send(Message::Pong(payload)).await {
                            return DeviceSocketRunResult::Failed(format!("回复远程 ping 失败：{error}"));
                        }
                    }
                    Some(Ok(_message)) => {}
                    Some(Err(error)) => {
                        return DeviceSocketRunResult::Failed(format!("读取远程设备连接失败：{error}"));
                    }
                    None => return DeviceSocketRunResult::Closed(DeviceSocketCloseReason::NetworkError),
                }
            }
        }
    }
}
```

Keep the existing `device_hello_message` and `device_heartbeat_message` functions from the foundation milestone in the same file.

- [ ] **Step 5: Run device socket tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/remote/device_socket.rs
git commit -m "feat: 新增远程设备 WebSocket 连接助手" -m "修改内容：新增设备 socket URL、Authorization header、hello/heartbeat 发送循环和关闭原因返回。" -m "修改原因：RemoteAgent 需要用 device token 主动连接远程服务端并维持在线状态。"
```

## Task 3: RemoteAgent Runner And Token Revoked Cleanup

**Files:**
- Modify: `src-tauri/src/remote/agent.rs`
- Test: `src-tauri/src/remote/agent.rs`

- [ ] **Step 1: Write failing runner tests**

Add tests to `src-tauri/src/remote/agent.rs`:

```rust
#[cfg(test)]
mod connection_tests {
    use super::*;
    use niuma_core::remote::agent_state::RemoteAgentState;
    use niuma_core::remote::config::{RemoteConfig, RemoteDeviceSummary};
    use niuma_core::remote::connection_policy::DeviceSocketCloseReason;

    #[test]
    fn build_connect_request_requires_bound_device_and_credential() {
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "NiuMa MacBook".to_string(),
        });
        let credential = test_credential("dvt_secret");

        let request = build_connect_request(&config, &credential).unwrap();

        assert_eq!(request.device_id, "dev_1");
        assert_eq!(request.device_token, "dvt_secret");
    }

    #[test]
    fn token_revoked_result_enters_token_revoked_state() {
        assert_eq!(
            state_after_socket_result(DeviceSocketRunResult::Closed(DeviceSocketCloseReason::TokenRevoked)),
            RemoteAgentState::TokenRevoked
        );
    }

    fn test_credential(token: &str) -> niuma_core::remote::credentials::RemoteCredentialPayload {
        niuma_core::remote::credentials::RemoteCredentialPayload {
            device_token: token.to_string(),
            device_identity_private_key: "identity-private-key".to_string(),
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::agent
```

Expected: FAIL because the connection helpers do not exist.

- [ ] **Step 3: Implement connect request builder and result transition**

Update `src-tauri/src/remote/agent.rs`:

```rust
use crate::remote::device_socket::{
    run_device_socket_once, DeviceSocketConnectRequest, DeviceSocketRunResult,
};
use niuma_core::remote::agent_state::{ReconnectPolicy, RemoteAgentState};
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::connection_policy::{DeviceSocketCloseReason, ReconnectBackoff};
use niuma_core::remote::credentials::{RemoteCredentialPayload, RemoteCredentialStore};
use std::time::Duration;
use tokio::time;

pub const DEVICE_HEARTBEAT_INTERVAL_SECONDS: u64 = 20;

pub struct RemoteAgent;

impl RemoteAgent {
    pub fn startup_state(config: &RemoteConfig, credentials: Option<&RemoteCredentialPayload>) -> RemoteAgentState {
        RemoteAgentState::startup(config.remote_access_enabled, credentials.is_some())
    }
}

pub fn build_connect_request(
    config: &RemoteConfig,
    credential: &RemoteCredentialPayload,
) -> Result<DeviceSocketConnectRequest, String> {
    let Some(device) = config.device.as_ref() else {
        return Err("远程设备未绑定".to_string());
    };
    Ok(DeviceSocketConnectRequest {
        server_url: config.server_url.clone(),
        device_id: device.id.clone(),
        device_token: credential.device_token.clone(),
        heartbeat_interval_seconds: DEVICE_HEARTBEAT_INTERVAL_SECONDS,
    })
}

pub fn state_after_socket_result(result: DeviceSocketRunResult) -> RemoteAgentState {
    match result {
        DeviceSocketRunResult::Closed(DeviceSocketCloseReason::TokenRevoked) => {
            RemoteAgentState::TokenRevoked
        }
        DeviceSocketRunResult::Closed(DeviceSocketCloseReason::ServerShutdown)
        | DeviceSocketRunResult::Closed(DeviceSocketCloseReason::NetworkError)
        | DeviceSocketRunResult::Failed(_) => RemoteAgentState::Reconnecting,
        DeviceSocketRunResult::Closed(DeviceSocketCloseReason::ProtocolError) => RemoteAgentState::Error,
    }
}

pub async fn run_agent_loop(
    mut load_config: impl FnMut() -> Result<RemoteConfig, String>,
    credential_store: impl RemoteCredentialStore,
) {
    let backoff = ReconnectBackoff::default();
    let mut attempt = 0u32;
    loop {
        let config = match load_config() {
            Ok(value) => value,
            Err(error) => {
                eprintln!("NiumaNotifier remote config load failed: {error}");
                time::sleep(Duration::from_secs(30)).await;
                continue;
            }
        };
        if !config.remote_access_enabled {
            time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let credential = match credential_store.load(&config.server_url) {
            Ok(value) => value,
            Err(_) => {
                time::sleep(Duration::from_secs(30)).await;
                continue;
            }
        };
        let request = match build_connect_request(&config, &credential) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("NiumaNotifier remote connect request not ready: {error}");
                time::sleep(Duration::from_secs(30)).await;
                continue;
            }
        };

        match state_after_socket_result(run_device_socket_once(request).await) {
            RemoteAgentState::TokenRevoked => {
                if let Err(error) = credential_store.clear(&config.server_url) {
                    eprintln!("NiumaNotifier remote credential clear failed: {error}");
                }
                break;
            }
            RemoteAgentState::Reconnecting => {
                let delay = backoff.delay_for_attempt(attempt);
                attempt = attempt.saturating_add(1);
                time::sleep(delay).await;
            }
            RemoteAgentState::Error => {
                time::sleep(Duration::from_secs(60)).await;
            }
            _ => {
                attempt = 0;
            }
        }
    }
}
```

The `run_agent_loop` function logs local agent runtime failures but does not write main state and does not call `StateMutationService`.

- [ ] **Step 4: Run agent tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::agent
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/remote/agent.rs
git commit -m "feat: 新增 RemoteAgent 连接循环" -m "修改内容：新增远程连接请求构造、socket 结果状态转换、退避重连和 token revoked 凭据清理。" -m "修改原因：本机需要在登录绑定后自动维持 /ws/device 在线，并在设备 token 吊销时停止重连。"
```

## Task 4: Background Startup And Shared Status Snapshot

**Files:**
- Create: `src-tauri/src/remote/status.rs`
- Modify: `src-tauri/src/remote/mod.rs`
- Modify: `src-tauri/src/remote/agent.rs`
- Modify: `src-tauri/src/background.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs`
- Test: `src-tauri/src/remote/status.rs`

- [ ] **Step 1: Write failing status tests**

Create `src-tauri/src/remote/status.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::agent_state::RemoteAgentState;

    #[test]
    fn status_snapshot_serializes_state_without_credentials() {
        let status = RemoteAgentStatus::new(RemoteAgentState::Online);
        let value = serde_json::to_value(status).unwrap();

        assert_eq!(value["state"], "online");
        assert!(value.get("device_token").is_none());
    }

    #[test]
    fn status_handle_updates_snapshot() {
        let handle = RemoteAgentStatusHandle::default();
        handle.set_state(RemoteAgentState::Connecting, None);
        assert_eq!(handle.snapshot().state, "connecting");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::status
```

Expected: FAIL because `RemoteAgentStatus` does not exist.

- [ ] **Step 3: Implement status snapshot**

Replace `src-tauri/src/remote/status.rs` with:

```rust
use niuma_core::remote::agent_state::RemoteAgentState;
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize)]
pub struct RemoteAgentStatus {
    pub state: &'static str,
    pub last_error: Option<String>,
}

impl RemoteAgentStatus {
    pub fn new(state: RemoteAgentState) -> Self {
        Self {
            state: state_label(state),
            last_error: None,
        }
    }
}

#[derive(Clone)]
pub struct RemoteAgentStatusHandle {
    inner: Arc<Mutex<RemoteAgentStatus>>,
}

impl Default for RemoteAgentStatusHandle {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RemoteAgentStatus::new(RemoteAgentState::NotConfigured))),
        }
    }
}

impl RemoteAgentStatusHandle {
    pub fn set_state(&self, state: RemoteAgentState, last_error: Option<String>) {
        if let Ok(mut value) = self.inner.lock() {
            *value = RemoteAgentStatus {
                state: state_label(state),
                last_error,
            };
        }
    }

    pub fn snapshot(&self) -> RemoteAgentStatus {
        self.inner
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| RemoteAgentStatus {
                state: "error",
                last_error: Some("远程状态锁定失败".to_string()),
            })
    }
}

fn state_label(state: RemoteAgentState) -> &'static str {
    match state {
        RemoteAgentState::Disabled => "disabled",
        RemoteAgentState::NotConfigured => "not_configured",
        RemoteAgentState::Binding => "binding",
        RemoteAgentState::Connecting => "connecting",
        RemoteAgentState::Online => "online",
        RemoteAgentState::Reconnecting => "reconnecting",
        RemoteAgentState::TokenRevoked => "token_revoked",
        RemoteAgentState::ServerUnreachable => "server_unreachable",
        RemoteAgentState::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::agent_state::RemoteAgentState;

    #[test]
    fn status_snapshot_serializes_state_without_credentials() {
        let status = RemoteAgentStatus::new(RemoteAgentState::Online);
        let value = serde_json::to_value(status).unwrap();

        assert_eq!(value["state"], "online");
        assert!(value.get("device_token").is_none());
    }

    #[test]
    fn status_handle_updates_snapshot() {
        let handle = RemoteAgentStatusHandle::default();
        handle.set_state(RemoteAgentState::Connecting, None);
        assert_eq!(handle.snapshot().state, "connecting");
    }
}
```

Update `src-tauri/src/remote/mod.rs`:

```rust
pub mod agent;
pub mod commands;
pub mod device_socket;
pub mod login_flow;
pub mod status;
```

- [ ] **Step 4: Wire status handle into agent loop**

Update `src-tauri/src/remote/agent.rs` imports:

```rust
use crate::remote::status::RemoteAgentStatusHandle;
```

Update `run_agent_loop` signature:

```rust
pub async fn run_agent_loop(
    mut load_config: impl FnMut() -> Result<RemoteConfig, String>,
    credential_store: impl RemoteCredentialStore,
    status: RemoteAgentStatusHandle,
)
```

Update status transitions inside `run_agent_loop`:

```rust
let config = match load_config() {
    Ok(value) => value,
    Err(error) => {
        status.set_state(RemoteAgentState::Error, Some(error.clone()));
        eprintln!("NiumaNotifier remote config load failed: {error}");
        time::sleep(Duration::from_secs(30)).await;
        continue;
    }
};
if !config.remote_access_enabled {
    status.set_state(RemoteAgentState::Disabled, None);
    time::sleep(Duration::from_secs(30)).await;
    continue;
}
let credential = match credential_store.load(&config.server_url) {
    Ok(value) => value,
    Err(_) => {
        status.set_state(RemoteAgentState::NotConfigured, None);
        time::sleep(Duration::from_secs(30)).await;
        continue;
    }
};
let request = match build_connect_request(&config, &credential) {
    Ok(value) => value,
    Err(error) => {
        status.set_state(RemoteAgentState::NotConfigured, Some(error.clone()));
        eprintln!("NiumaNotifier remote connect request not ready: {error}");
        time::sleep(Duration::from_secs(30)).await;
        continue;
    }
};
status.set_state(RemoteAgentState::Connecting, None);
let result_state = state_after_socket_result(run_device_socket_once(request).await);
status.set_state(result_state, None);
```

Keep the token-revoked cleanup branch from Task 3. After clearing credentials in that branch, leave status as `TokenRevoked` and break the loop.

- [ ] **Step 5: Start RemoteAgent from background service**

Update `src-tauri/src/background.rs`:

```rust
use crate::remote;
use crate::remote::status::RemoteAgentStatusHandle;
```

Update `spawn_background_services` signature:

```rust
pub fn spawn_background_services(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    tool_sessions: niuma_api::tool_sessions::ToolSessionRegistry,
    remote_agent_status: RemoteAgentStatusHandle,
)
```

After `spawn_stale_sweep_runtime(store.clone(), runtime_events.clone());`, add:

```rust
remote::agent::spawn_remote_agent_runtime(store.clone(), remote_agent_status.clone());
```

Add function to `src-tauri/src/remote/agent.rs`:

```rust
use crate::remote::status::RemoteAgentStatusHandle;
use niuma_core::remote::credentials::RestrictedFileCredentialStore;
use niuma_core::store::NiumaStore;
use std::thread;

pub fn spawn_remote_agent_runtime(store: NiumaStore, status: RemoteAgentStatusHandle) {
    if let Err(error) = thread::Builder::new()
        .name("remote-agent-runtime".to_string())
        .spawn(move || {
            let credential_store = RestrictedFileCredentialStore::new(
                NiumaStore::default_path()
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(std::env::temp_dir)
                    .join("remote-credentials"),
            );
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("NiumaNotifier remote runtime not started: {error}");
                    return;
                }
            };
            runtime.block_on(run_agent_loop(move || store.remote_config(), credential_store, status));
        })
    {
        eprintln!("NiumaNotifier remote agent startup thread not started: {error}");
    }
}
```

- [ ] **Step 6: Add status command**

Update `AppRuntimeState` in `src-tauri/src/commands.rs`:

```rust
pub(crate) remote_agent_status: crate::remote::status::RemoteAgentStatusHandle,
```

Update `src-tauri/src/commands.rs`:

```rust
#[tauri::command]
pub(crate) fn get_remote_agent_status(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    ApiResponse::ok(serde_json::json!({
        "status": runtime_state.remote_agent_status.snapshot()
    }))
}
```

Update `src-tauri/src/main.rs` state creation:

```rust
let remote_agent_status = remote::status::RemoteAgentStatusHandle::default();
```

Update `.manage(commands::AppRuntimeState { ... })`:

```rust
remote_agent_status: remote_agent_status.clone(),
```

Update `background::spawn_background_services(...)` call:

```rust
background::spawn_background_services(
    store.clone(),
    runtime_events.clone(),
    tool_sessions.clone(),
    remote_agent_status.clone(),
);
```

Register in `src-tauri/src/main.rs`:

```rust
commands::get_remote_agent_status,
```

- [ ] **Step 7: Run status tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::status remote::agent
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/remote/status.rs src-tauri/src/remote/mod.rs src-tauri/src/remote/agent.rs src-tauri/src/background.rs src-tauri/src/commands.rs src-tauri/src/main.rs
git commit -m "feat: 启动本机 RemoteAgent 后台运行时" -m "修改内容：新增远程状态快照、后台 RemoteAgent 启动线程和状态查询命令。" -m "修改原因：登录绑定后的本机需要自动连接远程服务端，并让设置页能读取远程运行状态。"
```

## Task 5: Settings UI Status Display

**Files:**
- Modify: `src/api.ts`
- Modify: `src/settingsView.ts`
- Modify: `src/main.ts`
- Modify: `src/i18n.ts`
- Test: `tests/remoteSettingsView.test.ts`

- [ ] **Step 1: Write failing render test**

Update `tests/remoteSettingsView.test.ts`:

```ts
if (!html.includes('远程状态') || !html.includes('在线')) {
  throw new Error('远程访问设置应显示 RemoteAgent 状态')
}
```

The existing `renderRemoteSettingsPanel` call in the test should pass:

```ts
agentStatus: {
  state: 'online',
  last_error: null
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run test:remote-settings-view
```

Expected: FAIL because the panel does not accept or render `agentStatus`.

- [ ] **Step 3: Add frontend API type**

Update `src/api.ts`:

```ts
export type RemoteAgentStatus = {
  state: string
  last_error: string | null
}

export async function getRemoteAgentStatus() {
  const response = await invoke<ApiResponse<{ status: RemoteAgentStatus }>>('get_remote_agent_status')
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data.status
}
```

- [ ] **Step 4: Render status in remote settings panel**

Update `RemoteSettingsRenderOptions` in `src/settingsView.ts`:

```ts
agentStatus: RemoteAgentStatus | null
```

Update imports:

```ts
import type { PluginConfigField, PluginManagementItem, RemoteAgentStatus, RemoteSettingsPayload } from './api'
```

Add to `renderRemoteSettingsPanel` summary:

```ts
const agentStatus = options.agentStatus?.state
  ? translateRemoteAgentState(options.language, options.agentStatus.state)
  : t.loading
```

Add rows:

```ts
<dt>${escapeHtml(t.remoteAgentStatus)}</dt>
<dd>${escapeHtml(agentStatus)}</dd>
${options.agentStatus?.last_error ? `<dt>${escapeHtml(t.error)}</dt><dd>${escapeHtml(options.agentStatus.last_error)}</dd>` : ''}
```

Add translator:

```ts
export function translateRemoteAgentState(language: LanguageCode, state: string) {
  const t = translations[language]
  return t.remoteAgentState[state] ?? state
}
```

- [ ] **Step 5: Load status in main UI**

Update `src/main.ts` imports:

```ts
import { getRemoteAgentStatus, type RemoteAgentStatus } from './api'
```

Add state:

```ts
let remoteAgentStatus: RemoteAgentStatus | null = null
```

Pass `agentStatus: remoteAgentStatus` to `renderRemoteSettingsPanel`.

Add loader:

```ts
async function loadRemoteAgentStatus() {
  try {
    remoteAgentStatus = await getRemoteAgentStatus()
  } catch {
    remoteAgentStatus = null
  }
  renderRemoteSettings()
}
```

Call `loadRemoteAgentStatus()` when switching to `remote-access`, and after successful `loadRemoteSettings()`.

- [ ] **Step 6: Add i18n labels**

Update `Translation` in `src/i18n.ts`:

```ts
remoteAgentStatus: string
remoteAgentState: Record<string, string>
```

Add six-language values:

```ts
remoteAgentStatus: '远程状态',
remoteAgentState: {
  disabled: '已关闭',
  not_configured: '未配置',
  binding: '绑定中',
  connecting: '连接中',
  online: '在线',
  reconnecting: '重连中',
  token_revoked: 'Token 已吊销',
  server_unreachable: '服务不可达',
  error: '错误'
}
```

Use equivalent translated values for `zh-TW`, `en`, `ja`, `ko`, and `de`.

- [ ] **Step 7: Run frontend tests**

Run:

```bash
npm run test:remote-settings-view
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/api.ts src/settingsView.ts src/main.ts src/i18n.ts tests/remoteSettingsView.test.ts
git commit -m "feat: 设置页显示远程连接状态" -m "修改内容：新增 RemoteAgent 状态查询 API、设置页状态展示和六语言状态文案。" -m "修改原因：用户需要判断本机是否已经连接外网服务端并可被外部客户端发现。"
```

## Task 6: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-5.

- [ ] **Step 1: Run core policy tests**

Run:

```bash
cargo test -p niuma-core remote::connection_policy
```

Expected: PASS.

- [ ] **Step 2: Run Tauri remote tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket remote::agent remote::status
```

Expected: PASS.

- [ ] **Step 3: Run focused frontend test**

Run:

```bash
npm run test:remote-settings-view
```

Expected: PASS.

- [ ] **Step 4: Run full frontend build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 5: Verify plugin boundary**

Run:

```bash
rg -n "RemoteAgent|remote-agent|/ws/device|device_token" builtin-plugins examples/plugins src-tauri/src/remote crates/niuma-core/src/remote
```

Expected: remote agent and `device_token` references appear only in `src-tauri/src/remote` and `crates/niuma-core/src/remote`, not in plugin manifests or plugin runtime source.

- [ ] **Step 6: Verify no main-state mutation**

Run:

```bash
rg -n "StateMutationService|MainStateService|append_event|save_runtime_state" src-tauri/src/remote crates/niuma-core/src/remote
```

Expected: no output for this milestone. RemoteAgent connection presence must not mutate local main state.

- [ ] **Step 7: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

## Self-Review

Spec coverage in this plan:

- RemoteAgent uses `device_token` to connect `/ws/device`: covered by Task 2.
- Local app does not expose a public port: only outbound WebSocket is introduced.
- Network errors use exponential backoff: covered by Task 1 and Task 3.
- Token revoked stops reconnect and clears credentials: covered by Task 1 and Task 3.
- Agent starts as host-integrated module, not plugin: covered by Task 4 and Task 6 boundary scan.
- Settings page can show whether the local service exists and is online: covered by Task 5.
- Main state is not directly mutated: covered by Task 6 scan.

Next milestone candidates:

- Process connection invitations and `/ws/client` signaling messages.
- Add live status handle updates from the running agent loop.
- Implement WebRTC transport and relay fallback.
- Attach E2EE RPC session handling after transport selection.
