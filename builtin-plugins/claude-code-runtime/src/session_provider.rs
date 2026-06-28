use std::io::{self, BufRead, Write};
#[cfg(test)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use niuma_core::models::ToolKind;
use niuma_core::tool_session_rpc::{
    ProviderRpcRequest, ProviderRpcResponse, SessionDetailParams, SessionDetailResult,
    SessionSnapshotParams, SessionSnapshotResult,
};

use crate::claude::session_repository::{ClaudeSessionRepository, ProviderError};

pub(crate) struct ClaudeSessionProvider {
    repository: Arc<Mutex<ClaudeSessionRepository>>,
}

impl ClaudeSessionProvider {
    pub(crate) fn with_repository(repository: Arc<Mutex<ClaudeSessionRepository>>) -> Self {
        Self { repository }
    }

    #[cfg(test)]
    pub(crate) fn with_claude_home(claude_home: PathBuf) -> Self {
        Self {
            repository: Arc::new(Mutex::new(ClaudeSessionRepository::new(claude_home))),
        }
    }

    pub(crate) fn handle_request(&mut self, request: ProviderRpcRequest) -> ProviderRpcResponse {
        match request.method.as_str() {
            "session_snapshot" => self.session_snapshot_response(request),
            "session_detail" => self.session_detail_response(request),
            method => ProviderRpcResponse::failure(
                request.id,
                "method_not_found",
                format!("provider method 不存在：{method}"),
            ),
        }
    }

    fn session_snapshot_response(&mut self, request: ProviderRpcRequest) -> ProviderRpcResponse {
        let params = match request.params_as::<SessionSnapshotParams>() {
            Ok(params) => params,
            Err(error) => {
                return ProviderRpcResponse::failure(request.id, "invalid_params", error);
            }
        };
        if params.tool != ToolKind::ClaudeCode {
            return ProviderRpcResponse::failure(
                request.id,
                "unsupported_tool",
                "Claude Code session provider 只支持 claude_code",
            );
        }
        match self.refresh_snapshot() {
            Ok(sessions) => ProviderRpcResponse::success(
                request.id,
                SessionSnapshotResult {
                    tool: ToolKind::ClaudeCode,
                    sessions,
                },
            )
            .expect("Claude Code session snapshot response must serialize"),
            Err(error) => ProviderRpcResponse::failure(request.id, "snapshot_failed", error),
        }
    }

    fn session_detail_response(&mut self, request: ProviderRpcRequest) -> ProviderRpcResponse {
        let params = match request.params_as::<SessionDetailParams>() {
            Ok(params) => params,
            Err(error) => {
                return ProviderRpcResponse::failure(request.id, "invalid_params", error);
            }
        };
        if params.tool != ToolKind::ClaudeCode {
            return ProviderRpcResponse::failure(
                request.id,
                "unsupported_tool",
                "Claude Code session provider 只支持 claude_code",
            );
        }
        match self.session_detail(params) {
            Ok(detail) => ProviderRpcResponse::success(request.id, SessionDetailResult { detail })
                .expect("Claude Code session detail response must serialize"),
            Err(ProviderError { code, message }) => {
                ProviderRpcResponse::failure(request.id, code, message)
            }
        }
    }

    fn refresh_snapshot(
        &mut self,
    ) -> Result<Vec<niuma_core::tool_session::ToolSessionListItem>, String> {
        self.repository
            .lock()
            .map_err(|_| "Claude Code session repository lock poisoned".to_string())?
            .refresh_snapshot()
    }

    fn session_detail(
        &mut self,
        params: SessionDetailParams,
    ) -> Result<niuma_core::tool_session::ToolSessionDetail, ProviderError> {
        self.repository
            .lock()
            .map_err(|_| ProviderError::internal("Claude Code session repository lock poisoned"))?
            .session_detail(&params)
    }
}

// 启动 stdio JSON Lines provider；stdout 必须只输出 RPC 消息，日志统一走 stderr。
pub(crate) fn run_stdio_session_provider_with_repository(
    repository: Arc<Mutex<ClaudeSessionRepository>>,
) {
    let stdin = io::stdin();
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let provider = Arc::new(Mutex::new(ClaudeSessionProvider::with_repository(
        repository,
    )));
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            eprintln!("NiumaNotifier Claude Code session provider stdin read failed");
            continue;
        };
        let request = match serde_json::from_str::<ProviderRpcRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!(
                    "NiumaNotifier Claude Code session provider ignored invalid JSON: {error}"
                );
                continue;
            }
        };
        let response = match provider.lock() {
            Ok(mut provider) => provider.handle_request(request),
            Err(_) => {
                eprintln!("NiumaNotifier Claude Code session provider state lock poisoned");
                break;
            }
        };
        if write_provider_message(&stdout, &response).is_err() {
            break;
        }
    }
}

fn write_provider_message<W, T>(writer: &Arc<Mutex<W>>, message: &T) -> Result<(), String>
where
    W: Write,
    T: serde::Serialize,
{
    let encoded = serde_json::to_string(message)
        .map_err(|error| format!("序列化 Claude Code provider RPC 消息失败：{error}"))?;
    let mut writer = writer
        .lock()
        .map_err(|_| "Claude Code provider stdout lock poisoned".to_string())?;
    writeln!(writer, "{encoded}")
        .map_err(|error| format!("写入 Claude Code provider stdout 失败：{error}"))
}
