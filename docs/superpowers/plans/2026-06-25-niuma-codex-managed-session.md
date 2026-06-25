# niuma-codex Managed Session Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 Rust 版 `niuma codex` 受管新会话，使 NiuMaNotifier 能识别由 niuma-codex 启动的 Codex session，并通过现有 approval API 与新增 control API 支持授权、等待输入、追加指令和中断。

**Architecture:** Codex session 文件仍由 watcher/provider 负责解析和展示；`niuma codex` 只负责 app-server relay、control socket、managed JSON registry 与控制能力。Approval 通过 `channel` 复用现有 approval store；Input 只由 watcher 生成 `InputRequested` 事件，relay 提供 pending request overlay。

**Tech Stack:** Rust workspace (`niuma-core`、`niuma-cli`、`niuma-api`、`builtin-plugins/codex-runtime`)、Axum Local API、Tauri/Vite TypeScript UI、JSON Lines 本地 socket、Codex app-server WebSocket JSON-RPC。

---

## File Structure

- Create `crates/niuma-core/src/codex_managed_session.rs`
  - Managed registry 数据结构、JSON 读写、消息 hash、绑定匹配。
- Modify `crates/niuma-core/src/lib.rs`
  - 暴露 `codex_managed_session` 模块。
- Modify `crates/niuma-core/src/platform/paths.rs`
  - 增加 managed session registry 路径函数。
- Modify `crates/niuma-core/src/models.rs`
  - 增加 approval channel/control ref，增加 tool session control 信息类型。
- Modify `crates/niuma-core/src/tool_session.rs`
  - `ToolSessionListItem`、`ToolSessionDetail` 增加 `control` 字段。
- Modify `crates/niuma-core/src/store.rs` and `crates/niuma-core/src/store/schema.rs`
  - approval request 持久化 channel/control_ref。
- Create `crates/niuma-cli/src/tools/codex/managed.rs`
  - `niuma codex ...` 入口、真实 Codex 查找、参数分类。
- Create `crates/niuma-cli/src/tools/codex/app_control.rs`
  - app-server 启动、relay、control socket、pending request 状态。
- Modify `crates/niuma-cli/src/cli.rs` and `crates/niuma-cli/src/main.rs`
  - 增加 `Codex` 子命令，允许 trailing args。
- Modify `builtin-plugins/codex-runtime/src/codex/session_repository.rs`
  - 计算 first user message hash，并把 bound control 信息 overlay 到 session list/detail。
- Modify `builtin-plugins/codex-runtime/src/session_provider.rs`
  - provider 返回 `control` 字段。
- Modify `crates/niuma-api/src/handlers/approval.rs`
  - `POST /approval-requests` 支持 channel/control_ref，`POST /approval-decisions` 按 channel 分发。
- Create `crates/niuma-api/src/handlers/tool_session_control.rs`
  - send、interrupt、answer-input API。
- Modify `crates/niuma-api/src/handlers.rs` and `crates/niuma-api/src/routes.rs`
  - 注册 control API。
- Modify `src/api.ts`
  - 增加 control 类型与 API 调用函数。
- Modify event/session UI files after locating current session detail/event center rendering:
  - `src/eventCenterView.ts`
  - `src/eventCenterRuntime.ts`
  - `src/statusView.ts` only for data pass-through, not status bar actions.
  - Session detail file should be located with `rg "session_detail|sessionDetail|session list" src`.
- Modify `src/i18n.ts`
  - 补齐六种语言文案。

---

### Task 1: Managed Registry Core

**Files:**
- Create: `crates/niuma-core/src/codex_managed_session.rs`
- Modify: `crates/niuma-core/src/lib.rs`
- Modify: `crates/niuma-core/src/platform/paths.rs`
- Test: inline unit tests in `codex_managed_session.rs`

- [ ] **Step 1: Write failing tests for normalization, hash, registry update, and binding**

Add tests to `crates/niuma-core/src/codex_managed_session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    #[test]
    fn message_hash_normalizes_whitespace_and_line_endings() {
        let left = first_user_message_hash("  hello\r\n  world  ");
        let right = first_user_message_hash("hello world");
        assert_eq!(left, right);
    }

    #[test]
    fn registry_round_trips_session_and_updates_atomically() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("codex.json");
        let mut registry = ManagedCodexRegistry::default();
        registry.upsert(ManagedCodexSession {
            wrapper_session_id: "niuma_codex_1".to_string(),
            state: ManagedCodexSessionState::BindingPending,
            cwd: "/repo".into(),
            pid: Some(42),
            real_socket: "/tmp/real.sock".into(),
            relay_socket: "/tmp/relay.sock".into(),
            control_socket: "/tmp/control.sock".into(),
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            first_user_message_hash: Some(first_user_message_hash("hello")),
            first_user_message_preview: Some("hello".into()),
            first_user_message_submitted_at: Some(Utc.timestamp_opt(1_005, 0).unwrap()),
            codex_session_id: None,
            codex_session_file_path: None,
            bound_at: None,
            binding_failure_reason: None,
        });
        write_registry_atomic(&path, &registry).unwrap();

        let loaded = read_registry(&path).unwrap();
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].wrapper_session_id, "niuma_codex_1");
    }

    #[test]
    fn binding_matches_only_unique_candidate_inside_ten_seconds() {
        let managed = ManagedCodexSession {
            wrapper_session_id: "niuma_codex_1".to_string(),
            state: ManagedCodexSessionState::BindingPending,
            cwd: "/repo".into(),
            pid: Some(42),
            real_socket: "/tmp/real.sock".into(),
            relay_socket: "/tmp/relay.sock".into(),
            control_socket: "/tmp/control.sock".into(),
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            first_user_message_hash: Some(first_user_message_hash("继续")),
            first_user_message_preview: Some("继续".into()),
            first_user_message_submitted_at: Some(Utc.timestamp_opt(1_010, 0).unwrap()),
            codex_session_id: None,
            codex_session_file_path: None,
            bound_at: None,
            binding_failure_reason: None,
        };
        let candidate = CodexSessionBindingCandidate {
            session_id: "codex-session-1".into(),
            session_file_path: "/codex/session.jsonl".into(),
            project_path: "/repo".into(),
            first_user_message_hash: Some(first_user_message_hash("继续")),
            first_user_message_at: Some(Utc.timestamp_opt(1_012, 0).unwrap()),
        };

        let result = match_managed_session(&managed, &[candidate], chrono::Duration::seconds(10));
        assert_eq!(result, BindingMatch::Unique {
            session_id: "codex-session-1".into(),
            session_file_path: "/codex/session.jsonl".into(),
        });
    }
}
```

- [ ] **Step 2: Run tests and confirm they fail**

Run:

```bash
cargo test -p niuma-core codex_managed_session -- --nocapture
```

Expected: fail because `codex_managed_session` does not exist.

- [ ] **Step 3: Implement registry types and helpers**

