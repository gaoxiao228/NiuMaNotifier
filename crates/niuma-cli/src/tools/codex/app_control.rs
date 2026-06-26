use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_managed_session::{
    first_user_message_hash, read_registry, update_registry, ManagedCodexSession,
    ManagedCodexSessionState,
};
use niuma_core::local_api_client::post_local_api;
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
    first_user_message_recorded: bool,
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
    pub description: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingInput {
    pub request_id: String,
    pub relay_request_id: String,
    pub relay_jsonrpc_id: Value,
    pub questions: Value,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub enum ObservedServerRequest {
    Approval(PendingApproval),
    Input(PendingInput),
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct FirstUserMessageSubmission {
    content: String,
    thread_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InstructionRequest {
    mode: &'static str,
    method: &'static str,
    params: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InterruptRequest {
    turn_id: String,
    method: &'static str,
    params: Value,
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

        // relay 只观察 JSON-RPC text frame；close/ping/pong 等控制帧继续原样转发，
        // 这里跳过即可，避免退出 Codex 时产生误导性的解析错误。
        if opcode != 0x1 {
            offset += payload_len;
            continue;
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
    pub fn observe_server_request(
        &mut self,
        request: AppServerRequest,
    ) -> Option<ObservedServerRequest> {
        match request.method.as_str() {
            "item/commandExecution/requestApproval" => self
                .observe_approval_request(request.jsonrpc_id, request.params)
                .map(ObservedServerRequest::Approval),
            "item/tool/requestUserInput" => self
                .observe_input_request(request.jsonrpc_id, request.params)
                .map(ObservedServerRequest::Input),
            _ => None,
        }
    }

    fn observe_approval_request(
        &mut self,
        jsonrpc_id: Value,
        params: Value,
    ) -> Option<PendingApproval> {
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

        let approval = PendingApproval {
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
            description: approval_description_from_params(&params),
        };
        self.pending_approvals.push(approval.clone());
        Some(approval)
    }

    fn observe_input_request(&mut self, jsonrpc_id: Value, params: Value) -> Option<PendingInput> {
        let relay_request_id = jsonrpc_id_key(&jsonrpc_id);
        let questions =
            normalize_input_questions(params.get("questions").unwrap_or(&Value::Array(Vec::new())));
        let input = PendingInput {
            request_id: format!(
                "codex-input:{}:{}",
                self.wrapper_session_id, relay_request_id
            ),
            relay_request_id,
            relay_jsonrpc_id: jsonrpc_id,
            questions,
        };
        self.pending_inputs.push(input.clone());
        Some(input)
    }

    pub fn observe_client_message(&mut self, message: &Value) -> Option<PendingApproval> {
        if message.get("method").is_some() {
            return None;
        }
        let Some(id) = message.get("id") else {
            return None;
        };
        let relay_request_id = jsonrpc_id_key(id);
        let resolved_approval = self
            .pending_approvals
            .iter()
            .position(|approval| approval.relay_request_id == relay_request_id)
            .map(|index| self.pending_approvals.remove(index));
        self.pending_inputs
            .retain(|input| input.relay_request_id != relay_request_id);
        resolved_approval
    }

    fn observe_first_user_message_submission(
        &mut self,
        message: &Value,
    ) -> Option<FirstUserMessageSubmission> {
        if self.first_user_message_recorded {
            return None;
        }
        let submission = first_user_message_submission_from_turn_start(message)?;
        self.first_user_message_recorded = true;
        Some(submission)
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
            "result": { "answers": codex_input_result_answers(answers) },
        });
        Ok(encode_websocket_text_frame(&payload.to_string(), true))
    }
}

fn approval_description_from_params(params: &Value) -> Option<String> {
    ["justification", "description", "reason"]
        .iter()
        .find_map(|key| {
            params
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
}

fn jsonrpc_id_key(jsonrpc_id: &Value) -> String {
    // request_id 需要稳定且便于查找；字符串 id 不保留 JSON 引号，复杂值保留紧凑 JSON。
    match jsonrpc_id {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => serde_json::to_string(jsonrpc_id).unwrap_or_else(|_| jsonrpc_id.to_string()),
    }
}

fn codex_input_result_answers(answers: Value) -> Value {
    let Some(answer_map) = answers.as_object() else {
        return answers;
    };
    let normalized = answer_map
        .iter()
        .map(|(question_id, values)| {
            // Codex app-server 的 requestUserInput response 需要每题包一层
            // `{ answers: [...] }`；Local API 对外保留更简单的 Record<string,string[]>。
            (question_id.clone(), json!({ "answers": values }))
        })
        .collect::<serde_json::Map<_, _>>();
    Value::Object(normalized)
}

fn normalize_input_questions(raw_questions: &Value) -> Value {
    let Some(items) = raw_questions.as_array() else {
        return Value::Array(Vec::new());
    };

    let mut normalized = Vec::new();
    for item in items {
        let Some(question) = item
            .get("question")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let fallback_id = format!("question_{}", normalized.len() + 1);
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&fallback_id);

        let options = item
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| {
                        let label = option
                            .get("label")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())?;
                        let mut normalized_option = json!({ "label": label });
                        if let Some(description) = option
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            normalized_option["description"] = json!(description);
                        }
                        Some(normalized_option)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        normalized.push(json!({
            "id": id,
            "question": question,
            "options": options
        }));
    }

    Value::Array(normalized)
}

fn first_user_message_submission_from_turn_start(
    message: &Value,
) -> Option<FirstUserMessageSubmission> {
    if message.get("method").and_then(Value::as_str) != Some("turn/start") {
        return None;
    }
    let params = message.get("params")?;
    let content = params
        .get("input")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|content| !content.trim().is_empty())?;
    let thread_id = params
        .get("threadId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string);

    Some(FirstUserMessageSubmission { content, thread_id })
}

fn instruction_request_for_loaded_thread(
    loaded_thread_id: &str,
    thread: &Value,
    content: &str,
) -> Result<InstructionRequest, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("指令不能为空".to_string());
    }
    let input = json!([{ "type": "text", "text": content }]);
    let status_type = thread
        .pointer("/status/type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    match status_type {
        "idle" => Ok(InstructionRequest {
            mode: "start",
            method: "turn/start",
            params: json!({
                "threadId": loaded_thread_id,
                "input": input
            }),
        }),
        "active" => {
            let turn_id = find_in_progress_turn_id(thread).ok_or_else(|| {
                "当前 thread 是 active，但找不到正在运行的 turn，暂不能 steer".to_string()
            })?;
            Ok(InstructionRequest {
                mode: "steer",
                method: "turn/steer",
                params: json!({
                    "threadId": loaded_thread_id,
                    "expectedTurnId": turn_id,
                    "input": input
                }),
            })
        }
        other => Err(format!("当前 thread 状态不支持发送：{other}")),
    }
}

fn find_in_progress_turn_id(thread: &Value) -> Option<String> {
    thread
        .get("turns")
        .and_then(Value::as_array)?
        .iter()
        .rev()
        .find(|turn| turn.get("status").and_then(Value::as_str) == Some("inProgress"))
        .and_then(|turn| turn.get("id").and_then(Value::as_str))
        .map(ToString::to_string)
}

fn interrupt_request_for_loaded_thread(
    loaded_thread_id: &str,
    thread: &Value,
) -> Result<InterruptRequest, String> {
    let turn_id = find_in_progress_turn_id(thread)
        .ok_or_else(|| "当前没有正在运行的任务，无法中断".to_string())?;
    Ok(InterruptRequest {
        turn_id: turn_id.clone(),
        method: "turn/interrupt",
        params: json!({
            "threadId": loaded_thread_id,
            "turnId": turn_id
        }),
    })
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
        registry_path.clone(),
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

fn mark_first_user_message_submitted(
    registry_path: &Path,
    wrapper_session_id: &str,
    submission: FirstUserMessageSubmission,
) -> Result<(), String> {
    let submitted_at = chrono::Utc::now();
    let preview = submission.content.chars().take(200).collect::<String>();
    let hash = first_user_message_hash(&submission.content);
    update_registry(registry_path, |registry| {
        if let Some(session) = registry
            .sessions
            .iter_mut()
            .find(|session| session.wrapper_session_id == wrapper_session_id)
        {
            if session.first_user_message_hash.is_none() {
                session.state = ManagedCodexSessionState::BindingPending;
                session.first_user_message_hash = Some(hash);
                session.first_user_message_preview = Some(preview);
                session.first_user_message_submitted_at = Some(submitted_at);
                session.codex_session_id = submission.thread_id;
                session.binding_failure_reason = None;
            }
        }
    })
    .map(|_| ())
}

fn submit_relay_approval_to_local_api(
    registry_path: &Path,
    wrapper_session_id: &str,
    approval: &PendingApproval,
) -> Result<(), String> {
    let body = relay_approval_request_body(registry_path, wrapper_session_id, approval)?;
    let response = post_local_api(
        &niuma_api::local_api_addr(),
        "/api/v1/approval-requests",
        Some(&body.to_string()),
    )?;
    let envelope: Value = serde_json::from_str(&response)
        .map_err(|error| format!("解析 Local API 授权响应失败：{error}"))?;
    if envelope.get("code").and_then(Value::as_i64) == Some(0) {
        return Ok(());
    }
    Err(format!(
        "Local API 拒绝 relay 授权上报：{}",
        envelope
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
    ))
}

fn submit_relay_approval_tool_resolved(approval: &PendingApproval) -> Result<(), String> {
    let body = json!({
        "request_id": approval.request_id,
        "resolved_by": "niuma-codex-relay",
        "reason": "user_decided_in_codex"
    });
    let response = post_local_api(
        &niuma_api::local_api_addr(),
        "/api/v1/approval-requests/tool-resolved",
        Some(&body.to_string()),
    )?;
    let envelope: Value = serde_json::from_str(&response)
        .map_err(|error| format!("解析 Local API 授权处理响应失败：{error}"))?;
    if envelope.get("code").and_then(Value::as_i64) == Some(0) {
        return Ok(());
    }
    Err(format!(
        "Local API 拒绝 relay 授权处理同步：{}",
        envelope
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
    ))
}

fn submit_relay_input_event_to_local_api(
    registry_path: &Path,
    wrapper_session_id: &str,
    input: &PendingInput,
) -> Result<(), String> {
    let body = relay_input_event_body(registry_path, wrapper_session_id, input)?;
    let response = post_local_api(
        &niuma_api::local_api_addr(),
        "/api/v1/plugin-events",
        Some(&body.to_string()),
    )?;
    let envelope: Value = serde_json::from_str(&response)
        .map_err(|error| format!("解析 Local API 输入事件响应失败：{error}"))?;
    if envelope.get("code").and_then(Value::as_i64) == Some(0) {
        return Ok(());
    }
    Err(format!(
        "Local API 拒绝 relay 输入事件上报：{}",
        envelope
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
    ))
}

fn relay_approval_request_body(
    registry_path: &Path,
    wrapper_session_id: &str,
    approval: &PendingApproval,
) -> Result<Value, String> {
    let registry = read_registry(registry_path)?;
    let managed = registry
        .sessions
        .iter()
        .find(|session| session.wrapper_session_id == wrapper_session_id);
    let cwd = managed
        .map(|session| session.cwd.clone())
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        });
    let codex_session_id = managed_bound_codex_session_id(managed);
    let session_id = codex_session_id
        .clone()
        .unwrap_or_else(|| wrapper_session_id.to_string());
    let turn_id = approval
        .turn_id
        .clone()
        .unwrap_or_else(|| approval.relay_request_id.clone());
    let mut control_ref = json!({
        "wrapper_session_id": wrapper_session_id,
        "relay_request_id": approval.relay_request_id,
        "turn_id": approval.turn_id,
        "item_id": approval.item_id
    });
    if let Some(codex_session_id) = codex_session_id {
        control_ref["codex_session_id"] = json!(codex_session_id);
    }

    Ok(json!({
        "request_id": approval.request_id,
        "tool": "codex",
        "session_id": session_id,
        "turn_id": turn_id,
        "tool_name": "Bash",
        "command": approval.command,
        "description": approval
            .description
            .clone()
            .or_else(|| approval.command.clone()),
        "project_path": cwd,
        "project_name": project_name_from_path(&cwd),
        "timeout_seconds": 600,
        "channel": "niuma_codex_relay",
        "control_ref": control_ref
    }))
}

