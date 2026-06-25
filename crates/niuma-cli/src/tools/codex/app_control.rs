use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_managed_session::{
    update_registry, ManagedCodexSession, ManagedCodexSessionState,
};
use niuma_core::platform::paths::codex_managed_registry_path;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

// Task 4 会从 app-server transport 填充该结构，本任务先固定可测试状态输入。
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct AppServerRequest {
    pub jsonrpc_id: Value,
    pub method: String,
    pub params: Value,
}

// Relay 内存态只保存当前 control socket 需要展示或回包的 pending request。
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppControlState {
    pub wrapper_session_id: String,
    pub pending_approvals: Vec<PendingApproval>,
    pub pending_inputs: Vec<PendingInput>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub request_id: String,
    pub relay_request_id: String,
    pub relay_jsonrpc_id: Value,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub command: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingInput {
    pub request_id: String,
    pub relay_request_id: String,
    pub relay_jsonrpc_id: Value,
    pub questions: Value,
}

// Control socket 第一版使用 JSON Lines，外层 type 字段保持 snake_case。
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlCommand {
    Requests,
    ApprovalDecision {
        request_id: String,
        decision: String,
    },
    AnswerInput {
        request_id: String,
        answers: Value,
    },
    SendInstruction {
        content: String,
    },
    Interrupt,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedWebSocketMessages {
    pub messages: Vec<Value>,
    pub rest: Vec<u8>,
}

#[allow(dead_code)]
pub fn encode_websocket_text_frame(text: &str, mask: bool) -> Vec<u8> {
    let payload = text.as_bytes();
    let mut output = Vec::new();
    output.push(0x81);

    let mask_bit = if mask { 0x80 } else { 0 };
    if payload.len() < 126 {
        output.push(mask_bit | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        output.push(mask_bit | 126);
        output.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        output.push(mask_bit | 127);
        output.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }

    if mask {
        let key = [0x12, 0x34, 0x56, 0x78];
        output.extend_from_slice(&key);
        for (index, byte) in payload.iter().enumerate() {
            output.push(byte ^ key[index % 4]);
        }
    } else {
        output.extend_from_slice(payload);
    }

    output
}

#[allow(dead_code)]
pub fn parse_websocket_text_frames(buffer: &[u8]) -> Result<ParsedWebSocketMessages, String> {
    let mut offset = 0usize;
    let mut messages = Vec::new();

    while buffer.len().saturating_sub(offset) >= 2 {
        let frame_start = offset;
        let first = buffer[offset];
        let second = buffer[offset + 1];
        let opcode = first & 0x0f;
        let masked = second & 0x80 != 0;
        let mut payload_len = (second & 0x7f) as usize;
        offset += 2;

        if opcode == 0x8 {
            offset += payload_len;
            continue;
        }
        if opcode != 0x1 {
            return Err("只支持 WebSocket text frame".to_string());
        }

        if payload_len == 126 {
            if buffer.len().saturating_sub(offset) < 2 {
                return Ok(ParsedWebSocketMessages {
                    messages,
                    rest: buffer[frame_start..].to_vec(),
                });
            }
            payload_len = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]) as usize;
            offset += 2;
        } else if payload_len == 127 {
            if buffer.len().saturating_sub(offset) < 8 {
                return Ok(ParsedWebSocketMessages {
                    messages,
                    rest: buffer[frame_start..].to_vec(),
                });
            }
            let len = u64::from_be_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
                buffer[offset + 4],
                buffer[offset + 5],
                buffer[offset + 6],
                buffer[offset + 7],
            ]);
            payload_len = usize::try_from(len).map_err(|_| "WebSocket frame 过大".to_string())?;
            offset += 8;
        }

        let mask_key = if masked {
            if buffer.len().saturating_sub(offset) < 4 {
                return Ok(ParsedWebSocketMessages {
                    messages,
                    rest: buffer[frame_start..].to_vec(),
                });
            }
            let key = [
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ];
            offset += 4;
            Some(key)
        } else {
            None
        };

        if buffer.len().saturating_sub(offset) < payload_len {
            return Ok(ParsedWebSocketMessages {
                messages,
                rest: buffer[frame_start..].to_vec(),
            });
        }

        let mut payload = buffer[offset..offset + payload_len].to_vec();
        if let Some(key) = mask_key {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= key[index % 4];
            }
        }

        let value = serde_json::from_slice(&payload)
            .map_err(|error| format!("解析 app-server JSON-RPC frame 失败：{error}"))?;
        messages.push(value);
        offset += payload_len;
    }

    Ok(ParsedWebSocketMessages {
        messages,
        rest: buffer[offset..].to_vec(),
    })
}