Create `crates/niuma-core/src/codex_managed_session.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedCodexRegistry {
    #[serde(default = "registry_version")]
    pub version: u32,
    #[serde(default)]
    pub sessions: Vec<ManagedCodexSession>,
}

fn registry_version() -> u32 {
    1
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedCodexSessionState {
    Created,
    WaitingFirstUserMessage,
    BindingPending,
    Bound,
    Ambiguous,
    Exited,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedCodexSession {
    pub wrapper_session_id: String,
    pub state: ManagedCodexSessionState,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub real_socket: String,
    pub relay_socket: String,
    pub control_socket: String,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message_submitted_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_failure_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexSessionBindingCandidate {
    pub session_id: String,
    pub session_file_path: String,
    pub project_path: String,
    pub first_user_message_hash: Option<String>,
    pub first_user_message_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindingMatch {
    None,
    Unique {
        session_id: String,
        session_file_path: String,
    },
    Ambiguous,
}

impl ManagedCodexRegistry {
    pub fn upsert(&mut self, session: ManagedCodexSession) {
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|item| item.wrapper_session_id == session.wrapper_session_id)
        {
            *existing = session;
        } else {
            self.sessions.push(session);
        }
    }
}

pub fn first_user_message_hash(value: &str) -> String {
    let normalized = normalize_first_user_message(value);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn normalize_first_user_message(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn read_registry(path: &Path) -> Result<ManagedCodexRegistry, String> {
    if !path.exists() {
        return Ok(ManagedCodexRegistry {
            version: registry_version(),
            sessions: Vec::new(),
        });
    }
    let body = fs::read_to_string(path).map_err(|error| format!("读取 Codex managed registry 失败：{error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("解析 Codex managed registry 失败：{error}"))
}

pub fn write_registry_atomic(path: &Path, registry: &ManagedCodexRegistry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建 Codex managed registry 目录失败：{error}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(registry)
        .map_err(|error| format!("序列化 Codex managed registry 失败：{error}"))?;
    fs::write(&tmp, format!("{body}\n")).map_err(|error| format!("写入 Codex managed registry 临时文件失败：{error}"))?;
    fs::rename(&tmp, path).map_err(|error| format!("替换 Codex managed registry 失败：{error}"))
}

pub fn match_managed_session(
    managed: &ManagedCodexSession,
    candidates: &[CodexSessionBindingCandidate],
    window: chrono::Duration,
) -> BindingMatch {
    let Some(hash) = managed.first_user_message_hash.as_deref() else {
        return BindingMatch::None;
    };
    let Some(submitted_at) = managed.first_user_message_submitted_at else {
        return BindingMatch::None;
    };
    let normalized_cwd = normalize_path_for_match(&managed.cwd);
    let matches = candidates
        .iter()
        .filter(|candidate| normalize_path_for_match(&candidate.project_path) == normalized_cwd)
        .filter(|candidate| candidate.first_user_message_hash.as_deref() == Some(hash))
        .filter(|candidate| {
            candidate
                .first_user_message_at
                .map(|created_at| (created_at - submitted_at).num_seconds().abs() <= window.num_seconds())
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => BindingMatch::None,
        [candidate] => BindingMatch::Unique {
            session_id: candidate.session_id.clone(),
            session_file_path: candidate.session_file_path.clone(),
        },
        _ => BindingMatch::Ambiguous,
    }
}

fn normalize_path_for_match(path: &str) -> String {
    PathBuf::from(path)
        .components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .trim_end_matches(std::path::MAIN_SEPARATOR)
        .to_string()
}
```

Add dependency if missing in `crates/niuma-core/Cargo.toml`:

```toml
sha2 = "0.10"
```

Modify `crates/niuma-core/src/lib.rs`:

```rust
pub mod codex_managed_session;
```

- [ ] **Step 4: Add registry path helper**

Modify `crates/niuma-core/src/platform/paths.rs` with a function matching existing path style:

```rust
pub fn codex_managed_registry_path() -> std::path::PathBuf {
    app_data_dir()
        .join("managed-sessions")
        .join("codex.json")
}
```

If `app_data_dir()` has a different existing name, use the existing app data function and keep this wrapper name.

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p niuma-core codex_managed_session -- --nocapture
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-core/src/codex_managed_session.rs crates/niuma-core/src/lib.rs crates/niuma-core/src/platform/paths.rs crates/niuma-core/Cargo.toml Cargo.lock
git commit -m "feat: 新增 Codex 受管会话注册表" -m "修改内容：新增 Codex managed session registry、第一条用户消息 hash、绑定匹配和 registry 路径。" -m "修改原因：为 niuma-codex 受管新会话识别与控制能力提供稳定运行时索引。"
```

### Task 2: CLI Entry, Codex Resolver, and Mode Classification

**Files:**
- Create: `crates/niuma-cli/src/tools/codex/managed.rs`
- Modify: `crates/niuma-cli/src/tools/codex/mod.rs`
- Modify: `crates/niuma-cli/src/cli.rs`
- Modify: `crates/niuma-cli/src/main.rs`
- Test: inline tests in `managed.rs`

- [ ] **Step 1: Write failing CLI classification tests**

Create `crates/niuma-cli/src/tools/codex/managed.rs` with tests first:

```rust
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
    }

    #[test]
    fn classifies_resume_exec_app_server_and_help_as_passthrough() {
        for args in [
            vec!["resume".to_string()],
            vec!["exec".to_string(), "echo hi".to_string()],
            vec!["app-server".to_string()],
            vec!["--help".to_string()],
            vec!["--version".to_string()],
            vec!["unknown-subcommand".to_string()],
        ] {
            assert_eq!(classify_codex_args(&args), CodexLaunchMode::Passthrough);
        }
    }
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test -p niuma-cli managed::tests::classifies -- --nocapture
```

Expected: fail because module/types are incomplete.

- [ ] **Step 3: Implement mode classification and resolver skeleton**

Implement in `managed.rs`:

```rust
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexLaunchMode {
    Managed,
    Passthrough,
}

const PASSTHROUGH_SUBCOMMANDS: &[&str] = &["resume", "exec", "app-server", "login", "logout", "auth"];

pub fn classify_codex_args(args: &[String]) -> CodexLaunchMode {
    let Some(first) = args.first().map(String::as_str) else {
        return CodexLaunchMode::Managed;
    };
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

pub fn run_codex_command(args: Vec<String>) -> ApiResponse<serde_json::Value> {
    let real_codex = match resolve_real_codex() {
        Ok(path) => path,
        Err(error) => return ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
    };
    match classify_codex_args(&args) {
        CodexLaunchMode::Managed => run_managed_codex(real_codex, args),
        CodexLaunchMode::Passthrough => run_passthrough_codex(real_codex, args),
    }
}

fn resolve_real_codex() -> Result<PathBuf, String> {
    if let Ok(value) = std::env::var("NIUMA_REAL_CODEX") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }
        return Err("NIUMA_REAL_CODEX 指向的 codex 不存在".to_string());
    }
    which::which("codex").map_err(|_| "找不到真实 codex，请设置 NIUMA_REAL_CODEX=/absolute/path/to/codex".to_string())
}

