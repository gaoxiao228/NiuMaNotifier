use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::Value;

pub(crate) fn claude_sessions() -> ApiResponse<Value> {
    ApiResponse::fail(
        ApiErrorCode::BusinessValidation,
        "niuma claude-sessions 尚未实现",
    )
}
