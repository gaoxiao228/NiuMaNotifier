use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn internal_codex_hook_keeps_stdout_empty_for_codex_protocol() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_niuma"))
        .args(["internal", "hook", "codex", "--source", "niuma-notifier"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("niuma 二进制必须可启动");

    child
        .stdin
        .as_mut()
        .expect("stdin 必须可写")
        .write_all(br#"{"hook_event_name":"SessionStart","session_id":"s1","cwd":"/tmp/demo"}"#)
        .expect("测试 payload 必须可写入");

    let output = child.wait_with_output().expect("niuma 进程必须退出");

    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "internal hook stdout 会被 Codex 当成 hook 协议解析，必须保持为空；实际输出：{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