fn run_passthrough_codex(real_codex: PathBuf, args: Vec<String>) -> ApiResponse<serde_json::Value> {
    let status = match Command::new(real_codex).args(args).status() {
        Ok(status) => status,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, format!("启动 codex 失败：{error}")),
    };
    let code = status.code().unwrap_or(1);
    ApiResponse::ok(json!({ "mode": "passthrough", "exit_code": code }))
}

fn run_managed_codex(real_codex: PathBuf, args: Vec<String>) -> ApiResponse<serde_json::Value> {
    crate::tools::codex::app_control::run_app_control(real_codex, args)
}
```

Add dependencies if absent in `crates/niuma-cli/Cargo.toml`:

```toml
which = "6"
```

- [ ] **Step 4: Wire CLI**

Modify `crates/niuma-cli/src/cli.rs`:

```rust
#[derive(Subcommand)]
pub(crate) enum Command {
    Doctor,
    Status { tool: Option<ToolArg> },
    Codex(CodexCommand),
    Hook(HookCommand),
    Internal(InternalRootCommand),
    SampleEvent,
    Reset,
    DismissBlocker,
    Serve,
}

#[derive(Args)]
pub(crate) struct CodexCommand {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) args: Vec<String>,
}
```

Modify `crates/niuma-cli/src/main.rs` match:

```rust
Command::Codex(command) => tools::codex::managed::run_codex_command(command.args),
```

Modify `crates/niuma-cli/src/tools/codex/mod.rs`:

```rust
pub mod app_control;
pub mod managed;
```

Create `crates/niuma-cli/src/tools/codex/app_control.rs` with a compile-safe error return that Task 4 replaces with the real app-server transport:

```rust
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::json;
use std::path::PathBuf;

pub fn run_app_control(_real_codex: PathBuf, _args: Vec<String>) -> ApiResponse<serde_json::Value> {
    ApiResponse::fail(
        ApiErrorCode::BusinessValidation,
        "niuma codex managed mode transport is not wired before Task 4",
    )
}
```

- [ ] **Step 5: Run CLI tests**

```bash
cargo test -p niuma-cli managed::tests::classifies -- --nocapture
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-cli/src/cli.rs crates/niuma-cli/src/main.rs crates/niuma-cli/src/tools/codex/mod.rs crates/niuma-cli/src/tools/codex/managed.rs crates/niuma-cli/src/tools/codex/app_control.rs crates/niuma-cli/Cargo.toml Cargo.lock
git commit -m "feat: 新增 niuma codex 命令入口" -m "修改内容：新增 niuma codex 参数透传入口、受管/直通模式分类和真实 Codex 查找骨架。" -m "修改原因：为 Rust 版 niuma-codex 受管启动提供 CLI 入口。"
```

### Task 3: App Control Relay and Control Socket

**Files:**
- Modify: `crates/niuma-cli/src/tools/codex/app_control.rs`
- Test: inline tests in `app_control.rs`

- [ ] **Step 1: Write tests for control messages and pending input/approval state**

Append tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_control_state_tracks_approval_request() {
        let mut state = AppControlState::default();
        state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!(7),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "item-1",
                "command": "cargo test"
            }),
        });
        assert_eq!(state.pending_approvals.len(), 1);
        assert_eq!(state.pending_approvals[0].item_id.as_deref(), Some("item-1"));
    }

    #[test]
    fn app_control_state_tracks_input_request() {
        let mut state = AppControlState::default();
        state.observe_server_request(AppServerRequest {
            jsonrpc_id: json!(9),
            method: "item/tool/requestUserInput".into(),
            params: json!({
                "questions": [{"id": "app_type", "options": [{"label": "CLI"}]}]
            }),
        });
        assert_eq!(state.pending_inputs.len(), 1);
        assert_eq!(state.pending_inputs[0].request_id, "codex-input:wrapper-test:9");
    }
}
```

- [ ] **Step 2: Run failing tests**

```bash
cargo test -p niuma-cli app_control::tests::app_control_state -- --nocapture
```

Expected: fail because state types do not exist.

- [ ] **Step 3: Implement focused relay state and control command types**

Add to `app_control.rs`:

```rust
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_managed_session::{
    first_user_message_hash, read_registry, write_registry_atomic, ManagedCodexRegistry,
    ManagedCodexSession, ManagedCodexSessionState,
};
use niuma_core::platform::paths::codex_managed_registry_path;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppServerRequest {
    pub jsonrpc_id: Value,
    pub method: String,
    pub params: Value,
}

#[derive(Clone, Debug, Default)]
pub struct AppControlState {
    pub wrapper_session_id: String,
    pub pending_approvals: Vec<PendingApproval>,
    pub pending_inputs: Vec<PendingInput>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub request_id: String,
    pub relay_request_id: String,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub command: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingInput {
    pub request_id: String,
    pub relay_request_id: String,
    pub questions: Value,
}

impl AppControlState {
    pub fn observe_server_request(&mut self, request: AppServerRequest) {
        if request.method == "item/commandExecution/requestApproval" {
            let relay_request_id = request.jsonrpc_id.to_string();
            let turn_id = request.params.get("turnId").and_then(Value::as_str).map(ToString::to_string);
            let item_id = request.params.get("itemId").and_then(Value::as_str).map(ToString::to_string);
            let stable_item = item_id.clone().unwrap_or_else(|| relay_request_id.clone());
            self.pending_approvals.push(PendingApproval {
                request_id: format!("codex-relay:{}:{}:{}", self.wrapper_session_id, turn_id.clone().unwrap_or_else(|| "unknown-turn".into()), stable_item),
                relay_request_id,
                turn_id,
                item_id,
                command: request.params.get("command").and_then(Value::as_str).map(ToString::to_string),
            });
        }
        if request.method == "item/tool/requestUserInput" {
            let relay_request_id = request.jsonrpc_id.to_string();
            self.pending_inputs.push(PendingInput {
                request_id: format!("codex-input:{}:{}", self.wrapper_session_id, relay_request_id.trim_matches('"')),
                relay_request_id,
                questions: request.params.get("questions").cloned().unwrap_or(Value::Array(Vec::new())),
            });
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlCommand {
    Requests,
    ApprovalDecision { request_id: String, decision: String },
    AnswerInput { request_id: String, answers: Value },
    SendInstruction { content: String },
    Interrupt,
}
```

- [ ] **Step 4: Implement managed run skeleton**

Replace temporary `run_app_control` body with a real skeleton that starts registry and then delegates to transport helpers:

