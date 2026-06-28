use crate::remote::webrtc_transport::{create_webrtc_answer, WebRtcTransportConfig};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSignalingSession {
    pub connection_id: String,
    pub client_id: String,
    pub transport: TransportPreference,
}

#[derive(Clone, Default)]
pub struct RemoteSignalingManager {
    sessions: Arc<Mutex<HashMap<String, RemoteSignalingSession>>>,
    status: Option<crate::remote::status::RemoteAgentStatusHandle>,
}

impl RemoteSignalingManager {
    pub fn with_status(status: crate::remote::status::RemoteAgentStatusHandle) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            status: Some(status),
        }
    }

    pub fn handle_message(&self, config: &RemoteConfig, message: DeviceSignalMessage) -> Vec<Value> {
        match message {
            DeviceSignalMessage::ConnectionInvite { data, .. } => self.handle_invite(config, data),
            DeviceSignalMessage::SignalOffer { data, .. } => self.handle_offer(data),
            DeviceSignalMessage::SignalIceCandidate { data, .. } => self.handle_ice_candidate(data),
            DeviceSignalMessage::SignalCancel { data, .. } => self.handle_cancel(data),
            DeviceSignalMessage::SignalAnswer { data, .. } => {
                vec![signal_cancel_message(&data.connection_id, "unexpected_answer")]
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
            TransportPreference::Relay => {
                return vec![connection_reject_message(
                    &invite.connection_id,
                    ConnectionRejectReason::UnsupportedTransport,
                )]
            }
        };
        if let Ok(mut sessions) = self.sessions.lock() {
            if !sessions.is_empty() && !sessions.contains_key(&invite.connection_id) {
                return vec![connection_reject_message(
                    &invite.connection_id,
                    ConnectionRejectReason::Busy,
                )];
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
        }
        vec![connection_accept_message(&invite.connection_id, transport)]
    }

    pub fn has_session(&self, connection_id: &str) -> bool {
        self.sessions
            .lock()
            .map(|sessions| sessions.contains_key(connection_id))
            .unwrap_or(false)
    }

    pub async fn handle_offer_async(
        &self,
        config: &RemoteConfig,
        offer: SignalOffer,
        webrtc_config: WebRtcTransportConfig,
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
        match create_webrtc_answer(&connection_id, offer, webrtc_config).await {
            Ok(mut result) => {
                if let Some(status) = &self.status {
                    status.set_selected_transport(Some(RemoteTransportKind::Webrtc));
                }
                let mut outbound = vec![result.answer_message];
                while let Ok(candidate) = result.ice_rx.try_recv() {
                    outbound.push(candidate);
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

    fn handle_cancel(&self, cancel: SignalCancel) -> Vec<Value> {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(&cancel.connection_id);
        }
        if let Some(status) = &self.status {
            status.set_active_connection(None);
        }
        vec![]
    }
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

    fn sample_invite() -> ConnectionInvite {
        ConnectionInvite {
            connection_id: "conn_1".to_string(),
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
}
