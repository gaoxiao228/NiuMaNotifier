use crate::remote::rpc_router::RemoteRpcContext;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use http::header::AUTHORIZATION;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelaySide {
    Client,
    Device,
}

impl RelaySide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Device => "device",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayFrame {
    pub version: u32,
    #[serde(rename = "type")]
    pub frame_type: String,
    pub id: String,
    pub connection_id: String,
    pub seq: u64,
    pub ciphertext: String,
}

#[derive(Debug, Clone)]
struct RelayOutboundSeq {
    next: u64,
}

impl RelayOutboundSeq {
    fn new() -> Self {
        Self { next: 1 }
    }

    fn allocate(&mut self) -> u64 {
        let seq = self.next;
        self.next += 1;
        seq
    }
}

pub fn relay_socket_url(
    server_url: &str,
    connection_id: &str,
    connection_token: &str,
    side: RelaySide,
) -> Result<String, String> {
    let mut url = reqwest::Url::parse(server_url)
        .map_err(|error| format!("远程服务地址格式错误：{error}"))?;

    let socket_scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        _ => return Err("远程 relay 仅支持 http/https 服务地址".to_string()),
    };
    url.set_scheme(socket_scheme)
        .map_err(|_| "远程 relay 服务地址 scheme 无法转换".to_string())?;

    // Relay bind 只使用连接级 token，不把设备 token 或原 URL 查询参数带到 WebSocket URL。
    url.set_path("/ws/relay");
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("connection_id", connection_id)
        .append_pair("connection_token", connection_token)
        .append_pair("side", side.as_str());

    Ok(url.to_string())
}

pub fn relay_device_authorization_header(device_token: &str) -> String {
    format!("Device {device_token}")
}

pub fn build_relay_socket_upgrade_request(
    socket_url: &str,
    device_token: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, String> {
    let mut request = socket_url
        .into_client_request()
        .map_err(|error| format!("构造 relay WebSocket 握手请求失败：{error}"))?;
    request.headers_mut().insert(
        AUTHORIZATION,
        relay_device_authorization_header(device_token)
            .parse()
            .map_err(|error| format!("构造 relay 授权头失败：{error}"))?,
    );
    Ok(request)
}

pub fn handle_relay_text_frame(
    text: String,
    rpc_context: &RemoteRpcContext,
) -> Result<Option<String>, String> {
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("relay 帧 JSON 解析失败：{error}"))?;
    if is_relay_ready_control_message(&value) {
        return Ok(None);
    }
    let frame: RelayFrame = serde_json::from_value(value)
        .map_err(|error| format!("relay 帧 JSON 解析失败：{error}"))?;
    if frame.version != 1 || frame.frame_type != "relay.frame" {
        return Err("relay 帧格式无效".to_string());
    }

    let Some(ciphertext) =
        crate::remote::relay_runtime::handle_relay_ciphertext(&frame.ciphertext, rpc_context)?
    else {
        return Ok(None);
    };
    let response = crate::remote::relay_runtime::build_relay_response_frame(&frame, ciphertext);
    serde_json::to_string(&response)
        .map(Some)
        .map_err(|error| format!("relay 响应帧 JSON 编码失败：{error}"))
}

fn is_relay_ready_control_message(value: &Value) -> bool {
    // relay.ready 是服务端控制消息，只表示 client/device 两侧 relay 均已绑定。
    value.get("version").and_then(Value::as_u64) == Some(1)
        && value.get("type").and_then(Value::as_str) == Some("relay.ready")
        && value.get("connection_id").and_then(Value::as_str).is_some()
}

