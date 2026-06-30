use std::process::Command;

#[test]
fn claude_hook_install_writes_permission_request_hook() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_niuma"))
        .args(["hook", "claude-code", "--install"])
        .env("CLAUDE_CONFIG_DIR", temp.path())
        .output()
        .expect("niuma 二进制必须可启动");

    assert!(
        output.status.success(),
        "hook install 应成功：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let settings = std::fs::read_to_string(temp.path().join("settings.json")).unwrap();
    assert!(settings.contains("\"PermissionRequest\""));
    assert!(settings.contains("internal hook claude-code --source niuma-notifier"));
}