```rust
pub fn run_app_control(real_codex: PathBuf, args: Vec<String>) -> ApiResponse<serde_json::Value> {
    match run_app_control_inner(real_codex, args) {
        Ok(code) => ApiResponse::ok(json!({ "mode": "managed", "exit_code": code })),
        Err(error) => ApiResponse::fail(ApiErrorCode::System, error),
    }
}

fn run_app_control_inner(real_codex: PathBuf, args: Vec<String>) -> Result<i32, String> {
    let wrapper_session_id = format!("niuma_codex_{}", uuid::Uuid::new_v4().simple());
    let registry_path = codex_managed_registry_path();
    let mut registry = read_registry(&registry_path)?;
    let now = chrono::Utc::now();
    let base_dir = registry_path
        .parent()
        .ok_or_else(|| "Codex managed registry 路径缺少父目录".to_string())?
        .join("sockets")
        .join(&wrapper_session_id);
    std::fs::create_dir_all(&base_dir).map_err(|error| format!("创建 niuma-codex socket 目录失败：{error}"))?;
    let session = ManagedCodexSession {
        wrapper_session_id: wrapper_session_id.clone(),
        state: ManagedCodexSessionState::WaitingFirstUserMessage,
        cwd: std::env::current_dir().map_err(|error| format!("读取当前目录失败：{error}"))?.to_string_lossy().to_string(),
        pid: Some(std::process::id()),
        real_socket: base_dir.join("real.sock").to_string_lossy().to_string(),
        relay_socket: base_dir.join("relay.sock").to_string_lossy().to_string(),
        control_socket: base_dir.join("control.sock").to_string_lossy().to_string(),
        started_at: now,
        first_user_message_hash: None,
        first_user_message_preview: None,
        first_user_message_submitted_at: None,
        codex_session_id: None,
        codex_session_file_path: None,
        bound_at: None,
        binding_failure_reason: None,
    };
    registry.upsert(session);
    write_registry_atomic(&registry_path, &registry)?;

    run_app_server_remote_processes(real_codex, args, wrapper_session_id, base_dir)
}

fn run_app_server_remote_processes(
    _real_codex: PathBuf,
    _args: Vec<String>,
    _wrapper_session_id: String,
    _base_dir: PathBuf,
) -> Result<i32, String> {
    Err("app-server relay transport is wired in Task 4".to_string())
}
```

Add dependencies to `crates/niuma-cli/Cargo.toml`:

```toml
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 5: Run state tests**

```bash
cargo test -p niuma-cli app_control::tests::app_control_state -- --nocapture
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-cli/src/tools/codex/app_control.rs crates/niuma-cli/Cargo.toml Cargo.lock
git commit -m "feat: 新增 Codex app control 状态模型" -m "修改内容：新增 app-server request 捕获状态、pending approval/input 记录、control command 类型和 managed 启动 registry 骨架。" -m "修改原因：为 niuma-codex relay 与 Local API 控制通道建立可测试的状态核心。"
```

### Task 4: App Server Transport and Control Socket I/O

**Files:**
- Modify: `crates/niuma-cli/src/tools/codex/app_control.rs`
- Test: inline tests in `app_control.rs`

- [ ] **Step 1: Add failing tests for WebSocket frame parsing and JSON Lines control messages**

Add tests:

```rust
#[test]
fn websocket_text_frame_round_trips_json() {
    let payload = serde_json::json!({"id": 1, "method": "thread/read"});
    let frame = encode_websocket_text_frame(&payload.to_string(), false);
    let parsed = parse_websocket_text_frames(&frame).unwrap();
    assert_eq!(parsed.messages, vec![payload]);
    assert!(parsed.rest.is_empty());
}

#[test]
fn control_command_parses_json_line() {
    let command = parse_control_command_line(
        r#"{"type":"send_instruction","content":"继续"}"#,
    )
    .unwrap();
    assert!(matches!(command, ControlCommand::SendInstruction { content } if content == "继续"));
}
```

- [ ] **Step 2: Run failing transport tests**

```bash
cargo test -p niuma-cli app_control::tests::websocket_text_frame_round_trips_json app_control::tests::control_command_parses_json_line -- --nocapture
```

Expected: fail because parser helpers are not implemented.

- [ ] **Step 3: Implement WebSocket frame helpers**

Add to `app_control.rs`:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedWebSocketMessages {
    pub messages: Vec<serde_json::Value>,
    pub rest: Vec<u8>,
}

pub fn encode_websocket_text_frame(text: &str, mask: bool) -> Vec<u8> {
    let payload = text.as_bytes();
    let mut out = Vec::new();
    out.push(0x81);
    if payload.len() < 126 {
        out.push((if mask { 0x80 } else { 0 }) | payload.len() as u8);
    } else {
        out.push((if mask { 0x80 } else { 0 }) | 126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    if mask {
        let key = [0x12, 0x34, 0x56, 0x78];
        out.extend_from_slice(&key);
        for (index, byte) in payload.iter().enumerate() {
            out.push(byte ^ key[index % 4]);
        }
    } else {
        out.extend_from_slice(payload);
    }
    out
}

pub fn parse_websocket_text_frames(buffer: &[u8]) -> Result<ParsedWebSocketMessages, String> {
    let mut offset = 0usize;
    let mut messages = Vec::new();
    while buffer.len().saturating_sub(offset) >= 2 {
        let first = buffer[offset];
        let second = buffer[offset + 1];
        if first & 0x0f != 0x1 {
            return Err("只支持 WebSocket text frame".to_string());
        }
        let masked = second & 0x80 != 0;
        let mut len = (second & 0x7f) as usize;
        offset += 2;
        if len == 126 {
            if buffer.len().saturating_sub(offset) < 2 {
                return Ok(ParsedWebSocketMessages { messages, rest: buffer[offset - 2..].to_vec() });
            }
            len = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]) as usize;
            offset += 2;
        }
        let mask_key = if masked {
            if buffer.len().saturating_sub(offset) < 4 {
                return Ok(ParsedWebSocketMessages { messages, rest: buffer[offset - 2..].to_vec() });
            }
            let key = [buffer[offset], buffer[offset + 1], buffer[offset + 2], buffer[offset + 3]];
            offset += 4;
            Some(key)
        } else {
            None
        };
        if buffer.len().saturating_sub(offset) < len {
            return Ok(ParsedWebSocketMessages { messages, rest: buffer[offset - 2..].to_vec() });
        }
        let mut payload = buffer[offset..offset + len].to_vec();
        if let Some(key) = mask_key {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= key[index % 4];
            }
        }
        let value = serde_json::from_slice(&payload)
            .map_err(|error| format!("解析 app-server JSON-RPC frame 失败：{error}"))?;
        messages.push(value);
        offset += len;
    }
    Ok(ParsedWebSocketMessages { messages, rest: buffer[offset..].to_vec() })
}
```

- [ ] **Step 4: Implement control command parsing**

Add:

```rust
pub fn parse_control_command_line(line: &str) -> Result<ControlCommand, String> {
    serde_json::from_str(line).map_err(|error| format!("解析 control command 失败：{error}"))
}
```

- [ ] **Step 5: Implement process orchestration**

Replace `run_app_server_remote_processes` with:

```rust
fn run_app_server_remote_processes(
    real_codex: PathBuf,
    args: Vec<String>,
    wrapper_session_id: String,
    base_dir: PathBuf,
) -> Result<i32, String> {
    let real_socket = base_dir.join("real.sock");
    let relay_socket = base_dir.join("relay.sock");
    let control_socket = base_dir.join("control.sock");
    let mut server = std::process::Command::new(&real_codex)
        .args(["app-server", "--listen"])
        .arg(format!("unix://{}", real_socket.display()))
        .spawn()
        .map_err(|error| format!("启动 codex app-server 失败：{error}"))?;
    wait_for_socket(&real_socket, std::time::Duration::from_secs(5))?;
    let relay_handle = start_relay_thread(real_socket.clone(), relay_socket.clone(), wrapper_session_id.clone())?;
    let control_handle = start_control_thread(control_socket.clone(), wrapper_session_id)?;
    let mut remote_args = vec!["--remote".to_string(), format!("unix://{}", relay_socket.display())];
    remote_args.extend(args);
    let status = std::process::Command::new(real_codex)
        .args(remote_args)
        .status()
        .map_err(|error| format!("启动 codex remote 失败：{error}"))?;
    let _ = server.kill();
    relay_handle.request_stop();
    control_handle.request_stop();
    Ok(status.code().unwrap_or(1))
}
```

Implement `wait_for_socket`, `start_relay_thread`, and `start_control_thread` with `std::os::unix::net::{UnixListener, UnixStream}` under `#[cfg(unix)]`. Under `#[cfg(not(unix))]`, return:

```rust
Err("niuma-codex managed mode 当前仅支持 Unix socket 平台".to_string())
```

The relay thread must:

- accept remote TUI connections on `relay_socket`;
- connect each accepted client to `real_socket`;
- copy client-to-server and server-to-client bytes;
- after WebSocket handshake, parse server-to-client text frames and call `state.observe_server_request(...)`;
- after WebSocket handshake, parse client-to-server text frames and remove pending requests when the TUI answers them.

The control thread must:

- accept JSON Lines commands on `control_socket`;
- call `parse_control_command_line`;
- return one JSON object per connection;
- for `Requests`, return current pending approvals and inputs;
- for `ApprovalDecision` and `AnswerInput`, write a JSON-RPC response frame back to the captured upstream connection stored in state;
- for `SendInstruction` and `Interrupt`, call the app-server client helpers implemented in the same file.

- [ ] **Step 6: Run transport tests**

```bash
cargo test -p niuma-cli app_control::tests -- --nocapture
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add crates/niuma-cli/src/tools/codex/app_control.rs
git commit -m "feat: 实现 Codex app-server relay 传输" -m "修改内容：实现 WebSocket frame 解析、app-server/remote 进程编排、relay 线程和 control socket JSON Lines 协议。" -m "修改原因：让 niuma codex 受管模式具备捕获 approval/input 和回传控制命令的传输能力。"
```

### Task 5: Session Hash and Binding Overlay in Codex Provider

**Files:**
- Modify: `builtin-plugins/codex-runtime/src/codex/session_repository.rs`
- Modify: `builtin-plugins/codex-runtime/src/session_provider.rs`
- Modify: `crates/niuma-core/src/tool_session.rs`
- Test: `builtin-plugins/codex-runtime/src/tests.rs`

- [ ] **Step 1: Add failing provider test for first message hash and control overlay**

In `builtin-plugins/codex-runtime/src/tests.rs`, add:

```rust
#[test]
fn codex_session_snapshot_marks_bound_managed_session_control_available() {
    let fixture = codex_provider_fixture();
    let path = fixture.write_session(
        "session-managed",
        "/tmp/managed-repo",
        "请继续",
        "好的",
    );
    let registry_path = fixture.codex_home.join("managed-sessions").join("codex.json");
    std::fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
    std::fs::write(
        &registry_path,
        serde_json::json!({
            "version": 1,
            "sessions": [{
                "wrapper_session_id": "niuma_codex_1",
                "state": "bound",
                "cwd": "/tmp/managed-repo",
                "pid": 42,
                "real_socket": "/tmp/real.sock",
                "relay_socket": "/tmp/relay.sock",
                "control_socket": "/tmp/control.sock",
                "started_at": "2026-06-22T01:00:00Z",
                "first_user_message_hash": niuma_core::codex_managed_session::first_user_message_hash("请继续"),
                "first_user_message_preview": "请继续",
                "first_user_message_submitted_at": "2026-06-22T01:00:01Z",
                "codex_session_id": "session-managed",
                "codex_session_file_path": path.to_string_lossy(),
                "bound_at": "2026-06-22T01:00:02Z"
            }]
        }).to_string(),
    ).unwrap();

    let sessions = fixture.snapshot();
    let session = sessions.iter().find(|item| item.session_id == "session-managed").unwrap();
    assert_eq!(session.control.as_ref().unwrap().available, true);
    assert!(session.control.as_ref().unwrap().capabilities.contains(&"send_instruction".to_string()));
}
```

If the existing fixture helpers differ, adapt the test to the existing `tests.rs` helper style while keeping the assertions.

- [ ] **Step 2: Run failing test**

```bash
cargo test -p niuma-codex-plugin-runtime codex_session_snapshot_marks_bound_managed_session_control_available -- --nocapture
```

Expected: fail because `control` field does not exist.

- [ ] **Step 3: Add control model to tool session types**

Modify `crates/niuma-core/src/tool_session.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolSessionControl {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}
```