#[allow(dead_code)]
pub fn parse_control_command_line(line: &str) -> Result<ControlCommand, String> {
    serde_json::from_str(line.trim_end())
        .map_err(|error| format!("解析 control command 失败：{error}"))
}

#[allow(dead_code)]
impl AppControlState {
    pub fn observe_server_request(&mut self, request: AppServerRequest) {
        match request.method.as_str() {
            "item/commandExecution/requestApproval" => {
                self.observe_approval_request(request.jsonrpc_id, request.params);
            }
            "item/tool/requestUserInput" => {
                self.observe_input_request(request.jsonrpc_id, request.params);
            }
            _ => {}
        }
    }

    fn observe_approval_request(&mut self, jsonrpc_id: Value, params: Value) {
        let relay_request_id = jsonrpc_id_key(&jsonrpc_id);
        let turn_id = params
            .get("turnId")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let item_id = params
            .get("itemId")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let stable_turn = turn_id
            .clone()
            .unwrap_or_else(|| "unknown-turn".to_string());
        let stable_item = item_id.clone().unwrap_or_else(|| relay_request_id.clone());

        self.pending_approvals.push(PendingApproval {
            request_id: format!(
                "codex-relay:{}:{}:{}",
                self.wrapper_session_id, stable_turn, stable_item
            ),
            relay_request_id,
            relay_jsonrpc_id: jsonrpc_id,
            turn_id,
            item_id,
            command: params
                .get("command")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        });
    }

    fn observe_input_request(&mut self, jsonrpc_id: Value, params: Value) {
        let relay_request_id = jsonrpc_id_key(&jsonrpc_id);
        self.pending_inputs.push(PendingInput {
            request_id: format!(
                "codex-input:{}:{}",
                self.wrapper_session_id, relay_request_id
            ),
            relay_request_id,
            relay_jsonrpc_id: jsonrpc_id,
            questions: params
                .get("questions")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        });
    }

    pub fn observe_client_message(&mut self, message: &Value) {
        if message.get("method").is_some() {
            return;
        }
        let Some(id) = message.get("id") else {
            return;
        };
        let relay_request_id = jsonrpc_id_key(id);
        self.pending_approvals
            .retain(|approval| approval.relay_request_id != relay_request_id);
        self.pending_inputs
            .retain(|input| input.relay_request_id != relay_request_id);
    }

    pub fn resolve_approval_decision_frame(
        &mut self,
        request_id: &str,
        decision: &str,
    ) -> Result<Vec<u8>, String> {
        let index = self
            .pending_approvals
            .iter()
            .position(|approval| approval.request_id == request_id)
            .ok_or_else(|| format!("找不到待处理权限请求：{request_id}"))?;
        let approval = self.pending_approvals.remove(index);
        let payload = json!({
            "id": approval.relay_jsonrpc_id,
            "result": { "decision": decision },
        });
        Ok(encode_websocket_text_frame(&payload.to_string(), true))
    }

    pub fn resolve_input_answer_frame(
        &mut self,
        request_id: &str,
        answers: Value,
    ) -> Result<Vec<u8>, String> {
        let index = self
            .pending_inputs
            .iter()
            .position(|input| input.request_id == request_id)
            .ok_or_else(|| format!("找不到待回答输入请求：{request_id}"))?;
        let input = self.pending_inputs.remove(index);
        let payload = json!({
            "id": input.relay_jsonrpc_id,
            "result": { "answers": answers },
        });
        Ok(encode_websocket_text_frame(&payload.to_string(), true))
    }
}

