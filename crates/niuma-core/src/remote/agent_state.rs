use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteAgentState {
    Disabled,
    NotConfigured,
    Binding,
    Connecting,
    Online,
    Reconnecting,
    TokenRevoked,
    ServerUnreachable,
    Error,
}

impl RemoteAgentState {
    pub fn startup(remote_access_enabled: bool, has_device_token: bool) -> Self {
        if !remote_access_enabled {
            return Self::Disabled;
        }
        if !has_device_token {
            return Self::NotConfigured;
        }
        Self::Connecting
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    pub should_reconnect: bool,
    pub clear_credentials: bool,
}

impl ReconnectPolicy {
    pub fn for_state(state: RemoteAgentState) -> Self {
        match state {
            RemoteAgentState::TokenRevoked => Self {
                should_reconnect: false,
                clear_credentials: true,
            },
            RemoteAgentState::Disabled | RemoteAgentState::NotConfigured => Self {
                should_reconnect: false,
                clear_credentials: false,
            },
            _ => Self {
                should_reconnect: true,
                clear_credentials: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_goes_disabled_when_remote_access_off() {
        let state = RemoteAgentState::startup(false, true);
        assert_eq!(state, RemoteAgentState::Disabled);
    }

    #[test]
    fn startup_goes_not_configured_without_device_token() {
        let state = RemoteAgentState::startup(true, false);
        assert_eq!(state, RemoteAgentState::NotConfigured);
    }

    #[test]
    fn token_revoked_stops_reconnect() {
        let policy = ReconnectPolicy::for_state(RemoteAgentState::TokenRevoked);
        assert!(!policy.should_reconnect);
        assert!(policy.clear_credentials);
    }
}
