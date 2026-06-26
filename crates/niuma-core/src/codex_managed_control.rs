use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

use crate::codex_managed_session::{read_registry, ManagedCodexSession, ManagedCodexSessionState};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

pub fn send_instruction(
    registry_path: &Path,
    session_id: &str,
    wrapper_session_id: &str,
    content: &str,
) -> Result<Value, String> {
    if content.trim().is_empty() {
        return Err("content 不能为空".to_string());
    }
    let session = managed_session_for_control(registry_path, session_id, wrapper_session_id)?;
    let command = json!({
        "type": "send_instruction",
        "content": content
    });
    ensure_control_ok(relay_control_command(&session.control_socket, &command)?)
}

pub fn answer_input(
    registry_path: &Path,
    session_id: &str,
    wrapper_session_id: &str,
    request_id: &str,
    answers: &Value,
) -> Result<Value, String> {
    let request_id = request_id.trim();
    if request_id.is_empty() {
        return Err("request_id 不能为空".to_string());
    }
    let Some(answer_map) = answers.as_object() else {
        return Err("answers 不能为空".to_string());
    };
    if answer_map.is_empty() {
        return Err("answers 不能为空".to_string());
    }
    if !answer_map.values().all(is_non_empty_string_array) {
        return Err("answers 必须是字符串数组对象".to_string());
    }
    let session = managed_session_for_control(registry_path, session_id, wrapper_session_id)?;
    let command = json!({
        "type": "answer_input",
        "request_id": request_id,
        "answers": answers
    });
    ensure_control_ok(relay_control_command(&session.control_socket, &command)?)
}

pub fn interrupt(
    registry_path: &Path,
    session_id: &str,
    wrapper_session_id: &str,
) -> Result<Value, String> {
    let session = managed_session_for_control(registry_path, session_id, wrapper_session_id)?;
    ensure_control_ok(relay_control_command(
        &session.control_socket,
        &json!({ "type": "interrupt" }),
    )?)
}

fn managed_session_for_control(
    registry_path: &Path,
    session_id: &str,
    wrapper_session_id: &str,
) -> Result<ManagedCodexSession, String> {
    if session_id.trim().is_empty() {
        return Err("session_id 不能为空".to_string());
    }
    if !wrapper_session_id.starts_with("niuma_codex_") {
        return Err("wrapper_session_id 必须以 niuma_codex_ 开头".to_string());
    }
    let registry = read_registry(registry_path)?;
    let session = registry
        .sessions
        .iter()
        .find(|session| session.wrapper_session_id == wrapper_session_id)
        .ok_or_else(|| format!("找不到 niuma-codex 会话：{wrapper_session_id}"))?;
    if session.codex_session_id.as_deref() != Some(session_id) {
        return Err("wrapper_session_id 与 session_id 不匹配".to_string());
    }
    if session.state != ManagedCodexSessionState::Bound {
        return Err(format!("会话不可控制：{}", session.wrapper_session_id));
    }
    if !process_exists(session.pid) {
        return Err(format!("会话进程不存在：{}", session.wrapper_session_id));
    }
    Ok(session.clone())
}

fn process_exists(pid: Option<u32>) -> bool {
    pid.map(process_exists_by_pid).unwrap_or(false)
}

fn is_non_empty_string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| !items.is_empty() && items.iter().all(Value::is_string))
}

