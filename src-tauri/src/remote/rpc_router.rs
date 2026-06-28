use niuma_core::remote::rpc_envelope::RemoteRpcEnvelope;
use serde_json::{json, Value};

pub fn handle_plain_rpc(request: Value) -> Result<Value, String> {
    let envelope: RemoteRpcEnvelope = serde_json::from_value(request)
        .map_err(|error| format!("RPC request envelope 校验失败：{error}"))?;

    let RemoteRpcEnvelope::Request {
        version,
        id,
        method,
        params: _,
    } = envelope
    else {
        return Err("RPC envelope 必须是 request 类型".to_string());
    };

    if version != 1 {
        return Err(format!("不支持的 RPC envelope version：{version}"));
    }

    let result = match method.as_str() {
        "rpc.ping" => json!({ "pong": true }),
        // Task 7 只返回占位状态，避免绕过 MainState 架构直接读取。
        "state.get" => json!({ "state": "unknown", "source": "remote_mvp" }),
        // 真实会话读取留给后续任务接入，这里保持最小可用响应。
        "session.list" => json!({ "list": [] }),
        _ => {
            return Ok(json!({
                "version": 1,
                "type": "response",
                "id": id,
                "ok": false,
                "error": {
                    "code": "method_not_found",
                    "message": format!("unknown RPC method: {method}")
                }
            }));
        }
    };

    Ok(json!({
        "version": 1,
        "type": "response",
        "id": id,
        "ok": true,
        "result": result
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn handles_rpc_ping() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_1",
            "method": "rpc.ping",
            "params": {}
        }))
        .unwrap();

        assert_eq!(
            response,
            json!({
                "version": 1,
                "type": "response",
                "id": "req_1",
                "ok": true,
                "result": { "pong": true }
            })
        );
    }

    #[test]
    fn handles_state_get_with_remote_mvp_placeholder() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_2",
            "method": "state.get",
            "params": {}
        }))
        .unwrap();

        assert_eq!(response["ok"], true);
        assert_eq!(
            response["result"],
            json!({ "state": "unknown", "source": "remote_mvp" })
        );
    }

    #[test]
    fn handles_session_list_with_empty_placeholder() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_3",
            "method": "session.list",
            "params": {}
        }))
        .unwrap();

        assert_eq!(response["ok"], true);
        assert_eq!(response["result"], json!({ "list": [] }));
    }

    #[test]
    fn returns_method_not_found_for_unknown_method() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_4",
            "method": "unknown.method",
            "params": {}
        }))
        .unwrap();

        assert_eq!(
            response,
            json!({
                "version": 1,
                "type": "response",
                "id": "req_4",
                "ok": false,
                "error": {
                    "code": "method_not_found",
                    "message": "unknown RPC method: unknown.method"
                }
            })
        );
    }

    #[test]
    fn rejects_request_without_id_or_method() {
        let missing_id = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "method": "rpc.ping",
            "params": {}
        }))
        .unwrap_err();
        let missing_method = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_5",
            "params": {}
        }))
        .unwrap_err();

        assert!(missing_id.contains("id"));
        assert!(missing_method.contains("method"));
    }
}
