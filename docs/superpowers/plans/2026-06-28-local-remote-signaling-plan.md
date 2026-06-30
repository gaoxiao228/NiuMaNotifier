# Local RemoteAgent Signaling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Teach the local RemoteAgent to accept remote connection invitations and exchange WebRTC signaling messages over the existing `/ws/device` socket.

**Architecture:** Signal message schemas and connection negotiation state live in `niuma_core::remote`; Tauri owns socket dispatch and per-connection signaling coordination. The device socket loop remains transport-agnostic: it parses inbound server messages, calls a signaling handler, and sends handler-produced outbound messages back through `/ws/device`. This milestone stops at signaling; WebRTC DataChannel creation, relay fallback, and E2EE RPC are separate milestones.

**Tech Stack:** Rust, `serde`, `serde_json`, `tokio`, `tokio-tungstenite`, existing RemoteAgent device socket, Rust unit tests.

---

## Prerequisites

Implement after:

- `docs/superpowers/plans/2026-06-28-local-remote-agent-foundation-plan.md`
- `docs/superpowers/plans/2026-06-28-local-remote-agent-connection-plan.md`
- Server counterpart: `docs/superpowers/plans/2026-06-28-remote-server-signaling-plan.md`

Required local pieces:

- `device_hello_message`
- `device_heartbeat_message`
- `run_device_socket_once`
- `RemoteAgentStatusHandle`
- `RemoteConfig.remote_control_enabled`

## Scope Check

This plan covers:

- Local signaling message models for `connection.invite`, `signal.offer`, `signal.answer`, `signal.ice_candidate`, and `signal.cancel`.
- Parsing inbound `/ws/device` JSON messages into typed signaling events.
- Maintaining a small in-memory map of active signaling sessions by `connection_id`.
- Rejecting invitations when `remote_access_enabled=false` or `remote_control_enabled=false`.
- Sending `connection.accept`, `connection.reject`, `signal.answer`, `signal.ice_candidate`, and `signal.cancel` messages back to the remote server.
- Updating RemoteAgent status to `connecting` during negotiation and `online` after cancellation or completion.

This plan does not cover:

- Creating a real WebRTC peer connection.
- TURN candidate gathering.
- Relay fallback.
- E2EE handshake.
- RPC method execution.
- Local audit log.

## Protocol Notes

Inbound device socket messages use the shared WebSocket envelope:

```json
{
  "version": 1,
  "type": "connection.invite",
  "id": "msg_001",
  "data": {
    "connection_id": "conn_1",
    "client_id": "web_1",
    "client_label": "Chrome on Mac",
    "transport_preference": "webrtc",
    "expires_at": "2026-06-28T00:02:00.000Z"
  }
}
```

Outbound accept:

```json
{
  "version": 1,
  "type": "connection.accept",
  "id": "msg_002",
  "data": {
    "connection_id": "conn_1",
    "transport": "webrtc"
  }
}
```

Outbound reject:

```json
{
  "version": 1,
  "type": "connection.reject",
  "id": "msg_003",
  "data": {
    "connection_id": "conn_1",
    "reason": "remote_control_disabled"
  }
}
```

Signal messages:

```json
{
  "version": 1,
  "type": "signal.answer",
  "id": "msg_004",
  "data": {
    "connection_id": "conn_1",
    "sdp": "v=0..."
  }
}
```

## File Structure

Create:

- `crates/niuma-core/src/remote/signaling.rs` - signaling message types, parser, outbound builders.
- `src-tauri/src/remote/signaling.rs` - local signaling session manager and policy handling.

Modify:

- `crates/niuma-core/src/remote/mod.rs` - export `signaling`.
- `src-tauri/src/remote/mod.rs` - export `signaling`.
- `src-tauri/src/remote/device_socket.rs` - dispatch inbound messages to a signaling handler and send outbound messages.
- `src-tauri/src/remote/agent.rs` - create signaling manager and pass it to device socket.
- `src-tauri/src/remote/status.rs` - keep existing status model; no schema change.

## Task 1: Core Signaling Message Models

