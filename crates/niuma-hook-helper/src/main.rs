use std::io::Read;

use chrono::Utc;
use niuma_api::local_api_addr;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::hook_payload::{HookPayloadParser, HookToolHint};
use niuma_core::local_api_client::submit_event_to_local_api;
use serde_json::json;

fn main() {
    let output = run();
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("API envelope 必须可序列化")
    );
}

fn run() -> ApiResponse<serde_json::Value> {
    let Some(tool_hint) = parse_tool_hint() else {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, "缺少或不支持 --tool 参数");
    };

    let mut input = Vec::new();
    if let Err(error) = std::io::stdin().read_to_end(&mut input) {
        return ApiResponse::fail(ApiErrorCode::System, format!("读取 stdin 失败：{error}"));
    }
    if input.is_empty() {
        return ApiResponse::fail(ApiErrorCode::BusinessValidation, "hook payload 不能为空");
    }

    match HookPayloadParser::parse(&input, tool_hint, Utc::now()) {
        Ok(Some(event)) => match submit_event_to_local_api(&local_api_addr(), &event) {
            Ok(api_body) => ApiResponse::ok(json!({
                "event": event,
                "submitted": true,
                "local_api": api_body
            })),
            Err(error) => ApiResponse::ok(json!({
                "event": event,
                "submitted": false,
                "submit_error": error
            })),
        },
        Ok(None) => ApiResponse::ok(json!({ "ignored": true })),
        Err(error) => ApiResponse::fail(
            ApiErrorCode::ParameterFormat,
            format!("JSON 解析失败：{error}"),
        ),
    }
}

fn parse_tool_hint() -> Option<HookToolHint> {
    let args = std::env::args().collect::<Vec<_>>();
    for index in 0..args.len() {
        let value = args[index].as_str();
        if value == "--tool" {
            return args.get(index + 1).and_then(|tool| tool_hint_from(tool));
        }
        if let Some(tool) = value.strip_prefix("--tool=") {
            return tool_hint_from(tool);
        }
    }
    None
}

fn tool_hint_from(value: &str) -> Option<HookToolHint> {
    match value.trim().to_lowercase().as_str() {
        "codex" => Some(HookToolHint::Codex),
        "claude" | "claude-code" | "claudecode" => Some(HookToolHint::ClaudeCode),
        _ => None,
    }
}
