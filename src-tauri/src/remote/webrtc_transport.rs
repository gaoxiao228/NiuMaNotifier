use niuma_core::remote::signaling::{
    signal_answer_message, signal_ice_candidate_message, SignalIceCandidate, SignalOffer,
};
use niuma_core::remote::transport::RemoteTransportFrame;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

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

pub struct WebRtcAnswerResult {
    pub answer_message: Value,
    pub ice_rx: mpsc::UnboundedReceiver<Value>,
    pub session: WebRtcSession,
}

#[derive(Clone)]
pub struct WebRtcSession {
    pub peer_connection: Arc<RTCPeerConnection>,
}

pub async fn create_webrtc_answer(
    connection_id: &str,
    offer: SignalOffer,
    config: WebRtcTransportConfig,
    rpc_context: crate::remote::rpc_router::RemoteRpcContext,
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
                if let Ok(json) = candidate.to_json() {
                    let _ = ice_tx.send(signal_ice_candidate_message(
                        &ice_connection_id,
                        &json.candidate,
                        json.sdp_mid.as_deref(),
                        json.sdp_mline_index.map(u32::from),
                    ));
                }
            }
        })
    }));
    let rpc_context_for_channel = rpc_context.clone();
    peer.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
        let rpc_context = rpc_context_for_channel.clone();
        Box::pin(async move {
            if channel.label() == WEBRTC_DATA_CHANNEL_LABEL {
                attach_data_channel_rpc(channel, rpc_context);
            }
        })
    }));
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
        session: WebRtcSession {
            peer_connection: peer,
        },
    })
}

pub async fn add_remote_ice_candidate(
    peer: &webrtc::peer_connection::RTCPeerConnection,
    candidate: SignalIceCandidate,
) -> Result<(), String> {
    peer.add_ice_candidate(RTCIceCandidateInit {
        candidate: candidate.candidate,
        sdp_mid: candidate.sdp_mid,
        sdp_mline_index: candidate
            .sdp_mline_index
            .and_then(|value| value.try_into().ok()),
        username_fragment: None,
    })
    .await
    .map_err(|error| format!("添加远程 ICE candidate 失败：{error}"))
}

pub fn validate_outbound_frame(
    connection_id: &str,
    payload: Vec<u8>,
) -> Result<RemoteTransportFrame, String> {
    if payload.is_empty() {
        return Err("远程传输帧不能为空".to_string());
    }
    Ok(RemoteTransportFrame::new(connection_id, payload))
}

pub fn attach_data_channel_rpc(
    data_channel: Arc<RTCDataChannel>,
    rpc_context: crate::remote::rpc_router::RemoteRpcContext,
) {
    let (notification_tx, mut notification_rx) = mpsc::unbounded_channel();
    let rpc_context = rpc_context.with_notification_sender(notification_tx);
    let notification_channel = Arc::clone(&data_channel);

    tokio::spawn(async move {
        while let Some(notification) = notification_rx.recv().await {
            let bytes = match serde_json::to_vec(&mark_webrtc_transport(notification)) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("NiumaNotifier WebRTC notification encode failed: {error}");
                    continue;
                }
            };
            let text = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(error) => {
                    eprintln!("NiumaNotifier WebRTC notification utf8 encode failed: {error}");
                    continue;
                }
            };
            if let Err(error) = notification_channel.send_text(text).await {
                eprintln!("NiumaNotifier WebRTC notification send failed: {error}");
                break;
            }
        }
    });

    let response_channel = Arc::clone(&data_channel);
    data_channel.on_message(Box::new(move |message: DataChannelMessage| {
        let rpc_context = rpc_context.clone();
        let response_channel = Arc::clone(&response_channel);
        Box::pin(async move {
            match handle_webrtc_payload_async(&message.data, &rpc_context).await {
                Ok(Some(response)) => {
                    let text = match String::from_utf8(response) {
                        Ok(text) => text,
                        Err(error) => {
                            eprintln!("NiumaNotifier WebRTC response utf8 encode failed: {error}");
                            return;
                        }
                    };
                    if let Err(error) = response_channel.send_text(text).await {
                        eprintln!("NiumaNotifier WebRTC response send failed: {error}");
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("NiumaNotifier WebRTC payload handling failed: {error}");
                }
            }
        })
    }));
}

fn is_plain_rpc_request_payload(payload: &Value) -> bool {
    // WebRTC 和 relay 共享 plain RPC envelope；字段完整性继续交给 rpc_router 校验。
    payload.get("version").and_then(Value::as_u64) == Some(1)
        && payload.get("type").and_then(Value::as_str) == Some("request")
}

fn mark_webrtc_transport(mut value: Value) -> Value {
    if let Some(item) = value.as_object_mut() {
        item.insert("transport".to_string(), json!({ "kind": "webrtc" }));
    }
    value
}

pub async fn handle_webrtc_payload_async(
    payload: &[u8],
    rpc_context: &crate::remote::rpc_router::RemoteRpcContext,
) -> Result<Option<Vec<u8>>, String> {
    let value: Value = serde_json::from_slice(payload)
        .map_err(|error| format!("WebRTC payload JSON 解析失败：{error}"))?;

    if value.get("type").and_then(Value::as_str) == Some("ping") {
        return serde_json::to_vec(&json!({ "type": "pong" }))
            .map(Some)
            .map_err(|error| format!("WebRTC pong JSON 编码失败：{error}"));
    }

    if is_plain_rpc_request_payload(&value) {
        let response =
            crate::remote::rpc_router::handle_plain_rpc_with_context_async(value, rpc_context)
                .await?;
        return serde_json::to_vec(&mark_webrtc_transport(response))
            .map(Some)
            .map_err(|error| format!("WebRTC RPC response JSON 编码失败：{error}"));
    }

    Ok(None)
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

#[cfg(test)]
mod frame_tests {
    use super::*;
    use crate::remote::rpc_router::RemoteRpcContext;
    use niuma_api::tool_sessions::ToolSessionRegistry;
    use niuma_core::store::NiumaStore;

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

    #[tokio::test]
    async fn handles_plain_rpc_payload_with_webrtc_transport_marker() {
        let context = test_rpc_context("webrtc-transport-rpc-ping");
        let request = serde_json::json!({
            "version": 1,
            "type": "request",
            "transport": { "kind": "webrtc" },
            "id": "req_1",
            "method": "rpc.ping",
            "params": {}
        });

        let response = handle_webrtc_payload_async(request.to_string().as_bytes(), &context)
            .await
            .unwrap()
            .unwrap();
        let response: serde_json::Value = serde_json::from_slice(&response).unwrap();

        assert_eq!(
            response,
            serde_json::json!({
                "version": 1,
                "type": "response",
                "transport": { "kind": "webrtc" },
                "id": "req_1",
                "ok": true,
                "result": { "pong": true }
            })
        );
    }

    fn test_rpc_context(name: &str) -> RemoteRpcContext {
        let path = std::env::temp_dir().join(format!("{name}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        RemoteRpcContext::new(NiumaStore::new(path), ToolSessionRegistry::new())
    }
}
