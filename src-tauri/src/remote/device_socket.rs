use futures_util::{SinkExt, StreamExt};
use http::header::AUTHORIZATION;
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::connection_policy::{
    classify_device_socket_close, device_socket_url, DeviceSocketCloseReason,
};
use niuma_core::remote::signaling::{parse_device_signal_message, DeviceSignalMessage};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DeviceSocketConnectRequest {
    pub server_url: String,
    pub device_id: String,
    pub device_token: String,
    pub heartbeat_interval_seconds: u64,
    pub remote_config: RemoteConfig,
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

pub fn build_device_socket_upgrade_request(
    socket_url: &str,
    device_token: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, String> {
    let mut request = socket_url
        .into_client_request()
        .map_err(|error| format!("构造远程 WebSocket 握手请求失败：{error}"))?;
    request.headers_mut().insert(
        AUTHORIZATION,
        device_authorization_header(device_token)
            .parse()
            .map_err(|error| format!("构造远程授权头失败：{error}"))?,
    );
    Ok(request)
}

pub async fn run_device_socket_once(
    request: DeviceSocketConnectRequest,
    signaling_manager: crate::remote::signaling::RemoteSignalingManager,
    webrtc_config: crate::remote::webrtc_transport::WebRtcTransportConfig,
) -> DeviceSocketRunResult {
    let socket_url = match request.socket_url() {
        Ok(value) => value,
        Err(error) => return DeviceSocketRunResult::Failed(error),
    };
    let upgrade_request =
        match build_device_socket_upgrade_request(&socket_url, &request.device_token) {
            Ok(value) => value,
            Err(error) => {
                return DeviceSocketRunResult::Failed(format!("构造远程连接请求失败：{error}"));
            }
        };
    let (stream, _) = match connect_async(upgrade_request).await {
        Ok(value) => value,
        Err(error) => return DeviceSocketRunResult::Failed(format!("远程设备连接失败：{error}")),
    };
    let (mut writer, mut reader) = stream.split();
    if let Err(error) = writer
        .send(Message::Text(
            device_hello_message(&request.device_id).to_string(),
        ))
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
                    Some(Ok(Message::Text(text))) => {
                        let message = match parse_device_text_message(text) {
                            Ok(Some(value)) => value,
                            Ok(None) => continue,
                            Err(error) => return DeviceSocketRunResult::Failed(error),
                        };
                        let outbound = match message {
                            DeviceSignalMessage::SignalOffer { data, .. } => {
                                signaling_manager
                                    .handle_offer_async(
                                        &request.remote_config,
                                        data,
                                        webrtc_config.clone(),
                                    )
                                    .await
                            }
                            other => signaling_manager.handle_message(&request.remote_config, other),
                        };
                        for message in outbound {
                            if let Err(error) = writer.send(Message::Text(message.to_string())).await {
                                return DeviceSocketRunResult::Failed(format!("发送远程信令响应失败：{error}"));
                            }
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

pub fn parse_device_text_message(text: String) -> Result<Option<DeviceSignalMessage>, String> {
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("远程设备消息 JSON 解析失败：{error}"))?;
    match parse_device_signal_message(value) {
        Ok(message) => Ok(Some(message)),
        Err(error) => {
            // 非信令文本消息留给后续协议扩展；当前连接循环只消费已知信令。
            if error.starts_with("未知远程信令消息类型") {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

pub fn dispatch_device_text_message(
    config: &RemoteConfig,
    text: String,
    mut handler: impl FnMut(&RemoteConfig, DeviceSignalMessage) -> Vec<Value>,
) -> Result<Vec<Value>, String> {
    Ok(parse_device_text_message(text)?
        .map(|message| handler(config, message))
        .unwrap_or_default())
}

pub fn is_async_webrtc_offer(message: &DeviceSignalMessage) -> bool {
    matches!(message, DeviceSignalMessage::SignalOffer { .. })
}

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
    fn builds_websocket_upgrade_request_with_required_headers() {
        let request =
            build_device_socket_upgrade_request("ws://127.0.0.1:27880/ws/device", "dvt_secret")
                .unwrap();

        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Device dvt_secret"
        );
        assert!(request.headers().contains_key("sec-websocket-key"));
        assert_eq!(request.headers().get("upgrade").unwrap(), "websocket");
    }

    #[test]
    fn token_is_not_embedded_in_url() {
        let request = DeviceSocketConnectRequest {
            server_url: "https://remote.example.com".to_string(),
            device_id: "dev_1".to_string(),
            device_token: "dvt_secret".to_string(),
            heartbeat_interval_seconds: 20,
            remote_config: niuma_core::remote::config::RemoteConfig::default_for_server(
                "https://remote.example.com",
            ),
        };

        assert_eq!(
            request.socket_url().unwrap(),
            "wss://remote.example.com/ws/device"
        );
        assert!(!request.socket_url().unwrap().contains("dvt_secret"));
    }
}

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
