use serde::{Deserialize, Serialize};

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
}
