use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_managed_session::{
    read_registry, ManagedCodexSession, ManagedCodexSessionState,
};
use niuma_core::platform::paths::codex_managed_registry_path;
use serde_json::{json, Value};
use std::time::Duration;

const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn codex_interrupt(wrapper_session_id: String) -> ApiResponse<Value> {
    match codex_interrupt_inner(&wrapper_session_id) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    }
}

fn codex_interrupt_inner(wrapper_session_id: &str) -> Result<Value, String> {
    if !wrapper_session_id.starts_with("niuma_codex_") {
        return Err("wrapper_session_id 必须以 niuma_codex_ 开头".to_string());
    }

    let registry_path = codex_managed_registry_path();
    let registry = read_registry(&registry_path)?;
    let session = registry
        .sessions
        .iter()
        .find(|session| session.wrapper_session_id == wrapper_session_id)
        .ok_or_else(|| format!("找不到 niuma-codex 会话：{wrapper_session_id}"))?;
    validate_session_available(session)?;

    let response = interrupt_via_control_socket(&session.control_socket)?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("中断会话失败");
        return Err(message.to_string());
    }

    Ok(json!({
        "wrapper_session_id": wrapper_session_id,
        "interrupted": true,
        "result": response.get("result").cloned().unwrap_or(Value::Null)
    }))
}

fn validate_session_available(session: &ManagedCodexSession) -> Result<(), String> {
    if session.state == ManagedCodexSessionState::Exited {
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

#[cfg(unix)]
fn interrupt_via_control_socket(control_socket: &str) -> Result<Value, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(control_socket)
        .map_err(|error| format!("连接 niuma-codex control socket 失败：{error}"))?;
    stream
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|error| format!("设置 control socket 读取超时失败：{error}"))?;
    stream
        .set_write_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|error| format!("设置 control socket 写入超时失败：{error}"))?;
    stream
        .write_all(b"{\"type\":\"interrupt\"}\n")
        .map_err(|error| format!("写入 control socket 失败：{error}"))?;

    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    reader
        .read_line(&mut line)
        .map_err(|error| format!("读取 control socket 响应失败：{error}"))?;
    serde_json::from_str(line.trim_end())
        .map_err(|error| format!("解析 control socket 响应失败：{error}"))
}

#[cfg(not(unix))]
fn interrupt_via_control_socket(_control_socket: &str) -> Result<Value, String> {
    Err("niuma-codex interrupt 当前仅支持 Unix socket 平台".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn session(state: ManagedCodexSessionState, pid: Option<u32>) -> ManagedCodexSession {
        ManagedCodexSession {
            wrapper_session_id: "niuma_codex_1".to_string(),
            state,
            cwd: "/repo".to_string(),
            pid,
            real_socket: "/tmp/real.sock".to_string(),
            relay_socket: "/tmp/relay.sock".to_string(),
            control_socket: "/tmp/control.sock".to_string(),
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            first_user_message_hash: None,
            first_user_message_preview: None,
            first_user_message_submitted_at: None,
            codex_session_id: None,
            codex_session_file_path: None,
            bound_at: None,
            binding_failure_reason: None,
        }
    }

    #[test]
    fn validate_session_rejects_exited_session() {
        let error = validate_session_available(&session(ManagedCodexSessionState::Exited, Some(1)))
            .unwrap_err();

        assert!(error.contains("会话已退出"));
    }
}
