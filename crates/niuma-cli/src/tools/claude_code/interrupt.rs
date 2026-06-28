use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::Value;

pub(crate) fn claude_interrupt(_wrapper_session_id: String) -> ApiResponse<Value> {
    ApiResponse::fail(
        ApiErrorCode::BusinessValidation,
        "niuma claude-interrupt 尚未实现",
    )
}
