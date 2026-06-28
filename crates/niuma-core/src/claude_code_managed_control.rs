use serde_json::Value;
use std::path::Path;

use crate::claude_code_managed_session::{
    read_registry, wrapper_session_id_from_channel_id, ManagedClaudeCodeSession,
    ManagedClaudeCodeSessionState,
};

pub fn send_instruction(
    registry_path: &Path,
    session_id: &str,
    channel_id: &str,
    content: &str,
) -> Result<Value, String> {
    send_instruction_with_registry_path(registry_path, session_id, channel_id, content)
}

pub fn interrupt(
    registry_path: &Path,
    session_id: &str,
    channel_id: &str,
) -> Result<Value, String> {
    let _session = managed_session_for_control(registry_path, session_id, channel_id)?;
    Err("Claude Code active turn interrupt 尚未实现".to_string())
}

fn send_instruction_with_registry_path(
    registry_path: &Path,
    session_id: &str,
    channel_id: &str,
    content: &str,
) -> Result<Value, String> {
    if content.trim().is_empty() {
        return Err("content 不能为空".to_string());
    }
    let _session = managed_session_for_control(registry_path, session_id, channel_id)?;
    // claude --resume 会新开进程，不等价于给指定 managed 进程发送实时指令。
    Err("Claude Code 实时发送指令尚未实现".to_string())
}

fn managed_session_for_control(
    registry_path: &Path,
    session_id: &str,
    channel_id: &str,
) -> Result<ManagedClaudeCodeSession, String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err("session_id 不能为空".to_string());
    }
    let channel_id = channel_id.trim();
    let wrapper_session_id = wrapper_session_id_from_channel_id(channel_id)
        .ok_or_else(|| format!("不支持的 session control channel：{channel_id}"))?;
    let registry = read_registry(registry_path)?;
    let session = registry
        .sessions
        .iter()
        .find(|session| session.wrapper_session_id == wrapper_session_id)
        .ok_or_else(|| format!("找不到 session control channel：{channel_id}"))?;
    if session.claude_session_id.as_deref() != Some(session_id) {
        return Err("channel_id 与 session_id 不匹配".to_string());
    }
    if session.state == ManagedClaudeCodeSessionState::Exited {
        return Err(format!("会话已退出：{}", session.wrapper_session_id));
    }
    if session.state == ManagedClaudeCodeSessionState::Unavailable {
        return Err(format!("session control channel 不可用：{channel_id}"));
    }
    if !process_exists(session.pid) {
        return Err(format!("会话进程不存在：{}", session.wrapper_session_id));
    }
    Ok(session.clone())
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
    use super::*;
    use crate::claude_code_managed_session::{
        managed_claude_code_channel_id, update_registry, ManagedClaudeCodeSession,
        ManagedClaudeCodeSessionState,
    };
    use chrono::{TimeZone, Utc};

    fn managed_session(
        wrapper_session_id: &str,
        claude_session_id: &str,
    ) -> ManagedClaudeCodeSession {
        ManagedClaudeCodeSession {
            wrapper_session_id: wrapper_session_id.to_string(),
            state: ManagedClaudeCodeSessionState::Started,
            cwd: "/tmp/repo".to_string(),
            pid: Some(std::process::id()),
            control_socket: None,
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            claude_session_id: Some(claude_session_id.to_string()),
            transcript_path: None,
            bound_at: None,
            binding_failure_reason: None,
        }
    }

    #[test]
    fn send_instruction_rejects_mismatched_session_id_before_command_access() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("claude-code.json");
        update_registry(&registry_path, |registry| {
            registry.upsert(managed_session("niuma_claude_1", "session-1"));
        })
        .unwrap();

        let error = send_instruction_with_registry_path(
            &registry_path,
            "other-session",
            &managed_claude_code_channel_id("niuma_claude_1"),
            "继续",
        )
        .unwrap_err();

        assert_eq!(error, "channel_id 与 session_id 不匹配");
    }

    #[test]
    fn send_instruction_rejects_missing_live_control_without_resuming() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("claude-code.json");
        update_registry(&registry_path, |registry| {
            registry.upsert(managed_session("niuma_claude_1", "session-1"));
        })
        .unwrap();

        let error = send_instruction_with_registry_path(
            &registry_path,
            "session-1",
            &managed_claude_code_channel_id("niuma_claude_1"),
            "继续",
        )
        .unwrap_err();

        assert_eq!(error, "Claude Code 实时发送指令尚未实现");
    }
}
