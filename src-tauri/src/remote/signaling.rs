use crate::remote::webrtc_transport::{
    add_remote_ice_candidate, create_webrtc_answer, WebRtcSession, WebRtcTransportConfig,
};
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::signaling::{
    connection_accept_message, connection_reject_message, signal_cancel_message, ConnectionInvite,
    ConnectionRejectReason, DeviceSignalMessage, SignalCancel, SignalIceCandidate, SignalOffer,
    TransportPreference,
};
use niuma_core::remote::transport::RemoteTransportKind;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSignalingSession {
    pub connection_id: String,
    pub client_id: String,
    pub transport: TransportPreference,
}

#[derive(Clone, Default)]
pub struct RemoteSignalingManager {
    sessions: Arc<Mutex<HashMap<String, RemoteSignalingSession>>>,
    webrtc_sessions: Arc<Mutex<HashMap<String, WebRtcSession>>>,
    status: Option<crate::remote::status::RemoteAgentStatusHandle>,
}

impl RemoteSignalingManager {
    pub fn with_status(status: crate::remote::status::RemoteAgentStatusHandle) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            webrtc_sessions: Arc::new(Mutex::new(HashMap::new())),
            status: Some(status),
        }
    }

    pub fn handle_message(
        &self,
        config: &RemoteConfig,
        message: DeviceSignalMessage,
    ) -> Vec<Value> {
        match message {
            DeviceSignalMessage::ConnectionInvite { data, .. } => self.handle_invite(config, data),
            DeviceSignalMessage::SignalOffer { data, .. } => self.handle_offer(data),
            DeviceSignalMessage::SignalIceCandidate { data, .. } => self.handle_ice_candidate(data),
            DeviceSignalMessage::SignalCancel { data, .. } => self.handle_cancel(data),
            DeviceSignalMessage::SignalAnswer { data, .. } => {
                vec![signal_cancel_message(
                    &data.connection_id,
                    "unexpected_answer",
                )]
            }
        }
    }

    pub fn handle_invite(&self, config: &RemoteConfig, invite: ConnectionInvite) -> Vec<Value> {
        if !config.remote_access_enabled {
            return vec![connection_reject_message(
                &invite.connection_id,
                ConnectionRejectReason::RemoteAccessDisabled,
            )];
        }
        if !config.remote_control_enabled {
            return vec![connection_reject_message(
                &invite.connection_id,
                ConnectionRejectReason::RemoteControlDisabled,
            )];
        }
        let transport = match invite.transport_preference {
            TransportPreference::Webrtc | TransportPreference::Auto => TransportPreference::Webrtc,
            // Task 5 只完成 relay bind 握手，后续 ping/pong 和帧收发由 relay_runtime 接入。
            TransportPreference::Relay => TransportPreference::Relay,
        };
        if let Ok(mut sessions) = self.sessions.lock() {
            if !sessions.is_empty() && !sessions.contains_key(&invite.connection_id) {
                let same_client = sessions
                    .values()
                    .all(|session| session.client_id == invite.client_id);
                if same_client {
                    // 浏览器刷新、超时或重连可能让旧连接没有及时收到 cancel；同一 client 的新邀请可替换旧会话。
                    sessions.clear();
                    if let Ok(mut webrtc_sessions) = self.webrtc_sessions.lock() {
                        webrtc_sessions.clear();
                    }
                } else {
                    return vec![connection_reject_message(
                        &invite.connection_id,
                        ConnectionRejectReason::Busy,
                    )];
                }
            }
            sessions.insert(
                invite.connection_id.clone(),
                RemoteSignalingSession {
                    connection_id: invite.connection_id.clone(),
                    client_id: invite.client_id,
                    transport: transport.clone(),
                },
            );
        }
        if let Some(status) = &self.status {
            status.set_active_connection(Some(invite.connection_id.clone()));
            if transport == TransportPreference::Relay {
                status.set_selected_transport(Some(RemoteTransportKind::Relay));
            }
        }
        vec![connection_accept_message(&invite.connection_id, transport)]
    }

    pub fn has_session(&self, connection_id: &str) -> bool {
        self.sessions
            .lock()
            .map(|sessions| sessions.contains_key(connection_id))
            .unwrap_or(false)
    }

    pub fn clear_session(&self, connection_id: &str) {
        let removed = self
            .sessions
            .lock()
            .map(|mut sessions| sessions.remove(connection_id).is_some())
            .unwrap_or(false);
        if !removed {
            return;
        }
        if let Ok(mut sessions) = self.webrtc_sessions.lock() {
            sessions.remove(connection_id);
        }

        if let Some(status) = &self.status {
            let snapshot = status.snapshot();
            if snapshot.active_connection_id.as_deref() == Some(connection_id) {
                status.set_active_connection(None);
                status.set_selected_transport(None);
            }
        }
    }

    pub async fn handle_offer_async(
        &self,
        config: &RemoteConfig,
        offer: SignalOffer,
        webrtc_config: WebRtcTransportConfig,
        rpc_context: crate::remote::rpc_router::RemoteRpcContext,
        signal_outbound: Option<UnboundedSender<Value>>,
    ) -> Vec<Value> {
        if !self.has_session(&offer.connection_id) {
            return vec![signal_cancel_message(
                &offer.connection_id,
                "unknown_connection",
            )];
        }
        if !config.remote_access_enabled || !config.remote_control_enabled {
            return vec![signal_cancel_message(
                &offer.connection_id,
                "remote_control_disabled",
            )];
        }
        let connection_id = offer.connection_id.clone();
        match create_webrtc_answer(&connection_id, offer, webrtc_config, rpc_context).await {
            Ok(mut result) => {
                if let Ok(mut sessions) = self.webrtc_sessions.lock() {
                    sessions.insert(connection_id.clone(), result.session);
                }
                if let Some(status) = &self.status {
                    status.set_selected_transport(Some(RemoteTransportKind::Webrtc));
                }
                let mut outbound = vec![result.answer_message];
                while let Ok(candidate) = result.ice_rx.try_recv() {
                    outbound.push(candidate);
                }
                if let Some(signal_outbound) = signal_outbound {
                    spawn_late_webrtc_ice_forwarder(result.ice_rx, signal_outbound);
                }
                outbound
            }
            Err(error) => vec![signal_cancel_message(
                &connection_id,
                &format!("webrtc_failed:{error}"),
            )],
        }
    }

    fn handle_offer(&self, offer: SignalOffer) -> Vec<Value> {
        if !self.has_session(&offer.connection_id) {
            return vec![signal_cancel_message(
                &offer.connection_id,
                "unknown_connection",
            )];
        }
        vec![]
    }

    fn handle_ice_candidate(&self, candidate: SignalIceCandidate) -> Vec<Value> {
        if !self.has_session(&candidate.connection_id) {
            return vec![signal_cancel_message(
                &candidate.connection_id,
                "unknown_connection",
            )];
        }
        vec![]
    }

    pub async fn handle_ice_candidate_async(&self, candidate: SignalIceCandidate) -> Vec<Value> {
        if !self.has_session(&candidate.connection_id) {
            return vec![signal_cancel_message(
                &candidate.connection_id,
                "unknown_connection",
            )];
        }
        let connection_id = candidate.connection_id.clone();

        let session = self
            .webrtc_sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(&connection_id).cloned());
        let Some(session) = session else {
            return vec![];
        };

        if let Err(error) = add_remote_ice_candidate(&session.peer_connection, candidate).await {
            return vec![signal_cancel_message(
                &connection_id,
                &format!("webrtc_ice_failed:{error}"),
            )];
        }
        vec![]
    }

    fn handle_cancel(&self, cancel: SignalCancel) -> Vec<Value> {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(&cancel.connection_id);
        }
        if let Ok(mut sessions) = self.webrtc_sessions.lock() {
            sessions.remove(&cancel.connection_id);
        }
        if let Some(status) = &self.status {
            status.set_active_connection(None);
        }
        vec![]
    }
}

