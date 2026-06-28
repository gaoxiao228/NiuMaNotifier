use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::claude_code_managed_session::read_registry;
use niuma_core::platform::paths::claude_code_managed_registry_path;
use serde_json::{json, Value};

pub(crate) fn claude_sessions() -> ApiResponse<Value> {
    let registry_path = claude_code_managed_registry_path();
    match read_registry(&registry_path) {
        Ok(registry) => {
            let total_count = registry.sessions.len();
            ApiResponse::ok(json!({
                "registry_path": registry_path.to_string_lossy(),
                "sessions": registry.sessions,
                "total_count": total_count
            }))
        }
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}
