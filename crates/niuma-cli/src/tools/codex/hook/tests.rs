use tempfile::tempdir;

use crate::tools::codex::hook::{
    codex_hooks_file, install_codex_hook, uninstall_codex_hook, CodexHookCommand,
};

#[test]
fn install_creates_user_hooks_file_with_permission_request_only() {
    let dir = tempdir().unwrap();
    let hooks_file = codex_hooks_file(dir.path());

    let result = install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();

    assert!(hooks_file.exists());
    assert!(result.installed);
    assert_eq!(result.command_mode, "installed");
    assert_eq!(result.events.get("PermissionRequest").unwrap(), "installed");
    assert!(!result.events.contains_key("SessionStart"));
    assert!(!result.events.contains_key("Stop"));

    let body = std::fs::read_to_string(hooks_file).unwrap();
    assert!(body.contains("niuma internal hook codex --source niuma-notifier"));
    assert!(body.contains("\"PermissionRequest\""));
    assert!(body.contains("\"timeout\": 660"));
    assert!(!body.contains("\"SessionStart\""));
    assert!(!body.contains("\"Stop\""));
}

#[test]
fn uninstall_removes_only_niuma_hooks() {
    let dir = tempdir().unwrap();
    install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();

    let result = uninstall_codex_hook(dir.path()).unwrap();

    assert!(!result.installed);
    assert!(!codex_hooks_file(dir.path()).exists());
}

#[test]
fn install_and_uninstall_preserve_user_defined_hooks_and_unknown_fields() {
    let dir = tempdir().unwrap();
    let hooks_file = codex_hooks_file(dir.path());
    std::fs::write(
        &hooks_file,
        r#"{
  "customRoot": { "enabled": true },
  "hooks": {
"SessionStart": [
  {
    "matcher": "startup",
    "hooks": [
      {
        "type": "command",
        "command": "echo user-session"
      }
    ]
  }
],
"PostToolUse": [
  {
    "matcher": "Write",
    "hooks": [
      {
        "type": "command",
        "command": "echo user-tool"
      }
    ]
  }
]
  }
}
"#,
    )
    .unwrap();

    install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();
    uninstall_codex_hook(dir.path()).unwrap();

    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(hooks_file).unwrap()).unwrap();
    assert_eq!(config["customRoot"]["enabled"], true);
    assert!(config.to_string().contains("echo user-session"));
    assert!(config.to_string().contains("echo user-tool"));
    assert!(!config.to_string().contains("--source niuma-notifier"));
}

#[test]
fn install_removes_legacy_niuma_session_hooks() {
    let dir = tempdir().unwrap();
    let hooks_file = codex_hooks_file(dir.path());
    std::fs::write(
        &hooks_file,
        r#"{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "niuma internal hook codex --source niuma-notifier"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "niuma internal hook codex --source niuma-notifier"
          }
        ]
      }
    ]
  }
}
"#,
    )
    .unwrap();

    install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();

    let body = std::fs::read_to_string(hooks_file).unwrap();
    assert!(!body.contains("\"SessionStart\""));
    assert!(!body.contains("\"Stop\""));
    assert!(body.contains("\"PermissionRequest\""));
}

#[test]
fn install_uses_config_toml_when_config_already_has_hooks() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("config.toml");
    std::fs::write(
        &config_file,
        r#"model = "gpt-5.5"

[[hooks.SessionStart]]

[[hooks.SessionStart.hooks]]
type = "command"
command = "echo user-session"
timeout = 10
"#,
    )
    .unwrap();

    install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();

    let config_body = std::fs::read_to_string(config_file).unwrap();
    assert!(config_body.contains("echo user-session"));
    assert!(config_body.contains("[[hooks.PermissionRequest]]"));
    assert!(config_body.contains("niuma internal hook codex --source niuma-notifier"));
    assert!(!codex_hooks_file(dir.path()).exists());
}

#[test]
fn install_ignores_config_hook_state_when_selecting_target() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("config.toml");
    let hooks_file = codex_hooks_file(dir.path());
    std::fs::write(
        &config_file,
        r#"model = "gpt-5.5"

[hooks.state]

[hooks.state."/Users/demo/.codex/hooks.json:permission_request:0:0"]
trusted_hash = "sha256:demo"
enabled = true
"#,
    )
    .unwrap();

    install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();

    let config_body = std::fs::read_to_string(config_file).unwrap();
    let hooks_body = std::fs::read_to_string(hooks_file).unwrap();
    assert!(!config_body.contains("--source niuma-notifier"));
    assert!(hooks_body.contains("niuma internal hook codex --source niuma-notifier"));
}

#[test]
fn install_migrates_niuma_hook_to_config_when_both_files_have_hooks() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("config.toml");
    let hooks_file = codex_hooks_file(dir.path());
    std::fs::write(
        &config_file,
        r#"model = "gpt-5.5"

[[hooks.PermissionRequest]]
matcher = "*"

[[hooks.PermissionRequest.hooks]]
type = "command"
command = "echo osxpush"
timeout = 660
"#,
    )
    .unwrap();
    std::fs::write(
        &hooks_file,
        r#"{
  "hooks": {
    "PermissionRequest": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "niuma internal hook codex --source niuma-notifier"
          }
        ]
      }
    ]
  }
}
"#,
    )
    .unwrap();

    install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();

    let config_body = std::fs::read_to_string(config_file).unwrap();
    assert!(config_body.contains("echo osxpush"));
    assert!(config_body.contains("niuma internal hook codex --source niuma-notifier"));
    assert!(!hooks_file.exists());
}

#[test]
fn uninstall_removes_niuma_from_config_toml_and_preserves_user_hooks() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("config.toml");
    std::fs::write(
        &config_file,
        r#"model = "gpt-5.5"

[[hooks.SessionStart]]

[[hooks.SessionStart.hooks]]
type = "command"
command = "echo user-session"
timeout = 10

[[hooks.PermissionRequest]]

[[hooks.PermissionRequest.hooks]]
type = "command"
command = "niuma internal hook codex --source niuma-notifier"
timeout = 660
"#,
    )
    .unwrap();

    let result = uninstall_codex_hook(dir.path()).unwrap();

    assert!(!result.installed);
    let config_body = std::fs::read_to_string(config_file).unwrap();
    assert!(config_body.contains("echo user-session"));
    assert!(!config_body.contains("--source niuma-notifier"));
}
