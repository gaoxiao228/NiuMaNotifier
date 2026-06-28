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
        .header(
            "Authorization",
            device_authorization_header(&request.device_token),
        )
        .body(())
    {
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
    fn token_is_not_embedded_in_url() {
        let request = DeviceSocketConnectRequest {
            server_url: "https://remote.example.com".to_string(),
            device_id: "dev_1".to_string(),
            device_token: "dvt_secret".to_string(),
            heartbeat_interval_seconds: 20,
        };

        assert_eq!(
            request.socket_url().unwrap(),
            "wss://remote.example.com/ws/device"
        );
        assert!(!request.socket_url().unwrap().contains("dvt_secret"));
    }
}
