use rand::RngCore;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInstallId([u8; 32]);

impl DeviceInstallId {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn derive_device_fingerprint(server_origin: &str, install_id: &DeviceInstallId) -> String {
    let mut hasher = Sha256::new();
    // 固定上下文隔离远程设备指纹，避免复用同一个安装 ID 的其他哈希用途。
    hasher.update(b"niuma-device-v1");
    hasher.update(server_origin.as_bytes());
    hasher.update(install_id.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_different_fingerprints_for_different_server_origins() {
        let install_id = DeviceInstallId::from_bytes([7u8; 32]);
        let official = derive_device_fingerprint("https://remote.niuma.example", &install_id);
        let self_hosted = derive_device_fingerprint("https://remote.example.com", &install_id);

        assert_ne!(official, self_hosted);
        assert_eq!(official.len(), 64);
    }
}
