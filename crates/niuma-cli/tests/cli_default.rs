use std::process::Command;

#[test]
fn bare_niuma_prints_help_instead_of_runtime_state_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_niuma"))
        .output()
        .expect("niuma 二进制必须可启动");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage: niuma"),
        "裸 niuma 应显示帮助入口，而不是读取当前运行状态；实际输出：{stdout}"
    );
    assert!(
        !stdout.contains("\"data\""),
        "裸 niuma 不应返回 Local API 状态 JSON；实际输出：{stdout}"
    );
}
