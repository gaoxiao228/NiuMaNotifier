# Local WebRTC Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the local RemoteAgent WebRTC DataChannel transport so an external Web client can negotiate a direct encrypted frame channel after signaling.

**Architecture:** `niuma_core::remote` owns transport-neutral frame and state contracts. `src-tauri::remote::webrtc_transport` owns the Rust WebRTC peer connection, answer creation, ICE candidate emission, and DataChannel frame send/receive. Existing signaling remains the carrier for offer/answer/ICE; later E2EE/RPC code will consume `RemoteTransportFrame` instead of WebRTC-specific message types.

**Tech Stack:** Rust, Tokio, `webrtc = "0.17"`, `serde`, `serde_json`, existing local signaling manager, Rust unit tests.

---

## Source Note

Use `webrtc = "0.17"` for this milestone. The upstream WebRTC.rs README describes v0.17.x as the final Tokio-coupled feature line receiving bug fixes and recommends it for Tokio-based production applications while v0.20+ is still under active architecture development.

## Prerequisites

Implement after:

- `docs/superpowers/plans/2026-06-28-local-remote-signaling-plan.md`
- Server counterpart: `docs/superpowers/plans/2026-06-28-remote-server-signaling-plan.md`

Required local pieces:

- `RemoteSignalingManager`
- `SignalOffer`
- `SignalIceCandidate`
- `signal_answer_message`
- `signal_ice_candidate_message`
- `RemoteAgentStatusHandle`

## Scope Check

This plan covers:

- Transport-neutral frame contract.
- WebRTC transport config and ICE server mapping.
- Creating a local peer connection from a remote offer.
- Producing a local answer message.
- Emitting local ICE candidate messages back through the existing signaling path.
- Opening a DataChannel named `niuma-e2ee`.
- Sending and receiving binary encrypted frames over the DataChannel.
- Updating status with selected transport `webrtc`.

This plan does not cover:

- Relay fallback.
- E2EE handshake contents.
- RPC envelope parsing or method execution.
- Browser-side WebRTC code.
- TURN server provisioning.
- Local audit log.

## File Structure

Create:

- `crates/niuma-core/src/remote/transport.rs` - transport frame, transport kind, transport state.
- `src-tauri/src/remote/webrtc_transport.rs` - WebRTC peer connection adapter.

Modify:

- `crates/niuma-core/src/remote/mod.rs` - export `transport`.
- `src-tauri/src/remote/mod.rs` - export `webrtc_transport`.
- `src-tauri/src/remote/signaling.rs` - call the WebRTC adapter on `signal.offer`.
- `src-tauri/src/remote/status.rs` - expose selected transport.
- `src-tauri/Cargo.toml` - add `webrtc = "0.17"`.

## Task 1: Core Transport Contract

