# Local Relay Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the local RemoteAgent relay fallback transport so encrypted frames can pass through `/ws/relay` when WebRTC DataChannel is unavailable.

**Architecture:** Relay reuses the transport-neutral `RemoteTransportFrame` contract from the local WebRTC milestone. `niuma_core::remote` owns relay frame shape and URL derivation; `src-tauri::remote::relay_transport` owns the device-side `/ws/relay` WebSocket, sequence numbers, ciphertext frame conversion, and status updates. The signaling manager selects relay only after WebRTC failure or when a client explicitly requests relay and local policy allows it.

**Tech Stack:** Rust, Tokio, `tokio-tungstenite`, `futures-util`, `base64`, `serde`, `serde_json`, existing RemoteTransportFrame, Rust unit tests.

---

## Prerequisites

Implement after:

- `docs/superpowers/plans/2026-06-28-local-webrtc-transport-plan.md`
- `docs/superpowers/plans/2026-06-28-local-remote-signaling-plan.md`
- Server counterpart: `docs/superpowers/plans/2026-06-28-remote-server-relay-plan.md`

Required local pieces:

- `RemoteTransportFrame`
- `RemoteTransportKind`
- `RemoteSignalingManager`
- `RemoteAgentStatusHandle`
- device-side connection token from the `connection.invite` payload or accepted connection state.

## Scope Check

This plan covers:

- Device-side relay URL derivation.
- Relay bind query parameters: `connection_id`, `connection_token`, `side=device`.
- `relay.frame` JSON encode/decode with base64 payload.
- Per-connection monotonic outbound sequence numbers.
- Device-side `/ws/relay` connect/send/receive loop.
- Switching selected transport status to `relay`.
- Signaling manager entrypoint to start relay fallback.

This plan does not cover:

- Server `/ws/relay` implementation.
- Browser relay client implementation.
- E2EE handshake.
- RPC envelope parsing or execution.
- Audit log.
- Multi-connection concurrency beyond one active local remote connection.

## Protocol Notes

Device-side relay URL:

```text
https://remote.example.com -> wss://remote.example.com/ws/relay?connection_id=conn_1&connection_token=cnt_1&side=device
http://127.0.0.1:27880 -> ws://127.0.0.1:27880/ws/relay?connection_id=conn_1&connection_token=cnt_1&side=device
```

Relay frame:

```json
{
  "version": 1,
  "type": "relay.frame",
  "id": "msg_001",
  "connection_id": "conn_1",
  "seq": 1,
  "ciphertext": "AQID"
}
```

`ciphertext` is encoded bytes from `RemoteTransportFrame.payload`. The relay transport does not inspect decrypted RPC content.

## File Structure

Create:

- `crates/niuma-core/src/remote/relay.rs` - relay URL, frame model, encode/decode helpers.
- `src-tauri/src/remote/relay_transport.rs` - device-side relay WebSocket adapter.

Modify:

- `crates/niuma-core/src/remote/mod.rs` - export `relay`.
- `crates/niuma-core/Cargo.toml` - add `base64`.
- `src-tauri/src/remote/mod.rs` - export `relay_transport`.
- `src-tauri/src/remote/signaling.rs` - accept relay-capable invite and start fallback.
- `src-tauri/src/remote/status.rs` - reuse selected transport field from WebRTC plan.
- `src-tauri/Cargo.toml` - add `base64` if only used Tauri-side by implementation choice.

## Task 1: Core Relay Frame Contract

**Files:**
- Create: `crates/niuma-core/src/remote/relay.rs`
- Modify: `crates/niuma-core/src/remote/mod.rs`
- Modify: `crates/niuma-core/Cargo.toml`
- Test: `crates/niuma-core/src/remote/relay.rs`

- [ ] **Step 1: Write failing relay contract tests**

Create `crates/niuma-core/src/remote/relay.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::transport::RemoteTransportFrame;

    #[test]
    fn derives_device_relay_url() {
        assert_eq!(
            device_relay_url("https://remote.example.com", "conn_1", "cnt_1").unwrap(),
            "wss://remote.example.com/ws/relay?connection_id=conn_1&connection_token=cnt_1&side=device"
        );
    }

    #[test]
    fn encodes_and_decodes_relay_frame_payload() {
        let frame = RemoteTransportFrame::new("conn_1", vec![1, 2, 3]);
        let relay = RelayFrame::from_transport_frame(frame, 7);

        assert_eq!(relay.seq, 7);
        assert_eq!(relay.ciphertext, "AQID");
        assert_eq!(relay.into_transport_frame().unwrap().payload, vec![1, 2, 3]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::relay
```

