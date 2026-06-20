use niuma_api::local_api_addr;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::local_api_client::{get_local_api, post_local_api};
use serde_json::json;

pub(crate) fn doctor() -> ApiResponse<serde_json::Value> {
    // MVP-0 先输出本机能力探针，后续再接真实 Local API、数据库和工具检测。
    ApiResponse::ok(json!({
        "app": "NiumaNotifier",
        "rust": "available",
        "local_api": "not_started",
        "database": "not_configured",
        "tools": {
            "codex": "unknown",
            "claude_code": "unknown"
        }
    }))
}

pub(crate) fn status() -> ApiResponse<serde_json::Value> {
    if let Ok(response) = local_api_envelope("GET", "/api/v1/main-state") {
        return response;
    }

    offline_runtime_state_response("读取运行状态")
}

pub(crate) fn sample_event() -> ApiResponse<serde_json::Value> {
    ApiResponse::ok(json!({
        "hook_event_name": "SessionStart",
        "session_id": "sample-session",
        "cwd": "/tmp/niuma-sample"
    }))
}

pub(crate) fn reset() -> ApiResponse<serde_json::Value> {
    if let Ok(response) = local_api_envelope_with_body(
        "POST",
        "/api/v1/state/reset",
        Some(reset_request_body().as_str()),
    ) {
        return response;
    }

    offline_runtime_state_response("重置运行状态")
}

pub(crate) fn dismiss_blocker() -> ApiResponse<serde_json::Value> {
    if let Ok(response) = local_api_envelope("POST", "/api/v1/blocker/dismiss") {
        return response;
    }

    offline_runtime_state_response("处理阻塞状态")
}

fn local_api_envelope(method: &str, path: &str) -> Result<ApiResponse<serde_json::Value>, String> {
    local_api_envelope_with_body(method, path, None)
}

fn local_api_envelope_with_body(
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<ApiResponse<serde_json::Value>, String> {
    let body = match method {
        "GET" => get_local_api(&local_api_addr(), path),
        "POST" => post_local_api(&local_api_addr(), path, body),
        _ => Err(format!("不支持的 Local API 方法：{method}")),
    }?;
    api_response_from_body(&body)
}

fn reset_request_body() -> String {
    json!({
        "confirm": "RESET_NIUMA_STATE",
        "reason": "cli_reset"
    })
    .to_string()
}

fn offline_runtime_state_response(action: &str) -> ApiResponse<serde_json::Value> {
    ApiResponse::fail(
        ApiErrorCode::ServiceUnavailable,
        format!("Local API 未连接，无法{action}；运行状态只保存在应用进程内，请先启动桌面端或 niuma serve"),
    )
}

fn api_response_from_body(body: &str) -> Result<ApiResponse<serde_json::Value>, String> {
    let value = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|error| format!("解析 Local API 响应失败：{error}"))?;
    let code = value
        .get("code")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| "Local API 响应缺少 code".to_string())? as i32;
    let message = value
        .get("message")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "Local API 响应缺少 message".to_string())?
        .to_string();
    let data = value
        .get("data")
        .cloned()
        .ok_or_else(|| "Local API 响应缺少 data".to_string())?;
    Ok(ApiResponse {
        code,
        message,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::{api_response_from_body, reset_request_body};

    #[test]
    fn api_response_from_body_accepts_standard_envelope() {
        let response =
            api_response_from_body(r#"{"code":0,"message":"ok","data":{"dismissed":false}}"#)
                .unwrap();

        assert_eq!(response.code, 0);
        assert_eq!(response.message, "ok");
        assert_eq!(response.data["dismissed"], false);
    }

    #[test]
    fn api_response_from_body_rejects_non_envelope_response() {
        assert!(api_response_from_body(r#"{"status":"ok"}"#).is_err());
    }

    #[test]
    fn reset_request_body_uses_formal_confirmation() {
        let body = reset_request_body();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(value["confirm"], "RESET_NIUMA_STATE");
        assert_eq!(value["reason"], "cli_reset");
    }

    #[test]
    fn offline_runtime_state_response_is_service_unavailable() {
        let response = super::offline_runtime_state_response("status");

        assert_eq!(response.code, 900004);
        assert!(response.message.contains("Local API 未连接"));
        assert!(response.message.contains("进程内"));
        assert!(response.data.is_null());
    }
}
