use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_managed_session::{
    update_registry, ManagedCodexSession, ManagedCodexSessionState,
};
use niuma_core::platform::paths::codex_managed_registry_path;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

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
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub command: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingInput {
    pub request_id: String,
    pub relay_request_id: String,
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
        let relay_request_id = jsonrpc_id.to_string();
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
            turn_id,
            item_id,
            command: params
                .get("command")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        });
    }

    fn observe_input_request(&mut self, jsonrpc_id: Value, params: Value) {
        let relay_request_id = jsonrpc_id.to_string();
        self.pending_inputs.push(PendingInput {
            request_id: format!(
                "codex-input:{}:{}",
                self.wrapper_session_id,
                relay_request_id.trim_matches('"')
            ),
            relay_request_id,
            questions: params
                .get("questions")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        });
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
    let base_dir = registry_path
        .parent()
        .ok_or_else(|| "Codex managed registry 路径缺少父目录".to_string())?
        .join("sockets")
        .join(&wrapper_session_id);

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

    run_app_server_remote_processes(real_codex, args, wrapper_session_id, base_dir)
}

fn run_app_server_remote_processes(
    _real_codex: PathBuf,
    _args: Vec<String>,
    _wrapper_session_id: String,
    _base_dir: PathBuf,
) -> Result<i32, String> {
    Err("app-server relay transport is wired in Task 4".to_string())
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
}
