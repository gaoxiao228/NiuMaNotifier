use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::claude_code_managed_control;
use niuma_core::claude_code_managed_session::{
    managed_claude_code_channel_id, read_registry, ManagedClaudeCodeSession,
    ManagedClaudeCodeSessionState,
};
use niuma_core::platform::paths::claude_code_managed_registry_path;
use serde_json::{json, Value};
use std::path::Path;

pub(crate) fn claude_send(wrapper_session_id: String, message: String) -> ApiResponse<Value> {
    match claude_send_inner(&wrapper_session_id, &message) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    }
}

fn claude_send_inner(wrapper_session_id: &str, message: &str) -> Result<Value, String> {
    claude_send_inner_with_registry_path(
        wrapper_session_id,
        message,
        &claude_code_managed_registry_path(),
    )
}

fn claude_send_inner_with_registry_path(
    wrapper_session_id: &str,
    message: &str,
    registry_path: &Path,
) -> Result<Value, String> {
    if !wrapper_session_id.starts_with("niuma_claude_") {
        return Err("wrapper_session_id 必须以 niuma_claude_ 开头".to_string());
    }
    if message.trim().is_empty() {
        return Err("message 不能为空".to_string());
    }
    let registry = read_registry(registry_path)?;
    let session = registry
        .sessions
        .iter()
        .find(|session| session.wrapper_session_id == wrapper_session_id)
        .ok_or_else(|| format!("找不到 niuma-claude 会话：{wrapper_session_id}"))?;
    validate_session_available(session)?;
    let claude_session_id = session
        .claude_session_id
        .as_deref()
        .ok_or_else(|| format!("会话尚未绑定 Claude session id：{wrapper_session_id}"))?;
    let channel_id = managed_claude_code_channel_id(wrapper_session_id);
    claude_code_managed_control::send_instruction(
        registry_path,
        claude_session_id,
        &channel_id,
        message,
    )?;
    Ok(json!({
        "wrapper_session_id": wrapper_session_id,
        "claude_session_id": claude_session_id,
        "sent": true
    }))
}

fn validate_session_available(session: &ManagedClaudeCodeSession) -> Result<(), String> {
    if session.state == ManagedClaudeCodeSessionState::Exited {
        return Err(format!("会话已退出：{}", session.wrapper_session_id));
    }
    if !process_exists(session.pid) {
        return Err(format!("会话进程不存在：{}", session.wrapper_session_id));
    }
    Ok(())
}

fn process_exists(pid: Option<u32>) -> bool {
    pid.map(process_exists_by_pid).unwrap_or(false)
}

#[cfg(unix)]
fn process_exists_by_pid(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error()
        .raw_os_error()
        .is_some_and(|code| code != libc::ESRCH)
}

#[cfg(not(unix))]
fn process_exists_by_pid(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use niuma_core::claude_code_managed_session::{
        update_registry, ManagedClaudeCodeSession, ManagedClaudeCodeSessionState,
    };

    use super::*;

    #[test]
    fn claude_send_rejects_missing_wrapper_session_like_codex() {
        let temp = tempfile::tempdir().unwrap();
        let registry_path = temp.path().join("claude-code.json");

        let error =
            claude_send_inner_with_registry_path("niuma_claude_missing", "继续", &registry_path)
                .unwrap_err();

        assert_eq!(error, "找不到 niuma-claude 会话：niuma_claude_missing");
    }

    #[test]
    fn claude_send_does_not_resume_when_live_control_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let registry_path = temp.path().join("claude-code.json");
        update_registry(&registry_path, |registry| {
            registry.upsert(ManagedClaudeCodeSession {
                wrapper_session_id: "niuma_claude_1".to_string(),
                state: ManagedClaudeCodeSessionState::Started,
                cwd: "/repo".to_string(),
                pid: Some(std::process::id()),
                control_socket: None,
                started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
                claude_session_id: Some("session-1".to_string()),
                transcript_path: None,
                bound_at: None,
                binding_failure_reason: None,
            });
        })
        .unwrap();

        let error = claude_send_inner_with_registry_path("niuma_claude_1", "继续", &registry_path)
            .unwrap_err();

        assert_eq!(error, "Claude Code 实时发送指令尚未实现");
    }

    #[test]
    fn validate_session_rejects_exited_session() {
        let session = ManagedClaudeCodeSession {
            wrapper_session_id: "niuma_claude_1".to_string(),
            state: ManagedClaudeCodeSessionState::Exited,
            cwd: "/repo".to_string(),
            pid: Some(std::process::id()),
            control_socket: None,
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            claude_session_id: Some("session-1".to_string()),
            transcript_path: None,
            bound_at: None,
            binding_failure_reason: None,
        };

        let error = validate_session_available(&session).unwrap_err();

        assert_eq!(error, "会话已退出：niuma_claude_1");
    }
}