fn relay_input_event_body(
    registry_path: &Path,
    wrapper_session_id: &str,
    input: &PendingInput,
) -> Result<Value, String> {
    let registry = read_registry(registry_path)?;
    let managed = registry
        .sessions
        .iter()
        .find(|session| session.wrapper_session_id == wrapper_session_id);
    let cwd = managed
        .map(|session| session.cwd.clone())
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        });
    let session_id =
        managed_bound_codex_session_id(managed).unwrap_or_else(|| wrapper_session_id.to_string());
    let input_ref = format!("input:{}", input.request_id);
    let questions = input.questions.clone();
    let first_question = first_input_question_text(&questions);
    let summary = first_question
        .as_ref()
        .map(|question| format!("Codex 等待输入：{question}"))
        .unwrap_or_else(|| "Codex 正在等待用户输入".to_string());
    let content = render_input_questions_content(&questions);
    let now = chrono::Utc::now();

    Ok(json!({
        "plugin_id": "builtin-codex",
        "events": [{
            "id": format!("codex-relay-input:{}:{}", session_id, input.request_id),
            "dedupe_key": format!("codex-relay-input:{}:{}", session_id, input.request_id),
            "source": "codex-relay",
            "tool": "codex",
            "session_id": session_id,
            "project_path": cwd,
            "project_name": project_name_from_path(&cwd),
            "event_type": "input_requested",
            "severity": "urgent",
            "summary": summary,
            "content": content,
            "attention_resolve_key": input_ref,
            "payload_ref": input_ref,
            "interaction": {
                "kind": "input",
                "handling": "niuma",
                "actionable": true,
                "request_id": input.request_id,
                "actions": ["submit"],
                "endpoint": "/api/v1/session-control/answer-input",
                "schema": {
                    "questions": questions
                }
            },
            "created_at": now
        }]
    }))
}