Add to both `ToolSessionListItem` and `ToolSessionDetail`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub control: Option<ToolSessionControl>,
```

Update constructors/tests that build these structs with:

```rust
control: None,
```

- [ ] **Step 4: Compute first user message hash internally**

In `session_repository.rs`, extend `FirstUserMessage`:

```rust
struct FirstUserMessage {
    preview: String,
    hash: String,
    created_at: DateTime<Utc>,
}
```

When assigning first user message, compute:

```rust
hash: niuma_core::codex_managed_session::first_user_message_hash(&message.content),
```

Add an internal field to `ToolSessionListItem` is not desired. Instead, create binding candidates inside provider/repository using the parsed `FirstUserMessage.hash`.

- [ ] **Step 5: Overlay bound control info**

Add helper in `session_repository.rs`:

```rust
fn control_for_session(session_id: &str, registry: &niuma_core::codex_managed_session::ManagedCodexRegistry) -> Option<niuma_core::tool_session::ToolSessionControl> {
    registry.sessions.iter()
        .find(|item| item.codex_session_id.as_deref() == Some(session_id) && item.state == niuma_core::codex_managed_session::ManagedCodexSessionState::Bound)
        .map(|item| niuma_core::tool_session::ToolSessionControl {
            available: true,
            provider: Some("niuma_codex".to_string()),
            wrapper_session_id: Some(item.wrapper_session_id.clone()),
            capabilities: vec![
                "send_instruction".to_string(),
                "answer_input".to_string(),
                "approve".to_string(),
                "reject".to_string(),
                "interrupt".to_string(),
            ],
        })
}
```

Read registry from `niuma_core::platform::paths::codex_managed_registry_path()` in snapshot refresh and detail building. If absent or invalid, log to stderr and return no control.

- [ ] **Step 6: Run provider tests**

```bash
cargo test -p niuma-codex-plugin-runtime codex_session_snapshot_marks_bound_managed_session_control_available -- --nocapture
cargo test -p niuma-codex-plugin-runtime -- --nocapture
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add crates/niuma-core/src/tool_session.rs builtin-plugins/codex-runtime/src/codex/session_repository.rs builtin-plugins/codex-runtime/src/session_provider.rs builtin-plugins/codex-runtime/src/tests.rs
git commit -m "feat: 标记 niuma-codex 受管会话控制能力" -m "修改内容：为 Codex session 列表和详情叠加受管 control 信息，并计算第一条用户消息 hash 支持绑定。" -m "修改原因：让 UI 和 API 能识别哪些 Codex session 可通过 niuma-codex 控制。"
```

### Task 6: Approval Channel and Arbitration

**Files:**
- Modify: `crates/niuma-core/src/models.rs`
- Modify: `crates/niuma-core/src/store/schema.rs`
- Modify: `crates/niuma-core/src/store.rs`
- Modify: `crates/niuma-api/src/handlers/approval.rs`
- Test: `crates/niuma-api/src/tests.rs`, `crates/niuma-core/src/store/tests.rs`

- [ ] **Step 1: Add failing API test for relay approval suppressed by existing hook approval**

Add to `crates/niuma-api/src/tests.rs`:

```rust
#[tokio::test]
async fn relay_approval_request_reuses_existing_hook_pending_approval() {
    let app = test_app();
    let hook_body = serde_json::json!({
        "request_id": "codex:session-1:turn-1:Bash:abc",
        "tool": "codex",
        "session_id": "session-1",
        "turn_id": "turn-1",
        "tool_name": "Bash",
        "command": "cargo test",
        "description": "cargo test",
        "project_path": "/repo",
        "project_name": "repo",
        "timeout_seconds": 600
    });
    post_json(&app, "/api/v1/approval-requests", hook_body).await;

    let relay_body = serde_json::json!({
        "request_id": "codex-relay:wrapper-1:turn-1:item-1",
        "tool": "codex",
        "session_id": "session-1",
        "turn_id": "turn-1",
        "tool_name": "Bash",
        "command": "cargo test",
        "description": "cargo test",
        "project_path": "/repo",
        "project_name": "repo",
        "channel": "niuma_codex_relay",
        "control_ref": {
            "wrapper_session_id": "wrapper-1",
            "relay_request_id": "7",
            "turn_id": "turn-1",
            "item_id": "item-1"
        }
    });
    let response = post_json(&app, "/api/v1/approval-requests", relay_body).await;
    assert_eq!(response["code"], 0);
    assert_eq!(response["data"]["accepted"], true);
    assert_eq!(response["data"]["deduped_by_channel"], "hook_proxy");
    assert_eq!(response["data"]["request_id"], "codex:session-1:turn-1:Bash:abc");
}
```

Use the existing API test helper names from `crates/niuma-api/src/tests.rs`; if there is no helper, add local helpers in that file:

```rust
async fn post_json(app: &Router, path: &str, body: serde_json::Value) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test -p niuma-api relay_approval_request_reuses_existing_hook_pending_approval -- --nocapture
```

Expected: fail because channel/control_ref are unsupported.

- [ ] **Step 3: Add model fields**

In `models.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalChannel {
    HookProxy,
    NiumaCodexRelay,
}

fn default_approval_channel() -> ApprovalChannel {
    ApprovalChannel::HookProxy
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalControlRef {
    pub wrapper_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_id: Option<String>,
    pub relay_request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
}
```

Add to `ApprovalRequest`:

```rust
#[serde(default = "default_approval_channel")]
pub channel: ApprovalChannel,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub control_ref: Option<ApprovalControlRef>,
```

- [ ] **Step 4: Persist channel/control_ref**

Modify store schema to include JSON/text columns:

```sql
approval_channel TEXT NOT NULL DEFAULT 'hook_proxy',
approval_control_ref_json TEXT
```

Update serialization/deserialization in `store.rs` so old rows default to `HookProxy`.

- [ ] **Step 5: Extend approval request body and arbitration**

In `approval.rs`, extend request body:

```rust
channel: Option<String>,
control_ref: Option<ApprovalControlRef>,
```

Parse channel:

```rust
fn parse_approval_channel(value: Option<&str>) -> Result<ApprovalChannel, String> {
    match value.unwrap_or("hook_proxy") {
        "hook_proxy" => Ok(ApprovalChannel::HookProxy),
        "niuma_codex_relay" => Ok(ApprovalChannel::NiumaCodexRelay),
        other => Err(format!("未知授权渠道：{other}")),
    }
}
```

Before inserting relay approval, compute the same fingerprint used by hook approval. If existing pending hook approval has the same fingerprint, return existing approval response with:

```json
{
  "accepted": true,
  "deduped_by_channel": "hook_proxy",
  "request_id": "existing-id"
}
```

- [ ] **Step 6: Run approval tests**

```bash
cargo test -p niuma-api relay_approval_request_reuses_existing_hook_pending_approval -- --nocapture
cargo test -p niuma-core store -- --nocapture
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add crates/niuma-core/src/models.rs crates/niuma-core/src/store.rs crates/niuma-core/src/store/schema.rs crates/niuma-core/src/store/tests.rs crates/niuma-api/src/handlers/approval.rs crates/niuma-api/src/tests.rs
git commit -m "feat: 支持 Codex 授权渠道仲裁" -m "修改内容：为 approval request 增加 channel/control_ref，并在 relay 与 hook 等价授权之间执行 fingerprint 仲裁。" -m "修改原因：避免 niuma-codex relay 与现有 Codex hook 对同一授权产生重复可操作项。"
```

### Task 7: Tool Session Control API

**Files:**
- Create: `crates/niuma-api/src/handlers/tool_session_control.rs`
- Modify: `crates/niuma-api/src/handlers.rs`
- Modify: `crates/niuma-api/src/routes.rs`
- Test: `crates/niuma-api/src/tests.rs`

- [ ] **Step 1: Add failing API tests for unbound session and answer input**

Add tests:

```rust
#[tokio::test]
async fn tool_session_control_send_fails_for_unbound_session() {
    let app = test_app();
    let response = post_json(&app, "/api/v1/tool-session-control/send", serde_json::json!({
        "tool": "codex",
        "session_id": "missing",
        "content": "继续"
    })).await;
    assert_ne!(response["code"], 0);
    assert!(response["message"].as_str().unwrap().contains("不可通过 niuma-codex 控制"));
}
```

- [ ] **Step 2: Run failing test**

```bash
cargo test -p niuma-api tool_session_control_send_fails_for_unbound_session -- --nocapture
```

Expected: route not found or handler missing.

- [ ] **Step 3: Implement handlers**

Create `tool_session_control.rs`:

```rust
use axum::body::Bytes;
use axum::extract::State;
use axum::response::Response;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_managed_session::read_registry;
use niuma_core::models::ToolKind;
use niuma_core::platform::paths::codex_managed_registry_path;
use serde::Deserialize;
use serde_json::json;

use crate::response::json_response;
use crate::state::AppState;

#[derive(Deserialize)]
pub(crate) struct SendBody {
    tool: String,
    session_id: String,
    content: String,
}

#[derive(Deserialize)]
pub(crate) struct InterruptBody {
    tool: String,
    session_id: String,
}

#[derive(Deserialize)]
pub(crate) struct AnswerInputBody {
    tool: String,
    session_id: String,
    request_id: String,
    answers: serde_json::Value,
}

pub(crate) async fn post_tool_session_control_send(State(_state): State<AppState>, body: Bytes) -> Response {
    let body = match serde_json::from_slice::<SendBody>(&body) {
        Ok(body) => body,
        Err(error) => return json_response(400, ApiResponse::fail(ApiErrorCode::ParameterFormat, format!("请求体无法解析：{error}"))),
    };
    match find_control_socket(&body.tool, &body.session_id) {
        Ok(control_socket) => match send_control_command(&control_socket, json!({"type":"send_instruction","content":body.content})) {
            Ok(value) => json_response(200, ApiResponse::ok(value)),
            Err(error) => json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, error)),
        },
        Err(error) => json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, error)),
    }
}