fn jsonrpc_id_key(jsonrpc_id: &Value) -> String {
    // request_id 需要稳定且便于查找；字符串 id 不保留 JSON 引号，复杂值保留紧凑 JSON。
    match jsonrpc_id {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => serde_json::to_string(jsonrpc_id).unwrap_or_else(|_| jsonrpc_id.to_string()),
    }
}

pub fn run_app_control(real_codex: PathBuf, args: Vec<String>) -> ApiResponse<Value> {
    match run_app_control_inner(real_codex, args) {
        Ok(code) => ApiResponse::ok(json!({ "mode": "managed", "exit_code": code })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn run_app_control_inner(real_codex: PathBuf, args: Vec<String>) -> Result<i32, String> {
    let wrapper_session_id = format!("niuma_codex_{}", uuid::Uuid::new_v4().simple());
    let registry_path = codex_managed_registry_path();
    let now = chrono::Utc::now();
    let cwd = std::env::current_dir()
        .map_err(|error| format!("读取当前目录失败：{error}"))?
        .to_string_lossy()
        .to_string();
    let base_dir = managed_socket_base_dir(&wrapper_session_id);

    std::fs::create_dir_all(&base_dir)
        .map_err(|error| format!("创建 niuma-codex socket 目录失败：{error}"))?;

    let session = ManagedCodexSession {
        wrapper_session_id: wrapper_session_id.clone(),
        state: ManagedCodexSessionState::WaitingFirstUserMessage,
        cwd,
        pid: Some(std::process::id()),
        real_socket: base_dir.join("real.sock").to_string_lossy().to_string(),
        relay_socket: base_dir.join("relay.sock").to_string_lossy().to_string(),
        control_socket: base_dir.join("control.sock").to_string_lossy().to_string(),
        started_at: now,
        first_user_message_hash: None,
        first_user_message_preview: None,
        first_user_message_submitted_at: None,
        codex_session_id: None,
        codex_session_file_path: None,
        bound_at: None,
        binding_failure_reason: None,
    };

    update_registry(&registry_path, |registry| registry.upsert(session))?;

    match run_app_server_remote_processes(
        real_codex,
        args,
        wrapper_session_id.clone(),
        base_dir.clone(),
    ) {
        Ok(code) => Ok(code),
        Err(error) => {
            let mark_result =
                mark_session_exited_after_failure(&registry_path, &wrapper_session_id, &error);
            cleanup_socket_base_dir(&base_dir);
            match mark_result {
                Ok(()) => Err(error),
                Err(mark_error) => Err(format!(
                    "{error}；标记 Codex managed session 退出失败：{mark_error}"
                )),
            }
        }
    }
}

fn managed_socket_base_dir(wrapper_session_id: &str) -> PathBuf {
    // Unix domain socket 路径受 SUN_LEN 限制，不能放在较长的 app data 目录下。
    let short_id = wrapper_session_id
        .strip_prefix("niuma_codex_")
        .unwrap_or(wrapper_session_id)
        .chars()
        .take(12)
        .collect::<String>();
    #[cfg(unix)]
    {
        PathBuf::from("/tmp").join("niuma-codex").join(short_id)
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join("niuma-codex").join(short_id)
    }
}

fn mark_session_exited_after_failure(
    registry_path: &Path,
    wrapper_session_id: &str,
    reason: &str,
) -> Result<(), String> {
    update_registry(registry_path, |registry| {
        if let Some(session) = registry
            .sessions
            .iter_mut()
            .find(|session| session.wrapper_session_id == wrapper_session_id)
        {
            session.state = ManagedCodexSessionState::Exited;
            session.binding_failure_reason = Some(reason.to_string());
        }
    })
    .map(|_| ())
}

fn cleanup_socket_base_dir(base_dir: &Path) {
    // Task 4 前 transport 可能尚未创建 socket 文件；不存在时不影响原始错误返回。
    if let Err(error) = std::fs::remove_dir_all(base_dir) {
        if error.kind() != std::io::ErrorKind::NotFound {
            let _ = error;
        }
    }
}

fn run_app_server_remote_processes(
    real_codex: PathBuf,
    args: Vec<String>,
    wrapper_session_id: String,
    base_dir: PathBuf,
) -> Result<i32, String> {
    run_app_server_remote_processes_impl(real_codex, args, wrapper_session_id, base_dir)
}

#[cfg(not(unix))]
fn run_app_server_remote_processes_impl(
    _real_codex: PathBuf,
    _args: Vec<String>,
    _wrapper_session_id: String,
    _base_dir: PathBuf,
) -> Result<i32, String> {
    Err("niuma-codex managed mode 当前仅支持 Unix socket 平台".to_string())
}

#[cfg(unix)]
fn run_app_server_remote_processes_impl(
    real_codex: PathBuf,
    args: Vec<String>,
    wrapper_session_id: String,
    base_dir: PathBuf,
) -> Result<i32, String> {
    use std::os::unix::net::UnixStream;

    let real_socket = base_dir.join("real.sock");
    let relay_socket = base_dir.join("relay.sock");
    let control_socket = base_dir.join("control.sock");
    remove_socket_if_exists(&real_socket)?;
    remove_socket_if_exists(&relay_socket)?;
    remove_socket_if_exists(&control_socket)?;

    let mut server = std::process::Command::new(&real_codex)
        .args(["app-server", "--listen"])
        .arg(format!("unix://{}", real_socket.display()))
        .spawn()
        .map_err(|error| format!("启动 codex app-server 失败：{error}"))?;
    if let Err(error) = wait_for_socket(&real_socket, Duration::from_secs(5)) {
        let _ = server.kill();
        let _ = server.wait();
        return Err(error);
    }

    let shared = Arc::new(RelaySharedState::new(wrapper_session_id));
    let relay_handle = match start_relay_thread(
        real_socket.clone(),
        relay_socket.clone(),
        Arc::clone(&shared),
    ) {
        Ok(handle) => handle,
        Err(error) => {
            let _ = server.kill();
            let _ = server.wait();
            return Err(error);
        }
    };
    if let Err(error) = wait_for_socket(&relay_socket, Duration::from_secs(5)) {
        let _ = server.kill();
        let _ = server.wait();
        relay_handle.request_stop();
        relay_handle.join();
        return Err(error);
    }
    let control_handle = match start_control_thread(control_socket.clone(), Arc::clone(&shared)) {
        Ok(handle) => handle,
        Err(error) => {
            let _ = server.kill();
            let _ = server.wait();
            relay_handle.request_stop();
            let _ = UnixStream::connect(&relay_socket);
            relay_handle.join();
            return Err(error);
        }
    };
    if let Err(error) = wait_for_socket(&control_socket, Duration::from_secs(5)) {
        let _ = server.kill();
        let _ = server.wait();
        relay_handle.request_stop();
        control_handle.request_stop();
        let _ = UnixStream::connect(&relay_socket);
        let _ = UnixStream::connect(&control_socket);
        relay_handle.join();
        control_handle.join();
        return Err(error);
    }

    let mut remote_args = vec![
        "--remote".to_string(),
        format!("unix://{}", relay_socket.display()),
    ];
    remote_args.extend(args);
    let remote_status = std::process::Command::new(&real_codex)
        .args(remote_args)
        .status()
        .map_err(|error| format!("启动 codex remote 失败：{error}"));

    let _ = server.kill();
    let _ = server.wait();
    relay_handle.request_stop();
    control_handle.request_stop();
    let _ = UnixStream::connect(&relay_socket);
    let _ = UnixStream::connect(&control_socket);
    relay_handle.join();
    control_handle.join();
    remove_socket_if_exists(&real_socket)?;
    remove_socket_if_exists(&relay_socket)?;
    remove_socket_if_exists(&control_socket)?;
    let status = remote_status?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(unix)]
struct RelaySharedState {
    state: Mutex<AppControlState>,
    upstream_writer: Mutex<Option<std::os::unix::net::UnixStream>>,
}

#[cfg(unix)]
impl RelaySharedState {
    fn new(wrapper_session_id: String) -> Self {
        Self {
            state: Mutex::new(AppControlState {
                wrapper_session_id,
                ..Default::default()
            }),
            upstream_writer: Mutex::new(None),
        }
    }
}

#[cfg(unix)]
struct ThreadHandle {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl ThreadHandle {
    fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    fn join(mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(unix)]
fn wait_for_socket(path: &Path, timeout: Duration) -> Result<(), String> {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("app-server socket 未创建：{}", path.display()))
}

#[cfg(unix)]
fn remove_socket_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("清理 socket 文件失败 {}：{error}", path.display())),
    }
}

#[cfg(unix)]
fn start_relay_thread(
    real_socket: PathBuf,
    relay_socket: PathBuf,
    shared: Arc<RelaySharedState>,
) -> Result<ThreadHandle, String> {
    use std::os::unix::net::UnixListener;

    let listener = UnixListener::bind(&relay_socket)
        .map_err(|error| format!("监听 niuma-codex relay socket 失败：{error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("设置 relay socket 非阻塞失败：{error}"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !thread_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((client, _)) => {
                    let real_socket = real_socket.clone();
                    let shared = Arc::clone(&shared);
                    thread::spawn(move || {
                        if let Err(error) = handle_relay_connection(client, &real_socket, shared) {
                            eprintln!("niuma-codex relay 连接处理失败：{error}");
                        }
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => {
                    eprintln!("niuma-codex relay accept 失败：{error}");
                    break;
                }
            }
        }
    });
    Ok(ThreadHandle {
        stop,
        handle: Some(handle),
    })
}

#[cfg(unix)]
fn handle_relay_connection(
    mut client: std::os::unix::net::UnixStream,
    real_socket: &Path,
    shared: Arc<RelaySharedState>,
) -> Result<(), String> {
    use std::os::unix::net::UnixStream;

    let mut upstream = UnixStream::connect(real_socket)
        .map_err(|error| format!("连接真实 codex app-server socket 失败：{error}"))?;
    let upstream_writer = upstream
        .try_clone()
        .map_err(|error| format!("复制上游 socket 失败：{error}"))?;
    {
        let mut writer = shared
            .upstream_writer
            .lock()
            .map_err(|_| "relay upstream writer 锁已损坏".to_string())?;
        *writer = Some(upstream_writer);
    }

    let mut client_reader = client
        .try_clone()
        .map_err(|error| format!("复制 TUI socket 失败：{error}"))?;
    let shared_for_client = Arc::clone(&shared);
    let client_to_server = thread::spawn(move || {
        let mut observer = WebSocketObserver::default();
        copy_observed(&mut client_reader, &mut upstream, |chunk| {
            observe_client_data(chunk, &mut observer, &shared_for_client)
        })
    });

    let mut upstream_reader = {
        let writer = shared
            .upstream_writer
            .lock()
            .map_err(|_| "relay upstream writer 锁已损坏".to_string())?;
        writer
            .as_ref()
            .ok_or_else(|| "relay upstream writer 未初始化".to_string())?
            .try_clone()
            .map_err(|error| format!("复制上游读取 socket 失败：{error}"))?
    };
    let shared_for_server = Arc::clone(&shared);
    let server_to_client = thread::spawn(move || {
        let mut observer = WebSocketObserver::default();
        copy_observed(&mut upstream_reader, &mut client, |chunk| {
            observe_server_data(chunk, &mut observer, &shared_for_server)
        })
    });

    let _ = client_to_server.join();
    let _ = server_to_client.join();
    Ok(())
}

#[cfg(unix)]
fn copy_observed<R, W, F>(reader: &mut R, writer: &mut W, mut observe: F) -> Result<(), String>
where
    R: Read,
    W: Write,
    F: FnMut(&[u8]),
{
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(size) => {
                let chunk = &buffer[..size];
                observe(chunk);
                writer
                    .write_all(chunk)
                    .map_err(|error| format!("relay 写入失败：{error}"))?;
            }
            Err(error) => return Err(format!("relay 读取失败：{error}")),
        }
    }
}

