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
        let message =
            connection_reject_message("conn_1", ConnectionRejectReason::RemoteControlDisabled);
        assert_eq!(message["type"], "connection.reject");
        assert_eq!(message["data"]["connection_id"], "conn_1");
        assert_eq!(message["data"]["reason"], "remote_control_disabled");
    }
}