pub(crate) async fn post_tool_session_control_interrupt(State(_state): State<AppState>, body: Bytes) -> Response {
    let body = match serde_json::from_slice::<InterruptBody>(&body) {
        Ok(body) => body,
        Err(error) => return json_response(400, ApiResponse::fail(ApiErrorCode::ParameterFormat, format!("请求体无法解析：{error}"))),
    };
    match find_control_socket(&body.tool, &body.session_id) {
        Ok(control_socket) => match send_control_command(&control_socket, json!({"type":"interrupt"})) {
            Ok(value) => json_response(200, ApiResponse::ok(value)),
            Err(error) => json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, error)),
        },
        Err(error) => json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, error)),
    }
}

pub(crate) async fn post_tool_session_control_answer_input(State(_state): State<AppState>, body: Bytes) -> Response {
    let body = match serde_json::from_slice::<AnswerInputBody>(&body) {
        Ok(body) => body,
        Err(error) => return json_response(400, ApiResponse::fail(ApiErrorCode::ParameterFormat, format!("请求体无法解析：{error}"))),
    };
    match find_control_socket(&body.tool, &body.session_id) {
        Ok(control_socket) => match send_control_command(&control_socket, json!({"type":"answer_input","request_id":body.request_id,"answers":body.answers})) {
            Ok(value) => json_response(200, ApiResponse::ok(value)),
            Err(error) => json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, error)),
        },
        Err(error) => json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, error)),
    }
}

fn find_control_socket(tool: &str, session_id: &str) -> Result<String, String> {
    if tool != "codex" {
        return Err("当前只支持 Codex session 控制".to_string());
    }
    let registry = read_registry(&codex_managed_registry_path())?;
    registry
        .sessions
        .iter()
        .find(|item| item.codex_session_id.as_deref() == Some(session_id))
        .map(|item| item.control_socket.clone())
        .ok_or_else(|| "当前 session 不可通过 niuma-codex 控制".to_string())
}

#[cfg(unix)]
fn send_control_command(control_socket: &str, message: serde_json::Value) -> Result<serde_json::Value, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(control_socket)
        .map_err(|error| format!("连接 niuma-codex control socket 失败：{error}"))?;
    stream
        .write_all(format!("{message}\n").as_bytes())
        .map_err(|error| format!("写入 niuma-codex control socket 失败：{error}"))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|error| format!("读取 niuma-codex control socket 响应失败：{error}"))?;
    let value: serde_json::Value = serde_json::from_str(&line)
        .map_err(|error| format!("解析 niuma-codex control socket 响应失败：{error}"))?;
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err(value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("niuma-codex control socket 返回失败")
            .to_string());
    }
    Ok(value)
}