Expected: FAIL because relay module and helpers do not exist.

- [ ] **Step 3: Add dependency**

Update `crates/niuma-core/Cargo.toml` dependencies:

```toml
base64 = "0.22"
```

- [ ] **Step 4: Implement relay contract**

Replace `crates/niuma-core/src/remote/relay.rs` with:

```rust
use crate::remote::transport::RemoteTransportFrame;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayFrame {
    pub version: u32,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub connection_id: String,
    pub seq: u64,
    pub ciphertext: String,
}

impl RelayFrame {
    pub fn from_transport_frame(frame: RemoteTransportFrame, seq: u64) -> Self {
        Self {
            version: 1,
            message_type: "relay.frame".to_string(),
            id: format!("msg_{seq}"),
            connection_id: frame.connection_id,
            seq,
            ciphertext: STANDARD.encode(frame.payload),
        }
    }

    pub fn into_transport_frame(self) -> Result<RemoteTransportFrame, String> {
        if self.version != 1 || self.message_type != "relay.frame" {
            return Err("relay frame 协议不支持".to_string());
        }
        let payload = STANDARD
            .decode(self.ciphertext)
            .map_err(|error| format!("relay frame payload 解码失败：{error}"))?;
        Ok(RemoteTransportFrame::new(self.connection_id, payload))
    }
}

pub fn device_relay_url(
    server_url: &str,
    connection_id: &str,
    connection_token: &str,
) -> Result<String, String> {
    let trimmed = server_url.trim().trim_end_matches('/');
    let prefix = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        return Err("远程服务地址必须以 http:// 或 https:// 开头".to_string());
    };
    Ok(format!(
        "{prefix}/ws/relay?connection_id={connection_id}&connection_token={connection_token}&side=device"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::transport::RemoteTransportFrame;

    #[test]
    fn derives_device_relay_url() {
        assert_eq!(
            device_relay_url("https://remote.example.com", "conn_1", "cnt_1").unwrap(),
            "wss://remote.example.com/ws/relay?connection_id=conn_1&connection_token=cnt_1&side=device"
        );
    }

    #[test]
    fn encodes_and_decodes_relay_frame_payload() {
        let frame = RemoteTransportFrame::new("conn_1", vec![1, 2, 3]);
        let relay = RelayFrame::from_transport_frame(frame, 7);

        assert_eq!(relay.seq, 7);
        assert_eq!(relay.ciphertext, "AQID");
        assert_eq!(relay.into_transport_frame().unwrap().payload, vec![1, 2, 3]);
    }
}
```

- [ ] **Step 5: Export module and run tests**

Update `crates/niuma-core/src/remote/mod.rs`:

```rust
pub mod agent_state;
pub mod config;
pub mod connection_policy;
pub mod credentials;
pub mod device_identity;
pub mod login_flow;
pub mod relay;
pub mod settings;
pub mod signaling;
pub mod transport;
```

Run:

```bash
cargo test -p niuma-core remote::relay
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-core/Cargo.toml crates/niuma-core/src/remote/relay.rs crates/niuma-core/src/remote/mod.rs
git commit -m "feat: 新增远程 relay 帧契约" -m "修改内容：新增 relay URL 派生、relay.frame 模型和 RemoteTransportFrame 编解码。" -m "修改原因：WebRTC 不可用时本机需要通过服务端 relay 转发端到端加密帧。"
```

## Task 2: Device-Side Relay Transport Adapter

**Files:**
- Create: `src-tauri/src/remote/relay_transport.rs`
- Modify: `src-tauri/src/remote/mod.rs`
- Test: `src-tauri/src/remote/relay_transport.rs`

- [ ] **Step 1: Write failing relay adapter tests**

Create `src-tauri/src/remote/relay_transport.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_counter_starts_at_one() {
        let counter = RelaySequence::default();
        assert_eq!(counter.next_seq(), 1);
        assert_eq!(counter.next_seq(), 2);
    }

    #[test]
    fn build_connect_request_hides_token_from_debug() {
        let request = RelayConnectRequest {
            server_url: "https://remote.example.com".to_string(),
            connection_id: "conn_1".to_string(),
            connection_token: "cnt_secret".to_string(),
        };

        assert!(!format!("{request:?}").contains("cnt_secret"));
        assert_eq!(
            request.relay_url().unwrap(),
            "wss://remote.example.com/ws/relay?connection_id=conn_1&connection_token=cnt_secret&side=device"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::relay_transport
```

Expected: FAIL because relay transport adapter does not exist.

- [ ] **Step 3: Implement relay connect request and sequence**