fn ensure_control_ok(response: Value) -> Result<Value, String> {
    if response.get("ok").and_then(Value::as_bool) == Some(false) {
        // control socket 明确拒绝时，向上返回 message，避免 API 误判为成功。
        return Err(response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("control socket command 失败")
            .to_string());
    }
    Ok(response)
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
fn relay_control_command(control_socket: &str, command: &Value) -> Result<Value, String> {
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
        .write_all(format!("{command}\n").as_bytes())
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
fn relay_control_command(_control_socket: &str, _command: &Value) -> Result<Value, String> {
    Err("niuma-codex control 当前仅支持 Unix socket 平台".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_managed_session::{
        write_registry_atomic, ManagedCodexRegistry, ManagedCodexSession,
    };
    use chrono::{TimeZone, Utc};

    fn registry_with_session(session: ManagedCodexSession) -> ManagedCodexRegistry {
        ManagedCodexRegistry {
            version: 1,
            sessions: vec![session],
        }
    }

    fn managed_session(wrapper_session_id: &str, codex_session_id: &str) -> ManagedCodexSession {
        ManagedCodexSession {
            wrapper_session_id: wrapper_session_id.to_string(),
            state: ManagedCodexSessionState::Bound,
            cwd: "/tmp/repo".to_string(),
            pid: Some(std::process::id()),
            real_socket: "/tmp/real.sock".to_string(),
            relay_socket: "/tmp/relay.sock".to_string(),
            control_socket: "/tmp/missing-control.sock".to_string(),
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            first_user_message_hash: None,
            first_user_message_preview: None,
            first_user_message_submitted_at: None,
            codex_session_id: Some(codex_session_id.to_string()),
            codex_session_file_path: None,
            bound_at: None,
            binding_failure_reason: None,
        }
    }

    #[test]
    fn send_instruction_rejects_mismatched_session_id_before_socket_access() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        write_registry_atomic(
            &registry_path,
            &registry_with_session(managed_session("niuma_codex_1", "session-1")),
        )
        .unwrap();

        let error =
            send_instruction(&registry_path, "other-session", "niuma_codex_1", "继续").unwrap_err();

        assert_eq!(error, "wrapper_session_id 与 session_id 不匹配");
    }

    #[test]
    fn answer_input_rejects_empty_answers_before_socket_access() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        write_registry_atomic(
            &registry_path,
            &registry_with_session(managed_session("niuma_codex_1", "session-1")),
        )
        .unwrap();

        let error = answer_input(
            &registry_path,
            "session-1",
            "niuma_codex_1",
            "request-1",
            &json!({}),
        )
        .unwrap_err();

        assert_eq!(error, "answers 不能为空");
    }

    #[test]
    fn answer_input_rejects_mismatched_session_id_before_socket_access() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        write_registry_atomic(
            &registry_path,
            &registry_with_session(managed_session("niuma_codex_1", "session-1")),
        )
        .unwrap();

        let error = answer_input(
            &registry_path,
            "other-session",
            "niuma_codex_1",
            "request-1",
            &json!({ "choice": ["yes"] }),
        )
        .unwrap_err();

        assert_eq!(error, "wrapper_session_id 与 session_id 不匹配");
    }

    #[test]
    fn answer_input_rejects_empty_request_id_before_socket_access() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        write_registry_atomic(
            &registry_path,
            &registry_with_session(managed_session("niuma_codex_1", "session-1")),
        )
        .unwrap();

        let error = answer_input(
            &registry_path,
            "session-1",
            "niuma_codex_1",
            "  ",
            &json!({ "choice": ["yes"] }),
        )
        .unwrap_err();

        assert_eq!(error, "request_id 不能为空");
    }

    #[test]
    fn answer_input_rejects_non_array_answer_values_before_socket_access() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        write_registry_atomic(
            &registry_path,
            &registry_with_session(managed_session("niuma_codex_1", "session-1")),
        )
        .unwrap();

        let error = answer_input(
            &registry_path,
            "session-1",
            "niuma_codex_1",
            "request-1",
            &json!({ "choice": "yes" }),
        )
        .unwrap_err();

        assert_eq!(error, "answers 必须是字符串数组对象");
    }

    #[cfg(unix)]
    #[test]
    fn answer_input_trims_request_id_and_returns_control_error_message() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        let control_socket = dir.path().join("control.sock");
        let listener = UnixListener::bind(&control_socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let command: Value = serde_json::from_str(line.trim_end()).unwrap();
            assert_eq!(command["type"], "answer_input");
            assert_eq!(command["request_id"], "request-1");
            assert_eq!(command["answers"], json!({ "choice": ["yes"] }));
            writeln!(
                stream,
                "{}",
                json!({ "ok": false, "message": "输入请求已过期" })
            )
            .unwrap();
        });
        let mut session = managed_session("niuma_codex_1", "session-1");
        session.control_socket = control_socket.to_string_lossy().to_string();
        write_registry_atomic(&registry_path, &registry_with_session(session)).unwrap();

        let error = answer_input(
            &registry_path,
            "session-1",
            "niuma_codex_1",
            " request-1 ",
            &json!({ "choice": ["yes"] }),
        )
        .unwrap_err();
        server.join().unwrap();

        assert_eq!(error, "输入请求已过期");
    }

    #[cfg(unix)]
    #[test]
    fn send_instruction_returns_control_error_message() {
        use std::io::Write;
        use std::os::unix::net::UnixListener;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        let control_socket = dir.path().join("control.sock");
        let listener = UnixListener::bind(&control_socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            writeln!(stream, "{}", json!({ "ok": false, "message": "发送失败" })).unwrap();
        });
        let mut session = managed_session("niuma_codex_1", "session-1");
        session.control_socket = control_socket.to_string_lossy().to_string();
        write_registry_atomic(&registry_path, &registry_with_session(session)).unwrap();

        let error =
            send_instruction(&registry_path, "session-1", "niuma_codex_1", "继续").unwrap_err();
        server.join().unwrap();

        assert_eq!(error, "发送失败");
    }

    #[cfg(unix)]
    #[test]
    fn interrupt_returns_control_error_message() {
        use std::io::Write;
        use std::os::unix::net::UnixListener;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        let control_socket = dir.path().join("control.sock");
        let listener = UnixListener::bind(&control_socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            writeln!(stream, "{}", json!({ "ok": false, "message": "中断失败" })).unwrap();
        });
        let mut session = managed_session("niuma_codex_1", "session-1");
        session.control_socket = control_socket.to_string_lossy().to_string();
        write_registry_atomic(&registry_path, &registry_with_session(session)).unwrap();

        let error = interrupt(&registry_path, "session-1", "niuma_codex_1").unwrap_err();
        server.join().unwrap();

        assert_eq!(error, "中断失败");
    }
}
