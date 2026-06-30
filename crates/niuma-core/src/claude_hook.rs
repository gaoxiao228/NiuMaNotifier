use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Map, Value};

// Claude 外层 hook timeout 必须大于内部 10 分钟授权代理等待，否则 UI 决策无法回传给 Claude。
const CLAUDE_PERMISSION_REQUEST_HOOK_TIMEOUT_SECONDS: u64 = 660;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaudeHookCommand {
    Installed,
    Dev { manifest_path: PathBuf },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ClaudeHookStatus {
    pub tool: String,
    pub claude_config_dir: PathBuf,
    pub settings_file: PathBuf,
    pub settings_file_exists: bool,
    pub installed: bool,
    pub command_mode: String,
    pub events: BTreeMap<String, String>,
}

pub fn claude_settings_file(claude_config_dir: &Path) -> PathBuf {
    claude_config_dir.join("settings.json")
}

pub fn install_claude_hook(
    claude_config_dir: &Path,
    command_mode: ClaudeHookCommand,
) -> Result<ClaudeHookStatus, String> {
    fs::create_dir_all(claude_config_dir)
        .map_err(|error| format!("创建 CLAUDE_CONFIG_DIR 失败：{error}"))?;
    let settings_file = claude_settings_file(claude_config_dir);
    let mut settings = read_settings_file(&settings_file)?;
    if settings_file.exists() {
        backup_settings_file(&settings_file)?;
    }
    remove_niuma_hooks(&mut settings);
    upsert_permission_request_hook(
        &mut settings,
        &hook_command(&command_mode),
        "Reporting Claude Code approval request to Niuma",
    );
    write_settings_file(&settings_file, &settings)?;
    read_claude_hook_status_with_mode(claude_config_dir, command_mode_label(&command_mode))
}

pub fn uninstall_claude_hook(claude_config_dir: &Path) -> Result<ClaudeHookStatus, String> {
    let settings_file = claude_settings_file(claude_config_dir);
    let mut settings = read_settings_file(&settings_file)?;
    if settings_contains_niuma_hook(&settings) {
        if settings_file.exists() {
            backup_settings_file(&settings_file)?;
        }
        remove_niuma_hooks(&mut settings);
        write_settings_file(&settings_file, &settings)?;
    }
    read_claude_hook_status_with_mode(claude_config_dir, "uninstalled")
}

pub fn read_claude_hook_status(claude_config_dir: &Path) -> Result<ClaudeHookStatus, String> {
    read_claude_hook_status_with_mode(claude_config_dir, "detected")
}

fn read_claude_hook_status_with_mode(
    claude_config_dir: &Path,
    command_mode: &str,
) -> Result<ClaudeHookStatus, String> {
    let settings_file = claude_settings_file(claude_config_dir);
    let settings = read_settings_file(&settings_file)?;
    Ok(status_from_settings(
        claude_config_dir,
        settings,
        command_mode,
    ))
}

fn read_settings_file(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let body = fs::read_to_string(path)
        .map_err(|error| format!("读取 Claude settings.json 失败：{error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("解析 Claude settings.json 失败：{error}"))
}

fn write_settings_file(path: &Path, settings: &Value) -> Result<(), String> {
    let body = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("序列化 Claude settings.json 失败：{error}"))?;
    fs::write(path, format!("{body}\n"))
        .map_err(|error| format!("写入 Claude settings.json 失败：{error}"))
}

fn backup_settings_file(path: &Path) -> Result<(), String> {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let backup = path.with_file_name(format!("settings.json.niuma-backup-{timestamp}"));
    fs::copy(path, backup)
        .map(|_| ())
        .map_err(|error| format!("备份 Claude settings.json 失败：{error}"))
}

fn hook_command(command_mode: &ClaudeHookCommand) -> String {
    match command_mode {
        ClaudeHookCommand::Installed => {
            "niuma internal hook claude-code --source niuma-notifier".to_string()
        }
        ClaudeHookCommand::Dev { manifest_path } => format!(
            "cargo run --manifest-path {} -p niuma-cli -- internal hook claude-code --source niuma-notifier",
            shell_quote(&manifest_path.display().to_string())
        ),
    }
}

fn command_mode_label(command_mode: &ClaudeHookCommand) -> &'static str {
    match command_mode {
        ClaudeHookCommand::Installed => "installed",
        ClaudeHookCommand::Dev { .. } => "dev",
    }
}

fn upsert_permission_request_hook(settings: &mut Value, command: &str, status_message: &str) {
    ensure_hooks_object(settings);
    let groups = settings["hooks"]
        .as_object_mut()
        .unwrap()
        .entry("PermissionRequest".to_string())
        .or_insert_with(|| json!([]));
    if !groups.is_array() {
        *groups = json!([]);
    }
    remove_niuma_hooks_from_groups(groups);

    let mut group = Map::new();
    group.insert(
        "hooks".to_string(),
        json!([{
            "type": "command",
            "command": command,
            "timeout": CLAUDE_PERMISSION_REQUEST_HOOK_TIMEOUT_SECONDS,
            "statusMessage": status_message
        }]),
    );
    groups.as_array_mut().unwrap().push(Value::Object(group));
}

fn ensure_hooks_object(settings: &mut Value) {
    if !settings.is_object() {
        *settings = json!({});
    }
    if !settings.get("hooks").map(Value::is_object).unwrap_or(false) {
        settings["hooks"] = json!({});
    }
}

fn remove_niuma_hooks(settings: &mut Value) {
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut() {
            remove_niuma_hooks_from_groups(groups);
        }
        hooks.retain(|_, groups| {
            groups
                .as_array()
                .map(|items| !items.is_empty())
                .unwrap_or(true)
        });
    }
}