Replace `src-tauri/src/remote/relay_transport.rs` with:

```rust
use futures_util::{SinkExt, StreamExt};
use niuma_core::remote::relay::{device_relay_url, RelayFrame};
use niuma_core::remote::transport::RemoteTransportFrame;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Default)]
pub struct RelaySequence {
    next: AtomicU64,
}

impl RelaySequence {
    pub fn next_seq(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst) + 1
    }
}

#[derive(Clone)]
pub struct RelayConnectRequest {
    pub server_url: String,
    pub connection_id: String,
    pub connection_token: String,
}

impl fmt::Debug for RelayConnectRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayConnectRequest")
            .field("server_url", &self.server_url)
            .field("connection_id", &self.connection_id)
            .field("connection_token", &"<redacted>")
            .finish()
    }
}

impl RelayConnectRequest {
    pub fn relay_url(&self) -> Result<String, String> {
        device_relay_url(&self.server_url, &self.connection_id, &self.connection_token)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayRunResult {
    Closed,
    Failed(String),
}

pub async fn run_relay_transport(
    request: RelayConnectRequest,
    mut outbound_rx: mpsc::UnboundedReceiver<RemoteTransportFrame>,
    inbound_tx: mpsc::UnboundedSender<RemoteTransportFrame>,
) -> RelayRunResult {
    let relay_url = match request.relay_url() {
        Ok(value) => value,
        Err(error) => return RelayRunResult::Failed(error),
    };
    let (stream, _) = match connect_async(relay_url).await {
        Ok(value) => value,
        Err(error) => return RelayRunResult::Failed(format!("连接 relay 失败：{error}")),
    };
    let (mut writer, mut reader) = stream.split();
    let sequence = RelaySequence::default();

    loop {
        tokio::select! {
            outbound = outbound_rx.recv() => {
                let Some(frame) = outbound else {
                    return RelayRunResult::Closed;
                };
                let relay = RelayFrame::from_transport_frame(frame, sequence.next_seq());
                let text = match serde_json::to_string(&relay) {
                    Ok(value) => value,
                    Err(error) => return RelayRunResult::Failed(format!("序列化 relay frame 失败：{error}")),
                };
                if let Err(error) = writer.send(Message::Text(text)).await {
                    return RelayRunResult::Failed(format!("发送 relay frame 失败：{error}"));
                }
            }
            incoming = reader.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let relay: RelayFrame = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(error) => return RelayRunResult::Failed(format!("解析 relay frame 失败：{error}")),
                        };
                        let frame = match relay.into_transport_frame() {
                            Ok(value) => value,
                            Err(error) => return RelayRunResult::Failed(error),
                        };
                        if inbound_tx.send(frame).is_err() {
                            return RelayRunResult::Closed;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return RelayRunResult::Closed,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return RelayRunResult::Failed(format!("读取 relay 失败：{error}")),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_counter_starts_at_one() {
        let counter = RelaySequence::default();
        assert_eq!(counter.next_seq(), 1);
        assert_eq!(counter.next_seq(), 2);
    }

    #[test]
    fn build_connect_request_hides_token_from_debug() {
        let request = RelayConnectRequest {
            server_url: "https://remote.example.com".to_string(),
            connection_id: "conn_1".to_string(),
            connection_token: "cnt_secret".to_string(),
        };

        assert!(!format!("{request:?}").contains("cnt_secret"));
        assert_eq!(
            request.relay_url().unwrap(),
            "wss://remote.example.com/ws/relay?connection_id=conn_1&connection_token=cnt_secret&side=device"
        );
    }
}
```

Update `src-tauri/src/remote/mod.rs`:

```rust
pub mod agent;
pub mod commands;
pub mod device_socket;
pub mod login_flow;
pub mod relay_transport;
pub mod signaling;
pub mod status;
pub mod webrtc_transport;
```

- [ ] **Step 4: Run relay adapter tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::relay_transport
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/remote/relay_transport.rs src-tauri/src/remote/mod.rs
git commit -m "feat: 新增本机 relay 传输适配器" -m "修改内容：新增 device 侧 relay 连接请求、序列号、密文帧发送和接收转换。" -m "修改原因：WebRTC 直连失败时本机需要通过服务端 relay 继续传输 E2EE 密文帧。"
```

## Task 3: Connection Token In Local Signaling Session

**Files:**
- Modify: `crates/niuma-core/src/remote/signaling.rs`
- Modify: `src-tauri/src/remote/signaling.rs`
- Test: `crates/niuma-core/src/remote/signaling.rs`
- Test: `src-tauri/src/remote/signaling.rs`

- [ ] **Step 1: Write failing connection token tests**

Update `crates/niuma-core/src/remote/signaling.rs` test `parses_connection_invite` so `data` includes:

```rust
"connection_token": "cnt_12345678901234567890123456789012",
```

Add assertion:

```rust
match message {
    DeviceSignalMessage::ConnectionInvite { data, .. } => {
        assert_eq!(data.connection_token, "cnt_12345678901234567890123456789012");
    }
    _ => panic!("expected invite"),
}
```

- [ ] **Step 2: Run core signaling test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::signaling
```