#[cfg(unix)]
#[derive(Default)]
struct WebSocketObserver {
    handshake_complete: bool,
    handshake_buffer: Vec<u8>,
    frame_buffer: Vec<u8>,
}

#[cfg(unix)]
fn frames_after_handshake(chunk: &[u8], observer: &mut WebSocketObserver) -> Option<Vec<u8>> {
    if observer.handshake_complete {
        return Some(chunk.to_vec());
    }

    observer.handshake_buffer.extend_from_slice(chunk);
    let Some(index) = find_header_end(&observer.handshake_buffer) else {
        return None;
    };
    observer.handshake_complete = true;
    let rest = observer.handshake_buffer[index + 4..].to_vec();
    observer.handshake_buffer.clear();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

#[cfg(unix)]
fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[cfg(unix)]
fn observe_server_data(chunk: &[u8], observer: &mut WebSocketObserver, shared: &RelaySharedState) {
    let Some(frames) = frames_after_handshake(chunk, observer) else {
        return;
    };
    observer.frame_buffer.extend_from_slice(&frames);
    match parse_websocket_text_frames(&observer.frame_buffer) {
        Ok(parsed) => {
            observer.frame_buffer = parsed.rest;
            if let Ok(mut state) = shared.state.lock() {
                for message in parsed.messages {
                    if let Some(method) = message.get("method").and_then(Value::as_str) {
                        state.observe_server_request(AppServerRequest {
                            jsonrpc_id: message.get("id").cloned().unwrap_or(Value::Null),
                            method: method.to_string(),
                            params: message.get("params").cloned().unwrap_or(Value::Null),
                        });
                    }
                }
            }
        }
        Err(error) => eprintln!("解析 Codex server WebSocket frame 失败：{error}"),
    }
}

#[cfg(unix)]
fn observe_client_data(chunk: &[u8], observer: &mut WebSocketObserver, shared: &RelaySharedState) {
    let Some(frames) = frames_after_handshake(chunk, observer) else {
        return;
    };
    observer.frame_buffer.extend_from_slice(&frames);
    match parse_websocket_text_frames(&observer.frame_buffer) {
        Ok(parsed) => {
            observer.frame_buffer = parsed.rest;
            if let Ok(mut state) = shared.state.lock() {
                for message in parsed.messages {
                    state.observe_client_message(&message);
                }
            }
        }
        Err(error) => eprintln!("解析 Codex client WebSocket frame 失败：{error}"),
    }
}

#[cfg(unix)]
fn start_control_thread(
    control_socket: PathBuf,
    shared: Arc<RelaySharedState>,
) -> Result<ThreadHandle, String> {
    use std::os::unix::net::UnixListener;

    let listener = UnixListener::bind(&control_socket)
        .map_err(|error| format!("监听 niuma-codex control socket 失败：{error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("设置 control socket 非阻塞失败：{error}"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !thread_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let shared = Arc::clone(&shared);
                    thread::spawn(move || {
                        if let Err(error) = handle_control_connection(stream, shared) {
                            eprintln!("niuma-codex control 连接处理失败：{error}");
                        }
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => {
                    eprintln!("niuma-codex control accept 失败：{error}");
                    break;
                }
            }
        }
    });
    Ok(ThreadHandle {
        stop,
        handle: Some(handle),
    })
}

#[cfg(unix)]
fn handle_control_connection(
    mut stream: std::os::unix::net::UnixStream,
    shared: Arc<RelaySharedState>,
) -> Result<(), String> {
    let mut line = String::new();
    {
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|error| format!("复制 control socket 失败：{error}"))?,
        );
        reader
            .read_line(&mut line)
            .map_err(|error| format!("读取 control command 失败：{error}"))?;
    }
    let response = match parse_control_command_line(&line) {
        Ok(command) => handle_control_command(command, &shared),
        Err(error) => json!({ "ok": false, "message": error }),
    };
    stream
        .write_all(format!("{response}\n").as_bytes())
        .map_err(|error| format!("写入 control response 失败：{error}"))
}