fn first_input_question_text(questions: &Value) -> Option<String> {
    questions
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("question"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn render_input_questions_content(questions: &Value) -> Option<String> {
    let rendered = questions
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let question = item
                .get("question")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let mut lines = vec![question.to_string()];
            if let Some(options) = item.get("options").and_then(Value::as_array) {
                for (index, option) in options.iter().enumerate() {
                    let Some(label) = option
                        .get("label")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    else {
                        continue;
                    };
                    let suffix = option
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(|description| format!(" - {description}"))
                        .unwrap_or_default();
                    lines.push(format!("{}. {label}{suffix}", index + 1));
                }
            }
            Some(lines.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    if rendered.trim().is_empty() {
        None
    } else {
        Some(rendered)
    }
}

fn project_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

fn managed_bound_codex_session_id(managed: Option<&ManagedCodexSession>) -> Option<String> {
    let session = managed?;
    // binding_pending 阶段可能只有临时 threadId；只有 bound 后才作为稳定 Codex session id。
    if session.state != ManagedCodexSessionState::Bound {
        return None;
    }
    session.codex_session_id.clone()
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
    registry_path: PathBuf,
    base_dir: PathBuf,
) -> Result<i32, String> {
    run_app_server_remote_processes_impl(
        real_codex,
        args,
        wrapper_session_id,
        registry_path,
        base_dir,
    )
}

#[cfg(not(unix))]
fn run_app_server_remote_processes_impl(
    _real_codex: PathBuf,
    _args: Vec<String>,
    _wrapper_session_id: String,
    _registry_path: PathBuf,
    _base_dir: PathBuf,
) -> Result<i32, String> {
    Err("niuma-codex managed mode 当前仅支持 Unix socket 平台".to_string())
}

#[cfg(unix)]
fn run_app_server_remote_processes_impl(
    real_codex: PathBuf,
    args: Vec<String>,
    wrapper_session_id: String,
    registry_path: PathBuf,
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

    let shared = Arc::new(RelaySharedState::new(
        wrapper_session_id,
        registry_path,
        real_socket.clone(),
    ));
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
    registry_path: PathBuf,
    real_socket: PathBuf,
}

#[cfg(unix)]
impl RelaySharedState {
    fn new(wrapper_session_id: String, registry_path: PathBuf, real_socket: PathBuf) -> Self {
        Self {
            state: Mutex::new(AppControlState {
                wrapper_session_id,
                ..Default::default()
            }),
            upstream_writer: Mutex::new(None),
            registry_path,
            real_socket,
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

    // relay listener 自身使用非阻塞 accept；macOS 上 accept 出来的 UnixStream
    // 会继承非阻塞状态。转发线程需要阻塞读写，否则两段 WebSocket 握手之间的
    // 短暂空窗会被误判为 WouldBlock 错误并提前断开 Codex remote。
    client
        .set_nonblocking(false)
        .map_err(|error| format!("设置 TUI socket 阻塞模式失败：{error}"))?;
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
                writer
                    .write_all(chunk)
                    .map_err(|error| format!("relay 写入失败：{error}"))?;
                observe(chunk);
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
            let mut approvals = Vec::new();
            let mut inputs = Vec::new();
            if let Ok(mut state) = shared.state.lock() {
                for message in parsed.messages {
                    if let Some(method) = message.get("method").and_then(Value::as_str) {
                        let observed = state.observe_server_request(AppServerRequest {
                            jsonrpc_id: message.get("id").cloned().unwrap_or(Value::Null),
                            method: method.to_string(),
                            params: message.get("params").cloned().unwrap_or(Value::Null),
                        });
                        if let Some(observed) = observed {
                            match observed {
                                ObservedServerRequest::Approval(approval) => {
                                    approvals.push((state.wrapper_session_id.clone(), approval));
                                }
                                ObservedServerRequest::Input(input) => {
                                    inputs.push((state.wrapper_session_id.clone(), input));
                                }
                            }
                        }
                    }
                }
            }
            for (wrapper_session_id, approval) in approvals {
                let registry_path = shared.registry_path.clone();
                spawn_relay_background("提交 niuma-codex relay 授权失败", move || {
                    submit_relay_approval_to_local_api(
                        &registry_path,
                        &wrapper_session_id,
                        &approval,
                    )
                });
            }
            for (wrapper_session_id, input) in inputs {
                let registry_path = shared.registry_path.clone();
                spawn_relay_background("提交 niuma-codex relay 输入事件失败", move || {
                    submit_relay_input_event_to_local_api(
                        &registry_path,
                        &wrapper_session_id,
                        &input,
                    )
                });
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
            let mut first_user_submissions = Vec::new();
            let mut resolved_approvals = Vec::new();
            if let Ok(mut state) = shared.state.lock() {
                for message in parsed.messages {
                    if let Some(submission) = state.observe_first_user_message_submission(&message)
                    {
                        let wrapper_session_id = state.wrapper_session_id.clone();
                        first_user_submissions.push((wrapper_session_id, submission));
                    }
                    if let Some(approval) = state.observe_client_message(&message) {
                        resolved_approvals.push(approval);
                    }
                }
            }
            for (wrapper_session_id, submission) in first_user_submissions {
                let registry_path = shared.registry_path.clone();
                spawn_relay_background(
                    "记录 niuma-codex 第一条用户消息失败",
                    move || {
                        mark_first_user_message_submitted(
                            &registry_path,
                            &wrapper_session_id,
                            submission,
                        )
                    },
                );
            }
            for approval in resolved_approvals {
                spawn_relay_background("同步 Codex 侧授权处理结果失败", move || {
                    submit_relay_approval_tool_resolved(&approval)
                });
            }
        }
        Err(error) => eprintln!("解析 Codex client WebSocket frame 失败：{error}"),
    }
}

#[cfg(unix)]
fn spawn_relay_background<F>(label: &'static str, action: F)
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    thread::spawn(move || {
        if let Err(error) = action() {
            eprintln!("{label}：{error}");
        }
    });
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
            match send_instruction_to_app_server(&shared.real_socket, &content) {
                Ok(result) => json!({ "ok": true, "result": result }),
                Err(error) => json!({ "ok": false, "message": error }),
            }
        }
        ControlCommand::Interrupt => match interrupt_app_server_turn(&shared.real_socket) {
            Ok(result) => json!({ "ok": true, "result": result }),
            Err(error) => json!({ "ok": false, "message": error }),
        },
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

#[cfg(unix)]
fn send_instruction_to_app_server(real_socket: &Path, content: &str) -> Result<Value, String> {
    let mut client = AppServerRpcClient::connect(real_socket)?;
    client.initialize()?;
    let loaded = client.read_loaded_thread(true)?;
    let request = instruction_request_for_loaded_thread(
        loaded.loaded_thread_id.as_str(),
        &loaded.thread,
        content,
    )?;
    let result = client.request(request.method, request.params)?;
    Ok(json!({
        "mode": request.mode,
        "loaded_thread_id": loaded.loaded_thread_id,
        "response": result
    }))
}

#[cfg(unix)]
fn interrupt_app_server_turn(real_socket: &Path) -> Result<Value, String> {
    let mut client = AppServerRpcClient::connect(real_socket)?;
    client.initialize()?;
    let loaded = client.read_loaded_thread(true)?;
    let request =
        interrupt_request_for_loaded_thread(loaded.loaded_thread_id.as_str(), &loaded.thread)?;
    client.request(request.method, request.params)?;
    Ok(json!({
        "loaded_thread_id": loaded.loaded_thread_id,
        "turn_id": request.turn_id
    }))
}

#[cfg(unix)]
struct LoadedThread {
    loaded_thread_id: String,
    thread: Value,
}

#[cfg(unix)]
struct AppServerRpcClient {
    stream: std::os::unix::net::UnixStream,
    next_id: u64,
    read_buffer: Vec<u8>,
}

#[cfg(unix)]
impl AppServerRpcClient {
    fn connect(real_socket: &Path) -> Result<Self, String> {
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(real_socket)
            .map_err(|error| format!("连接真实 codex app-server socket 失败：{error}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| format!("设置 app-server 读取超时失败：{error}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| format!("设置 app-server 写入超时失败：{error}"))?;
        perform_app_server_websocket_handshake(&mut stream)?;
        Ok(Self {
            stream,
            next_id: 1,
            read_buffer: Vec::new(),
        })
    }

    fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "niuma-cli",
                    "title": "Niuma CLI",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }),
        )?;
        self.notify("initialized", None)
    }

    fn read_loaded_thread(&mut self, include_turns: bool) -> Result<LoadedThread, String> {
        let loaded = self.request(
            "thread/loaded/list",
            json!({ "limit": 100, "cursor": null }),
        )?;
        let loaded_thread_id = loaded
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "当前 app-server 没有 loaded thread，请先让 codex --remote 打开一个会话".to_string()
            })?
            .to_string();

        let thread = match self.read_thread(&loaded_thread_id, include_turns) {
            Ok(thread) => thread,
            Err(error) if include_turns && is_unmaterialized_thread_error(&error) => {
                self.read_thread(&loaded_thread_id, false)?
            }
            Err(error) => return Err(error),
        };

        Ok(LoadedThread {
            loaded_thread_id,
            thread,
        })
    }

    fn read_thread(&mut self, thread_id: &str, include_turns: bool) -> Result<Value, String> {
        let result = self.request(
            "thread/read",
            json!({
                "threadId": thread_id,
                "includeTurns": include_turns
            }),
        )?;
        result
            .get("thread")
            .cloned()
            .ok_or_else(|| "thread/read 响应缺少 thread".to_string())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_json(&json!({
            "id": id,
            "method": method,
            "params": params
        }))?;
        self.read_response(id, method)
    }

    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), String> {
        let payload = match params {
            Some(params) => json!({ "method": method, "params": params }),
            None => json!({ "method": method }),
        };
        self.write_json(&payload)
    }

    fn write_json(&mut self, payload: &Value) -> Result<(), String> {
        let frame = encode_websocket_text_frame(&payload.to_string(), true);
        self.stream
            .write_all(&frame)
            .map_err(|error| format!("写入 app-server WebSocket 失败：{error}"))
    }

    fn read_response(&mut self, id: u64, method: &str) -> Result<Value, String> {
        loop {
            let mut chunk = [0_u8; 8192];
            let size = self
                .stream
                .read(&mut chunk)
                .map_err(|error| format!("读取 app-server 响应失败 {method}：{error}"))?;
            if size == 0 {
                return Err(format!("app-server 在响应 {method} 前关闭连接"));
            }
            self.read_buffer.extend_from_slice(&chunk[..size]);
            let parsed = parse_websocket_text_frames(&self.read_buffer)?;
            self.read_buffer = parsed.rest;
            for message in parsed.messages {
                if message.get("id").and_then(Value::as_u64) != Some(id) {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    let message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .unwrap_or_else(|| error.to_string());
                    return Err(message);
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }
}

#[cfg(unix)]
fn perform_app_server_websocket_handshake(
    stream: &mut std::os::unix::net::UnixStream,
) -> Result<(), String> {
    let request = [
        "GET /rpc HTTP/1.1",
        "Host: localhost",
        "Connection: Upgrade",
        "Upgrade: websocket",
        "Sec-WebSocket-Version: 13",
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
        "",
        "",
    ]
    .join("\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("写入 app-server WebSocket 握手失败：{error}"))?;

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let size = stream
            .read(&mut chunk)
            .map_err(|error| format!("读取 app-server WebSocket 握手失败：{error}"))?;
        if size == 0 {
            return Err("app-server WebSocket 握手前关闭连接".to_string());
        }
        buffer.extend_from_slice(&chunk[..size]);
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&buffer[..index]).to_string();
            if !header.starts_with("HTTP/1.1 101") {
                return Err(format!(
                    "app-server WebSocket 握手失败：{}",
                    header.lines().next().unwrap_or("unknown")
                ));
            }
            return Ok(());
        }
    }
}

