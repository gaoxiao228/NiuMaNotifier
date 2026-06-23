use std::io::{self, BufRead, Write};

use niuma_core::models::ToolKind;
use niuma_core::tool_session_rpc::{
    ProviderRpcRequest, ProviderRpcResponse, SessionDetailParams, SessionSnapshotParams,
    SessionSnapshotResult,
};

// 启动 stdio JSON Lines provider；每行一个请求，每行一个响应。
pub fn run_stdio_session_provider() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            eprintln!("NiumaNotifier Codex session provider stdin read failed");
            continue;
        };
        let request = match serde_json::from_str::<ProviderRpcRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("NiumaNotifier Codex session provider ignored invalid JSON: {error}");
                continue;
            }
        };
        let response = handle_session_provider_request(request);
        let Ok(encoded) = serde_json::to_string(&response) else {
            eprintln!("NiumaNotifier Codex session provider response serialize failed");
            continue;
        };
        if writeln!(stdout, "{encoded}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            break;
        }
    }
}

// Task 6 只提供可启动 stub；真实 Codex 文件扫描和消息解析由 Task 7 接入。
pub fn handle_session_provider_request(request: ProviderRpcRequest) -> ProviderRpcResponse {
    match request.method.as_str() {
        "session_snapshot" => session_snapshot_response(request),
        "session_detail" => session_detail_response(request),
        method => ProviderRpcResponse::failure(
            request.id,
            "method_not_found",
            format!("provider method 不存在：{method}"),
        ),
    }
}

fn session_snapshot_response(request: ProviderRpcRequest) -> ProviderRpcResponse {
    let tool = request
        .params_as::<SessionSnapshotParams>()
        .map(|params| params.tool)
        .unwrap_or(ToolKind::Codex);
    ProviderRpcResponse::success(
        request.id,
        SessionSnapshotResult {
            tool,
            sessions: Vec::new(),
        },
    )
    .expect("empty session snapshot response must serialize")
}

fn session_detail_response(request: ProviderRpcRequest) -> ProviderRpcResponse {
    let session_id = request
        .params_as::<SessionDetailParams>()
        .map(|params| params.session_id)
        .unwrap_or_else(|_| "<unknown>".to_string());
    ProviderRpcResponse::failure(
        request.id,
        "session_not_found",
        format!("session_id 不存在：{session_id}"),
    )
}
