use niuma_core::remote::agent_state::RemoteAgentState;
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::credentials::RemoteCredentialPayload;

pub struct RemoteAgent;

impl RemoteAgent {
    pub fn startup_state(
        config: &RemoteConfig,
        credentials: Option<&RemoteCredentialPayload>,
    ) -> RemoteAgentState {
        RemoteAgentState::startup(config.remote_access_enabled, credentials.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_state_uses_remote_config_and_credential_presence() {
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        assert_eq!(
            RemoteAgent::startup_state(&config, None),
            RemoteAgentState::NotConfigured
        );
    }
}
