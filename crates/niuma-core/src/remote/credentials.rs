use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCredentialPayload {
    pub device_token: String,
    pub device_identity_private_key: String,
}

impl fmt::Debug for RemoteCredentialPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteCredentialPayload")
            .field("device_token", &"<redacted>")
            .field("device_identity_private_key", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum RemoteCredentialError {
    #[error("credential not found")]
    NotFound,
    #[error("credential io failed: {0}")]
    Io(String),
    #[error("credential serialization failed: {0}")]
    Serialization(String),
}

impl From<io::Error> for RemoteCredentialError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            Self::NotFound
        } else {
            Self::Io(error.to_string())
        }
    }
}

pub trait RemoteCredentialStore {
    fn load(&self, server_url: &str) -> Result<RemoteCredentialPayload, RemoteCredentialError>;

    fn save(
        &self,
        server_url: &str,
        payload: &RemoteCredentialPayload,
    ) -> Result<(), RemoteCredentialError>;

    fn clear(&self, server_url: &str) -> Result<(), RemoteCredentialError>;
}

#[derive(Debug, Clone)]
pub struct RestrictedFileCredentialStore {
    base_dir: PathBuf,
}

impl RestrictedFileCredentialStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn credential_path(&self, server_url: &str) -> PathBuf {
        let filename = server_url.replace("://", "_").replace(['/', ':'], "_");
        self.base_dir
            .join(format!("{filename}.remote-credential.json"))
    }
}

impl RemoteCredentialStore for RestrictedFileCredentialStore {
    fn load(&self, server_url: &str) -> Result<RemoteCredentialPayload, RemoteCredentialError> {
        let path = self.credential_path(server_url);
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| RemoteCredentialError::Serialization(error.to_string()))
    }

    fn save(
        &self,
        server_url: &str,
        payload: &RemoteCredentialPayload,
    ) -> Result<(), RemoteCredentialError> {
        fs::create_dir_all(&self.base_dir)?;
        let path = self.credential_path(server_url);
        let bytes = serde_json::to_vec(payload)
            .map_err(|error| RemoteCredentialError::Serialization(error.to_string()))?;
        fs::write(&path, bytes)?;
        restrict_file_to_current_user(&path)?;
        Ok(())
    }

    fn clear(&self, server_url: &str) -> Result<(), RemoteCredentialError> {
        let path = self.credential_path(server_url);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(RemoteCredentialError::Io(error.to_string())),
        }
    }
}

#[cfg(unix)]
fn restrict_file_to_current_user(path: &std::path::Path) -> Result<(), RemoteCredentialError> {
    use std::os::unix::fs::PermissionsExt;

    // 受限文件 fallback 只允许当前用户读写，避免 device token 被其他本机用户读取。
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_file_to_current_user(_path: &std::path::Path) -> Result<(), RemoteCredentialError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_payload_does_not_debug_print_token() {
        let payload = RemoteCredentialPayload {
            device_token: "secret-device-token".to_string(),
            device_identity_private_key: "secret-private-key".to_string(),
        };

        let debug = format!("{payload:?}");
        assert!(!debug.contains("secret-device-token"));
        assert!(!debug.contains("secret-private-key"));
    }

    #[test]
    fn credential_path_is_server_scoped() {
        let store = RestrictedFileCredentialStore::new(PathBuf::from("/tmp/niuma"));
        assert_ne!(
            store.credential_path("https://remote.niuma.example"),
            store.credential_path("https://remote.example.com")
        );
    }

    #[test]
    fn restricted_file_store_saves_loads_and_clears_server_scoped_credential() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store = RestrictedFileCredentialStore::new(dir.path().to_path_buf());
        let payload = RemoteCredentialPayload {
            device_token: "device-token".to_string(),
            device_identity_private_key: "identity-private-key".to_string(),
        };

        store
            .save("https://remote.example.com", &payload)
            .expect("save credential");

        let loaded = store
            .load("https://remote.example.com")
            .expect("load credential");
        assert_eq!(loaded, payload);

        store
            .clear("https://remote.example.com")
            .expect("clear credential");
        assert!(matches!(
            store.load("https://remote.example.com"),
            Err(RemoteCredentialError::NotFound)
        ));
    }
}
