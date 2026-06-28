use base64::Engine;
use serde_json::{json, Value};
use uuid::Uuid;

// Task 6 接入实际 runtime 前，这些配置会先作为 relay runtime 的稳定骨架保留。
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayRuntimeConfig {
    pub socket_url: String,
    pub connection_id: String,
}

impl RelayRuntimeConfig {
    #[allow(dead_code)]
    pub fn new(socket_url: impl Into<String>, connection_id: impl Into<String>) -> Self {
        Self {
            socket_url: socket_url.into(),
            connection_id: connection_id.into(),
        }
    }
}

/// Relay 收发、加密帧 ping/pong 和状态同步由 Task 6 接入。
#[allow(dead_code)]
pub fn relay_runtime_pending() -> bool {
    true
}

pub fn handle_relay_ciphertext(ciphertext: &str) -> Result<Option<String>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(ciphertext)
        .map_err(|error| format!("relay payload base64 解码失败：{error}"))?;
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("relay payload JSON 解析失败：{error}"))?;

    if payload.get("type").and_then(Value::as_str) != Some("ping") {
        return Ok(None);
    }

    let response = serde_json::to_vec(&json!({ "type": "pong" }))
        .map_err(|error| format!("relay pong JSON 编码失败：{error}"))?;
    Ok(Some(
        base64::engine::general_purpose::STANDARD.encode(response),
    ))
}

pub fn build_relay_response_frame(
    input: &crate::remote::relay_transport::RelayFrame,
    ciphertext: String,
) -> crate::remote::relay_transport::RelayFrame {
    crate::remote::relay_transport::RelayFrame {
        version: 1,
        frame_type: "relay.frame".to_string(),
        id: format!("msg_{}", Uuid::new_v4()),
        connection_id: input.connection_id.clone(),
        seq: input.seq + 1,
        ciphertext,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::relay_transport::RelayFrame;

    #[test]
    fn handles_ping_ciphertext_with_pong_response() {
        let response = handle_relay_ciphertext("eyJ0eXBlIjoicGluZyJ9").unwrap();

        assert_eq!(
            decode_relay_payload_for_test(&response.unwrap()),
            serde_json::json!({ "type": "pong" })
        );
    }

    #[test]
    fn rejects_invalid_base64_ciphertext() {
        let error = handle_relay_ciphertext("not-base64中文").unwrap_err();

        assert!(error.contains("base64"));
    }

    #[test]
    fn ignores_unknown_payload_type() {
        let response = handle_relay_ciphertext("eyJ0eXBlIjoidW5rbm93biJ9").unwrap();

        assert_eq!(response, None);
    }

    #[test]
    fn builds_response_frame_with_incremented_sequence() {
        let input = RelayFrame {
            version: 1,
            frame_type: "relay.frame".to_string(),
            id: "msg_1".to_string(),
            connection_id: "conn_1".to_string(),
            seq: 41,
            ciphertext: "request".to_string(),
        };

        let response = build_relay_response_frame(&input, "response".to_string());

        assert_eq!(response.version, 1);
        assert_eq!(response.frame_type, "relay.frame");
        assert_eq!(response.connection_id, "conn_1");
        assert_eq!(response.seq, 42);
        assert_eq!(response.ciphertext, "response");
        assert!(response.id.starts_with("msg_"));
    }

    fn decode_relay_payload_for_test(encoded: &str) -> serde_json::Value {
        use base64::Engine;

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
}
