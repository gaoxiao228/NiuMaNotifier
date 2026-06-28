use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::Value;

pub(crate) fn claude_interrupt(_wrapper_session_id: String) -> ApiResponse<Value> {
    if !_wrapper_session_id.starts_with("niuma_claude_") {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            "wrapper_session_id 必须以 niuma_claude_ 开头",
        );
    }
    ApiResponse::fail(
        ApiErrorCode::BusinessValidation,
        "Claude Code active turn interrupt 尚未实现",
    )
}
