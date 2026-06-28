use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::Value;

pub(crate) fn claude_send(_wrapper_session_id: String, _message: String) -> ApiResponse<Value> {
    ApiResponse::fail(
        ApiErrorCode::BusinessValidation,
        "niuma claude-send 尚未实现",
    )
}
