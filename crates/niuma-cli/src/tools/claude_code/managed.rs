use std::path::PathBuf;
use std::process::Command;

use niuma_core::claude_code_managed_session::{
    update_registry, ManagedClaudeCodeSession, ManagedClaudeCodeSessionState,
};
use niuma_core::platform::paths::claude_code_managed_registry_path;
use uuid::Uuid;

const REAL_CLAUDE_ENV: &str = "NIUMA_REAL_CLAUDE";

pub(crate) fn run_claude_command(args: Vec<String>) -> ! {
    match run_claude_command_inner(args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn run_claude_command_inner(mut args: Vec<String>) -> Result<i32, String> {
    let real_claude = resolve_real_claude()?;
    let wrapper_session_id = new_wrapper_session_id();
    let mut claude_session_id = session_id_from_args(&args);
    if claude_session_id.is_none() && !args_has_session_id(&args) {
        let generated_session_id = Uuid::new_v4().to_string();
        // 明确传入 session id，provider 绑定时不需要从目录名猜测会话身份。
        args.push("--session-id".to_string());
        args.push(generated_session_id.clone());
        claude_session_id = Some(generated_session_id);
    }
    let cwd = std::env::current_dir()
        .map_err(|error| format!("读取当前目录失败：{error}"))?
        .to_string_lossy()
        .to_string();
    let registry_path = claude_code_managed_registry_path();
    let started_at = chrono::Utc::now();
    update_registry(&registry_path, |registry| {
        registry.upsert(ManagedClaudeCodeSession {
            wrapper_session_id: wrapper_session_id.clone(),
            state: ManagedClaudeCodeSessionState::Started,
            cwd: cwd.clone(),
            pid: None,
            control_socket: None,
            started_at,
            claude_session_id: claude_session_id.clone(),
            transcript_path: None,
            bound_at: None,
            binding_failure_reason: None,
        });
    })?;

    let mut child = Command::new(&real_claude)
        .args(&args)
        .spawn()
        .map_err(|error| format!("启动 Claude Code 失败：{error}"))?;
    update_registry(&registry_path, |registry| {
        if let Some(session) = registry
            .sessions
            .iter_mut()
            .find(|session| session.wrapper_session_id == wrapper_session_id)
        {
            session.pid = Some(child.id());
        }
    })?;
    let status = child
        .wait()
        .map_err(|error| format!("等待 Claude Code 退出失败：{error}"))?;
    update_registry(&registry_path, |registry| {
        if let Some(session) = registry
            .sessions
            .iter_mut()
            .find(|session| session.wrapper_session_id == wrapper_session_id)
        {
            session.state = ManagedClaudeCodeSessionState::Exited;
        }
    })?;
    Ok(status.code().unwrap_or(1))
}

fn new_wrapper_session_id() -> String {
    format!("niuma_claude_{}", Uuid::new_v4().simple())
}

pub(super) fn resolve_real_claude() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(REAL_CLAUDE_ENV).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(format!("{REAL_CLAUDE_ENV} 必须是绝对路径"));
        }
        return Ok(path);
    }
    let current_exe = std::env::current_exe().ok();
    let path = which::which("claude").map_err(|error| format!("找不到真实 claude：{error}"))?;
    if current_exe.as_ref() == Some(&path) {
        return Err("PATH 中的 claude 指向当前 Niuma wrapper，拒绝递归启动".to_string());
    }
    Ok(path)
}

fn args_has_session_id(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--session-id" || arg.starts_with("--session-id="))
}

fn session_id_from_args(args: &[String]) -> Option<String> {
    for (index, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix("--session-id=") {
            return non_empty_session_id(value);
        }
        if arg == "--session-id" {
            return args
                .get(index + 1)
                .and_then(|value| non_empty_session_id(value));
        }
    }
    None
}

fn non_empty_session_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_wrapper_session_id_uses_claude_prefix() {
        let id = new_wrapper_session_id();

        assert!(id.starts_with("niuma_claude_"));
    }

    #[test]
    fn real_claude_lookup_prefers_env_override() {
        let temp = tempfile::tempdir().unwrap();
        let fake = temp.path().join("claude");
        std::fs::write(&fake, "#!/bin/sh\n").unwrap();
        std::env::set_var("NIUMA_REAL_CLAUDE", &fake);

        let resolved = resolve_real_claude().unwrap();

        std::env::remove_var("NIUMA_REAL_CLAUDE");
        assert_eq!(resolved, fake);
    }

    #[test]
    fn session_id_from_args_reads_standalone_and_equals_forms() {
        assert_eq!(
            session_id_from_args(&["--session-id".to_string(), "user-session".to_string()]),
            Some("user-session".to_string())
        );
        assert_eq!(
            session_id_from_args(&["--session-id=user-session".to_string()]),
            Some("user-session".to_string())
        );
    }

    #[test]
    fn session_id_from_args_ignores_missing_value() {
        assert_eq!(session_id_from_args(&["--session-id".to_string()]), None);
        assert_eq!(session_id_from_args(&["--session-id=".to_string()]), None);
    }
}
