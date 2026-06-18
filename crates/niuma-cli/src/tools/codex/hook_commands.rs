use niuma_api::local_api_addr;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::tools::codex::hook::{
    install_codex_hook, read_codex_hook_status, uninstall_codex_hook, CodexHookCommand,
};
use serde_json::json;

use crate::cli::{HookCommand, ToolArg};

pub(crate) fn run_hook_command(command: HookCommand) -> ApiResponse<serde_json::Value> {
    let mode_count = [command.install, command.uninstall, command.doctor]
        .iter()
        .filter(|enabled| **enabled)
        .count();
    if mode_count > 1 {
        return ApiResponse::fail(
            ApiErrorCode::BusinessValidation,
            "hook codex 只能同时指定一种操作",
        );
    }

    match command.tool {
        ToolArg::Codex if command.install => codex_hook_install(),
        ToolArg::Codex if command.uninstall => codex_hook_uninstall(),
        ToolArg::Codex if command.doctor => codex_hook_doctor(),
        ToolArg::Codex => codex_hook_status(),
    }
}

fn codex_hook_command_mode() -> CodexHookCommand {
    if niuma_core::platform::executable::command_on_path("niuma") {
        CodexHookCommand::Installed
    } else {
        CodexHookCommand::Dev {
            manifest_path: repo_manifest_path(),
        }
    }
}

pub(crate) fn codex_hook_status() -> ApiResponse<serde_json::Value> {
    match read_codex_hook_status(&codex_home()) {
        Ok(status) => ApiResponse::ok(json!(status)),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn codex_hook_install() -> ApiResponse<serde_json::Value> {
    match install_codex_hook(&codex_home(), codex_hook_command_mode()) {
        Ok(status) => ApiResponse::ok(json!({
            "status": status,
            "next_step": "在 Codex 中执行 /hooks，审核并信任 Niuma hook。"
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn codex_hook_uninstall() -> ApiResponse<serde_json::Value> {
    match uninstall_codex_hook(&codex_home()) {
        Ok(status) => ApiResponse::ok(json!(status)),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn codex_hook_doctor() -> ApiResponse<serde_json::Value> {
    match read_codex_hook_status(&codex_home()) {
        Ok(status) => ApiResponse::ok(json!({
            "status": status,
            "checks": {
                "niuma_on_path": niuma_core::platform::executable::command_on_path("niuma"),
                "local_api": local_api_addr()
            }
        })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn codex_home() -> std::path::PathBuf {
    // Codex 默认读取 ~/.codex；测试和开发环境可通过 CODEX_HOME 覆盖。
    niuma_core::config::codex_home()
}

fn repo_manifest_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("Cargo.toml")
}