**Files:**
- Create: `crates/niuma-core/src/remote/transport.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Test: `crates/niuma-core/src/remote/transport.rs`

- [ ] **Step 1: Write failing transport tests**

Create `crates/niuma-core/src/remote/transport.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_requires_connection_id_and_payload() {
        let frame = RemoteTransportFrame::new("conn_1", vec![1, 2, 3]);

        assert_eq!(frame.connection_id, "conn_1");
        assert_eq!(frame.payload, vec![1, 2, 3]);
    }

    #[test]
    fn transport_kind_serializes_as_snake_case() {
        let value = serde_json::to_value(RemoteTransportKind::Webrtc).unwrap();
        assert_eq!(value, "webrtc");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::transport
```

Expected: FAIL because `transport` is not exported and types do not exist.

- [ ] **Step 3: Implement transport contract**

Replace `crates/niuma-core/src/remote/transport.rs` with:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTransportKind {
    Webrtc,
    Relay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTransportState {
    Connecting,
    Open,
    Closed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTransportFrame {
    pub connection_id: String,
    pub payload: Vec<u8>,
}

impl RemoteTransportFrame {
    pub fn new(connection_id: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            connection_id: connection_id.into(),
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_requires_connection_id_and_payload() {
        let frame = RemoteTransportFrame::new("conn_1", vec![1, 2, 3]);

        assert_eq!(frame.connection_id, "conn_1");
        assert_eq!(frame.payload, vec![1, 2, 3]);
    }

    #[test]
    fn transport_kind_serializes_as_snake_case() {
        let value = serde_json::to_value(RemoteTransportKind::Webrtc).unwrap();
        assert_eq!(value, "webrtc");
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
pub mod transport;
```

Run:

```bash
cargo test -p niuma-core remote::transport
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/niuma-core/src/remote/transport.rs crates/niuma-core/src/remote/mod.rs
git commit -m "feat: 新增远程传输帧契约" -m "修改内容：新增远程 transport 类型、状态和二进制帧模型。" -m "修改原因：WebRTC 和 relay 需要共享同一层加密帧接口，避免 RPC 层绑定具体传输实现。"
```

## Task 2: WebRTC Config And Offer Answer Adapter

**Files:**
- Create: `src-tauri/src/remote/webrtc_transport.rs`
- Modify: `src-tauri/src/remote/mod.rs`
- Modify: `src-tauri/Cargo.toml`
- Test: `src-tauri/src/remote/webrtc_transport.rs`

- [ ] **Step 1: Write failing config tests**

Create `src-tauri/src/remote/webrtc_transport.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_data_channel_label_is_stable() {
        assert_eq!(WEBRTC_DATA_CHANNEL_LABEL, "niuma-e2ee");
    }

    #[test]
    fn maps_ice_server_urls() {
        let config = WebRtcTransportConfig {
            ice_servers: vec![IceServerConfig {
                urls: vec!["stun:stun.example.com:3478".to_string()],
                username: None,
                credential: None,
            }],
        };

        let rtc = config.to_rtc_configuration();
        assert_eq!(rtc.ice_servers[0].urls[0], "stun:stun.example.com:3478");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::webrtc_transport
```

Expected: FAIL because `webrtc_transport` is not exported and config types do not exist.

- [ ] **Step 3: Add dependency**

Update `src-tauri/Cargo.toml` dependencies:

```toml
webrtc = "0.17"
```

- [ ] **Step 4: Implement WebRTC config and answer creation**

Replace `src-tauri/src/remote/webrtc_transport.rs` with:

```rust
use niuma_core::remote::signaling::{
    signal_answer_message, signal_ice_candidate_message, SignalIceCandidate, SignalOffer,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use webrtc::api::APIBuilder;
use webrtc::data_channel::RTCDataChannel;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

pub const WEBRTC_DATA_CHANNEL_LABEL: &str = "niuma-e2ee";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IceServerConfig {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WebRtcTransportConfig {
    pub ice_servers: Vec<IceServerConfig>,
}

impl WebRtcTransportConfig {
    pub fn to_rtc_configuration(&self) -> RTCConfiguration {
        RTCConfiguration {
            ice_servers: self
                .ice_servers
                .iter()
                .map(|server| RTCIceServer {
                    urls: server.urls.clone(),
                    username: server.username.clone().unwrap_or_default(),
                    credential: server.credential.clone().unwrap_or_default(),
                })
                .collect(),
            ..Default::default()
        }
    }
}

#[derive(Debug)]
pub struct WebRtcAnswerResult {
    pub answer_message: Value,
    pub ice_rx: mpsc::UnboundedReceiver<Value>,
    pub data_channel: Arc<RTCDataChannel>,
}

pub async fn create_webrtc_answer(
    connection_id: &str,
    offer: SignalOffer,
    config: WebRtcTransportConfig,
) -> Result<WebRtcAnswerResult, String> {
    let api = APIBuilder::new().build();
    let peer = Arc::new(
        api.new_peer_connection(config.to_rtc_configuration())
            .await
            .map_err(|error| format!("创建 WebRTC PeerConnection 失败：{error}"))?,
    );
    let (ice_tx, ice_rx) = mpsc::unbounded_channel();
    let ice_connection_id = connection_id.to_string();
    peer.on_ice_candidate(Box::new(move |candidate| {
        let ice_tx = ice_tx.clone();
        let ice_connection_id = ice_connection_id.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                if let Ok(json) = candidate.to_json().await {
                    let _ = ice_tx.send(signal_ice_candidate_message(
                        &ice_connection_id,
                        &json.candidate,
                        json.sdp_mid.as_deref(),
                        json.sdp_mline_index,
                    ));
                }
            }
        })
    }));
    let data_channel = peer
        .create_data_channel(
            WEBRTC_DATA_CHANNEL_LABEL,
            Some(RTCDataChannelInit {
                ordered: Some(true),
                ..Default::default()
            }),
        )
        .await
        .map_err(|error| format!("创建 WebRTC DataChannel 失败：{error}"))?;
    let remote_offer = RTCSessionDescription::offer(offer.sdp)
        .map_err(|error| format!("解析 WebRTC offer 失败：{error}"))?;
    peer.set_remote_description(remote_offer)
        .await
        .map_err(|error| format!("设置 WebRTC remote description 失败：{error}"))?;
    let answer = peer
        .create_answer(None)
        .await
        .map_err(|error| format!("创建 WebRTC answer 失败：{error}"))?;
    peer.set_local_description(answer.clone())
        .await
        .map_err(|error| format!("设置 WebRTC local description 失败：{error}"))?;
    Ok(WebRtcAnswerResult {
        answer_message: signal_answer_message(connection_id, &answer.sdp),
        ice_rx,
        data_channel,
    })
}

pub async fn add_remote_ice_candidate(
    peer: &webrtc::peer_connection::RTCPeerConnection,
    candidate: SignalIceCandidate,
) -> Result<(), String> {
    peer.add_ice_candidate(RTCIceCandidateInit {
        candidate: candidate.candidate,
        sdp_mid: candidate.sdp_mid,
        sdp_mline_index: candidate.sdp_mline_index,
        username_fragment: None,
    })
    .await
    .map_err(|error| format!("添加远程 ICE candidate 失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_data_channel_label_is_stable() {
        assert_eq!(WEBRTC_DATA_CHANNEL_LABEL, "niuma-e2ee");
    }

    #[test]
    fn maps_ice_server_urls() {
        let config = WebRtcTransportConfig {
            ice_servers: vec![IceServerConfig {
                urls: vec!["stun:stun.example.com:3478".to_string()],
                username: None,
                credential: None,
            }],
        };

        let rtc = config.to_rtc_configuration();
        assert_eq!(rtc.ice_servers[0].urls[0], "stun:stun.example.com:3478");
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
pub mod webrtc_transport;
```

- [ ] **Step 5: Run config tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::webrtc_transport
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/remote/webrtc_transport.rs src-tauri/src/remote/mod.rs
git commit -m "feat: 新增本机 WebRTC transport 壳层" -m "修改内容：新增 WebRTC ICE 配置映射、answer 创建、ICE candidate 输出和 DataChannel 创建。" -m "修改原因：RemoteAgent 需要在信令协商后建立直连 DataChannel，作为 E2EE RPC 的底层传输。"
```

## Task 3: DataChannel Frame Adapter

**Files:**
- Modify: `src-tauri/src/remote/webrtc_transport.rs`
- Test: `src-tauri/src/remote/webrtc_transport.rs`

- [ ] **Step 1: Write failing frame adapter tests**

Add tests to `src-tauri/src/remote/webrtc_transport.rs`:

```rust
#[cfg(test)]
mod frame_tests {
    use super::*;

    #[test]
    fn validates_non_empty_transport_frame() {
        let frame = validate_outbound_frame("conn_1", vec![1]).unwrap();
        assert_eq!(frame.connection_id, "conn_1");
        assert_eq!(frame.payload, vec![1]);
    }

    #[test]
    fn rejects_empty_transport_frame() {
        assert_eq!(
            validate_outbound_frame("conn_1", vec![]).unwrap_err(),
            "远程传输帧不能为空"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::webrtc_transport::frame_tests
```

Expected: FAIL because `validate_outbound_frame` does not exist.

- [ ] **Step 3: Implement frame adapter**

Update `src-tauri/src/remote/webrtc_transport.rs`:

```rust
use niuma_core::remote::transport::RemoteTransportFrame;
use webrtc::data_channel::data_channel_message::DataChannelMessage;

pub fn validate_outbound_frame(
    connection_id: &str,
    payload: Vec<u8>,
) -> Result<RemoteTransportFrame, String> {
    if payload.is_empty() {
        return Err("远程传输帧不能为空".to_string());
    }
    Ok(RemoteTransportFrame::new(connection_id, payload))
}

pub async fn send_data_channel_frame(
    data_channel: &RTCDataChannel,
    frame: RemoteTransportFrame,
) -> Result<(), String> {
    data_channel
        .send(&frame.payload.into())
        .await
        .map_err(|error| format!("发送 WebRTC DataChannel 帧失败：{error}"))
}

pub fn data_channel_message_to_frame(
    connection_id: &str,
    message: DataChannelMessage,
) -> Result<RemoteTransportFrame, String> {
    validate_outbound_frame(connection_id, message.data.to_vec())
}
```

- [ ] **Step 4: Run frame tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::webrtc_transport::frame_tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/remote/webrtc_transport.rs
git commit -m "feat: 新增 WebRTC DataChannel 帧适配" -m "修改内容：新增 DataChannel 二进制帧校验、发送和接收转换。" -m "修改原因：E2EE RPC 层需要通过统一 RemoteTransportFrame 在 WebRTC DataChannel 上传输密文。"
```

## Task 4: Integrate WebRTC With Signaling Manager

**Files:**
- Modify: `src-tauri/src/remote/signaling.rs`
- Modify: `src-tauri/src/remote/status.rs`
- Test: `src-tauri/src/remote/signaling.rs`

- [ ] **Step 1: Write failing integration policy tests**

Add tests to `src-tauri/src/remote/signaling.rs`:

```rust
#[cfg(test)]
mod webrtc_policy_tests {
    use super::*;
    use niuma_core::remote::config::RemoteConfig;
    use niuma_core::remote::signaling::{DeviceSignalMessage, SignalOffer};

    #[test]
    fn offer_without_session_is_cancelled() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");

        let outbound = manager.handle_message(
            &config,
            DeviceSignalMessage::SignalOffer {
                id: "msg_1".to_string(),
                data: SignalOffer {
                    connection_id: "conn_missing".to_string(),
                    sdp: "v=0".to_string(),
                },
            },
        );

        assert_eq!(outbound[0]["type"], "signal.cancel");
        assert_eq!(outbound[0]["data"]["reason"], "unknown_connection");
    }
}
```

- [ ] **Step 2: Run test to verify current policy**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::signaling::webrtc_policy_tests
```

Expected: PASS after the signaling plan; keep this as the guard before adding async WebRTC creation.

- [ ] **Step 3: Add transport state to status**

Update `RemoteAgentStatus` in `src-tauri/src/remote/status.rs`:

```rust
pub selected_transport: Option<niuma_core::remote::transport::RemoteTransportKind>,
```

Update constructors and fallback snapshots with `selected_transport: None`.

Add method:

```rust
pub fn set_selected_transport(
    &self,
    transport: Option<niuma_core::remote::transport::RemoteTransportKind>,
) {
    if let Ok(mut value) = self.inner.lock() {
        value.selected_transport = transport;
    }
}
```

- [ ] **Step 4: Add async WebRTC offer handling entrypoint**

Add to `src-tauri/src/remote/signaling.rs`:

```rust
use crate::remote::webrtc_transport::{create_webrtc_answer, WebRtcTransportConfig};
use niuma_core::remote::transport::RemoteTransportKind;

impl RemoteSignalingManager {
    pub async fn handle_offer_async(
        &self,
        config: &RemoteConfig,
        offer: SignalOffer,
        webrtc_config: WebRtcTransportConfig,
    ) -> Vec<Value> {
        if !self.has_session(&offer.connection_id) {
            return vec![signal_cancel_message(&offer.connection_id, "unknown_connection")];
        }
        if !config.remote_access_enabled || !config.remote_control_enabled {
            return vec![signal_cancel_message(&offer.connection_id, "remote_control_disabled")];
        }
        match create_webrtc_answer(&offer.connection_id, offer, webrtc_config).await {
            Ok(mut result) => {
                if let Some(status) = &self.status {
                    status.set_selected_transport(Some(RemoteTransportKind::Webrtc));
                }
                let mut outbound = vec![result.answer_message];
                while let Ok(candidate) = result.ice_rx.try_recv() {
                    outbound.push(candidate);
                }
                outbound
            }
            Err(error) => vec![signal_cancel_message("unknown", &format!("webrtc_failed:{error}"))],
        }
    }
}
```

Keep synchronous `handle_message` unchanged for unit-testable policy handling. The device socket loop will call `handle_offer_async` in the next task when it receives a `signal.offer`.

- [ ] **Step 5: Run signaling tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::signaling remote::status
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/remote/signaling.rs src-tauri/src/remote/status.rs
git commit -m "feat: 接入 WebRTC offer 处理入口" -m "修改内容：新增异步 offer 处理入口、WebRTC answer 生成调用和选中传输状态写入。" -m "修改原因：RemoteAgent 收到 Web 客户端 offer 后需要生成 answer 并进入 WebRTC 直连协商。"
```

## Task 5: Device Socket Async Offer Dispatch

**Files:**
- Modify: `src-tauri/src/remote/device_socket.rs`
- Modify: `src-tauri/src/remote/agent.rs`
- Test: `src-tauri/src/remote/device_socket.rs`

- [ ] **Step 1: Write failing offer routing tests**

Add a pure routing test in `src-tauri/src/remote/device_socket.rs`:

```rust
#[cfg(test)]
mod offer_routing_tests {
    use super::*;
    use niuma_core::remote::signaling::{DeviceSignalMessage, SignalOffer};

    #[test]
    fn detects_offer_message_for_async_route() {
        let message = DeviceSignalMessage::SignalOffer {
            id: "msg_1".to_string(),
            data: SignalOffer {
                connection_id: "conn_1".to_string(),
                sdp: "v=0".to_string(),
            },
        };

        assert!(is_async_webrtc_offer(&message));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket::offer_routing_tests
```

Expected: FAIL because `is_async_webrtc_offer` does not exist.

- [ ] **Step 3: Add offer routing helper**

Update `src-tauri/src/remote/device_socket.rs`:

```rust
pub fn is_async_webrtc_offer(message: &DeviceSignalMessage) -> bool {
    matches!(message, DeviceSignalMessage::SignalOffer { .. })
}
```

- [ ] **Step 4: Wire async offer handling into socket loop**

Update `run_device_socket_once` signature:

```rust
pub async fn run_device_socket_once(
    request: DeviceSocketConnectRequest,
    signaling_manager: crate::remote::signaling::RemoteSignalingManager,
    webrtc_config: crate::remote::webrtc_transport::WebRtcTransportConfig,
) -> DeviceSocketRunResult
```

In the `Message::Text(text)` branch, parse to `DeviceSignalMessage` first. For `SignalOffer`, call:

```rust
let outbound = match message {
    DeviceSignalMessage::SignalOffer { data, .. } => {
        signaling_manager
            .handle_offer_async(&request.remote_config, data, webrtc_config.clone())
            .await
    }
    other => signaling_manager.handle_message(&request.remote_config, other),
};
```

Send every outbound JSON value through the existing writer path.

Update `src-tauri/src/remote/agent.rs` call:

```rust
let webrtc_config = crate::remote::webrtc_transport::WebRtcTransportConfig::default();
let result_state = state_after_socket_result(
    run_device_socket_once(request, signaling_manager.clone(), webrtc_config).await
);
```

- [ ] **Step 5: Run device socket tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::device_socket remote::agent
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/remote/device_socket.rs src-tauri/src/remote/agent.rs
git commit -m "feat: 设备连接分发 WebRTC offer" -m "修改内容：设备 WebSocket 收到 signal.offer 时调用异步 WebRTC answer 处理，并继续通过 /ws/device 返回信令消息。" -m "修改原因：WebRTC 直连协商必须由本机根据远端 offer 生成 answer 和 ICE candidate。"
```

## Task 6: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-5.

- [ ] **Step 1: Run core transport tests**

Run:

```bash
cargo test -p niuma-core remote::transport
```

Expected: PASS.

- [ ] **Step 2: Run Tauri WebRTC transport tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::webrtc_transport remote::signaling remote::device_socket
```

Expected: PASS.

- [ ] **Step 3: Run full frontend build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 4: Verify relay is not implemented in this milestone**

Run:

```bash
rg -n "/ws/relay|relay_frame|RelayTransport" src-tauri/src/remote crates/niuma-core/src/remote
```

Expected: no relay implementation references in source files touched by this milestone. Existing capability flags such as `supports_relay` may remain because they belong to device capability advertisement.

- [ ] **Step 5: Verify RPC is not implemented in this milestone**

Run:

```bash
rg -n "RemoteRpcRouter|StateMutationService|session.send_instruction|interaction.decide_approval" src-tauri/src/remote crates/niuma-core/src/remote
```

Expected: no output for this milestone.

- [ ] **Step 6: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

## Self-Review

Spec coverage in this plan:

- WebRTC preferred transport: covered by Tasks 2, 4, and 5.
- DataChannel carries encrypted frames but not plaintext RPC: covered by Task 3.
- Signaling continues over `/ws/device`: covered by Task 5.
- Relay fallback remains separate: covered by Task 6 scan.
- RPC and state mutation remain untouched: covered by Task 6 scan.

Next milestone candidates:

- Local relay fallback transport.
- Browser-side WebRTC transport in remote web console.
- E2EE handshake and encrypted RPC frame routing.
