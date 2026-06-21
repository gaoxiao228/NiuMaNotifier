use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Map, Value};

// Codex 外层 hook timeout 必须大于内部 10 分钟授权代理等待，否则 UI 决策无法回传给 Codex。
const CODEX_PERMISSION_REQUEST_HOOK_TIMEOUT_SECONDS: u64 = 660;

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
    pub config_file: PathBuf,
    pub hooks_file_exists: bool,
    pub config_file_exists: bool,
    pub active_file: Option<PathBuf>,
    pub active_format: String,
    pub installed: bool,
    pub command_mode: String,
    pub needs_trust_review: bool,
    pub events: BTreeMap<String, String>,
}

pub fn codex_hooks_file(codex_home: &Path) -> PathBuf {
    codex_home.join("hooks.json")
}

pub fn codex_config_file(codex_home: &Path) -> PathBuf {
    codex_home.join("config.toml")
}

pub fn install_codex_hook(
    codex_home: &Path,
    command_mode: CodexHookCommand,
) -> Result<CodexHookStatus, String> {
    fs::create_dir_all(codex_home).map_err(|error| format!("创建 CODEX_HOME 失败：{error}"))?;
    let hooks_file = codex_hooks_file(codex_home);
    let config_file = codex_config_file(codex_home);
    let mut hooks_config = read_hooks_file(&hooks_file)?;
    let mut config_toml = read_config_file(&config_file)?;
    let target = select_hook_target(&hooks_config, &config_toml);

    let command = hook_command(&command_mode);
    match target {
        HookTarget::HooksJson => {
            if hooks_file.exists() {
                backup_hooks_file(&hooks_file)?;
            }
            // 旧版本曾安装 SessionStart/Stop；当前 Codex hook 只保留权限请求，先清理旧 Niuma hook。
            remove_niuma_hooks(&mut hooks_config);
            upsert_event(
                &mut hooks_config,
                "PermissionRequest",
                None,
                &command,
                "Reporting Codex approval request to Niuma",
            );
            write_hooks_file(&hooks_file, &hooks_config)?;
        }
        HookTarget::ConfigToml => {
            if config_file.exists() {
                backup_config_file(&config_file)?;
            }
            remove_niuma_hooks_from_config(&mut config_toml);
            upsert_config_event(
                &mut config_toml,
                "PermissionRequest",
                None,
                &command,
                "Reporting Codex approval request to Niuma",
            );
            write_config_file(&config_file, &config_toml)?;
            cleanup_hooks_file_niuma_entries(&hooks_file, &mut hooks_config)?;
        }
    }

    Ok(status_from_sources(
        codex_home,
        read_hooks_file(&hooks_file)?,
        read_config_file(&config_file)?,
        command_mode_label(&command_mode),
    ))
}

pub fn uninstall_codex_hook(codex_home: &Path) -> Result<CodexHookStatus, String> {
    let hooks_file = codex_hooks_file(codex_home);
    let config_file = codex_config_file(codex_home);
    let mut hooks_config = read_hooks_file(&hooks_file)?;
    let mut config_toml = read_config_file(&config_file)?;
    cleanup_hooks_file_niuma_entries(&hooks_file, &mut hooks_config)?;
    if config_toml_contains_niuma_hook(&config_toml) {
        if config_file.exists() {
            backup_config_file(&config_file)?;
        }
        remove_niuma_hooks_from_config(&mut config_toml);
        write_config_file(&config_file, &config_toml)?;
    }
    Ok(status_from_sources(
        codex_home,
        read_hooks_file(&hooks_file)?,
        read_config_file(&config_file)?,
        "uninstalled",
    ))
}

pub fn read_codex_hook_status(codex_home: &Path) -> Result<CodexHookStatus, String> {
    let hooks_config = read_hooks_file(&codex_hooks_file(codex_home))?;
    let config_toml = read_config_file(&codex_config_file(codex_home))?;
    Ok(status_from_sources(
        codex_home,
        hooks_config,
        config_toml,
        "detected",
    ))
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

fn read_config_file(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).map_err(|error| format!("读取 Codex config.toml 失败：{error}"))
}

fn write_config_file(path: &Path, body: &str) -> Result<(), String> {
    fs::write(path, body).map_err(|error| format!("写入 Codex config.toml 失败：{error}"))
}

fn backup_hooks_file(path: &Path) -> Result<(), String> {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let backup = path.with_file_name(format!("hooks.json.niuma-backup-{timestamp}"));
    fs::copy(path, backup)
        .map(|_| ())
        .map_err(|error| format!("备份 Codex hooks.json 失败：{error}"))
}

fn backup_config_file(path: &Path) -> Result<(), String> {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let backup = path.with_file_name(format!("config.toml.niuma-backup-{timestamp}"));
    fs::copy(path, backup)
        .map(|_| ())
        .map_err(|error| format!("备份 Codex config.toml 失败：{error}"))
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
            "timeout": CODEX_PERMISSION_REQUEST_HOOK_TIMEOUT_SECONDS,
            "statusMessage": status_message
        }]),
    );
    groups.as_array_mut().unwrap().push(Value::Object(group));
}

fn upsert_config_event(
    config: &mut String,
    event: &str,
    matcher: Option<&str>,
    command: &str,
    status_message: &str,
) {
    if !config.is_empty() && !config.ends_with('\n') {
        config.push('\n');
    }
    if !config.is_empty() {
        config.push('\n');
    }
    config.push_str(&format!("[[hooks.{event}]]\n"));
    if let Some(matcher) = matcher {
        config.push_str(&format!("matcher = {}\n", toml_string(matcher)));
    }
    config.push_str(&format!(
        "\n[[hooks.{event}.hooks]]\ntype = \"command\"\ncommand = {}\ntimeout = {}\nstatusMessage = {}\n",
        toml_string(command),
        CODEX_PERMISSION_REQUEST_HOOK_TIMEOUT_SECONDS,
        toml_string(status_message)
    ));
}

