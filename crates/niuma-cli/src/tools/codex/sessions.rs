use niuma_core::api_response::ApiResponse;
use niuma_core::codex_managed_session::{
    read_registry, ManagedCodexSession, ManagedCodexSessionState,
};
use niuma_core::platform::paths::codex_managed_registry_path;
use serde_json::{json, Value};
use std::time::Duration;

const CONTROL_PROBE_TIMEOUT: Duration = Duration::from_millis(200);

pub(crate) fn codex_sessions() -> ApiResponse<Value> {
    let registry_path = codex_managed_registry_path();
    match read_registry(&registry_path) {
        Ok(registry) => ApiResponse::ok(active_sessions_response(
            registry_path.to_string_lossy().as_ref(),
            registry.sessions,
        )),
        Err(error) => ApiResponse::fail(niuma_core::api_response::ApiErrorCode::System, error),
    }
}

fn active_sessions_response(registry_path: &str, sessions: Vec<ManagedCodexSession>) -> Value {
    active_sessions_response_with_probes(
        registry_path,
        sessions,
        process_exists,
        control_socket_responds,
    )
}

fn active_sessions_response_with_probes(
    registry_path: &str,
    sessions: Vec<ManagedCodexSession>,
    process_exists: impl Fn(u32) -> bool,
    control_socket_responds: impl Fn(&ManagedCodexSession) -> bool,
) -> Value {
    let total_count = sessions.len();
    let active_sessions = sessions
        .into_iter()
        .filter_map(|session| {
            let pid_alive = session.pid.map(&process_exists).unwrap_or(false);
            let control_socket_responsive = pid_alive && control_socket_responds(&session);
            (session.state != ManagedCodexSessionState::Exited && control_socket_responsive)
                .then(|| session_summary(session, Some(pid_alive), Some(control_socket_responsive)))
        })
        .collect::<Vec<_>>();

    json!({
        "registry_path": registry_path,
        "sessions": active_sessions,
        "total_count": total_count,
        "active_count": active_sessions.len()
    })
}

fn session_summary(
    session: ManagedCodexSession,
    pid_alive: Option<bool>,
    control_socket_responsive: Option<bool>,
) -> Value {
    // CLI 查询只保留排查 niuma-codex managed relay 需要的关键字段，避免直接倾倒完整 registry。
    json!({
        "wrapper_session_id": session.wrapper_session_id,
        "state": session.state,
        "pid": session.pid,
        "pid_alive": pid_alive,
        "control_socket_responsive": control_socket_responsive,
        "cwd": session.cwd,
        "started_at": session.started_at,
        "first_user_message_preview": session.first_user_message_preview,
        "first_user_message_submitted_at": session.first_user_message_submitted_at,
        "codex_session_id": session.codex_session_id,
        "codex_session_file_path": session.codex_session_file_path,
        "bound_at": session.bound_at,
        "control_socket": session.control_socket,
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
    // 非 Unix 平台先保持保守展示；后续如支持 Windows managed relay，再接平台化进程探测。
    true
}

#[cfg(unix)]
fn control_socket_responds(session: &ManagedCodexSession) -> bool {
    use std::io::{ErrorKind, Read, Write};
    use std::os::unix::net::UnixStream;

    let Ok(mut stream) = UnixStream::connect(&session.control_socket) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(CONTROL_PROBE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONTROL_PROBE_TIMEOUT));
    if stream.write_all(b"{\"type\":\"requests\"}\n").is_err() {
        return false;
    }

    let mut body = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => body.extend_from_slice(&buffer[..size]),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                break
            }
            Err(_) => return false,
        }
    }

    !body.is_empty() && String::from_utf8_lossy(&body).contains("\"ok\":true")
}

#[cfg(not(unix))]
fn control_socket_responds(_session: &ManagedCodexSession) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use niuma_core::codex_managed_session::{ManagedCodexSession, ManagedCodexSessionState};
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    fn session(
        wrapper_session_id: &str,
        state: ManagedCodexSessionState,
        pid: Option<u32>,
    ) -> ManagedCodexSession {
        ManagedCodexSession {
            wrapper_session_id: wrapper_session_id.to_string(),
            state,
            state_changed_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            cwd: "/repo".to_string(),
            pid,
            real_socket: "/tmp/real.sock".to_string(),
            relay_socket: "/tmp/relay.sock".to_string(),
            control_socket: "/tmp/control.sock".to_string(),
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            first_user_message_hash: None,
            first_user_message_preview: Some("你好".to_string()),
            first_user_message_submitted_at: None,
            codex_session_id: Some("codex-session-1".to_string()),
            codex_session_file_path: Some("/codex/session.jsonl".to_string()),
            bound_at: None,
            binding_failure_reason: None,
        }
    }

    #[test]
    fn list_active_sessions_excludes_exited_sessions_and_reports_pid_liveness() {
        let sessions = vec![
            session(
                "niuma_codex_active",
                ManagedCodexSessionState::Bound,
                Some(100),
            ),
            session(
                "niuma_codex_stale",
                ManagedCodexSessionState::Bound,
                Some(200),
            ),
            session(
                "niuma_codex_exited",
                ManagedCodexSessionState::Exited,
                Some(100),
            ),
        ];

        let value = super::active_sessions_response_with_probes(
            "/tmp/codex.json",
            sessions,
            |pid| pid == 100,
            |_| true,
        );

        assert_eq!(value["total_count"], 3);
        assert_eq!(value["active_count"], 1);
        assert_eq!(
            value["sessions"][0]["wrapper_session_id"],
            "niuma_codex_active"
        );
        assert_eq!(value["sessions"][0]["pid_alive"], true);
    }

    #[test]
    fn list_active_sessions_excludes_alive_but_unresponsive_sessions() {
        let sessions = vec![
            session(
                "niuma_codex_responsive",
                ManagedCodexSessionState::Bound,
                Some(100),
            ),
            session(
                "niuma_codex_suspended",
                ManagedCodexSessionState::Bound,
                Some(200),
            ),
        ];

        let value = super::active_sessions_response_with_probes(
            "/tmp/codex.json",
            sessions,
            |_| true,
            |session| session.wrapper_session_id == "niuma_codex_responsive",
        );

        assert_eq!(value["active_count"], 1);
        assert_eq!(
            value["sessions"][0]["wrapper_session_id"],
            "niuma_codex_responsive"
        );
        assert_eq!(value["sessions"][0]["control_socket_responsive"], true);
    }

    #[cfg(unix)]
    #[test]
    fn control_socket_probe_accepts_long_requests_response() {
        let socket_path = std::env::temp_dir().join(format!(
            "niuma-codex-sessions-test-{}.sock",
            std::process::id()
        ));
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 128];
            let _ = stream.read(&mut request).unwrap();
            let long_prefix = "x".repeat(400);
            // 真实 requests 响应可能很长，ok 字段在响应末尾时也应被识别。
            let response = format!(
                r#"{{"approvals":[],"inputs":[{{"questions":"{long_prefix}"}}],"ok":true}}"#
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let mut managed = session(
            "niuma_codex_responsive",
            ManagedCodexSessionState::Bound,
            Some(100),
        );
        managed.control_socket = socket_path.to_string_lossy().to_string();

        assert!(super::control_socket_responds(&managed));

        handle.join().unwrap();
        let _ = fs::remove_file(socket_path);
    }
}