fn is_unmaterialized_thread_error(error: &str) -> bool {
    error.contains("not materialized yet; includeTurns is unavailable before first user message")
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
    fn app_control_state_tracks_approval_justification_as_description() {
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
                "command": "/bin/zsh -lc 'echo \"1234\"'",
                "justification": "是否允许执行用于模拟真实授权弹框的命令： echo \"1234\"?"
            }),
        });

        assert_eq!(
            state.pending_approvals[0].description.as_deref(),
            Some("是否允许执行用于模拟真实授权弹框的命令： echo \"1234\"?")
        );
    }

    #[test]
    fn client_response_returns_resolved_pending_approval() {
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

        let resolved = state
            .observe_client_message(&json!({"id": 7, "result": {"decision": "allow"}}))
            .expect("client response should resolve pending approval");

        assert_eq!(
            resolved.request_id,
            "codex-relay:wrapper-test:turn-1:item-1"
        );
        assert!(state.pending_approvals.is_empty());
    }

    #[test]
    fn app_control_state_tracks_input_request() {
        let mut state = AppControlState {
            wrapper_session_id: "wrapper-test".to_string(),
            ..Default::default()
        };

        let observed = state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!(9),
            method: "item/tool/requestUserInput".into(),
            params: json!({
                "questions": [
                    {
                        "id": "",
                        "question": "选择应用类型",
                        "options": [
                            {"label": " CLI ", "description": " 命令行 "},
                            {"label": ""},
                            {"description": "缺少 label"}
                        ]
                    },
                    {
                        "id": "details",
                        "question": " ",
                        "options": [{"label": "Web"}]
                    }
                ]
            }),
        });

        let ObservedServerRequest::Input(input) =
            observed.expect("input request should be observed")
        else {
            panic!("expected input request");
        };
        assert_eq!(state.pending_inputs.len(), 1);
        assert_eq!(
            state.pending_inputs[0].request_id,
            "codex-input:wrapper-test:9"
        );
        assert_eq!(state.pending_inputs[0], input);
        assert_eq!(
            state.pending_inputs[0].questions,
            json!([{
                "id": "question_1",
                "question": "选择应用类型",
                "options": [{"label": "CLI", "description": "命令行"}]
            }])
        );
    }

    #[test]
    fn input_questions_are_normalized_for_actionable_event() {
        let questions = normalize_input_questions(&json!([
            {
                "question": "项目名称？",
                "options": [
                    {"label": "默认", "description": " 使用当前目录 "},
                    {"label": "  "},
                    {"label": "自定义", "description": ""}
                ]
            },
            {"id": "skip_empty", "question": ""},
            {
                "id": "confirm",
                "question": "是否继续？",
                "options": [{"label": "是"}]
            }
        ]));

        assert_eq!(
            questions,
            json!([
                {
                    "id": "question_1",
                    "question": "项目名称？",
                    "options": [
                        {"label": "默认", "description": "使用当前目录"},
                        {"label": "自定义"}
                    ]
                },
                {
                    "id": "confirm",
                    "question": "是否继续？",
                    "options": [{"label": "是"}]
                }
            ])
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
    fn websocket_parser_ignores_non_text_frames() {
        let mut frames = vec![0x88, 0x80, 0x12, 0x34, 0x56, 0x78];
        frames.extend(encode_websocket_text_frame(
            &json!({"id": 1, "result": "ok"}).to_string(),
            false,
        ));

        let parsed = parse_websocket_text_frames(&frames).unwrap();

        assert_eq!(parsed.messages, vec![json!({"id": 1, "result": "ok"})]);
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

    #[test]
    fn input_answer_frame_uses_codex_nested_answers_shape() {
        let mut state = AppControlState {
            wrapper_session_id: "wrapper-test".to_string(),
            ..Default::default()
        };
        state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!(9),
            method: "item/tool/requestUserInput".into(),
            params: json!({
                "questions": [{
                    "id": "app_form",
                    "question": "选择形态",
                    "options": [{"label": "CLI"}]
                }]
            }),
        });

        let frame = state
            .resolve_input_answer_frame(
                "codex-input:wrapper-test:9",
                json!({ "app_form": ["CLI"] }),
            )
            .unwrap();
        let decoded = parse_websocket_text_frames(&frame).unwrap();

        assert_eq!(
            decoded.messages,
            vec![json!({
                "id": 9,
                "result": {
                    "answers": {
                        "app_form": {
                            "answers": ["CLI"]
                        }
                    }
                }
            })]
        );
        assert!(state.pending_inputs.is_empty());
    }

    #[test]
    fn relay_approval_request_body_uses_bound_codex_session() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        update_registry(&registry_path, |registry| {
            registry.upsert(ManagedCodexSession {
                wrapper_session_id: "wrapper-test".to_string(),
                state: ManagedCodexSessionState::Bound,
                cwd: "/tmp/demo-project".to_string(),
                pid: Some(42),
                real_socket: "/tmp/real.sock".to_string(),
                relay_socket: "/tmp/relay.sock".to_string(),
                control_socket: "/tmp/control.sock".to_string(),
                started_at: chrono::Utc::now(),
                first_user_message_hash: None,
                first_user_message_preview: None,
                first_user_message_submitted_at: None,
                codex_session_id: Some("codex-session-1".to_string()),
                codex_session_file_path: None,
                bound_at: Some(chrono::Utc::now()),
                binding_failure_reason: None,
            });
        })
        .unwrap();
        let approval = PendingApproval {
            request_id: "codex-relay:wrapper-test:turn-1:item-1".to_string(),
            relay_request_id: "7".to_string(),
            relay_jsonrpc_id: json!(7),
            turn_id: Some("turn-1".to_string()),
            item_id: Some("item-1".to_string()),
            command: Some("cargo test".to_string()),
            description: Some("运行测试".to_string()),
        };

        let body = relay_approval_request_body(&registry_path, "wrapper-test", &approval).unwrap();

        assert_eq!(body["request_id"], "codex-relay:wrapper-test:turn-1:item-1");
        assert_eq!(body["channel"], "niuma_codex_relay");
        assert_eq!(body["session_id"], "codex-session-1");
        assert_eq!(body["project_path"], "/tmp/demo-project");
        assert_eq!(body["project_name"], "demo-project");
        assert_eq!(body["control_ref"]["wrapper_session_id"], "wrapper-test");
        assert_eq!(body["control_ref"]["codex_session_id"], "codex-session-1");
        assert_eq!(body["control_ref"]["relay_request_id"], "7");
        assert_eq!(body["description"], "运行测试");
    }

    #[test]
    fn relay_input_event_body_uses_bound_codex_session_and_schema() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        update_registry(&registry_path, |registry| {
            registry.upsert(ManagedCodexSession {
                wrapper_session_id: "wrapper-test".to_string(),
                state: ManagedCodexSessionState::Bound,
                cwd: "/tmp/demo-project".to_string(),
                pid: Some(42),
                real_socket: "/tmp/real.sock".to_string(),
                relay_socket: "/tmp/relay.sock".to_string(),
                control_socket: "/tmp/control.sock".to_string(),
                started_at: chrono::Utc::now(),
                first_user_message_hash: None,
                first_user_message_preview: None,
                first_user_message_submitted_at: None,
                codex_session_id: Some("codex-session-1".to_string()),
                codex_session_file_path: None,
                bound_at: Some(chrono::Utc::now()),
                binding_failure_reason: None,
            });
        })
        .unwrap();
        let input = PendingInput {
            request_id: "codex-input:wrapper-test:9".to_string(),
            relay_request_id: "9".to_string(),
            relay_jsonrpc_id: json!(9),
            questions: json!([
                {
                    "id": "question_1",
                    "question": "选择应用类型",
                    "options": [
                        {"label": "CLI", "description": "命令行"},
                        {"label": "Web"}
                    ]
                }
            ]),
        };

        let body = relay_input_event_body(&registry_path, "wrapper-test", &input).unwrap();

        assert_eq!(body["plugin_id"], "builtin-codex");
        let event = &body["events"][0];
        assert_eq!(event["event_type"], "input_requested");
        assert_eq!(event["severity"], "urgent");
        assert_eq!(event["source"], "codex-relay");
        assert_eq!(event["tool"], "codex");
        assert_eq!(event["session_id"], "codex-session-1");
        assert_eq!(event["project_path"], "/tmp/demo-project");
        assert_eq!(event["project_name"], "demo-project");
        assert_eq!(
            event["attention_resolve_key"],
            "input:codex-input:wrapper-test:9"
        );
        assert_eq!(event["payload_ref"], "input:codex-input:wrapper-test:9");
        assert_eq!(event["summary"], "Codex 等待输入：选择应用类型");
        assert_eq!(event["content"], "选择应用类型\n1. CLI - 命令行\n2. Web");
        assert_eq!(
            event["interaction"],
            json!({
                "kind": "input",
                "handling": "niuma",
                "actionable": true,
                "request_id": "codex-input:wrapper-test:9",
                "actions": ["submit"],
                "endpoint": "/api/v1/session-control/answer-input",
                "schema": {
                    "questions": [
                        {
                            "id": "question_1",
                            "question": "选择应用类型",
                            "options": [
                                {"label": "CLI", "description": "命令行"},
                                {"label": "Web"}
                            ]
                        }
                    ]
                }
            })
        );
    }

    #[test]
    fn relay_input_event_body_ignores_unbound_codex_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        update_registry(&registry_path, |registry| {
            registry.upsert(ManagedCodexSession {
                wrapper_session_id: "wrapper-test".to_string(),
                state: ManagedCodexSessionState::BindingPending,
                cwd: "/tmp/demo-project".to_string(),
                pid: Some(42),
                real_socket: "/tmp/real.sock".to_string(),
                relay_socket: "/tmp/relay.sock".to_string(),
                control_socket: "/tmp/control.sock".to_string(),
                started_at: chrono::Utc::now(),
                first_user_message_hash: None,
                first_user_message_preview: None,
                first_user_message_submitted_at: None,
                codex_session_id: Some("temporary-thread-id".to_string()),
                codex_session_file_path: None,
                bound_at: None,
                binding_failure_reason: None,
            });
        })
        .unwrap();
        let input = PendingInput {
            request_id: "codex-input:wrapper-test:9".to_string(),
            relay_request_id: "9".to_string(),
            relay_jsonrpc_id: json!(9),
            questions: json!([{
                "id": "question_1",
                "question": "选择应用类型",
                "options": [{"label": "CLI"}]
            }]),
        };

        let body = relay_input_event_body(&registry_path, "wrapper-test", &input).unwrap();

        assert_eq!(body["events"][0]["session_id"], "wrapper-test");
    }

    #[test]
    fn turn_start_extracts_first_user_message_submission() {
        let message = json!({
            "id": 7,
            "method": "turn/start",
            "params": {
                "threadId": "session-1",
                "input": [
                    {"type": "text", "text": "第一段"},
                    {"type": "text", "text": "第二段"}
                ]
            }
        });

        let submission = first_user_message_submission_from_turn_start(&message).unwrap();

        assert_eq!(
            submission,
            FirstUserMessageSubmission {
                content: "第一段\n第二段".to_string(),
                thread_id: Some("session-1".to_string()),
            }
        );
    }

    #[test]
    fn instruction_request_uses_turn_start_for_idle_thread() {
        let request = instruction_request_for_loaded_thread(
            "thread-1",
            &json!({"status": {"type": "idle"}}),
            "继续",
        )
        .unwrap();

        assert_eq!(request.mode, "start");
        assert_eq!(request.method, "turn/start");
        assert_eq!(
            request.params,
            json!({
                "threadId": "thread-1",
                "input": [{"type": "text", "text": "继续"}]
            })
        );
    }

    #[test]
    fn instruction_request_uses_turn_steer_for_active_thread() {
        let request = instruction_request_for_loaded_thread(
            "thread-1",
            &json!({
                "status": {"type": "active"},
                "turns": [
                    {"id": "old-turn", "status": "completed"},
                    {"id": "turn-2", "status": "inProgress"}
                ]
            }),
            "新的要求",
        )
        .unwrap();

        assert_eq!(request.mode, "steer");
        assert_eq!(request.method, "turn/steer");
        assert_eq!(
            request.params,
            json!({
                "threadId": "thread-1",
                "expectedTurnId": "turn-2",
                "input": [{"type": "text", "text": "新的要求"}]
            })
        );
    }

    #[test]
    fn interrupt_request_uses_last_in_progress_turn() {
        let request = interrupt_request_for_loaded_thread(
            "thread-1",
            &json!({
                "status": {"type": "active"},
                "turns": [
                    {"id": "turn-1", "status": "inProgress"},
                    {"id": "turn-2", "status": "completed"},
                    {"id": "turn-3", "status": "inProgress"}
                ]
            }),
        )
        .unwrap();

        assert_eq!(request.turn_id, "turn-3");
        assert_eq!(request.method, "turn/interrupt");
        assert_eq!(
            request.params,
            json!({
                "threadId": "thread-1",
                "turnId": "turn-3"
            })
        );
    }

    #[test]
    fn first_user_message_submission_marks_registry_binding_pending() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("codex.json");
        let wrapper_session_id = "niuma_codex_1".to_string();
        update_registry(&registry_path, |registry| {
            registry.upsert(ManagedCodexSession {
                wrapper_session_id: wrapper_session_id.clone(),
                state: ManagedCodexSessionState::WaitingFirstUserMessage,
                cwd: "/tmp/repo".to_string(),
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

        mark_first_user_message_submitted(
            &registry_path,
            &wrapper_session_id,
            FirstUserMessageSubmission {
                content: "请继续".to_string(),
                thread_id: Some("session-1".to_string()),
            },
        )
        .unwrap();

        let registry = niuma_core::codex_managed_session::read_registry(&registry_path).unwrap();
        let session = &registry.sessions[0];
        assert_eq!(session.state, ManagedCodexSessionState::BindingPending);
        assert_eq!(
            session.first_user_message_hash.as_deref(),
            Some(first_user_message_hash("请继续").as_str())
        );
        assert_eq!(
            session.first_user_message_preview.as_deref(),
            Some("请继续")
        );
        assert_eq!(session.codex_session_id.as_deref(), Some("session-1"));
        assert!(session.first_user_message_submitted_at.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn relay_switches_nonblocking_client_back_to_blocking_mode() {
        use std::net::Shutdown;
        use std::os::unix::net::{UnixListener, UnixStream};

        let dir = tempfile::tempdir().unwrap();
        let real_socket = dir.path().join("real.sock");
        let listener = UnixListener::bind(&real_socket).unwrap();
        let upstream = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"ping");
            stream.write_all(b"pong").unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
        });

        let (mut tui, relay_client) = UnixStream::pair().unwrap();
        relay_client.set_nonblocking(true).unwrap();
        let shared = Arc::new(RelaySharedState::new(
            "wrapper-test".to_string(),
            dir.path().join("codex.json"),
            real_socket.clone(),
        ));
        let relay = thread::spawn({
            let real_socket = real_socket.clone();
            let shared = Arc::clone(&shared);
            move || handle_relay_connection(relay_client, &real_socket, shared)
        });

        // 留出一个短暂空窗，模拟 Codex HTTP 101 之后下一帧尚未到达的情况。
        thread::sleep(Duration::from_millis(30));
        tui.write_all(b"ping").unwrap();
        tui.shutdown(Shutdown::Write).unwrap();

        let mut response = [0u8; 4];
        tui.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"pong");

        upstream.join().unwrap();
        relay.join().unwrap().unwrap();
    }
}