fn is_niuma_codex_hook(command: &str) -> bool {
    command.contains("internal hook codex") && command.contains("--source niuma-notifier")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HookTarget {
    HooksJson,
    ConfigToml,
}

fn select_hook_target(hooks_config: &Value, config_toml: &str) -> HookTarget {
    if config_has_hook_definitions(config_toml) {
        HookTarget::ConfigToml
    } else if json_has_hook_definitions(hooks_config) {
        HookTarget::HooksJson
    } else {
        HookTarget::HooksJson
    }
}

fn status_from_sources(
    codex_home: &Path,
    hooks_config: Value,
    config_toml: String,
    command_mode: &str,
) -> CodexHookStatus {
    let mut events = BTreeMap::new();
    for event in ["PermissionRequest"] {
        let installed = json_event_has_niuma_hook(&hooks_config, event)
            || config_event_has_niuma_hook(&config_toml, event);
        events.insert(
            event.to_string(),
            if installed { "installed" } else { "missing" }.to_string(),
        );
    }
    let installed = events.values().all(|value| value == "installed");
    let active_file = if config_toml_contains_niuma_hook(&config_toml) {
        Some(codex_config_file(codex_home))
    } else if json_contains_niuma_hook(&hooks_config) {
        Some(codex_hooks_file(codex_home))
    } else {
        None
    };
    let active_format = active_file
        .as_ref()
        .map(|path| {
            if path.file_name().and_then(|name| name.to_str()) == Some("config.toml") {
                "config.toml"
            } else {
                "hooks.json"
            }
        })
        .unwrap_or("none")
        .to_string();
    CodexHookStatus {
        tool: "codex".to_string(),
        codex_home: codex_home.to_path_buf(),
        hooks_file: codex_hooks_file(codex_home),
        config_file: codex_config_file(codex_home),
        hooks_file_exists: codex_hooks_file(codex_home).exists(),
        config_file_exists: codex_config_file(codex_home).exists(),
        active_file,
        active_format,
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

fn cleanup_hooks_file_niuma_entries(path: &Path, config: &mut Value) -> Result<(), String> {
    if !path.exists() || !json_contains_niuma_hook(config) {
        return Ok(());
    }
    backup_hooks_file(path)?;
    remove_niuma_hooks(config);
    if json_can_remove_file(config) {
        fs::remove_file(path).map_err(|error| format!("删除 Codex hooks.json 失败：{error}"))
    } else {
        write_hooks_file(path, config)
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

fn json_has_hook_definitions(config: &Value) -> bool {
    config
        .get("hooks")
        .and_then(Value::as_object)
        .map(|hooks| {
            hooks.values().any(|groups| {
                groups
                    .as_array()
                    .map(|groups| {
                        groups.iter().any(|group| {
                            group
                                .get("hooks")
                                .and_then(Value::as_array)
                                .map(|commands| !commands.is_empty())
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn json_event_has_niuma_hook(config: &Value, event: &str) -> bool {
    config
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
        .unwrap_or(false)
}

fn json_contains_niuma_hook(config: &Value) -> bool {
    ["PermissionRequest", "SessionStart", "Stop"]
        .iter()
        .any(|event| json_event_has_niuma_hook(config, event))
}

fn json_can_remove_file(config: &Value) -> bool {
    let Some(root) = config.as_object() else {
        return false;
    };
    root.keys().all(|key| key == "hooks") && !json_has_hook_definitions(config)
}

fn config_has_hook_definitions(config: &str) -> bool {
    config.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("[[hooks.") && trimmed.ends_with(".hooks]]")
    })
}

fn config_toml_contains_niuma_hook(config: &str) -> bool {
    config.lines().any(|line| {
        line.trim_start().starts_with("command")
            && line.contains("internal hook codex")
            && line.contains("--source niuma-notifier")
    })
}

fn config_event_has_niuma_hook(config: &str, event: &str) -> bool {
    let mut in_event_hook = false;
    let event_hook_header = format!("[[hooks.{event}.hooks]]");
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[[") && trimmed.ends_with("]]") {
            in_event_hook = trimmed == event_hook_header;
            continue;
        }
        if in_event_hook
            && trimmed.starts_with("command")
            && line.contains("internal hook codex")
            && line.contains("--source niuma-notifier")
        {
            return true;
        }
    }
    false
}

fn remove_niuma_hooks_from_config(config: &mut String) {
    let mut kept_blocks = Vec::new();
    for block in split_toml_blocks(config) {
        if toml_block_is_niuma_hook(&block) {
            continue;
        }
        kept_blocks.push(block);
    }
    *config = kept_blocks.concat();
}

fn split_toml_blocks(config: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    for line in config.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("[[") && trimmed.ends_with("]]") && !current.is_empty() {
            blocks.push(current);
            current = String::new();
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

fn toml_block_is_niuma_hook(block: &str) -> bool {
    let Some(header) = block.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    header.trim().starts_with("[[hooks.")
        && header.trim().ends_with(".hooks]]")
        && block.lines().any(|line| {
            line.trim_start().starts_with("command")
                && line.contains("internal hook codex")
                && line.contains("--source niuma-notifier")
        })
}

fn toml_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}