#[cfg(not(unix))]
fn send_control_command(_control_socket: &str, _message: serde_json::Value) -> Result<serde_json::Value, String> {
    Err("niuma-codex control socket 当前仅支持 Unix 平台".to_string())
}
```

- [ ] **Step 4: Register routes**

Modify `handlers.rs` to export:

```rust
pub(crate) use tool_session_control::{
    post_tool_session_control_answer_input, post_tool_session_control_interrupt,
    post_tool_session_control_send,
};
mod tool_session_control;
```

Modify `routes.rs` imports and router:

```rust
post_tool_session_control_answer_input, post_tool_session_control_interrupt,
post_tool_session_control_send,
```

Routes:

```rust
.route(
    "/api/v1/tool-session-control/send",
    post(post_tool_session_control_send).options(preflight),
)
.route(
    "/api/v1/tool-session-control/interrupt",
    post(post_tool_session_control_interrupt).options(preflight),
)
.route(
    "/api/v1/tool-session-control/answer-input",
    post(post_tool_session_control_answer_input).options(preflight),
)
```

- [ ] **Step 5: Run API tests**

```bash
cargo test -p niuma-api tool_session_control_send_fails_for_unbound_session -- --nocapture
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-api/src/handlers.rs crates/niuma-api/src/handlers/tool_session_control.rs crates/niuma-api/src/routes.rs crates/niuma-api/src/tests.rs
git commit -m "feat: 新增工具会话控制 API" -m "修改内容：新增 send、interrupt、answer-input Local API，并通过 managed registry 定位 niuma-codex control socket。" -m "修改原因：为 UI 和插件提供统一入口控制受管 Codex session。"
```

### Task 8: Frontend Types, API Calls, and Minimal UI

**Files:**
- Modify: `src/api.ts`
- Modify: `src/i18n.ts`
- Modify: `src/eventCenterView.ts`
- Modify: session detail rendering file found by `rg "session_detail|sessionDetail|session list" src`
- Test: relevant `tests/*.test.ts`

- [ ] **Step 1: Locate session detail renderer**

Run:

```bash
rg "session_detail|sessionDetail|session list|SessionDetail|session_id" src tests
```

Expected: identify the file rendering session details. Use that file in following steps.

- [ ] **Step 2: Add API types and calls**

In `src/api.ts`, extend types:

```ts
export type ToolSessionControl = {
  available: boolean
  provider?: string | null
  wrapper_session_id?: string | null
  capabilities: string[]
}

export type ToolSessionControlResult = {
  sent?: boolean
  interrupted?: boolean
  answered?: boolean
  request_id?: string
}
```

Add functions:

```ts
export async function sendToolSessionInstruction(tool: string, sessionId: string, content: string): Promise<ApiResponse<ToolSessionControlResult>> {
  return postLocalApi('/api/v1/tool-session-control/send', {
    tool,
    session_id: sessionId,
    content
  })
}

export async function interruptToolSession(tool: string, sessionId: string): Promise<ApiResponse<ToolSessionControlResult>> {
  return postLocalApi('/api/v1/tool-session-control/interrupt', {
    tool,
    session_id: sessionId
  })
}

export async function answerToolSessionInput(tool: string, sessionId: string, requestId: string, answers: Record<string, string[]>): Promise<ApiResponse<ToolSessionControlResult>> {
  return postLocalApi('/api/v1/tool-session-control/answer-input', {
    tool,
    session_id: sessionId,
    request_id: requestId,
    answers
  })
}
```

If existing helper is named differently than `postLocalApi`, use the existing local POST helper and keep these exported function names.

- [ ] **Step 3: Add i18n keys**

In `src/i18n.ts`, add keys to `Translation`:

```ts
sendInstruction: string
instructionPlaceholder: string
interruptSession: string
answerInput: string
controlUnavailable: string
requestExpired: string
```

Add translations:

```ts
// zh-CN
sendInstruction: '发送新指令',
instructionPlaceholder: '输入要发送给 Codex 的新指令',
interruptSession: '中断',
answerInput: '回答输入',
controlUnavailable: '控制通道不可用',
requestExpired: '请求已过期',
```

Use equivalent concise translations for `zh-TW`、`en`、`ja`、`ko`、`de`.

- [ ] **Step 4: Render event center input/approval actions from interaction**

In `src/eventCenterView.ts`, use existing `EventInteractionDetail` fields. Add a guard:

```ts
function isNiumaActionableInteraction(interaction: EventInteractionDetail | null | undefined): boolean {
  return interaction?.handling === 'niuma' && interaction.actionable === true
}
```

For `interaction.kind === 'input'`, render a compact input + button and call `answerToolSessionInput(...)` using `interaction.request_id`.

For `interaction.kind === 'approval'`, keep existing approval action path through `/api/v1/approval-decisions`.

- [ ] **Step 5: Render session detail send/interrupt controls**

In the session detail renderer located in Step 1, when `detail.control?.available` is true:

```ts
const canSendInstruction = detail.control?.capabilities.includes('send_instruction')
const canInterrupt = detail.control?.capabilities.includes('interrupt')
```

Render:

- textarea/input for instruction content.
- button using `sendToolSessionInstruction(detail.tool, detail.session_id, content)`.
- icon/text button for interrupt using `interruptToolSession(detail.tool, detail.session_id)`.

Do not add actions to the status bar.

- [ ] **Step 6: Add frontend tests**

Add or modify tests to assert:

```ts
expect(rendered).toContain(translations['zh-CN'].sendInstruction)
expect(rendered).toContain(translations['zh-CN'].interruptSession)
```

For non-control session:

```ts
expect(rendered).not.toContain(translations['zh-CN'].sendInstruction)
```

- [ ] **Step 7: Run frontend checks**

```bash
npm test -- --runInBand
npm run check
```

Expected: pass.

- [ ] **Step 8: Commit**

```bash
git add src/api.ts src/i18n.ts src/eventCenterView.ts <session-detail-file> tests
git commit -m "feat: 增加 Codex 受管会话前端控制入口" -m "修改内容：新增工具会话控制 API 调用、事件中心 input 操作、session 详情发送指令和中断入口，并补齐多语言文案。" -m "修改原因：让用户能在事件中心和会话详情中操作 niuma-codex 受管 session。"
```

### Task 9: End-to-End Verification and Docs

**Files:**
- Modify: `docs/integration/plugin-development.md`
- Modify: `docs/integration/plugin-development_zh.md`
- Modify: `docs/integration/sse-external-integration.md`
- Modify: `docs/integration/sse-external-integration_zh.md`
- Test: full relevant Rust/TS commands

- [ ] **Step 1: Update docs**

Document:

- `niuma codex ...` starts managed new interactive sessions.
- `resume/exec/app-server/help/version` are passthrough in v1.
- `ToolSessionControl` field in session list/detail.
- `ApprovalRequest.channel` values.
- `InputRequested` event source remains watcher; relay only overlays pending input control.

- [ ] **Step 2: Run backend verification**

```bash
cargo test -p niuma-core -- --nocapture
cargo test -p niuma-api -- --nocapture
cargo test -p niuma-cli -- --nocapture
cargo test -p niuma-codex-plugin-runtime -- --nocapture
```

Expected: pass.

- [ ] **Step 3: Run frontend verification**

```bash
npm test -- --runInBand
npm run check
```

Expected: pass.

- [ ] **Step 4: Manual smoke test**

With real Codex available:

```bash
NIUMA_REAL_CODEX="$(command -v codex)" cargo run -p niuma-cli -- codex --help
```

Expected: passthrough exits successfully and does not create a bound managed session.

```bash
NIUMA_REAL_CODEX="$(command -v codex)" cargo run -p niuma-cli -- codex
```

Expected:

- Registry file contains `waiting_first_user_message`.
- After first user prompt, registry contains `binding_pending`.
- After watcher sees session file, registry contains `bound`.
- Session detail shows `control.available = true`.

- [ ] **Step 5: Commit docs**

```bash
git add docs/integration/plugin-development.md docs/integration/plugin-development_zh.md docs/integration/sse-external-integration.md docs/integration/sse-external-integration_zh.md
git commit -m "docs: 更新 niuma-codex 受管会话集成说明" -m "修改内容：补充 niuma codex 受管会话、approval channel、session control 和 input overlay 的集成说明。" -m "修改原因：同步外部接口文档，避免插件和外部客户端误判事件来源与控制入口。"
```

---

## Self-Review

- Spec coverage: 计划覆盖 Rust wrapper 入口、JSON registry、绑定、approval channel/fingerprint 仲裁、input watcher 单源事件、control API、session/detail control overlay、前端入口、i18n 和文档。
- Scope control: 第一版仍不实现 `resume` 可控绑定、不接管普通 Codex 会话、不做状态栏按钮、不建 input store。
- Known implementation risk: Task 4 的真实 WebSocket relay transport 是最大风险，应在实现时先用小型 frame parser 单测和真实 Codex smoke test 验证，再扩大到完整控制命令。