fn spawn_late_webrtc_ice_forwarder(
    mut ice_rx: UnboundedReceiver<Value>,
    signal_outbound: UnboundedSender<Value>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(candidate) = ice_rx.recv().await {
            if signal_outbound.send(candidate).is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::config::RemoteConfig;
    use niuma_core::remote::signaling::{ConnectionInvite, TransportPreference};

    #[test]
    fn rejects_invite_when_remote_control_disabled() {
        let manager = RemoteSignalingManager::default();
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.remote_control_enabled = false;

        let outbound = manager.handle_invite(&config, sample_invite());

        assert_eq!(outbound[0]["type"], "connection.reject");
        assert_eq!(outbound[0]["data"]["reason"], "remote_control_disabled");
    }

    #[test]
    fn accepts_first_invite_and_tracks_session() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");

        let outbound = manager.handle_invite(&config, sample_invite());

        assert_eq!(outbound[0]["type"], "connection.accept");
        assert!(manager.has_session("conn_1"));
    }

    #[test]
    fn accepts_relay_invite_and_tracks_session() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        let mut invite = sample_invite();
        invite.transport_preference = TransportPreference::Relay;

        let outbound = manager.handle_invite(&config, invite);

        assert_eq!(outbound[0]["type"], "connection.accept");
        assert_eq!(outbound[0]["data"]["transport"], "relay");
        assert!(manager.has_session("conn_1"));
    }

    #[test]
    fn replaces_stale_session_from_same_client() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        let mut next_invite = sample_invite();
        next_invite.connection_id = "conn_2".to_string();

        manager.handle_invite(&config, sample_invite());
        let outbound = manager.handle_invite(&config, next_invite);

        assert_eq!(outbound[0]["type"], "connection.accept");
        assert!(!manager.has_session("conn_1"));
        assert!(manager.has_session("conn_2"));
    }

    #[test]
    fn rejects_parallel_invite_from_different_client() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        let mut next_invite = sample_invite();
        next_invite.connection_id = "conn_2".to_string();
        next_invite.client_id = "other_web".to_string();

        manager.handle_invite(&config, sample_invite());
        let outbound = manager.handle_invite(&config, next_invite);

        assert_eq!(outbound[0]["type"], "connection.reject");
        assert_eq!(outbound[0]["data"]["reason"], "busy");
        assert!(manager.has_session("conn_1"));
    }

    #[test]
    fn clears_session_when_transport_ends() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        let mut next_invite = sample_invite();
        next_invite.connection_id = "conn_2".to_string();
        next_invite.client_id = "other_web".to_string();

        manager.handle_invite(&config, sample_invite());
        manager.clear_session("conn_1");
        let outbound = manager.handle_invite(&config, next_invite);

        assert_eq!(outbound[0]["type"], "connection.accept");
        assert!(!manager.has_session("conn_1"));
        assert!(manager.has_session("conn_2"));
    }

    fn sample_invite() -> ConnectionInvite {
        ConnectionInvite {
            connection_id: "conn_1".to_string(),
            connection_token: Some("cnt_relay_secret".to_string()),
            client_id: "web_1".to_string(),
            client_label: Some("Chrome".to_string()),
            transport_preference: TransportPreference::Webrtc,
            expires_at: "2026-06-28T00:02:00.000Z".to_string(),
        }
    }
}