Expected: FAIL because `ConnectionInvite` does not include `connection_token`.

- [ ] **Step 3: Add connection token to invite and session**

Update `ConnectionInvite` in `crates/niuma-core/src/remote/signaling.rs`:

```rust
pub struct ConnectionInvite {
    pub connection_id: String,
    pub connection_token: String,
    pub client_id: String,
    pub client_label: Option<String>,
    pub transport_preference: TransportPreference,
    pub expires_at: String,
}
```

Update all tests and sample invites to include a realistic `cnt_...` value.

Update `RemoteSignalingSession` in `src-tauri/src/remote/signaling.rs`:

```rust
pub struct RemoteSignalingSession {
    pub connection_id: String,
    pub connection_token: String,
    pub client_id: String,
    pub transport: TransportPreference,
}
```

When inserting a session:

```rust
connection_token: invite.connection_token.clone(),
```

Add helper:

```rust
pub fn session(&self, connection_id: &str) -> Option<RemoteSignalingSession> {
    self.sessions
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(connection_id).cloned())
}
```

- [ ] **Step 4: Run signaling tests**

Run:

```bash
cargo test -p niuma-core remote::signaling
cargo test --manifest-path src-tauri/Cargo.toml remote::signaling
```

Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/niuma-core/src/remote/signaling.rs src-tauri/src/remote/signaling.rs
git commit -m "feat: 本机信令会话保存连接令牌" -m "修改内容：连接邀请模型和本机会话状态新增 connection_token，并提供按 connection_id 读取会话的 helper。" -m "修改原因：本机作为 device 侧连接 /ws/relay 时需要使用服务端签发的短期 connection token。"
```

## Task 4: Relay Fallback Entry Point

**Files:**
- Modify: `src-tauri/src/remote/signaling.rs`
- Modify: `src-tauri/src/remote/status.rs`
- Test: `src-tauri/src/remote/signaling.rs`

- [ ] **Step 1: Write failing relay fallback tests**

Add tests to `src-tauri/src/remote/signaling.rs`:

```rust
#[cfg(test)]
mod relay_fallback_tests {
    use super::*;
    use niuma_core::remote::config::RemoteConfig;
    use niuma_core::remote::signaling::{ConnectionInvite, TransportPreference};

    #[test]
    fn relay_requested_invite_is_accepted_when_control_enabled() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");

        let outbound = manager.handle_invite(&config, ConnectionInvite {
            connection_id: "conn_1".to_string(),
            connection_token: "cnt_12345678901234567890123456789012".to_string(),
            client_id: "web_1".to_string(),
            client_label: Some("Chrome".to_string()),
            transport_preference: TransportPreference::Relay,
            expires_at: "2026-06-28T00:02:00.000Z".to_string(),
        });

