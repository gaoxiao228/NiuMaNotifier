use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::remote::config::{RemoteConfig, RemoteDeviceSummary, RemoteUserSummary};
use niuma_core::remote::credentials::{
    RemoteCredentialPayload, RemoteCredentialStore, RestrictedFileCredentialStore,
};
use niuma_core::remote::login_flow::DesktopLoginBindingResult;
use niuma_core::remote::settings::normalize_server_url;
use niuma_core::store::NiumaStore;
use serde_json::json;
use std::path::PathBuf;

pub fn remote_settings_payload(config: RemoteConfig, has_credential: bool) -> serde_json::Value {
    let bound = config.device.is_some() && has_credential;
    json!({
        "server_url": config.server_url,
        "remote_access_enabled": config.remote_access_enabled,
        "remote_control_enabled": config.remote_control_enabled,
        "user": config.user,
        "device": config.device,
        "bound": bound,
        "has_credential": has_credential,
        "last_connected_at": config.last_connected_at
    })
}

pub fn save_remote_settings_to_store(
    store: &NiumaStore,
    server_url: String,
    remote_access_enabled: bool,
    remote_control_enabled: bool,
) -> ApiResponse<serde_json::Value> {
    let server_url = match normalize_server_url(&server_url) {
        Ok(value) => value,
        Err(error) => return ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    };
    let mut config = match store.remote_config() {
        Ok(config) => config,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    if config.server_url != server_url {
        config.user = None;
        config.device = None;
        config.last_connected_at = None;
    }
    config.server_url = server_url;
    config.remote_access_enabled = remote_access_enabled;
    config.remote_control_enabled = remote_control_enabled;
    match store.save_remote_config(&config) {
        Ok(()) => ApiResponse::ok(json!({
            "saved": true,
            "settings": remote_settings_payload(config, false)
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

pub fn apply_remote_binding_result(
    store: &NiumaStore,
    credential_store: &dyn RemoteCredentialStore,
    server_url: &str,
    device_identity_private_key: String,
    result: DesktopLoginBindingResult,
) -> ApiResponse<serde_json::Value> {
    let credential = RemoteCredentialPayload {
        device_token: result.device_token,
        device_identity_private_key,
    };
    if let Err(error) = credential_store.save(server_url, &credential) {
        return ApiResponse::fail(ApiErrorCode::System, error.to_string());
    }
    let mut config = match store.remote_config() {
        Ok(config) => config,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    config.server_url = server_url.to_string();
    config.user = Some(RemoteUserSummary {
        id: result.user_id,
        email: result.user_email,
        role: result.user_role,
    });
    config.device = Some(RemoteDeviceSummary {
        id: result.device_id,
        name: result.device_name,
    });
    match store.save_remote_config(&config) {
        Ok(()) => ApiResponse::ok(json!({
            "completed": true,
            "settings": remote_settings_payload(config, true)
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

pub fn credential_store_for_data_dir(data_dir: PathBuf) -> RestrictedFileCredentialStore {
    RestrictedFileCredentialStore::new(data_dir.join("remote-credentials"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::config::{RemoteConfig, RemoteDeviceSummary, RemoteUserSummary};

    #[test]
    fn remote_settings_payload_does_not_include_device_token() {
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.user = Some(RemoteUserSummary {
            id: "user_1".to_string(),
            email: "user@example.com".to_string(),
            role: "owner".to_string(),
        });
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "NiuMa MacBook".to_string(),
        });

        let payload = remote_settings_payload(config, true);
        assert_eq!(payload["server_url"], "https://remote.example.com");
        assert_eq!(payload["bound"], true);
        assert_eq!(payload["has_credential"], true);
        assert!(payload.get("device_token").is_none());
    }
}
