use tempfile::tempdir;

use crate::tools::codex::hook::{
    codex_hooks_file, install_codex_hook, uninstall_codex_hook, CodexHookCommand,
};

#[test]
fn install_creates_user_hooks_file_with_three_events() {
    let dir = tempdir().unwrap();
    let hooks_file = codex_hooks_file(dir.path());

    let result = install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();

    assert!(hooks_file.exists());
    assert!(result.installed);
    assert_eq!(result.command_mode, "installed");
    assert_eq!(result.events.get("SessionStart").unwrap(), "installed");
    assert_eq!(result.events.get("PermissionRequest").unwrap(), "installed");
    assert_eq!(result.events.get("Stop").unwrap(), "installed");

    let body = std::fs::read_to_string(hooks_file).unwrap();
    assert!(body.contains("niuma internal hook codex --source niuma-notifier"));
}

#[test]
fn uninstall_removes_only_niuma_hooks() {
    let dir = tempdir().unwrap();
    install_codex_hook(dir.path(), CodexHookCommand::Installed).unwrap();

    let result = uninstall_codex_hook(dir.path()).unwrap();

    assert!(!result.installed);
    let body = std::fs::read_to_string(codex_hooks_file(dir.path())).unwrap();
    assert!(!body.contains("--source niuma-notifier"));
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
