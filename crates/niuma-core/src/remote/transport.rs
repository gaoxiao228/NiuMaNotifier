use super::e2ee::RpcFrame;
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

pub trait RemoteEncryptedTransport {
    /// 传输层只处理密文帧，不接触 RPC 明文。
    fn send_frame(&self, frame: RpcFrame);

    fn close(&self, reason: &str);
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