**Files:**
- Create: `crates/niuma-core/src/remote/signaling.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Test: `crates/niuma-core/src/remote/signaling.rs`

- [ ] **Step 1: Write failing signaling model tests**

Create `crates/niuma-core/src/remote/signaling.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_connection_invite() {
        let message = parse_device_signal_message(json!({
            "version": 1,
            "type": "connection.invite",
            "id": "msg_1",
            "data": {
                "connection_id": "conn_1",
                "client_id": "web_1",
                "client_label": "Chrome",
                "transport_preference": "webrtc",
                "expires_at": "2026-06-28T00:02:00.000Z"
            }
        }))
        .unwrap();

        assert_eq!(message.id(), "msg_1");
        assert_eq!(message.connection_id(), "conn_1");
    }

    #[test]
    fn builds_connection_reject() {
        let message = connection_reject_message("conn_1", ConnectionRejectReason::RemoteControlDisabled);
        assert_eq!(message["type"], "connection.reject");
        assert_eq!(message["data"]["connection_id"], "conn_1");
        assert_eq!(message["data"]["reason"], "remote_control_disabled");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::signaling
```

Expected: FAIL because `signaling` is not exported and message models do not exist.

- [ ] **Step 3: Implement signaling models and builders**

Replace `crates/niuma-core/src/remote/signaling.rs` with:

```rust
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceSignalEnvelope {
    pub version: u32,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionInvite {
    pub connection_id: String,
    pub client_id: String,
    pub client_label: Option<String>,
    pub transport_preference: TransportPreference,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransportPreference {
    Webrtc,
    Relay,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalOffer {
    pub connection_id: String,
    pub sdp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalAnswer {
    pub connection_id: String,
    pub sdp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalIceCandidate {
    pub connection_id: String,
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalCancel {
    pub connection_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceSignalMessage {
    ConnectionInvite { id: String, data: ConnectionInvite },
    SignalOffer { id: String, data: SignalOffer },
    SignalAnswer { id: String, data: SignalAnswer },
    SignalIceCandidate { id: String, data: SignalIceCandidate },
    SignalCancel { id: String, data: SignalCancel },
}

impl DeviceSignalMessage {
    pub fn id(&self) -> &str {
        match self {
            Self::ConnectionInvite { id, .. }
            | Self::SignalOffer { id, .. }
            | Self::SignalAnswer { id, .. }
            | Self::SignalIceCandidate { id, .. }
            | Self::SignalCancel { id, .. } => id,
        }
    }

    pub fn connection_id(&self) -> &str {
        match self {
            Self::ConnectionInvite { data, .. } => &data.connection_id,
            Self::SignalOffer { data, .. } => &data.connection_id,
            Self::SignalAnswer { data, .. } => &data.connection_id,
            Self::SignalIceCandidate { data, .. } => &data.connection_id,
            Self::SignalCancel { data, .. } => &data.connection_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionRejectReason {
    RemoteAccessDisabled,
    RemoteControlDisabled,
    Busy,
    Expired,
    UnsupportedTransport,
}

impl ConnectionRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RemoteAccessDisabled => "remote_access_disabled",
            Self::RemoteControlDisabled => "remote_control_disabled",
            Self::Busy => "busy",
            Self::Expired => "expired",
            Self::UnsupportedTransport => "unsupported_transport",
        }
    }
}

pub fn parse_device_signal_message(value: Value) -> Result<DeviceSignalMessage, String> {
    let envelope: DeviceSignalEnvelope =
        serde_json::from_value(value).map_err(|error| format!("远程信令消息格式错误：{error}"))?;
    if envelope.version != 1 {
        return Err("远程信令协议版本不支持".to_string());
    }
    match envelope.message_type.as_str() {
        "connection.invite" => Ok(DeviceSignalMessage::ConnectionInvite {
            id: envelope.id,
            data: serde_json::from_value(envelope.data)
                .map_err(|error| format!("远程连接邀请格式错误：{error}"))?,
        }),
        "signal.offer" => Ok(DeviceSignalMessage::SignalOffer {
            id: envelope.id,
            data: serde_json::from_value(envelope.data)
                .map_err(|error| format!("远程 offer 格式错误：{error}"))?,
        }),
        "signal.answer" => Ok(DeviceSignalMessage::SignalAnswer {
            id: envelope.id,
            data: serde_json::from_value(envelope.data)
                .map_err(|error| format!("远程 answer 格式错误：{error}"))?,
        }),
        "signal.ice_candidate" => Ok(DeviceSignalMessage::SignalIceCandidate {
            id: envelope.id,
            data: serde_json::from_value(envelope.data)
                .map_err(|error| format!("远程 ICE candidate 格式错误：{error}"))?,
        }),
        "signal.cancel" => Ok(DeviceSignalMessage::SignalCancel {
            id: envelope.id,
            data: serde_json::from_value(envelope.data)
                .map_err(|error| format!("远程 cancel 格式错误：{error}"))?,
        }),
        _ => Err(format!("未知远程信令消息类型：{}", envelope.message_type)),
    }
}

pub fn connection_accept_message(connection_id: &str, transport: TransportPreference) -> Value {
    json!({
        "version": 1,
        "type": "connection.accept",
        "id": format!("msg_{}", uuid_like_id()),
        "data": {
            "connection_id": connection_id,
            "transport": transport
        }
    })
}

pub fn connection_reject_message(connection_id: &str, reason: ConnectionRejectReason) -> Value {
    json!({
        "version": 1,
        "type": "connection.reject",
        "id": format!("msg_{}", uuid_like_id()),
        "data": {
            "connection_id": connection_id,
            "reason": reason.as_str()
        }
    })
}

pub fn signal_answer_message(connection_id: &str, sdp: &str) -> Value {
    json!({
        "version": 1,
        "type": "signal.answer",
        "id": format!("msg_{}", uuid_like_id()),
        "data": {
            "connection_id": connection_id,
            "sdp": sdp
        }
    })
}

pub fn signal_ice_candidate_message(
    connection_id: &str,
    candidate: &str,
    sdp_mid: Option<&str>,
    sdp_mline_index: Option<u32>,
) -> Value {
    json!({
        "version": 1,
        "type": "signal.ice_candidate",
        "id": format!("msg_{}", uuid_like_id()),
        "data": {
            "connection_id": connection_id,
            "candidate": candidate,
            "sdp_mid": sdp_mid,
            "sdp_mline_index": sdp_mline_index
        }
    })
}

pub fn signal_cancel_message(connection_id: &str, reason: &str) -> Value {
    json!({
        "version": 1,
        "type": "signal.cancel",
        "id": format!("msg_{}", uuid_like_id()),
        "data": {
            "connection_id": connection_id,
            "reason": reason
        }
    })
}

fn uuid_like_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_connection_invite() {
        let message = parse_device_signal_message(json!({
            "version": 1,
            "type": "connection.invite",
            "id": "msg_1",
            "data": {
                "connection_id": "conn_1",
                "client_id": "web_1",
                "client_label": "Chrome",
                "transport_preference": "webrtc",
                "expires_at": "2026-06-28T00:02:00.000Z"
            }
        }))
        .unwrap();

        assert_eq!(message.id(), "msg_1");
        assert_eq!(message.connection_id(), "conn_1");
    }

    #[test]
    fn builds_connection_reject() {
        let message = connection_reject_message("conn_1", ConnectionRejectReason::RemoteControlDisabled);
        assert_eq!(message["type"], "connection.reject");
        assert_eq!(message["data"]["connection_id"], "conn_1");
        assert_eq!(message["data"]["reason"], "remote_control_disabled");
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
pub mod signaling;
```

Run:

```bash
cargo test -p niuma-core remote::signaling
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/niuma-core/src/remote/signaling.rs crates/niuma-core/src/remote/mod.rs
git commit -m "feat: 新增远程信令消息模型" -m "修改内容：新增连接邀请、WebRTC 信令消息解析和本机信令响应构造。" -m "修改原因：RemoteAgent 需要理解服务端转发的连接邀请和信令消息。"
```

## Task 2: Local Signaling Session Manager

**Files:**
- Create: `src-tauri/src/remote/signaling.rs`
- Modify: `src-tauri/src/remote/mod.rs`
- Test: `src-tauri/src/remote/signaling.rs`

- [ ] **Step 1: Write failing session manager tests**

Create `src-tauri/src/remote/signaling.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::config::RemoteConfig;
    use niuma_core::remote::signaling::{ConnectionInvite, TransportPreference};

    #[test]
    fn rejects_invite_when_remote_control_disabled() {
        let manager = RemoteSignalingManager::default();
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.remote_control_enabled = false;

        let outbound = manager.handle_invite(&config, sample_invite());

        assert_eq!(outbound[0]["type"], "connection.reject");
        assert_eq!(outbound[0]["data"]["reason"], "remote_control_disabled");
    }

    #[test]
    fn accepts_first_invite_and_tracks_session() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");

        let outbound = manager.handle_invite(&config, sample_invite());

        assert_eq!(outbound[0]["type"], "connection.accept");
        assert!(manager.has_session("conn_1"));
    }

    fn sample_invite() -> ConnectionInvite {
        ConnectionInvite {
            connection_id: "conn_1".to_string(),
            client_id: "web_1".to_string(),
            client_label: Some("Chrome".to_string()),
            transport_preference: TransportPreference::Webrtc,
            expires_at: "2026-06-28T00:02:00.000Z".to_string(),
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::signaling
```

Expected: FAIL because `RemoteSignalingManager` does not exist.

- [ ] **Step 3: Implement signaling session manager**

Replace `src-tauri/src/remote/signaling.rs` with:

```rust
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::signaling::{
    connection_accept_message, connection_reject_message, signal_cancel_message,
    ConnectionInvite, ConnectionRejectReason, DeviceSignalMessage, SignalCancel, SignalIceCandidate,
    SignalOffer, TransportPreference,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSignalingSession {
    pub connection_id: String,
    pub client_id: String,
    pub transport: TransportPreference,
}

#[derive(Clone, Default)]
pub struct RemoteSignalingManager {
    sessions: Arc<Mutex<HashMap<String, RemoteSignalingSession>>>,
}

impl RemoteSignalingManager {
    pub fn handle_message(&self, config: &RemoteConfig, message: DeviceSignalMessage) -> Vec<Value> {
        match message {
            DeviceSignalMessage::ConnectionInvite { data, .. } => self.handle_invite(config, data),
            DeviceSignalMessage::SignalOffer { data, .. } => self.handle_offer(data),
            DeviceSignalMessage::SignalIceCandidate { data, .. } => self.handle_ice_candidate(data),
            DeviceSignalMessage::SignalCancel { data, .. } => self.handle_cancel(data),
            DeviceSignalMessage::SignalAnswer { data, .. } => {
                vec![signal_cancel_message(&data.connection_id, "unexpected_answer")]
            }
        }
    }

    pub fn handle_invite(&self, config: &RemoteConfig, invite: ConnectionInvite) -> Vec<Value> {
        if !config.remote_access_enabled {
            return vec![connection_reject_message(
                &invite.connection_id,
                ConnectionRejectReason::RemoteAccessDisabled,
            )];
        }
        if !config.remote_control_enabled {
            return vec![connection_reject_message(
                &invite.connection_id,
                ConnectionRejectReason::RemoteControlDisabled,
            )];
        }
        let transport = match invite.transport_preference {
            TransportPreference::Webrtc | TransportPreference::Auto => TransportPreference::Webrtc,
            TransportPreference::Relay => {
                return vec![connection_reject_message(
                    &invite.connection_id,
                    ConnectionRejectReason::UnsupportedTransport,
                )]
            }
        };
        if let Ok(mut sessions) = self.sessions.lock() {
            if !sessions.is_empty() && !sessions.contains_key(&invite.connection_id) {
                return vec![connection_reject_message(
                    &invite.connection_id,
                    ConnectionRejectReason::Busy,
                )];
            }
            sessions.insert(
                invite.connection_id.clone(),
                RemoteSignalingSession {
                    connection_id: invite.connection_id.clone(),
                    client_id: invite.client_id,
                    transport: transport.clone(),
                },
            );
        }
        vec![connection_accept_message(&invite.connection_id, transport)]
    }

    pub fn has_session(&self, connection_id: &str) -> bool {
        self.sessions
            .lock()
            .map(|sessions| sessions.contains_key(connection_id))
            .unwrap_or(false)
    }

    fn handle_offer(&self, offer: SignalOffer) -> Vec<Value> {
        if !self.has_session(&offer.connection_id) {
            return vec![signal_cancel_message(&offer.connection_id, "unknown_connection")];
        }
        vec![]
    }

    fn handle_ice_candidate(&self, candidate: SignalIceCandidate) -> Vec<Value> {
        if !self.has_session(&candidate.connection_id) {
            return vec![signal_cancel_message(&candidate.connection_id, "unknown_connection")];
        }
        vec![]
    }

    fn handle_cancel(&self, cancel: SignalCancel) -> Vec<Value> {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(&cancel.connection_id);
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::config::RemoteConfig;
    use niuma_core::remote::signaling::{ConnectionInvite, TransportPreference};

    #[test]
    fn rejects_invite_when_remote_control_disabled() {
        let manager = RemoteSignalingManager::default();
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.remote_control_enabled = false;

        let outbound = manager.handle_invite(&config, sample_invite());

        assert_eq!(outbound[0]["type"], "connection.reject");
        assert_eq!(outbound[0]["data"]["reason"], "remote_control_disabled");
    }

    #[test]
    fn accepts_first_invite_and_tracks_session() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");

        let outbound = manager.handle_invite(&config, sample_invite());

        assert_eq!(outbound[0]["type"], "connection.accept");
        assert!(manager.has_session("conn_1"));
    }

    fn sample_invite() -> ConnectionInvite {
        ConnectionInvite {
            connection_id: "conn_1".to_string(),
            client_id: "web_1".to_string(),
            client_label: Some("Chrome".to_string()),
            transport_preference: TransportPreference::Webrtc,
            expires_at: "2026-06-28T00:02:00.000Z".to_string(),
        }
    }
}
```

Update `src-tauri/src/remote/mod.rs`:

```rust
pub mod agent;
pub mod commands;
pub mod device_socket;
pub mod login_flow;
pub mod signaling;
pub mod status;
```

- [ ] **Step 4: Run signaling manager tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::signaling
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/remote/signaling.rs src-tauri/src/remote/mod.rs
git commit -m "feat: 新增本机远程信令会话管理" -m "修改内容：新增连接邀请策略、单连接会话跟踪和信令取消处理。" -m "修改原因：RemoteAgent 收到外部客户端连接邀请后需要判断本机是否允许远程控制并进入协商状态。"
```

## Task 3: Device Socket Dispatch To Signaling Manager

**Files:**
- Modify: `src-tauri/src/remote/device_socket.rs`
- Modify: `src-tauri/src/remote/agent.rs`
- Test: `src-tauri/src/remote/device_socket.rs`

- [ ] **Step 1: Write failing dispatch tests**

Add tests to `src-tauri/src/remote/device_socket.rs`:

```rust
#[cfg(test)]
mod signaling_dispatch_tests {
    use super::*;
    use niuma_core::remote::config::RemoteConfig;
    use serde_json::json;

    #[test]
    fn dispatches_connection_invite_to_handler() {
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        let outbound = dispatch_device_text_message(
            &config,
            json!({
                "version": 1,
                "type": "connection.invite",
                "id": "msg_1",
                "data": {
                    "connection_id": "conn_1",
                    "client_id": "web_1",
                    "client_label": "Chrome",
                    "transport_preference": "webrtc",
                    "expires_at": "2026-06-28T00:02:00.000Z"
                }
            })
            .to_string(),
            |_, message| {
                assert_eq!(message.connection_id(), "conn_1");
                vec![serde_json::json!({
                    "version": 1,
                    "type": "connection.accept",
                    "id": "msg_2",
                    "data": { "connection_id": "conn_1", "transport": "webrtc" }
                })]
            },
        );

        assert_eq!(outbound.unwrap()[0]["type"], "connection.accept");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket::signaling_dispatch_tests
```

Expected: FAIL because `dispatch_device_text_message` does not exist.

- [ ] **Step 3: Implement text dispatch helper**

Update `src-tauri/src/remote/device_socket.rs` imports:

```rust
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::signaling::{parse_device_signal_message, DeviceSignalMessage};
```

Add helper:

```rust
pub fn dispatch_device_text_message(
    config: &RemoteConfig,
    text: String,
    mut handler: impl FnMut(&RemoteConfig, DeviceSignalMessage) -> Vec<Value>,
) -> Result<Vec<Value>, String> {
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("远程设备消息 JSON 解析失败：{error}"))?;
    match parse_device_signal_message(value) {
        Ok(message) => Ok(handler(config, message)),
        Err(error) => {
            if error.starts_with("未知远程信令消息类型") {
                Ok(vec![])
            } else {
                Err(error)
            }
        }
    }
}
```

- [ ] **Step 4: Add signaling handler to socket run request**

Update `DeviceSocketConnectRequest` in `src-tauri/src/remote/device_socket.rs`:

```rust
pub struct DeviceSocketConnectRequest {
    pub server_url: String,
    pub device_id: String,
    pub device_token: String,
    pub heartbeat_interval_seconds: u64,
    pub remote_config: RemoteConfig,
}
```

Update tests and `build_connect_request` to pass `remote_config: config.clone()`.

Update `run_device_socket_once` signature:

```rust
pub async fn run_device_socket_once(
    request: DeviceSocketConnectRequest,
    signaling_manager: crate::remote::signaling::RemoteSignalingManager,
) -> DeviceSocketRunResult
```

In the socket read loop, replace the generic text ignore branch with:

```rust
Some(Ok(Message::Text(text))) => {
    let outbound = match dispatch_device_text_message(
        &request.remote_config,
        text,
        |config, message| signaling_manager.handle_message(config, message),
    ) {
        Ok(messages) => messages,
        Err(error) => return DeviceSocketRunResult::Failed(error),
    };
    for message in outbound {
        if let Err(error) = writer.send(Message::Text(message.to_string())).await {
            return DeviceSocketRunResult::Failed(format!("发送远程信令响应失败：{error}"));
        }
    }
}
```

Update `src-tauri/src/remote/agent.rs` so `run_agent_loop` constructs one manager and passes it to every socket run:

```rust
let signaling_manager = crate::remote::signaling::RemoteSignalingManager::default();
```

And:

```rust
let result_state = state_after_socket_result(
    run_device_socket_once(request, signaling_manager.clone()).await
);
```

- [ ] **Step 5: Run dispatch tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket::signaling_dispatch_tests remote::agent
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/remote/device_socket.rs src-tauri/src/remote/agent.rs
git commit -m "feat: 接入本机远程信令分发" -m "修改内容：让设备 WebSocket 解析服务端信令消息，调用本机信令管理器并发送 accept/reject/cancel 等响应。" -m "修改原因：外部客户端创建远程连接后，本机 RemoteAgent 需要通过 /ws/device 参与连接协商。"
```

## Task 4: Signaling Status And Settings Visibility

**Files:**
- Modify: `src-tauri/src/remote/status.rs`
- Modify: `src-tauri/src/remote/signaling.rs`
- Modify: `src-tauri/src/remote/agent.rs`
- Modify: `src/i18n.ts`
- Test: `src-tauri/src/remote/status.rs`

- [ ] **Step 1: Write failing status test**

Add to `src-tauri/src/remote/status.rs` tests:

```rust
#[test]
fn status_can_show_signaling_connection() {
    let handle = RemoteAgentStatusHandle::default();
    handle.set_active_connection(Some("conn_1".to_string()));
    let snapshot = handle.snapshot();

    assert_eq!(snapshot.active_connection_id.as_deref(), Some("conn_1"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::status
```

Expected: FAIL because `active_connection_id` is not part of the status snapshot.

- [ ] **Step 3: Extend status snapshot**

Update `RemoteAgentStatus` in `src-tauri/src/remote/status.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct RemoteAgentStatus {
    pub state: &'static str,
    pub last_error: Option<String>,
    pub active_connection_id: Option<String>,
}
```

Update constructors and lock fallback to include `active_connection_id: None`.

Add method:

```rust
pub fn set_active_connection(&self, connection_id: Option<String>) {
    if let Ok(mut value) = self.inner.lock() {
        value.active_connection_id = connection_id;
    }
}
```

- [ ] **Step 4: Update status from signaling manager**

Update `RemoteSignalingManager` in `src-tauri/src/remote/signaling.rs` to accept optional status handle:

```rust
#[derive(Clone, Default)]
pub struct RemoteSignalingManager {
    sessions: Arc<Mutex<HashMap<String, RemoteSignalingSession>>>,
    status: Option<crate::remote::status::RemoteAgentStatusHandle>,
}

impl RemoteSignalingManager {
    pub fn with_status(status: crate::remote::status::RemoteAgentStatusHandle) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            status: Some(status),
        }
    }
}
```

After accepting an invite:

```rust
if let Some(status) = &self.status {
    status.set_active_connection(Some(invite.connection_id.clone()));
}
```

After handling cancel:

```rust
if let Some(status) = &self.status {
    status.set_active_connection(None);
}
```

Update `src-tauri/src/remote/agent.rs` manager construction:

```rust
let signaling_manager = crate::remote::signaling::RemoteSignalingManager::with_status(status.clone());
```

- [ ] **Step 5: Add frontend status label**

Update `src/api.ts`:

```ts
export type RemoteAgentStatus = {
  state: string
  last_error: string | null
  active_connection_id?: string | null
}
```

Update `src/settingsView.ts` remote status summary to show active connection when present:

```ts
${options.agentStatus?.active_connection_id ? `<dt>${escapeHtml(t.remoteActiveConnection)}</dt><dd>${escapeHtml(options.agentStatus.active_connection_id)}</dd>` : ''}
```

Update `Translation` in `src/i18n.ts`:

```ts
remoteActiveConnection: string
```

Add translations:

```ts
// zh-CN
remoteActiveConnection: '当前远程连接',
// zh-TW
remoteActiveConnection: '目前遠端連線',
// en
remoteActiveConnection: 'Active remote connection',
// ja
remoteActiveConnection: '現在のリモート接続',
// ko
remoteActiveConnection: '현재 원격 연결',
// de
remoteActiveConnection: 'Aktive Remoteverbindung',
```

- [ ] **Step 6: Run status and frontend tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::status remote::signaling
npm run test:remote-settings-view
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/remote/status.rs src-tauri/src/remote/signaling.rs src-tauri/src/remote/agent.rs src/api.ts src/settingsView.ts src/i18n.ts
git commit -m "feat: 显示远程信令连接状态" -m "修改内容：RemoteAgent 状态快照新增当前连接 ID，并在设置页显示活跃远程连接。" -m "修改原因：用户需要判断外部客户端是否正在与本机进行远程连接协商。"
```

## Task 5: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-4.

- [ ] **Step 1: Run core signaling tests**

Run:

```bash
cargo test -p niuma-core remote::signaling
```

Expected: PASS.

- [ ] **Step 2: Run Tauri signaling tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::signaling remote::device_socket::signaling_dispatch_tests remote::status
```

Expected: PASS.

- [ ] **Step 3: Run frontend remote settings test**

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

- [ ] **Step 5: Verify this milestone does not implement WebRTC or relay**

Run:

```bash
rg -n "RTCPeerConnection|DataChannel|/ws/relay|relay_frame|RemoteRpcRouter|StateMutationService" src-tauri/src/remote crates/niuma-core/src/remote
```

Expected: no output for this milestone. Signal message parsing and routing are present, but transport and RPC are not implemented here.

- [ ] **Step 6: Verify no plugin boundary leak**

Run:

```bash
rg -n "connection.invite|signal.offer|signal.answer|signal.ice_candidate" builtin-plugins examples/plugins src-tauri/src/remote crates/niuma-core/src/remote
```

Expected: signal strings appear only in `src-tauri/src/remote` and `crates/niuma-core/src/remote`, not in plugin directories.

- [ ] **Step 7: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

## Self-Review

Spec coverage in this plan:

- RemoteAgent receives Web client connection invitations: covered by Tasks 1-3.
- RemoteAgent respects local remote access and remote control switches: covered by Task 2.
- RemoteAgent participates in WebRTC signaling without implementing transport: covered by Task 3.
- Settings page can show active connection negotiation: covered by Task 4.
- Plugin system remains untouched: covered by Task 5 scan.
- Main state and RPC execution remain untouched: covered by Task 5 scan.

Next milestone candidates:

- Local WebRTC DataChannel transport implementation.
- Relay fallback transport implementation.
- E2EE handshake over established transport.
