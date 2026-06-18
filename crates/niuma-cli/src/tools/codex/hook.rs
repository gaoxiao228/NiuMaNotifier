use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexHookCommand {
    Installed,
    Dev { manifest_path: PathBuf },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CodexHookStatus {
    pub tool: String,
    pub codex_home: PathBuf,
    pub hooks_file: PathBuf,
    pub hooks_file_exists: bool,
    pub installed: bool,
    pub command_mode: String,
    pub needs_trust_review: bool,
    pub events: BTreeMap<String, String>,
}

pub fn codex_hooks_file(codex_home: &Path) -> PathBuf {
    codex_home.join("hooks.json")
}

pub fn install_codex_hook(
    codex_home: &Path,
    command_mode: CodexHookCommand,
) -> Result<CodexHookStatus, String> {
    fs::create_dir_all(codex_home).map_err(|error| format!("创建 CODEX_HOME 失败：{error}"))?;
    let hooks_file = codex_hooks_file(codex_home);
    let existed = hooks_file.exists();
    let mut config = read_hooks_file(&hooks_file)?;
    if existed {
        backup_hooks_file(&hooks_file)?;
    }

    let command = hook_command(&command_mode);
    upsert_event(
        &mut config,
        "SessionStart",
        Some("startup|resume|clear|compact"),
        &command,
        "Reporting Codex status to Niuma",
    );
    upsert_event(
        &mut config,
        "PermissionRequest",
        None,
        &command,
        "Reporting Codex approval request to Niuma",
    );
    upsert_event(
        &mut config,
        "Stop",
        None,
        &command,
        "Reporting Codex completion to Niuma",
    );

    write_hooks_file(&hooks_file, &config)?;
    Ok(status_from_config(
        codex_home,
        config,
        command_mode_label(&command_mode),
    ))
}

pub fn uninstall_codex_hook(codex_home: &Path) -> Result<CodexHookStatus, String> {
    let hooks_file = codex_hooks_file(codex_home);
    let mut config = read_hooks_file(&hooks_file)?;
    remove_niuma_hooks(&mut config);
    write_hooks_file(&hooks_file, &config)?;
    Ok(status_from_config(codex_home, config, "uninstalled"))
}

pub fn read_codex_hook_status(codex_home: &Path) -> Result<CodexHookStatus, String> {
    let config = read_hooks_file(&codex_hooks_file(codex_home))?;
    Ok(status_from_config(codex_home, config, "detected"))
}

fn read_hooks_file(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({ "hooks": {} }));
    }
    let body =
        fs::read_to_string(path).map_err(|error| format!("读取 Codex hooks.json 失败：{error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("解析 Codex hooks.json 失败：{error}"))
}

fn write_hooks_file(path: &Path, config: &Value) -> Result<(), String> {
    let body = serde_json::to_string_pretty(config)
        .map_err(|error| format!("序列化 Codex hooks.json 失败：{error}"))?;
    fs::write(path, format!("{body}\n"))
        .map_err(|error| format!("写入 Codex hooks.json 失败：{error}"))
}

fn backup_hooks_file(path: &Path) -> Result<(), String> {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let backup = path.with_file_name(format!("hooks.json.niuma-backup-{timestamp}"));
    fs::copy(path, backup)
        .map(|_| ())
        .map_err(|error| format!("备份 Codex hooks.json 失败：{error}"))
}

fn hook_command(command_mode: &CodexHookCommand) -> String {
    match command_mode {
        CodexHookCommand::Installed => {
            "niuma internal hook codex --source niuma-notifier".to_string()
        }
        CodexHookCommand::Dev { manifest_path } => format!(
            "cargo run --manifest-path {} -p niuma-cli -- internal hook codex --source niuma-notifier",
            shell_quote(&manifest_path.display().to_string())
        ),
    }
}

fn command_mode_label(command_mode: &CodexHookCommand) -> &'static str {
    match command_mode {
        CodexHookCommand::Installed => "installed",
        CodexHookCommand::Dev { .. } => "dev",
    }
}

fn upsert_event(
    config: &mut Value,
    event: &str,
    matcher: Option<&str>,
    command: &str,
    status_message: &str,
) {
    ensure_hooks_object(config);
    let groups = config["hooks"]
        .as_object_mut()
        .unwrap()
        .entry(event.to_string())
        .or_insert_with(|| json!([]));
    if !groups.is_array() {
        *groups = json!([]);
    }
    remove_niuma_hooks_from_groups(groups);

    // matcher 为 None 时不写入字段，避免给不需要 matcher 的事件留下 null。
    let mut group = Map::new();
    if let Some(matcher) = matcher {
        group.insert("matcher".to_string(), json!(matcher));
    }
    group.insert(
        "hooks".to_string(),
        json!([{
            "type": "command",
            "command": command,
            "timeout": 30,
            "statusMessage": status_message
        }]),
    );
    groups.as_array_mut().unwrap().push(Value::Object(group));
}

fn is_niuma_codex_hook(command: &str) -> bool {
    command.contains("internal hook codex") && command.contains("--source niuma-notifier")
}

fn status_from_config(codex_home: &Path, config: Value, command_mode: &str) -> CodexHookStatus {
    let mut events = BTreeMap::new();
    for event in ["SessionStart", "PermissionRequest", "Stop"] {
        let installed = config
            .get("hooks")
            .and_then(Value::as_object)
            .and_then(|hooks| hooks.get(event))
            .and_then(Value::as_array)
            .map(|groups| {
                groups.iter().any(|group| {
                    group
                        .get("hooks")
                        .and_then(Value::as_array)
                        .map(|hooks| {
                            hooks.iter().any(|hook| {
                                hook.get("command")
                                    .and_then(Value::as_str)
                                    .map(is_niuma_codex_hook)
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        events.insert(
            event.to_string(),
            if installed { "installed" } else { "missing" }.to_string(),
        );
    }
    let installed = events.values().all(|value| value == "installed");
    CodexHookStatus {
        tool: "codex".to_string(),
        codex_home: codex_home.to_path_buf(),
        hooks_file: codex_hooks_file(codex_home),
        hooks_file_exists: codex_hooks_file(codex_home).exists(),
        installed,
        command_mode: command_mode.to_string(),
        needs_trust_review: installed,
        events,
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn ensure_hooks_object(config: &mut Value) {
    if !config.is_object() {
        *config = json!({});
    }
    if !config.get("hooks").map(Value::is_object).unwrap_or(false) {
        config["hooks"] = json!({});
    }
}

fn remove_niuma_hooks(config: &mut Value) {
    if let Some(hooks) = config.get_mut("hooks").and_then(Value::as_object_mut) {
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
    let Some(groups_array) = groups.as_array_mut() else {
        return;
    };
    for group in groups_array.iter_mut() {
        if let Some(commands) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            commands.retain(|hook| {
                !hook
                    .get("command")
                    .and_then(Value::as_str)
                    .map(is_niuma_codex_hook)
                    .unwrap_or(false)
            });
        }
    }
    groups_array.retain(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .map(|commands| !commands.is_empty())
            .unwrap_or(true)
    });
}

#[cfg(test)]
mod tests;
