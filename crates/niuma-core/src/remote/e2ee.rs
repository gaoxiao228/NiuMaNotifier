use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RemoteCryptoError {
    #[error("key derivation failed")]
    KeyDerivation,
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed")]
    Decrypt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcFrame {
    pub version: u8,
    pub r#type: String,
    pub connection_id: String,
    pub seq: u64,
    pub nonce: String,
    pub ciphertext: String,
}

pub struct RemoteCryptoSession {
    connection_id: String,
    send_key: Aes256Gcm,
    _receive_key: Aes256Gcm,
    next_seq: u64,
}

fn derive_keys(
    shared_secret: [u8; 32],
    context: &str,
) -> Result<([u8; 32], [u8; 32]), RemoteCryptoError> {
    let salt = format!("niuma-remote-e2ee-v1:{context}");
    let hk = Hkdf::<Sha256>::new(Some(salt.as_bytes()), &shared_secret);
    let mut output = [0u8; 64];
    hk.expand(b"client->device|device->client", &mut output)
        .map_err(|_| RemoteCryptoError::KeyDerivation)?;
    let mut send = [0u8; 32];
    let mut recv = [0u8; 32];
    send.copy_from_slice(&output[32..64]);
    recv.copy_from_slice(&output[0..32]);
    Ok((send, recv))
}

impl RemoteCryptoSession {
    pub fn for_test(
        connection_id: &str,
        device_id: &str,
        client_id: &str,
        shared_secret: [u8; 32],
    ) -> Self {
        let context = format!("{connection_id}:{device_id}:{client_id}");
        let (send, recv) = derive_keys(shared_secret, &context).unwrap();
        Self {
            connection_id: connection_id.to_string(),
            send_key: Aes256Gcm::new_from_slice(&send).unwrap(),
            _receive_key: Aes256Gcm::new_from_slice(&recv).unwrap(),
            next_seq: 1,
        }
    }

    pub fn encrypt_request(
        &mut self,
        id: &str,
        method: &str,
        params_json: &str,
    ) -> Result<RpcFrame, RemoteCryptoError> {
        let payload = serde_json::json!({
            "version": 1,
            "type": "request",
            "id": id,
            "method": method,
            "params": serde_json::from_str::<serde_json::Value>(params_json).unwrap_or(serde_json::json!({}))
        });
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = self
            .send_key
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                payload.to_string().as_bytes(),
            )
            .map_err(|_| RemoteCryptoError::Encrypt)?;
        let frame = RpcFrame {
            version: 1,
            r#type: "rpc.frame".to_string(),
            connection_id: self.connection_id.clone(),
            seq: self.next_seq,
            nonce: URL_SAFE_NO_PAD.encode(nonce_bytes),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        };
        self.next_seq += 1;
        Ok(frame)
    }

    pub fn decrypt_for_test(&self, frame: &RpcFrame) -> Result<String, RemoteCryptoError> {
        let nonce = URL_SAFE_NO_PAD
            .decode(&frame.nonce)
            .map_err(|_| RemoteCryptoError::Decrypt)?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&frame.ciphertext)
            .map_err(|_| RemoteCryptoError::Decrypt)?;
        let plaintext = self
            .send_key
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_slice())
            .map_err(|_| RemoteCryptoError::Decrypt)?;
        String::from_utf8(plaintext).map_err(|_| RemoteCryptoError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_directional_keys_and_encrypts_frame() {
        let mut session = RemoteCryptoSession::for_test("conn_1", "dev_1", "web_1", [7u8; 32]);
        let frame = session
            .encrypt_request("req_1", "device.get_health", "{}")
            .unwrap();
        assert_eq!(frame.r#type, "rpc.frame");
        assert!(!frame.ciphertext.contains("device.get_health"));
        let payload = session.decrypt_for_test(&frame).unwrap();
        assert!(payload.contains("device.get_health"));
    }
}
