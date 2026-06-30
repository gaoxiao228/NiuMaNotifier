use niuma_core::remote::agent_state::RemoteAgentState;
use niuma_core::remote::transport::RemoteTransportKind;
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize)]
pub struct RemoteAgentStatus {
    pub state: &'static str,
    pub last_error: Option<String>,
    pub active_connection_id: Option<String>,
    pub selected_transport: Option<RemoteTransportKind>,
    pub available_transports: Vec<RemoteTransportKind>,
}

impl RemoteAgentStatus {
    pub fn new(state: RemoteAgentState) -> Self {
        Self {
            state: state_label(state),
            last_error: None,
            active_connection_id: None,
            selected_transport: None,
            available_transports: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct RemoteAgentStatusHandle {
    inner: Arc<Mutex<RemoteAgentStatus>>,
}

impl Default for RemoteAgentStatusHandle {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RemoteAgentStatus::new(
                RemoteAgentState::NotConfigured,
            ))),
        }
    }
}

impl RemoteAgentStatusHandle {
    pub fn set_state(&self, state: RemoteAgentState, last_error: Option<String>) {
        if let Ok(mut value) = self.inner.lock() {
            *value = RemoteAgentStatus {
                state: state_label(state),
                last_error,
                active_connection_id: value.active_connection_id.clone(),
                selected_transport: value.selected_transport,
                available_transports: value.available_transports.clone(),
            };
        }
    }

    pub fn set_active_connection(&self, connection_id: Option<String>) {
        if let Ok(mut value) = self.inner.lock() {
            value.active_connection_id = connection_id;
        }
    }

    pub fn clear_connection(&self) {
        if let Ok(mut value) = self.inner.lock() {
            value.active_connection_id = None;
            value.selected_transport = None;
            value.available_transports.clear();
        }
    }

    pub fn set_selected_transport(&self, transport: Option<RemoteTransportKind>) {
        if let Ok(mut value) = self.inner.lock() {
            value.selected_transport = transport;
        }
    }

    pub fn add_available_transport(&self, transport: RemoteTransportKind) {
        if let Ok(mut value) = self.inner.lock() {
            if !value.available_transports.contains(&transport) {
                value.available_transports.push(transport);
            }
        }
    }

    pub fn snapshot(&self) -> RemoteAgentStatus {
        self.inner
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| RemoteAgentStatus {
                state: "error",
                last_error: Some("远程状态锁定失败".to_string()),
                active_connection_id: None,
                selected_transport: None,
                available_transports: Vec::new(),
            })
    }
}

fn state_label(state: RemoteAgentState) -> &'static str {
    match state {
        RemoteAgentState::Disabled => "disabled",
        RemoteAgentState::NotConfigured => "not_configured",
        RemoteAgentState::Binding => "binding",
        RemoteAgentState::Connecting => "connecting",
        RemoteAgentState::Online => "online",
        RemoteAgentState::Reconnecting => "reconnecting",
        RemoteAgentState::TokenRevoked => "token_revoked",
        RemoteAgentState::ServerUnreachable => "server_unreachable",
        RemoteAgentState::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::agent_state::RemoteAgentState;

    #[test]
    fn status_snapshot_serializes_state_without_credentials() {
        let status = RemoteAgentStatus::new(RemoteAgentState::Online);
        let value = serde_json::to_value(status).unwrap();

        assert_eq!(value["state"], "online");
        assert!(value.get("device_token").is_none());
    }

    #[test]
    fn status_handle_updates_snapshot() {
        let handle = RemoteAgentStatusHandle::default();
        handle.set_state(RemoteAgentState::Connecting, None);
        assert_eq!(handle.snapshot().state, "connecting");
    }

    #[test]
    fn status_can_show_signaling_connection() {
        let handle = RemoteAgentStatusHandle::default();
        handle.set_active_connection(Some("conn_1".to_string()));
        let snapshot = handle.snapshot();

        assert_eq!(snapshot.active_connection_id.as_deref(), Some("conn_1"));
    }

    #[test]
    fn status_tracks_available_and_selected_transports() {
        let handle = RemoteAgentStatusHandle::default();
        handle.add_available_transport(RemoteTransportKind::Relay);
        handle.set_selected_transport(Some(RemoteTransportKind::Relay));
        handle.add_available_transport(RemoteTransportKind::Webrtc);
        handle.set_selected_transport(Some(RemoteTransportKind::Webrtc));
        let snapshot = handle.snapshot();

        assert_eq!(
            snapshot.available_transports,
            vec![RemoteTransportKind::Relay, RemoteTransportKind::Webrtc]
        );
        assert_eq!(snapshot.selected_transport, Some(RemoteTransportKind::Webrtc));
    }
}
