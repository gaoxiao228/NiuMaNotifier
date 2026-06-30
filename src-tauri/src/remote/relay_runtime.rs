use crate::remote::rpc_router::RemoteRpcContext;
use base64::Engine;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn handle_relay_ciphertext(
    ciphertext: &str,
    rpc_context: &RemoteRpcContext,
) -> Result<Option<String>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(ciphertext)
        .map_err(|error| format!("relay payload base64 解码失败：{error}"))?;
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("relay payload JSON 解析失败：{error}"))?;

    if payload.get("type").and_then(Value::as_str) == Some("ping") {
        let response = serde_json::to_vec(&json!({ "type": "pong" }))
            .map_err(|error| format!("relay pong JSON 编码失败：{error}"))?;
        return Ok(Some(
            base64::engine::general_purpose::STANDARD.encode(response),
        ));
    }

    if is_plain_rpc_request_payload(&payload) {
        let response =
            crate::remote::rpc_router::handle_plain_rpc_with_context(payload, rpc_context)?;
        let response_bytes = serde_json::to_vec(&response)
            .map_err(|error| format!("relay RPC response JSON 编码失败：{error}"))?;
        return Ok(Some(
            base64::engine::general_purpose::STANDARD.encode(response_bytes),
        ));
    }

    Ok(None)
}

pub async fn handle_relay_ciphertext_async(
    ciphertext: &str,
    rpc_context: &RemoteRpcContext,
) -> Result<Option<String>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(ciphertext)
        .map_err(|error| format!("relay payload base64 解码失败：{error}"))?;
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("relay payload JSON 解析失败：{error}"))?;

    if payload.get("type").and_then(Value::as_str) == Some("ping") {
        let response = serde_json::to_vec(&json!({ "type": "pong" }))
            .map_err(|error| format!("relay pong JSON 编码失败：{error}"))?;
        return Ok(Some(
            base64::engine::general_purpose::STANDARD.encode(response),
        ));
    }

    if is_plain_rpc_request_payload(&payload) {
        let response =
            crate::remote::rpc_router::handle_plain_rpc_with_context_async(payload, rpc_context)
                .await?;
        let response_bytes = serde_json::to_vec(&response)
            .map_err(|error| format!("relay RPC response JSON 编码失败：{error}"))?;
        return Ok(Some(
            base64::engine::general_purpose::STANDARD.encode(response_bytes),
        ));
    }

    Ok(None)
}

fn is_plain_rpc_request_payload(payload: &Value) -> bool {
    // relay 层只做 envelope 分流；字段完整性由 rpc_router 统一校验。
    payload.get("version").and_then(Value::as_u64) == Some(1)
        && payload.get("type").and_then(Value::as_str) == Some("request")
}

pub fn build_relay_response_frame(
    input: &crate::remote::relay_transport::RelayFrame,
    ciphertext: String,
) -> crate::remote::relay_transport::RelayFrame {
    build_relay_response_frame_with_seq(input, input.seq + 1, ciphertext)
}

pub fn build_relay_response_frame_with_seq(
    input: &crate::remote::relay_transport::RelayFrame,
    seq: u64,
    ciphertext: String,
) -> crate::remote::relay_transport::RelayFrame {
    crate::remote::relay_transport::RelayFrame {
        version: 1,
        frame_type: "relay.frame".to_string(),
        id: format!("msg_{}", Uuid::new_v4()),
        connection_id: input.connection_id.clone(),
        seq,
        ciphertext,
    }
}

pub fn build_relay_notification_frame(
    connection_id: &str,
    seq: u64,
    ciphertext: String,
) -> crate::remote::relay_transport::RelayFrame {
    crate::remote::relay_transport::RelayFrame {
        version: 1,
        frame_type: "relay.frame".to_string(),
        id: format!("msg_{}", Uuid::new_v4()),
        connection_id: connection_id.to_string(),
        seq,
        ciphertext,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::relay_transport::RelayFrame;
    use crate::remote::rpc_router::RemoteRpcContext;
    use niuma_api::tool_sessions::ToolSessionRegistry;
    use niuma_core::store::NiumaStore;

    #[test]
    fn handles_ping_ciphertext_with_pong_response() {
        let context = test_rpc_context("relay-runtime-ping");

        let response = handle_relay_ciphertext("eyJ0eXBlIjoicGluZyJ9", &context).unwrap();

        assert_eq!(
            decode_relay_payload_for_test(&response.unwrap()),
            serde_json::json!({ "type": "pong" })
        );
    }

    #[test]
    fn handles_plain_rpc_ping_ciphertext_with_response_envelope() {
        let context = test_rpc_context("relay-runtime-rpc-ping");
        let request = serde_json::json!({
            "version": 1,
            "type": "request",
            "id": "req_1",
            "method": "rpc.ping",
            "params": {}
        });
        let encoded_request = encode_relay_payload_for_test(&request);

        let response = handle_relay_ciphertext(&encoded_request, &context).unwrap();

        assert_eq!(
            decode_relay_payload_for_test(&response.unwrap()),
            serde_json::json!({
                "version": 1,
                "type": "response",
                "transport": { "kind": "relay" },
                "id": "req_1",
                "ok": true,
                "result": { "pong": true }
            })
        );
    }

    #[test]
    fn handles_plain_rpc_project_groups_ciphertext_with_context() {
        let context = test_rpc_context("relay-runtime-project-groups");
        let request = serde_json::json!({
            "version": 1,
            "type": "request",
            "id": "req_project_groups",
            "method": "session.project_groups",
            "params": { "page": 1, "page_size": 20 }
        });
        let encoded_request = encode_relay_payload_for_test(&request);

        let response = handle_relay_ciphertext(&encoded_request, &context).unwrap();

        assert_eq!(
            decode_relay_payload_for_test(&response.unwrap()),
            serde_json::json!({
                "version": 1,
                "type": "response",
                "transport": { "kind": "relay" },
                "id": "req_project_groups",
                "ok": true,
                "result": { "list": [], "page": 1, "page_size": 20, "total": 0 }
            })
        );
    }

    #[test]
    fn rejects_invalid_base64_ciphertext() {
        let context = test_rpc_context("relay-runtime-invalid-base64");

        let error = handle_relay_ciphertext("not-base64中文", &context).unwrap_err();

        assert!(error.contains("base64"));
    }

    #[test]
    fn ignores_unknown_payload_type() {
        let context = test_rpc_context("relay-runtime-unknown");

        let response = handle_relay_ciphertext("eyJ0eXBlIjoidW5rbm93biJ9", &context).unwrap();

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

    fn encode_relay_payload_for_test(value: &serde_json::Value) -> String {
        use base64::Engine;

        let bytes = serde_json::to_vec(value).unwrap();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn test_rpc_context(name: &str) -> RemoteRpcContext {
        let path = std::env::temp_dir().join(format!("{name}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        RemoteRpcContext::new(NiumaStore::new(path), ToolSessionRegistry::new())
    }
}