        assert_eq!(outbound[0]["type"], "connection.accept");
        assert_eq!(outbound[0]["data"]["transport"], "relay");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::signaling::relay_fallback_tests
```

Expected: FAIL because relay invites are currently rejected as unsupported.

- [ ] **Step 3: Accept relay invite and update status**

Update transport choice in `RemoteSignalingManager::handle_invite`:

```rust
let transport = match invite.transport_preference {
    TransportPreference::Webrtc | TransportPreference::Auto => TransportPreference::Webrtc,
    TransportPreference::Relay => TransportPreference::Relay,
};
```

After accepting a relay invite:

```rust
if let Some(status) = &self.status {
    if transport == TransportPreference::Relay {
        status.set_selected_transport(Some(niuma_core::remote::transport::RemoteTransportKind::Relay));
    }
}
```

- [ ] **Step 4: Add relay fallback starter**

Add to `src-tauri/src/remote/signaling.rs`:

```rust
pub fn relay_request_for_session(
    &self,
    server_url: &str,
    connection_id: &str,
) -> Result<crate::remote::relay_transport::RelayConnectRequest, String> {
    let session = self
        .session(connection_id)
        .ok_or_else(|| "远程连接会话不存在".to_string())?;
    Ok(crate::remote::relay_transport::RelayConnectRequest {
        server_url: server_url.to_string(),
        connection_id: session.connection_id,
        connection_token: session.connection_token,
    })
}
```

This function builds the device-side relay connect request for explicit relay invites. WebRTC timeout based automatic fallback is owned by a separate transport-selection milestone because it depends on WebRTC failure detection.

- [ ] **Step 5: Run signaling tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::signaling
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/remote/signaling.rs src-tauri/src/remote/status.rs
git commit -m "feat: 新增本机 relay fallback 入口" -m "修改内容：本机信令管理器支持 relay 邀请、保存 relay 选中状态并可生成 device 侧 relay 连接请求。" -m "修改原因：当 WebRTC 不可用或外部客户端请求 relay 时，本机需要进入服务端密文转发通道。"
```

## Task 5: Relay Transport Status And Verification

**Files:**
- Modify: `src/i18n.ts`
- Modify: `src/settingsView.ts`
- Test: `tests/remoteSettingsView.test.ts`

- [ ] **Step 1: Write failing remote settings render test**

Update `tests/remoteSettingsView.test.ts` to pass agent status:

```ts
agentStatus: {
  state: 'online',
  selected_transport: 'relay',
  active_connection_id: 'conn_1',
  last_error: null
}
```

Add assertion:

```ts
if (!html.includes('relay')) {
  throw new Error('远程设置页应显示当前使用 relay 传输')
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run test:remote-settings-view
```

Expected: FAIL because settings panel does not render selected relay transport.

- [ ] **Step 3: Render selected transport**

Update `src/settingsView.ts` remote status summary:

```ts
${options.agentStatus?.selected_transport ? `<dt>${escapeHtml(t.remoteSelectedTransport)}</dt><dd>${escapeHtml(options.agentStatus.selected_transport)}</dd>` : ''}
```

Update `Translation` in `src/i18n.ts`:

```ts
remoteSelectedTransport: string
```

Add translations:

```ts
// zh-CN
remoteSelectedTransport: '当前传输',
// zh-TW
remoteSelectedTransport: '目前傳輸',
// en
remoteSelectedTransport: 'Current transport',
// ja
remoteSelectedTransport: '現在の転送方式',
// ko
remoteSelectedTransport: '현재 전송 방식',
// de
remoteSelectedTransport: 'Aktueller Transport',
```

- [ ] **Step 4: Run frontend test**

Run:

```bash
npm run test:remote-settings-view
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/i18n.ts src/settingsView.ts tests/remoteSettingsView.test.ts
git commit -m "feat: 设置页显示 relay 传输状态" -m "修改内容：远程设置页展示当前选中的传输方式，并补齐六语言文案。" -m "修改原因：用户需要区分当前远程连接是 WebRTC 直连还是 relay fallback。"
```

## Task 6: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-5.

- [ ] **Step 1: Run core relay tests**

Run:

```bash
cargo test -p niuma-core remote::relay remote::signaling
```

Expected: PASS.

- [ ] **Step 2: Run Tauri relay tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::relay_transport remote::signaling
```

Expected: PASS.

- [ ] **Step 3: Run frontend settings test**

Run:

```bash
npm run test:remote-settings-view
```

Expected: PASS.

- [ ] **Step 4: Verify RPC plaintext is not introduced**

Run:

```bash
rg -n "RemoteRpcRouter|session.send_instruction|interaction.decide_approval|StateMutationService" src-tauri/src/remote crates/niuma-core/src/remote
```

Expected: no output for this milestone.

- [ ] **Step 5: Verify relay logs do not include ciphertext or token**

Run:

```bash
rg -n "ciphertext|connection_token|device_token" src-tauri/src/remote/relay_transport.rs crates/niuma-core/src/remote/relay.rs
```

Expected: references are limited to frame fields, URL construction, and redacted debug implementation. No `eprintln!`, `println!`, or log formatting should include token or ciphertext values.

- [ ] **Step 6: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

## Self-Review

Spec coverage in this plan:

- WebRTC fallback through relay: covered by Tasks 2 and 4.
- Relay forwards encrypted frames only: covered by Task 1 and Task 2.
- Service-side token remains opaque and redacted in debug: covered by Task 2 and Task 6 scan.
- Settings page distinguishes WebRTC and relay: covered by Task 5.
- RPC plaintext and state mutation remain untouched: covered by Task 6 scan.

Next milestone candidates:

- Browser-side relay fallback client.
- Transport selection timeout that starts relay after WebRTC fails.
- E2EE handshake over WebRTC or relay transport.