pub async fn run_device_relay_once(
    server_url: &str,
    connection_id: &str,
    connection_token: &str,
    device_token: &str,
    rpc_context: RemoteRpcContext,
) -> Result<(), String> {
    let socket_url = relay_socket_url(
        server_url,
        connection_id,
        connection_token,
        RelaySide::Device,
    )?;
    let request = build_relay_socket_upgrade_request(&socket_url, device_token)?;
    let (stream, _) = connect_async(request)
        .await
        .map_err(|error| format!("relay 设备连接失败：{error}"))?;
    let (mut writer, mut reader) = stream.split();
    let (notification_tx, mut notification_rx) = tokio::sync::mpsc::unbounded_channel();
    let rpc_context = rpc_context.with_notification_sender(notification_tx);
    let mut outbound_seq = RelayOutboundSeq::new();

    loop {
        tokio::select! {
            next = reader.next() => {
                let Some(next) = next else {
                    return Ok(());
                };
                match next {
                    Ok(Message::Text(text)) => {
                        let value: Value = serde_json::from_str(&text)
                            .map_err(|error| format!("relay 帧 JSON 解析失败：{error}"))?;
                        if is_relay_ready_control_message(&value) {
                            continue;
                        }
                        let frame: RelayFrame = serde_json::from_value(value)
                            .map_err(|error| format!("relay 帧 JSON 解析失败：{error}"))?;
                        if frame.version != 1 || frame.frame_type != "relay.frame" {
                            return Err("relay 帧格式无效".to_string());
                        }
                        if let Some(ciphertext) = crate::remote::relay_runtime::handle_relay_ciphertext_async(
                            &frame.ciphertext,
                            &rpc_context,
                        ).await? {
                            // 设备侧 response 和 stream notification 必须共用同一条单调递增序列。
                            let response = crate::remote::relay_runtime::build_relay_response_frame_with_seq(
                                &frame,
                                outbound_seq.allocate(),
                                ciphertext,
                            );
                            let response_text = serde_json::to_string(&response)
                                .map_err(|error| format!("relay 响应帧 JSON 编码失败：{error}"))?;
                            writer
                                .send(Message::Text(response_text))
                                .await
                                .map_err(|error| format!("发送 relay 响应失败：{error}"))?;
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        writer
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|error| format!("回复 relay ping 失败：{error}"))?;
                    }
                    Ok(Message::Close(_)) => return Ok(()),
                    Ok(_) => {}
                    Err(error) => return Err(format!("读取 relay 连接失败：{error}")),
                }
            }
            notification = notification_rx.recv() => {
                let Some(notification) = notification else {
                    continue;
                };
                let bytes = serde_json::to_vec(&notification)
                    .map_err(|error| format!("relay 通知 JSON 编码失败：{error}"))?;
                let ciphertext = base64::engine::general_purpose::STANDARD.encode(bytes);
                let frame = crate::remote::relay_runtime::build_relay_notification_frame(
                    connection_id,
                    outbound_seq.allocate(),
                    ciphertext,
                );
                let text = serde_json::to_string(&frame)
                    .map_err(|error| format!("relay 通知帧 JSON 编码失败：{error}"))?;
                writer
                    .send(Message::Text(text))
                    .await
                    .map_err(|error| format!("发送 relay 通知失败：{error}"))?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_api::tool_sessions::ToolSessionRegistry;
    use niuma_core::store::NiumaStore;

    #[test]
    fn builds_ws_url_from_http_server() {
        let url = relay_socket_url(
            "http://remote.example.com",
            "conn_1",
            "cnt_secret",
            RelaySide::Client,
        )
        .unwrap();

        assert_eq!(
            url,
            "ws://remote.example.com/ws/relay?connection_id=conn_1&connection_token=cnt_secret&side=client"
        );
    }

    #[test]
    fn builds_wss_url_from_https_server_with_trailing_slash() {
        let url = relay_socket_url(
            "https://remote.example.com/",
            "conn_1",
            "cnt_secret",
            RelaySide::Device,
        )
        .unwrap();

        assert_eq!(
            url,
            "wss://remote.example.com/ws/relay?connection_id=conn_1&connection_token=cnt_secret&side=device"
        );
    }

    #[test]
    fn rejects_non_http_server_url() {
        let error = relay_socket_url(
            "file:///tmp/remote",
            "conn_1",
            "cnt_secret",
            RelaySide::Client,
        )
        .unwrap_err();

        assert!(error.contains("仅支持 http/https"));
    }

    #[test]
    fn keeps_only_connection_token_in_query() {
        let url = relay_socket_url(
            "https://remote.example.com/path?device_token=dvt_secret",
            "conn_1",
            "cnt_secret",
            RelaySide::Device,
        )
        .unwrap();

        assert!(url.contains("connection_token=cnt_secret"));
        assert!(url.contains("side=device"));
        assert!(!url.contains("dvt_secret"));
        assert_eq!(RelaySide::Device.as_str(), "device");
    }

    #[test]
    fn relay_frame_matches_server_schema_fields() {
        let frame = RelayFrame {
            version: 1,
            frame_type: "relay.frame".to_string(),
            id: "frame_1".to_string(),
            connection_id: "conn_1".to_string(),
            seq: 1,
            ciphertext: "encrypted".to_string(),
        };

        let value = serde_json::to_value(frame).unwrap();

        assert_eq!(value["version"], 1);
        assert_eq!(value["type"], "relay.frame");
        assert_eq!(value["connection_id"], "conn_1");
        assert_eq!(value["seq"], 1);
        assert_eq!(value["ciphertext"], "encrypted");
    }

    #[test]
    fn dispatches_ping_text_frame_to_response_text_frame() {
        let context = test_rpc_context("relay-transport-ping-frame");
        let response = handle_relay_text_frame(
            serde_json::json!({
                "version": 1,
                "type": "relay.frame",
                "id": "msg_1",
                "connection_id": "conn_1",
                "seq": 1,
                "ciphertext": "eyJ0eXBlIjoicGluZyJ9"
            })
            .to_string(),
            &context,
        )
        .unwrap()
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&response).unwrap();

        assert_eq!(value["type"], "relay.frame");
        assert_eq!(value["connection_id"], "conn_1");
        assert_eq!(value["seq"], 2);
    }

    #[test]
    fn ignores_unknown_runtime_payloads() {
        let context = test_rpc_context("relay-transport-unknown");
        let response = handle_relay_text_frame(
            serde_json::json!({
                "version": 1,
                "type": "relay.frame",
                "id": "msg_1",
                "connection_id": "conn_1",
                "seq": 1,
                "ciphertext": "eyJ0eXBlIjoidW5rbm93biJ9"
            })
            .to_string(),
            &context,
        )
        .unwrap();

        assert_eq!(response, None);
    }

    #[test]
    fn ignores_relay_ready_control_message() {
        let context = test_rpc_context("relay-transport-ready-control");
        let response = handle_relay_text_frame(
            serde_json::json!({
                "version": 1,
                "type": "relay.ready",
                "connection_id": "conn_1"
            })
            .to_string(),
            &context,
        )
        .unwrap();

        assert_eq!(response, None);
    }

    #[test]
    fn builds_device_authorization_header() {
        assert_eq!(
            relay_device_authorization_header("dvt_secret"),
            "Device dvt_secret"
        );
    }

    #[test]
    fn builds_relay_upgrade_request_with_websocket_headers() {
        let request = build_relay_socket_upgrade_request(
            "ws://127.0.0.1:27880/ws/relay?connection_id=conn_1&connection_token=cnt_secret&side=device",
            "dvt_secret",
        )
        .unwrap();

        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Device dvt_secret"
        );
        assert!(request.headers().contains_key("sec-websocket-key"));
        assert_eq!(request.headers().get("upgrade").unwrap(), "websocket");
    }

    #[test]
    fn outbound_sequence_is_monotonic_for_responses_and_notifications() {
        let mut seq = RelayOutboundSeq::new();

        assert_eq!(seq.allocate(), 1);
        assert_eq!(seq.allocate(), 2);
        assert_eq!(seq.allocate(), 3);
    }

    fn test_rpc_context(name: &str) -> RemoteRpcContext {
        let path = std::env::temp_dir().join(format!("{name}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        RemoteRpcContext::new(NiumaStore::new(path), ToolSessionRegistry::new())
    }
}
