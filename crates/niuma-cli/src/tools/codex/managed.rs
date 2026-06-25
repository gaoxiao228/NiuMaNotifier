use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexLaunchMode {
    Managed,
    Passthrough,
}

const PASSTHROUGH_SUBCOMMANDS: &[&str] =
    &["resume", "exec", "app-server", "login", "logout", "auth"];

pub fn classify_codex_args(args: &[String]) -> CodexLaunchMode {
    let Some(first) = args.first().map(String::as_str) else {
        return CodexLaunchMode::Managed;
    };

    // Codex 自带帮助、版本和已有子命令先保持原样直通，避免 wrapper 改变原生命令语义。
    if matches!(first, "--help" | "-h" | "--version" | "-V") {
        return CodexLaunchMode::Passthrough;
    }
    if first.starts_with('-') {
        return CodexLaunchMode::Managed;
    }
    if PASSTHROUGH_SUBCOMMANDS.contains(&first) {
        return CodexLaunchMode::Passthrough;
    }

    CodexLaunchMode::Passthrough
}

pub fn run_codex_command(args: Vec<String>) -> ApiResponse<Value> {
    let real_codex = match resolve_real_codex() {
        Ok(path) => path,
        Err(error) => return ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    };

    match classify_codex_args(&args) {
        CodexLaunchMode::Managed => run_managed_codex(real_codex, args),
        CodexLaunchMode::Passthrough => run_passthrough_codex(real_codex, args),
    }
}

fn resolve_real_codex() -> Result<PathBuf, String> {
    if let Ok(value) = std::env::var("NIUMA_REAL_CODEX") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }

        return Err("NIUMA_REAL_CODEX 指向的 codex 不存在或不是文件".to_string());
    }

    which::which("codex").map_err(|_| {
        "找不到真实 codex，请设置 NIUMA_REAL_CODEX=/absolute/path/to/codex".to_string()
    })
}

fn run_passthrough_codex(real_codex: PathBuf, args: Vec<String>) -> ApiResponse<Value> {
    let status = match Command::new(real_codex).args(args).status() {
        Ok(status) => status,
        Err(error) => {
            return ApiResponse::fail(ApiErrorCode::System, format!("启动 codex 失败：{error}"))
        }
    };

    ApiResponse::ok(json!({
        "mode": "passthrough",
        "exit_code": status.code().unwrap_or(1),
    }))
}

fn run_managed_codex(real_codex: PathBuf, args: Vec<String>) -> ApiResponse<Value> {
    crate::tools::codex::app_control::run_app_control(real_codex, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_empty_and_flags_as_managed() {
        assert_eq!(classify_codex_args(&[]), CodexLaunchMode::Managed);
        assert_eq!(
            classify_codex_args(&["--model".into(), "gpt-5".into()]),
            CodexLaunchMode::Managed
        );
        assert_eq!(
            classify_codex_args(&["-c".into(), "model=gpt-5".into()]),
            CodexLaunchMode::Managed
        );
    }

    #[test]
    fn classifies_known_passthrough_commands_and_help_as_passthrough() {
        for args in [
            vec!["resume".to_string()],
            vec!["exec".to_string(), "echo hi".to_string()],
            vec!["app-server".to_string()],
            vec!["login".to_string()],
            vec!["logout".to_string()],
            vec!["auth".to_string()],
            vec!["--help".to_string()],
            vec!["-h".to_string()],
            vec!["--version".to_string()],
            vec!["-V".to_string()],
            vec!["unknown-subcommand".to_string()],
        ] {
            assert_eq!(classify_codex_args(&args), CodexLaunchMode::Passthrough);
        }
    }
}
