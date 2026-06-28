use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::claude_code_managed_session::{
    read_registry, ManagedClaudeCodeSession, ManagedClaudeCodeSessionState,
};
use niuma_core::platform::paths::claude_code_managed_registry_path;
use serde_json::{json, Value};

pub(crate) fn claude_sessions() -> ApiResponse<Value> {
    let registry_path = claude_code_managed_registry_path();
    match read_registry(&registry_path) {
        Ok(registry) => ApiResponse::ok(active_sessions_response(
            registry_path.to_string_lossy().as_ref(),
            registry.sessions,
        )),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn active_sessions_response(registry_path: &str, sessions: Vec<ManagedClaudeCodeSession>) -> Value {
    active_sessions_response_with_probe(registry_path, sessions, process_exists)
}

fn active_sessions_response_with_probe(
    registry_path: &str,
    sessions: Vec<ManagedClaudeCodeSession>,
    process_exists: impl Fn(u32) -> bool,
) -> Value {
    let total_count = sessions.len();
    let active_sessions = sessions
        .into_iter()
        .filter_map(|session| {
            let pid_alive = session.pid.map(&process_exists).unwrap_or(false);
            (session.state != ManagedClaudeCodeSessionState::Exited
                && session.state != ManagedClaudeCodeSessionState::Unavailable
                && pid_alive)
                .then(|| session_summary(session, Some(pid_alive)))
        })
        .collect::<Vec<_>>();

    json!({
        "registry_path": registry_path,
        "sessions": active_sessions,
        "total_count": total_count,
        "active_count": active_sessions.len()
    })
}

fn session_summary(session: ManagedClaudeCodeSession, pid_alive: Option<bool>) -> Value {
    // CLI 查询只保留排查 niuma-claude managed 需要的关键字段，避免直接倾倒完整 registry。
    json!({
        "wrapper_session_id": session.wrapper_session_id,
        "state": session.state,
        "pid": session.pid,
        "pid_alive": pid_alive,
        "cwd": session.cwd,
        "started_at": session.started_at,
        "claude_session_id": session.claude_session_id,
        "transcript_path": session.transcript_path,
        "bound_at": session.bound_at,
        "binding_failure_reason": session.binding_failure_reason
    })
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error()
        .raw_os_error()
        .is_some_and(|code| code != libc::ESRCH)
}

#[cfg(not(unix))]
fn process_exists(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use niuma_core::claude_code_managed_session::{
        ManagedClaudeCodeSession, ManagedClaudeCodeSessionState,
    };

    fn session(
        wrapper_session_id: &str,
        state: ManagedClaudeCodeSessionState,
        pid: Option<u32>,
    ) -> ManagedClaudeCodeSession {
        ManagedClaudeCodeSession {
            wrapper_session_id: wrapper_session_id.to_string(),
            state,
            cwd: "/repo".to_string(),
            pid,
            control_socket: None,
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            claude_session_id: Some(format!("{wrapper_session_id}-session")),
            transcript_path: None,
            bound_at: None,
            binding_failure_reason: None,
        }
    }

    #[test]
    fn list_active_sessions_excludes_exited_and_dead_processes() {
        let value = super::active_sessions_response_with_probe(
            "/tmp/claude-code.json",
            vec![
                session(
                    "niuma_claude_active",
                    ManagedClaudeCodeSessionState::Started,
                    Some(100),
                ),
                session(
                    "niuma_claude_dead",
                    ManagedClaudeCodeSessionState::Started,
                    Some(200),
                ),
                session(
                    "niuma_claude_exited",
                    ManagedClaudeCodeSessionState::Exited,
                    Some(100),
                ),
            ],
            |pid| pid == 100,
        );

        assert_eq!(value["total_count"], 3);
        assert_eq!(value["active_count"], 1);
        assert_eq!(
            value["sessions"][0]["wrapper_session_id"],
            "niuma_claude_active"
        );
        assert_eq!(value["sessions"][0]["pid_alive"], true);
    }
}
