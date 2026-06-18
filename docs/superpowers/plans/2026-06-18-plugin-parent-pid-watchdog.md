# Plugin Parent PID Watchdog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make plugin processes exit on their own when the main NiumaNotifier app process disappears unexpectedly.

**Architecture:** The main app injects `NIUMA_PARENT_PID` when spawning plugin processes. The built-in Codex plugin reads that environment variable and starts a small watchdog thread that exits the plugin when the parent PID no longer exists. External plugins receive the same environment contract through the plugin development document.

**Tech Stack:** Rust 2021, Tauri, `std::process::Command`, Unix `kill(pid, 0)` through FFI, existing Rust unit tests.

---

## File Structure

- Modify `src-tauri/src/tools/plugin_runtime.rs`: add the `NIUMA_PARENT_PID` environment variable during plugin command construction and test the command environment without spawning a process.
- Modify `builtin-plugins/codex-runtime/src/lib.rs`: add parent process watchdog helpers and start them from `run_from_env`.
- Modify `docs/integration/plugin-development_zh.md`: document `NIUMA_PARENT_PID` for external plugin authors.

## Task 1: Inject Parent PID Into Plugin Processes

**Files:**
- Modify: `src-tauri/src/tools/plugin_runtime.rs`
- Test: `src-tauri/src/tools/plugin_runtime.rs`

- [ ] **Step 1: Write the failing test**

Add this test to the existing `#[cfg(test)] mod tests` in `src-tauri/src/tools/plugin_runtime.rs`:

```rust
    #[test]
    fn build_plugin_command_injects_parent_pid() {
        let manifest = PluginManifest {
            id: "demo".to_string(),
            tool_id: ToolKind::Custom("demo".to_string()),
            display_name: "Demo".to_string(),
            version: "0.1.0".to_string(),
            command: Some("definitely-missing-niuma-command".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::EventWatcher],
            icon_url: None,
            source: PluginSource::External,
            base_dir: Some(PathBuf::from("/tmp/plugin-demo")),
        };

        let command = build_plugin_command(&manifest).unwrap();

        assert_eq!(
            command_env_value(&command, "NIUMA_PARENT_PID"),
            Some(std::process::id().to_string())
        );
    }

    fn command_env_value(command: &Command, key: &str) -> Option<String> {
        command.get_envs().find_map(|(name, value)| {
            if name == std::ffi::OsStr::new(key) {
                value.map(|value| value.to_string_lossy().to_string())
            } else {
                None
            }
        })
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test -p niuma-desktop build_plugin_command_injects_parent_pid
```

Expected: FAIL because `build_plugin_command` does not exist yet.

- [ ] **Step 3: Write the minimal implementation**

In `src-tauri/src/tools/plugin_runtime.rs`, add a constant near `FALLBACK_RECONCILE_INTERVAL`:

```rust
const PARENT_PID_ENV: &str = "NIUMA_PARENT_PID";
```

Replace `spawn_plugin_process` with a wrapper around a new command builder:

```rust
fn spawn_plugin_process(manifest: &PluginManifest) -> Result<Child, String> {
    build_plugin_command(manifest)?
        .spawn()
        .map_err(|error| format!("启动插件进程失败：{error}"))
}

fn build_plugin_command(manifest: &PluginManifest) -> Result<Command, String> {
    let command = manifest
        .command
        .as_ref()
        .ok_or_else(|| "外部插件缺少 command".to_string())?;
    let command_path = resolve_command_path(manifest, command);
    let mut process = Command::new(command_path);
    process
        .args(&manifest.args)
        .env(
            "NIUMA_LOCAL_API_URL",
            format!("http://{}", niuma_api::local_api_addr()),
        )
        .env("NIUMA_PLUGIN_ID", &manifest.id)
        .env("NIUMA_TOOL_ID", manifest.tool_id.as_str())
        .env(PARENT_PID_ENV, std::process::id().to_string())
        .env(
            "NIUMA_STATE_PATH",
            SqliteStateStore::default_path()
                .to_string_lossy()
                .to_string(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in &manifest.env {
        process.env(key, value);
    }
    if let Some(base_dir) = &manifest.base_dir {
        process.current_dir(base_dir);
    }
    Ok(process)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run:

```bash
cargo test -p niuma-desktop build_plugin_command_injects_parent_pid
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/tools/plugin_runtime.rs
git commit -m "feat: 注入插件父进程 PID" -m "修改内容：启动插件进程时注入 NIUMA_PARENT_PID，并为命令构建逻辑增加单元测试。" -m "修改原因：让插件能够检测主 App 是否已退出。"
```

## Task 2: Add Watchdog To Built-In Codex Plugin

**Files:**
- Modify: `builtin-plugins/codex-runtime/src/lib.rs`
- Modify: `builtin-plugins/codex-runtime/Cargo.toml`
- Test: `builtin-plugins/codex-runtime/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests to `#[cfg(test)]` coverage in `builtin-plugins/codex-runtime/src/lib.rs`:

