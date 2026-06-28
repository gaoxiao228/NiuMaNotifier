use niuma_core::remote::transport::RemoteTransportFrame;
use niuma_core::remote::signaling::{
    signal_answer_message, signal_ice_candidate_message, SignalIceCandidate, SignalOffer,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
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
        sdp_mline_index: candidate.sdp_mline_index.and_then(|value| value.try_into().ok()),
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

pub async fn send_data_channel_frame(
    data_channel: &RTCDataChannel,
    frame: RemoteTransportFrame,
) -> Result<(), String> {
    data_channel
        .send(&frame.payload.into())
        .await
        .map(|_| ())
        .map_err(|error| format!("发送 WebRTC DataChannel 帧失败：{error}"))
}

pub fn data_channel_message_to_frame(
    connection_id: &str,
    message: DataChannelMessage,
) -> Result<RemoteTransportFrame, String> {
    validate_outbound_frame(connection_id, message.data.to_vec())
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
