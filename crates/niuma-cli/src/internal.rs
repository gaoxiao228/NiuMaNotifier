use niuma_core::api_response::{ApiErrorCode, ApiResponse};

use crate::cli::{InternalCommand, InternalRootCommand, ToolArg};
use crate::hook_runtime;

pub(crate) fn run_internal_command(command: InternalRootCommand) {
    let response = match command.command {
        InternalCommand::Hook {
            tool: ToolArg::Codex,
            source,
        } => {
            if source.as_deref() != Some("niuma-notifier") {
                ApiResponse::fail(
                    ApiErrorCode::BusinessValidation,
                    "internal hook source 不合法",
                )
            } else {
                hook_runtime::run_codex_hook()
            }
        }
        InternalCommand::Hook {
            tool: ToolArg::ClaudeCode,
            ..
        } => ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            "Claude Code internal hook 尚未实现",
        ),
    };
    // Codex 会把 hook stdout 当成事件专用协议 JSON 解析；Niuma 的调试 envelope 只能写 stderr。
    eprintln!(
        "{}",
        serde_json::to_string_pretty(&response).expect("API envelope 必须可序列化")
    );
}
