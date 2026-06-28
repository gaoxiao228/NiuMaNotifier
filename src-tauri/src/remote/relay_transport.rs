use futures_util::{SinkExt, StreamExt};
use http::Request;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::connect_async;
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

pub fn relay_socket_url(
    server_url: &str,
    connection_id: &str,
    connection_token: &str,
    side: RelaySide,
) -> Result<String, String> {
    let mut url =
        reqwest::Url::parse(server_url).map_err(|error| format!("远程服务地址格式错误：{error}"))?;

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

pub fn handle_relay_text_frame(text: String) -> Result<Option<String>, String> {
    let frame: RelayFrame =
        serde_json::from_str(&text).map_err(|error| format!("relay 帧 JSON 解析失败：{error}"))?;
    if frame.version != 1 || frame.frame_type != "relay.frame" {
        return Err("relay 帧格式无效".to_string());
    }

    let Some(ciphertext) = crate::remote::relay_runtime::handle_relay_ciphertext(&frame.ciphertext)?
    else {
        return Ok(None);
    };
    let response = crate::remote::relay_runtime::build_relay_response_frame(&frame, ciphertext);
    serde_json::to_string(&response)
        .map(Some)
        .map_err(|error| format!("relay 响应帧 JSON 编码失败：{error}"))
}

#[allow(dead_code)]
pub async fn run_device_relay_once(
    server_url: &str,
    connection_id: &str,
    connection_token: &str,
    device_token: &str,
) -> Result<(), String> {
    let socket_url = relay_socket_url(
        server_url,
        connection_id,
        connection_token,
        RelaySide::Device,
    )?;
    let request = Request::builder()
        .uri(&socket_url)
        .header(
            "Authorization",
            relay_device_authorization_header(device_token),
        )
        .body(())
        .map_err(|error| format!("构造 relay 连接请求失败：{error}"))?;
    let (stream, _) = connect_async(request)
        .await
        .map_err(|error| format!("relay 设备连接失败：{error}"))?;
    let (mut writer, mut reader) = stream.split();

    while let Some(next) = reader.next().await {
        match next {
            Ok(Message::Text(text)) => {
                if let Some(response) = handle_relay_text_frame(text.to_string())? {
                    writer
                        .send(Message::Text(response))
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
