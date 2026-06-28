use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::claude_code_managed_session::read_registry;
use niuma_core::platform::paths::claude_code_managed_registry_path;
use serde_json::{json, Value};
use std::process::Command;

use super::managed::resolve_real_claude;

pub(crate) fn claude_send(wrapper_session_id: String, message: String) -> ApiResponse<Value> {
    match claude_send_inner(&wrapper_session_id, &message) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    }
}

fn claude_send_inner(wrapper_session_id: &str, message: &str) -> Result<Value, String> {
    if !wrapper_session_id.starts_with("niuma_claude_") {
        return Err("wrapper_session_id 必须以 niuma_claude_ 开头".to_string());
    }
    if message.trim().is_empty() {
        return Err("message 不能为空".to_string());
    }
    let registry = read_registry(&claude_code_managed_registry_path())?;
    let session = registry
        .sessions
        .iter()
        .find(|session| session.wrapper_session_id == wrapper_session_id)
        .ok_or_else(|| format!("找不到 niuma-claude 会话：{wrapper_session_id}"))?;
    let claude_session_id = session
        .claude_session_id
        .as_deref()
        .ok_or_else(|| format!("会话尚未绑定 Claude session id：{wrapper_session_id}"))?;
    let real_claude = resolve_real_claude()?;
    let status = Command::new(real_claude)
        .arg("--resume")
        .arg(claude_session_id)
        .arg(message)
        .status()
        .map_err(|error| format!("执行 claude --resume 失败：{error}"))?;
    Ok(json!({
        "wrapper_session_id": wrapper_session_id,
        "claude_session_id": claude_session_id,
        "sent": status.success(),
        "exit_code": status.code()
    }))
}