```rust
#[cfg(test)]
mod parent_watchdog_tests {
    use super::*;

    #[test]
    fn parent_pid_from_env_ignores_missing_or_invalid_values() {
        assert_eq!(parse_parent_pid(None), None);
        assert_eq!(parse_parent_pid(Some("not-a-pid")), None);
        assert_eq!(parse_parent_pid(Some("")), None);
    }

    #[test]
    fn parent_pid_from_env_accepts_positive_pid() {
        assert_eq!(parse_parent_pid(Some("123")), Some(123));
    }

    #[cfg(unix)]
    #[test]
    fn unix_parent_process_probe_reports_missing_pid() {
        assert!(!parent_process_exists(999_999_999));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p niuma-codex-plugin-runtime parent_watchdog_tests
```

Expected: FAIL because `parse_parent_pid` and `parent_process_exists` do not exist yet.

- [ ] **Step 3: Add the Unix watchdog implementation**

Add `libc` to `builtin-plugins/codex-runtime/Cargo.toml`:

```toml
libc = "0.2"
```

Add these helpers near `run_from_env` in `builtin-plugins/codex-runtime/src/lib.rs`:

```rust
const PARENT_PID_ENV: &str = "NIUMA_PARENT_PID";
const PARENT_WATCHDOG_INTERVAL: Duration = Duration::from_secs(2);

pub fn run_from_env() {
    start_parent_watchdog_from_env();
    match LocalApiCodexEventSink::from_env() {
        Ok(event_sink) => run_runtime(Box::new(event_sink), None),
        Err(error) => {
            eprintln!("NiumaNotifier Codex plugin process not started: {error}");
            std::process::exit(1);
        }
    }
}

fn start_parent_watchdog_from_env() {
    let Some(parent_pid) = parse_parent_pid(std::env::var(PARENT_PID_ENV).ok().as_deref()) else {
        return;
    };
    if let Err(error) = thread::Builder::new()
        .name("niuma-parent-watchdog".to_string())
        .spawn(move || run_parent_watchdog(parent_pid))
    {
        eprintln!("NiumaNotifier parent watchdog not started: {error}");
    }
}

fn run_parent_watchdog(parent_pid: u32) {
    loop {
        thread::sleep(PARENT_WATCHDOG_INTERVAL);
        if !parent_process_exists(parent_pid) {
            eprintln!("NiumaNotifier parent process {parent_pid} is gone; plugin exiting");
            std::process::exit(0);
        }
    }
}

fn parse_parent_pid(value: Option<&str>) -> Option<u32> {
    value.and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0)
}

#[cfg(unix)]
fn parent_process_exists(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error()
        .raw_os_error()
        .is_some_and(|code| code != libc::ESRCH)
}

#[cfg(not(unix))]
fn parent_process_exists(_pid: u32) -> bool {
    true
}
```

Keep all existing comments in the file. If adding new comments, use concise Chinese comments only where the code is not self-explanatory.

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
cargo test -p niuma-codex-plugin-runtime parent_watchdog_tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add builtin-plugins/codex-runtime/Cargo.toml builtin-plugins/codex-runtime/src/lib.rs Cargo.lock
git commit -m "feat: 内置 Codex 插件检测父进程退出" -m "修改内容：内置 Codex 插件读取 NIUMA_PARENT_PID 并启动 watchdog，在父进程不存在时主动退出。" -m "修改原因：避免主 App 闪退后内置插件进程长期遗留。"
```

## Task 3: Document External Plugin Contract

**Files:**
- Modify: `docs/integration/plugin-development_zh.md`

- [ ] **Step 1: Write the documentation change**

In the “启动环境变量” table in `docs/integration/plugin-development_zh.md`, add this row after `NIUMA_TOOL_ID`:

```markdown
| `NIUMA_PARENT_PID` | 主 App 进程 PID。插件可定时检测该进程是否仍存在；如果不存在，应主动退出，避免主 App 闪退后遗留插件进程。 |
```

After the table, add this paragraph:

```markdown
建议外部插件把 `NIUMA_PARENT_PID` 作为自清理信号使用。该变量缺失或格式错误时，插件应保持兼容并继续运行；只有确认父进程不存在时才主动退出。
```

- [ ] **Step 2: Review the documentation diff**

Run:

```bash
git diff -- docs/integration/plugin-development_zh.md
```

Expected: diff only adds the new environment variable and compatibility note.

- [ ] **Step 3: Commit**

```bash
git add docs/integration/plugin-development_zh.md
git commit -m "docs: 说明插件父进程 PID 环境变量" -m "修改内容：在插件开发文档中补充 NIUMA_PARENT_PID 的含义和外部插件兼容建议。" -m "修改原因：让外部插件作者按同一协议实现主 App 退出后的自清理。"
```

## Task 4: Run Focused Verification

**Files:**
- Verify: `src-tauri/src/tools/plugin_runtime.rs`
- Verify: `builtin-plugins/codex-runtime/src/lib.rs`
- Verify: `docs/integration/plugin-development_zh.md`

- [ ] **Step 1: Run desktop plugin runtime tests**

Run:

```bash
cargo test -p niuma-desktop plugin_runtime
```

Expected: PASS.

- [ ] **Step 2: Run Codex plugin runtime tests**

Run:

```bash
cargo test -p niuma-codex-plugin-runtime parent_watchdog_tests
```

Expected: PASS.

- [ ] **Step 3: Run package-level Codex plugin tests**

Run:

```bash
cargo test -p niuma-codex-plugin-runtime
```

Expected: PASS.

- [ ] **Step 4: Check staged and unstaged changes**

Run:

```bash
git status --short
```

Expected: only pre-existing unrelated workspace changes remain, or no changes from this implementation.