fn remove_niuma_hooks_from_groups(groups: &mut Value) {
    let Some(groups) = groups.as_array_mut() else {
        return;
    };
    for group in groups.iter_mut() {
        if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            hooks.retain(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .map(|command| !is_niuma_claude_hook(command))
                    .unwrap_or(true)
            });
        }
    }
    groups.retain(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .map(|hooks| !hooks.is_empty())
            .unwrap_or(true)
    });
}

fn settings_contains_niuma_hook(settings: &Value) -> bool {
    settings
        .get("hooks")
        .and_then(Value::as_object)
        .map(|events| {
            events.values().any(|groups| {
                groups
                    .as_array()
                    .map(|items| groups_contain_niuma_hook(items))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn groups_contain_niuma_hook(groups: &[Value]) -> bool {
    groups.iter().any(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .map(|hooks| hooks.iter().any(hook_is_niuma_claude))
            .unwrap_or(false)
    })
}

fn hook_is_niuma_claude(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .map(is_niuma_claude_hook)
        .unwrap_or(false)
}

fn is_niuma_claude_hook(command: &str) -> bool {
    command.contains("internal hook claude-code") && command.contains("--source niuma-notifier")
}

fn status_from_settings(
    claude_config_dir: &Path,
    settings: Value,
    command_mode: &str,
) -> ClaudeHookStatus {
    let installed = settings
        .get("hooks")
        .and_then(|hooks| hooks.get("PermissionRequest"))
        .and_then(Value::as_array)
        .map(|items| groups_contain_niuma_hook(items))
        .unwrap_or(false);
    let mut events = BTreeMap::new();
    events.insert(
        "PermissionRequest".to_string(),
        if installed { "installed" } else { "missing" }.to_string(),
    );
    ClaudeHookStatus {
        tool: "claude_code".to_string(),
        claude_config_dir: claude_config_dir.to_path_buf(),
        settings_file: claude_settings_file(claude_config_dir),
        settings_file_exists: claude_settings_file(claude_config_dir).exists(),
        installed,
        command_mode: command_mode.to_string(),
        events,
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_creates_settings_json_with_permission_request_hook() {
        let dir = tempfile::tempdir().unwrap();
        let status = install_claude_hook(dir.path(), ClaudeHookCommand::Installed).unwrap();
        let settings_path = claude_settings_file(dir.path());
        let body = std::fs::read_to_string(&settings_path).unwrap();

        assert!(settings_path.exists());
        assert!(body.contains("\"PermissionRequest\""));
        assert!(body.contains("niuma internal hook claude-code --source niuma-notifier"));
        assert_eq!(status.tool, "claude_code");
        assert!(status.installed);
        assert_eq!(status.events.get("PermissionRequest").unwrap(), "installed");
    }

    #[test]
    fn uninstall_removes_only_niuma_claude_hook() {
        let dir = tempfile::tempdir().unwrap();
        install_claude_hook(dir.path(), ClaudeHookCommand::Installed).unwrap();

        let status = uninstall_claude_hook(dir.path()).unwrap();
        let body = std::fs::read_to_string(claude_settings_file(dir.path())).unwrap();

        assert!(!status.installed);
        assert!(!body.contains("internal hook claude-code"));
    }
}
