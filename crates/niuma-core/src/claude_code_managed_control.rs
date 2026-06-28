use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::claude_code_managed_session::{
    read_registry, wrapper_session_id_from_channel_id, ManagedClaudeCodeSession,
    ManagedClaudeCodeSessionState,
};

const REAL_CLAUDE_ENV: &str = "NIUMA_REAL_CLAUDE";

pub fn send_instruction(
    registry_path: &Path,
    session_id: &str,
    channel_id: &str,
    content: &str,
) -> Result<Value, String> {
    let claude_command = resolve_real_claude_command()?;
    send_instruction_with_command(
        registry_path,
        session_id,
        channel_id,
        content,
        &claude_command,
    )
}

pub fn interrupt(
    registry_path: &Path,
    session_id: &str,
    channel_id: &str,
) -> Result<Value, String> {
    let _session = managed_session_for_control(registry_path, session_id, channel_id)?;
    Err("Claude Code active turn interrupt 尚未实现".to_string())
}

fn send_instruction_with_command(
    registry_path: &Path,
    session_id: &str,
    channel_id: &str,
    content: &str,
    claude_command: &Path,
) -> Result<Value, String> {
    if content.trim().is_empty() {
        return Err("content 不能为空".to_string());
    }
    let session = managed_session_for_control(registry_path, session_id, channel_id)?;
    let claude_session_id = session.claude_session_id.as_deref().ok_or_else(|| {
        format!(
            "会话尚未绑定 Claude session id：{}",
            session.wrapper_session_id
        )
    })?;
    let status = Command::new(claude_command)
        .arg("--resume")
        .arg(claude_session_id)
        .arg(content)
        .status()
        .map_err(|error| format!("执行 claude --resume 失败：{error}"))?;
    if !status.success() {
        return Err(format!(
            "claude --resume 退出失败：{}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "无退出码".to_string())
        ));
    }
    Ok(json!({
        "wrapper_session_id": session.wrapper_session_id,
        "claude_session_id": claude_session_id,
        "exit_code": status.code()
    }))
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
    if session.state == ManagedClaudeCodeSessionState::Unavailable {
        return Err(format!("session control channel 不可用：{channel_id}"));
    }
    Ok(session.clone())
}

fn resolve_real_claude_command() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(REAL_CLAUDE_ENV).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(format!("{REAL_CLAUDE_ENV} 必须是绝对路径"));
        }
        return Ok(path);
    }
    // API 进程不是 claude wrapper；未配置覆盖时直接使用 PATH 中的 claude。
    Ok(PathBuf::from("claude"))
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

        let error = send_instruction_with_command(
            &registry_path,
            "other-session",
            &managed_claude_code_channel_id("niuma_claude_1"),
            "继续",
            Path::new("/missing/claude"),
        )
        .unwrap_err();

        assert_eq!(error, "channel_id 与 session_id 不匹配");
    }

    #[cfg(unix)]
    #[test]
    fn send_instruction_runs_claude_resume_with_bound_session_id() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("claude-code.json");
        let args_path = dir.path().join("args.txt");
        let fake_claude = dir.path().join("claude");
        std::fs::write(
            &fake_claude,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                args_path.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_claude, permissions).unwrap();
        update_registry(&registry_path, |registry| {
            registry.upsert(managed_session("niuma_claude_1", "session-1"));
        })
        .unwrap();

        let result = send_instruction_with_command(
            &registry_path,
            "session-1",
            &managed_claude_code_channel_id("niuma_claude_1"),
            "继续",
            &fake_claude,
        )
        .unwrap();

        assert_eq!(result["claude_session_id"], "session-1");
        assert_eq!(
            std::fs::read_to_string(args_path).unwrap(),
            "--resume\nsession-1\n继续\n"
        );
    }
}