#[cfg(unix)]
fn handle_control_command(command: ControlCommand, shared: &RelaySharedState) -> Value {
    match command {
        ControlCommand::Requests => match shared.state.lock() {
            Ok(state) => json!({
                "ok": true,
                "approvals": state.pending_approvals,
                "inputs": state.pending_inputs,
            }),
            Err(_) => json!({ "ok": false, "message": "control state 锁已损坏" }),
        },
        ControlCommand::ApprovalDecision {
            request_id,
            decision,
        } => write_control_response_frame(shared, |state| {
            state.resolve_approval_decision_frame(&request_id, &decision)
        }),
        ControlCommand::AnswerInput {
            request_id,
            answers,
        } => write_control_response_frame(shared, |state| {
            state.resolve_input_answer_frame(&request_id, answers)
        }),
        ControlCommand::SendInstruction { content } => {
            json!({ "ok": false, "message": format!("send_instruction transport 尚未接入 app-server RPC：{content}") })
        }
        ControlCommand::Interrupt => {
            json!({ "ok": false, "message": "interrupt transport 尚未接入 app-server RPC" })
        }
    }
}

#[cfg(unix)]
fn write_control_response_frame<F>(shared: &RelaySharedState, build_frame: F) -> Value
where
    F: FnOnce(&mut AppControlState) -> Result<Vec<u8>, String>,
{
    let frame = match shared.state.lock() {
        Ok(mut state) => match build_frame(&mut state) {
            Ok(frame) => frame,
            Err(error) => return json!({ "ok": false, "message": error }),
        },
        Err(_) => return json!({ "ok": false, "message": "control state 锁已损坏" }),
    };

    let mut writer = match shared.upstream_writer.lock() {
        Ok(writer) => writer,
        Err(_) => return json!({ "ok": false, "message": "relay upstream writer 锁已损坏" }),
    };
    let Some(stream) = writer.as_mut() else {
        return json!({ "ok": false, "message": "当前没有可回写的 Codex TUI WebSocket" });
    };
    match stream.write_all(&frame) {
        Ok(()) => json!({ "ok": true }),
        Err(error) => {
            json!({ "ok": false, "message": format!("写回 Codex app-server 失败：{error}") })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_control_state_tracks_approval_request() {
        let mut state = AppControlState {
            wrapper_session_id: "wrapper-test".to_string(),
            ..Default::default()
        };

        state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!(7),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({
                "turnId": "turn-1",
                "itemId": "item-1",
                "command": "cargo test"
            }),
        });

        assert_eq!(state.pending_approvals.len(), 1);
        assert_eq!(
            state.pending_approvals[0].request_id,
            "codex-relay:wrapper-test:turn-1:item-1"
        );
        assert_eq!(state.pending_approvals[0].relay_request_id, "7");
        assert_eq!(
            state.pending_approvals[0].command.as_deref(),
            Some("cargo test")
        );
    }

    #[test]
    fn app_control_state_tracks_input_request() {
        let mut state = AppControlState {
            wrapper_session_id: "wrapper-test".to_string(),
            ..Default::default()
        };

        state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!(9),
            method: "item/tool/requestUserInput".into(),
            params: json!({
                "questions": [{"id": "app_type", "options": [{"label": "CLI"}]}]
            }),
        });

        assert_eq!(state.pending_inputs.len(), 1);
        assert_eq!(
            state.pending_inputs[0].request_id,
            "codex-input:wrapper-test:9"
        );
        assert_eq!(
            state.pending_inputs[0].questions,
            json!([{"id": "app_type", "options": [{"label": "CLI"}]}])
        );
    }

    #[test]
    fn control_command_json_line_deserializes() {
        let command: ControlCommand = serde_json::from_str(
            r#"{"type":"approval_decision","request_id":"req-1","decision":"approved"}"#,
        )
        .unwrap();

        assert_eq!(
            command,
            ControlCommand::ApprovalDecision {
                request_id: "req-1".to_string(),
                decision: "approved".to_string()
            }
        );
    }

    #[test]
    fn jsonrpc_id_key_has_stable_display_without_extra_quotes() {
        assert_eq!(jsonrpc_id_key(&json!(7)), "7");
        assert_eq!(jsonrpc_id_key(&json!("7")), "7");
        assert_eq!(jsonrpc_id_key(&json!("abc")), "abc");
    }

    #[test]
    fn approval_fallback_and_input_request_ids_use_normalized_jsonrpc_id() {
        let mut state = AppControlState {
            wrapper_session_id: "wrapper-test".to_string(),
            ..Default::default()
        };

        state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!("abc"),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({
                "turnId": "turn-1",
                "command": "cargo test"
            }),
        });
        state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!("7"),
            method: "item/tool/requestUserInput".into(),
            params: json!({}),
        });

        assert_eq!(
            state.pending_approvals[0].request_id,
            "codex-relay:wrapper-test:turn-1:abc"
        );
        assert_eq!(state.pending_approvals[0].relay_request_id, "abc");
        assert_eq!(state.pending_approvals[0].relay_jsonrpc_id, json!("abc"));
        assert_eq!(
            state.pending_inputs[0].request_id,
            "codex-input:wrapper-test:7"
        );
        assert_eq!(state.pending_inputs[0].relay_request_id, "7");
        assert_eq!(state.pending_inputs[0].relay_jsonrpc_id, json!("7"));
    }

    #[test]
    fn mark_session_exited_after_failure_updates_registry_reason() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        let wrapper_session_id = "wrapper-test".to_string();
        update_registry(&registry_path, |registry| {
            registry.upsert(ManagedCodexSession {
                wrapper_session_id: wrapper_session_id.clone(),
                state: ManagedCodexSessionState::WaitingFirstUserMessage,
                cwd: "/tmp/project".to_string(),
                pid: Some(42),
                real_socket: "/tmp/real.sock".to_string(),
                relay_socket: "/tmp/relay.sock".to_string(),
                control_socket: "/tmp/control.sock".to_string(),
                started_at: chrono::Utc::now(),
                first_user_message_hash: None,
                first_user_message_preview: None,
                first_user_message_submitted_at: None,
                codex_session_id: None,
                codex_session_file_path: None,
                bound_at: None,
                binding_failure_reason: None,
            });
        })
        .unwrap();

        mark_session_exited_after_failure(&registry_path, &wrapper_session_id, "transport failed")
            .unwrap();

        let registry = niuma_core::codex_managed_session::read_registry(&registry_path).unwrap();
        let session = registry
            .sessions
            .iter()
            .find(|session| session.wrapper_session_id == wrapper_session_id)
            .unwrap();
        assert_eq!(session.state, ManagedCodexSessionState::Exited);
        assert_eq!(
            session.binding_failure_reason.as_deref(),
            Some("transport failed")
        );
    }

    #[test]
    fn managed_socket_base_dir_keeps_unix_socket_paths_short() {
        let base_dir = managed_socket_base_dir("niuma_codex_eb6c63c67b9d4bc9b6e20fd7e0fad8a6");
        let real_socket = base_dir.join("real.sock");

        assert!(
            real_socket.to_string_lossy().len() < 100,
            "socket path is too long: {}",
            real_socket.display()
        );
        #[cfg(unix)]
        assert!(real_socket.starts_with("/tmp/niuma-codex"));
    }

    #[test]
    fn websocket_text_frame_round_trips_json() {
        let payload = json!({"id": 1, "method": "thread/read"});
        let frame = encode_websocket_text_frame(&payload.to_string(), false);

        let parsed = parse_websocket_text_frames(&frame).unwrap();

        assert_eq!(parsed.messages, vec![payload]);
        assert!(parsed.rest.is_empty());
    }

    #[test]
    fn control_command_parses_json_line() {
        let command =
            parse_control_command_line(r#"{"type":"send_instruction","content":"继续"}"#).unwrap();

        assert!(
            matches!(command, ControlCommand::SendInstruction { content } if content == "继续")
        );
    }

    #[test]
    fn approval_decision_builds_masked_jsonrpc_response_and_removes_pending() {
        let mut state = AppControlState {
            wrapper_session_id: "wrapper-test".to_string(),
            ..Default::default()
        };
        state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!(15),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({
                "turnId": "turn-1",
                "itemId": "item-1",
                "command": "cargo test"
            }),
        });

        let frame = state
            .resolve_approval_decision_frame("codex-relay:wrapper-test:turn-1:item-1", "accept")
            .unwrap();
        let decoded = parse_websocket_text_frames(&frame).unwrap();

        assert_eq!(
            decoded.messages,
            vec![json!({"id": 15, "result": {"decision": "accept"}})]
        );
        assert!(state.pending_approvals.is_empty());
    }
}
