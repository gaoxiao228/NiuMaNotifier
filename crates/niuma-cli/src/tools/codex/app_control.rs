use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::Value;
use std::path::PathBuf;

pub fn run_app_control(_real_codex: PathBuf, _args: Vec<String>) -> ApiResponse<Value> {
    // Task 4 会接入真正的 app-server relay；这里先让 managed 模式显式失败但保持可编译。
    ApiResponse::fail(
        ApiErrorCode::BusinessValidation,
        "niuma codex managed mode transport is not wired before Task 4",
    )
}