#[cfg(test)]
mod webrtc_policy_tests {
    use super::*;
    use niuma_core::remote::config::RemoteConfig;
    use niuma_core::remote::signaling::{DeviceSignalMessage, SignalOffer};
    use tokio::sync::mpsc;

    #[test]
    fn offer_without_session_is_cancelled() {
        let manager = RemoteSignalingManager::default();
        let config = RemoteConfig::default_for_server("https://remote.example.com");

        let outbound = manager.handle_message(
            &config,
            DeviceSignalMessage::SignalOffer {
                id: "msg_1".to_string(),
                data: SignalOffer {
                    connection_id: "conn_missing".to_string(),
                    sdp: "v=0".to_string(),
                },
            },
        );

        assert_eq!(outbound[0]["type"], "signal.cancel");
        assert_eq!(outbound[0]["data"]["reason"], "unknown_connection");
    }

    #[tokio::test]
    async fn forwards_late_webrtc_ice_candidates_to_signal_outbound() {
        let (ice_tx, ice_rx) = mpsc::unbounded_channel();
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel();

        let handle = spawn_late_webrtc_ice_forwarder(ice_rx, outbound_tx);
        ice_tx
            .send(serde_json::json!({
                "version": 1,
                "type": "signal.ice_candidate",
                "id": "msg_ice",
                "data": {
                    "connection_id": "conn_1",
                    "candidate": "candidate:1"
                }
            }))
            .unwrap();

        let outbound = outbound_rx.recv().await.unwrap();
        handle.abort();

        assert_eq!(outbound["type"], "signal.ice_candidate");
        assert_eq!(outbound["data"]["connection_id"], "conn_1");
    }
}
