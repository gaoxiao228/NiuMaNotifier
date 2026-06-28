use crate::remote::device_socket::{
    run_device_socket_once, DeviceSocketConnectRequest, DeviceSocketRunResult,
};
use crate::remote::status::RemoteAgentStatusHandle;
use niuma_core::remote::agent_state::RemoteAgentState;
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::connection_policy::{DeviceSocketCloseReason, ReconnectBackoff};
use niuma_core::remote::credentials::{
    RemoteCredentialPayload, RemoteCredentialStore, RestrictedFileCredentialStore,
};
use niuma_core::store::NiumaStore;
use std::thread;
use std::time::Duration;
use tokio::time;

pub const DEVICE_HEARTBEAT_INTERVAL_SECONDS: u64 = 20;

pub struct RemoteAgent;

impl RemoteAgent {
    pub fn startup_state(
        config: &RemoteConfig,
        credentials: Option<&RemoteCredentialPayload>,
    ) -> RemoteAgentState {
        RemoteAgentState::startup(config.remote_access_enabled, credentials.is_some())
    }
}

pub fn build_connect_request(
    config: &RemoteConfig,
    credential: &RemoteCredentialPayload,
) -> Result<DeviceSocketConnectRequest, String> {
    let Some(device) = config.device.as_ref() else {
        return Err("远程设备未绑定".to_string());
    };
    Ok(DeviceSocketConnectRequest {
        server_url: config.server_url.clone(),
        device_id: device.id.clone(),
        device_token: credential.device_token.clone(),
        heartbeat_interval_seconds: DEVICE_HEARTBEAT_INTERVAL_SECONDS,
        remote_config: config.clone(),
    })
}

pub fn state_after_socket_result(result: DeviceSocketRunResult) -> RemoteAgentState {
    match result {
        DeviceSocketRunResult::Closed(DeviceSocketCloseReason::TokenRevoked) => {
            RemoteAgentState::TokenRevoked
        }
        DeviceSocketRunResult::Closed(DeviceSocketCloseReason::ServerShutdown)
        | DeviceSocketRunResult::Closed(DeviceSocketCloseReason::NetworkError)
        | DeviceSocketRunResult::Failed(_) => RemoteAgentState::Reconnecting,
        DeviceSocketRunResult::Closed(DeviceSocketCloseReason::ProtocolError) => {
            RemoteAgentState::Error
        }
    }
}

pub async fn run_agent_loop(
    mut load_config: impl FnMut() -> Result<RemoteConfig, String>,
    credential_store: impl RemoteCredentialStore,
    status: RemoteAgentStatusHandle,
) {
    let backoff = ReconnectBackoff::default();
    let signaling_manager = crate::remote::signaling::RemoteSignalingManager::with_status(status.clone());
    let mut attempt = 0u32;
    loop {
        let config = match load_config() {
            Ok(value) => value,
            Err(error) => {
                status.set_state(RemoteAgentState::Error, Some(error.clone()));
                eprintln!("NiumaNotifier remote config load failed: {error}");
                time::sleep(Duration::from_secs(30)).await;
                continue;
            }
        };
        if !config.remote_access_enabled {
            status.set_state(RemoteAgentState::Disabled, None);
            time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        let credential = match credential_store.load(&config.server_url) {
            Ok(value) => value,
            Err(_) => {
                status.set_state(RemoteAgentState::NotConfigured, None);
                time::sleep(Duration::from_secs(30)).await;
                continue;
            }
        };
        let request = match build_connect_request(&config, &credential) {
            Ok(value) => value,
            Err(error) => {
                status.set_state(RemoteAgentState::NotConfigured, Some(error.clone()));
                eprintln!("NiumaNotifier remote connect request not ready: {error}");
                time::sleep(Duration::from_secs(30)).await;
                continue;
            }
        };

        status.set_state(RemoteAgentState::Connecting, None);
        let webrtc_config = crate::remote::webrtc_transport::WebRtcTransportConfig::default();
        let result_state = state_after_socket_result(
            run_device_socket_once(request, signaling_manager.clone(), webrtc_config).await,
        );
        status.set_state(result_state, None);
        match result_state {
            RemoteAgentState::TokenRevoked => {
                if let Err(error) = credential_store.clear(&config.server_url) {
                    eprintln!("NiumaNotifier remote credential clear failed: {error}");
                }
                break;
            }
            RemoteAgentState::Reconnecting => {
                let delay = backoff.delay_for_attempt(attempt);
                attempt = attempt.saturating_add(1);
                time::sleep(delay).await;
            }
            RemoteAgentState::Error => {
                time::sleep(Duration::from_secs(60)).await;
            }
            _ => {
                attempt = 0;
            }
        }
    }
}

pub fn spawn_remote_agent_runtime(store: NiumaStore, status: RemoteAgentStatusHandle) {
    if let Err(error) = thread::Builder::new()
        .name("remote-agent-runtime".to_string())
        .spawn(move || {
            let credential_store = RestrictedFileCredentialStore::new(
                NiumaStore::default_path()
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(std::env::temp_dir)
                    .join("remote-credentials"),
            );
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("NiumaNotifier remote runtime not started: {error}");
                    return;
                }
            };
            runtime.block_on(run_agent_loop(
                move || store.remote_config(),
                credential_store,
                status,
            ));
        })
    {
        eprintln!("NiumaNotifier remote agent startup thread not started: {error}");
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

#[cfg(test)]
mod connection_tests {
    use super::*;
    use crate::remote::device_socket::DeviceSocketRunResult;
    use niuma_core::remote::agent_state::RemoteAgentState;
    use niuma_core::remote::config::{RemoteConfig, RemoteDeviceSummary};
    use niuma_core::remote::connection_policy::DeviceSocketCloseReason;

    #[test]
    fn build_connect_request_requires_bound_device_and_credential() {
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "NiuMa MacBook".to_string(),
        });
        let credential = test_credential("dvt_secret");

        let request = build_connect_request(&config, &credential).unwrap();

        assert_eq!(request.device_id, "dev_1");
        assert_eq!(request.device_token, "dvt_secret");
    }

    #[test]
    fn token_revoked_result_enters_token_revoked_state() {
        assert_eq!(
            state_after_socket_result(DeviceSocketRunResult::Closed(
                DeviceSocketCloseReason::TokenRevoked
            )),
            RemoteAgentState::TokenRevoked
        );
    }

    fn test_credential(token: &str) -> niuma_core::remote::credentials::RemoteCredentialPayload {
        niuma_core::remote::credentials::RemoteCredentialPayload {
            device_token: token.to_string(),
            device_identity_private_key: "identity-private-key".to_string(),
        }
    }
}
