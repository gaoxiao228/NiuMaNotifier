use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::Value;
use std::fs;
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

pub fn run_codex_command(args: Vec<String>) {
    let real_codex = match resolve_real_codex() {
        Ok(path) => path,
        Err(error) => {
            print_and_exit(ApiResponse::fail(ApiErrorCode::BusinessValidation, error));
        }
    };

    match classify_codex_args(&args) {
        CodexLaunchMode::Managed => {
            let response = run_managed_codex(real_codex, args);
            print_and_exit(response);
        }
        CodexLaunchMode::Passthrough => match run_passthrough_codex(real_codex, args) {
            Ok(code) => std::process::exit(code),
            Err(error) => print_and_exit(ApiResponse::fail(ApiErrorCode::System, error)),
        },
    }
}

fn resolve_real_codex() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("获取 niuma 当前可执行文件失败：{error}"))?;

    if let Ok(value) = std::env::var("NIUMA_REAL_CODEX") {
        return validate_real_codex_path(PathBuf::from(value), &current_exe)
            .map_err(|error| format!("NIUMA_REAL_CODEX 无效：{error}"));
    }

    let path = which::which("codex").map_err(|_| {
        "找不到真实 codex，请设置 NIUMA_REAL_CODEX=/absolute/path/to/codex".to_string()
    })?;
    validate_real_codex_path(path, &current_exe).map_err(|error| {
        format!("PATH 中的 codex 不可用：{error}；请设置 NIUMA_REAL_CODEX=/absolute/path/to/codex")
    })
}

fn validate_real_codex_path(path: PathBuf, current_exe: &PathBuf) -> Result<PathBuf, String> {
    // 统一 canonicalize 后比较，避免 NIUMA_REAL_CODEX 或 PATH 命中 wrapper 自身导致递归。
    if !path.is_absolute() {
        return Err("必须使用绝对路径".to_string());
    }
    if !path.is_file() {
        return Err("路径不存在或不是文件".to_string());
    }

    let real_codex = path
        .canonicalize()
        .map_err(|error| format!("解析真实 codex 路径失败：{error}"))?;
    let current_exe = current_exe
        .canonicalize()
        .map_err(|error| format!("解析 niuma 当前可执行文件失败：{error}"))?;
    if real_codex == current_exe {
        return Err("真实 codex 不能指向 niuma 自身".to_string());
    }
    if !is_executable_file(&real_codex)? {
        return Err("真实 codex 没有可执行权限".to_string());
    }

    Ok(real_codex)
}

#[cfg(unix)]
fn is_executable_file(path: &PathBuf) -> Result<bool, String> {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path)
        .map_err(|error| format!("读取真实 codex 文件权限失败：{error}"))?
        .permissions()
        .mode();
    Ok(mode & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &PathBuf) -> Result<bool, String> {
    Ok(path.is_file())
}

fn run_passthrough_codex(real_codex: PathBuf, args: Vec<String>) -> Result<i32, String> {
    let status = match Command::new(real_codex).args(args).status() {
        Ok(status) => status,
        Err(error) => return Err(format!("启动 codex 失败：{error}")),
    };

    Ok(status.code().unwrap_or(1))
}

fn run_managed_codex(real_codex: PathBuf, args: Vec<String>) -> ApiResponse<Value> {
    crate::tools::codex::app_control::run_app_control(real_codex, args)
}

fn print_and_exit(response: ApiResponse<Value>) -> ! {
    // managed/stub 错误仍走统一响应格式；passthrough 路径不会进入这里。
    let code = response.code;
    crate::output::print_response(&response);
    std::process::exit(if code == 0 { 0 } else { 1 });
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

    #[test]
    fn rejects_relative_real_codex_path() {
        let current_exe = PathBuf::from("/tmp/niuma");
        let error = validate_real_codex_path(PathBuf::from("codex"), &current_exe).unwrap_err();
        assert!(error.contains("绝对路径"));
    }

    #[test]
    fn rejects_current_exe_as_real_codex() {
        let current_exe = std::env::current_exe().unwrap();
        let error = validate_real_codex_path(current_exe.clone(), &current_exe).unwrap_err();
        assert!(error.contains("不能指向 niuma 自身"));
    }

    #[cfg(unix)]
    #[test]
    fn passthrough_returns_real_codex_exit_code() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("codex-exit-7");
        fs::write(&script, "#!/bin/sh\nexit 7\n").unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        assert_eq!(run_passthrough_codex(script, Vec::new()).unwrap(), 7);
    }
}
