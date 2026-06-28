# Claude Code Watcher Managed Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Claude Code integration that matches the existing Codex plugin baseline: watcher-first session events, session list/detail provider support, and a later managed `niuma claude` control path.

**Architecture:** Phase 1 adds a `builtin-claude-code` plugin and a `claude-code-runtime` crate that watches Claude JSONL transcripts under `~/.claude/projects`, parses incremental rows, emits Niuma events, and serves provider JSON Lines RPC from the same process. Phase 2 wires the plugin into builtin registry and desktop plugin runtime. Phase 3 adds `niuma claude` managed launch/control using the existing session-control channel model; hooks and `--remote-control` remain optional enhancements.

**Tech Stack:** Rust workspace, `notify`, `serde_json`, `chrono`, `niuma-core` plugin/session models, existing provider RPC JSON Lines protocol, existing Local API event envelope.

---

## Scope And Execution Order

This plan intentionally starts with watcher/provider because that is the reliability requirement: no hooks, no wrapper, and no remote-control dependency. Managed control is later because it depends on stable session discovery and registry binding.

Implement and commit in this order:

1. Builtin plugin registration and workspace crate skeleton.
2. Claude transcript parser and detail message normalization.
3. Claude session repository and provider RPC.
4. Claude watcher runtime and Local API event emission.
5. Desktop/runtime integration and smoke tests.
6. Managed `niuma claude` registry and CLI control commands.

Do not implement hooks in this plan. Hooks are an optional enhancement package after watcher/provider works.

## File Structure

Create:

- `builtin-plugins/claude-code/plugin.json`: builtin plugin manifest, `tool_id = "claude_code"`.
- `builtin-plugins/claude-code-runtime/Cargo.toml`: runtime crate manifest.
- `builtin-plugins/claude-code-runtime/src/main.rs`: binary entrypoint.
- `builtin-plugins/claude-code-runtime/src/lib.rs`: combined watcher/provider runtime orchestration.
- `builtin-plugins/claude-code-runtime/src/claude/mod.rs`: Claude module exports.
- `builtin-plugins/claude-code-runtime/src/claude/discovery.rs`: Claude home and JSONL discovery.
- `builtin-plugins/claude-code-runtime/src/claude/session_event_cursor.rs`: incremental tail scanner.
- `builtin-plugins/claude-code-runtime/src/claude/session_file_index.rs`: line index and file signature helpers.
- `builtin-plugins/claude-code-runtime/src/claude/session_protocol/current.rs`: Claude JSONL parser.
- `builtin-plugins/claude-code-runtime/src/claude/session_repository.rs`: session snapshot/detail repository.
- `builtin-plugins/claude-code-runtime/src/session_messages.rs`: detail message normalization.
- `builtin-plugins/claude-code-runtime/src/session_provider.rs`: provider JSON Lines RPC.
- `builtin-plugins/claude-code-runtime/src/tests.rs`: runtime/provider tests.
- `crates/niuma-core/src/claude_code_managed_session.rs`: managed registry model and helpers.
- `crates/niuma-cli/src/tools/claude_code/mod.rs`: CLI module root.
- `crates/niuma-cli/src/tools/claude_code/managed.rs`: `niuma claude` launcher.
- `crates/niuma-cli/src/tools/claude_code/sessions.rs`: `niuma claude-sessions`.
- `crates/niuma-cli/src/tools/claude_code/send.rs`: `niuma claude-send`.
- `crates/niuma-cli/src/tools/claude_code/interrupt.rs`: `niuma claude-interrupt`.

Modify:

- `Cargo.toml`: add `builtin-plugins/claude-code-runtime`.
- `crates/niuma-core/src/plugin/builtin.rs`: add builtin Claude manifest and command env override.
- `crates/niuma-core/src/plugin/mod.rs`: export builtin Claude helpers.
- `crates/niuma-core/src/plugin/registry.rs`: register builtin Claude plugin.
- `crates/niuma-core/src/plugin/tests.rs`: assert builtin Claude plugin behavior.
- `crates/niuma-core/src/config.rs`: add `claude_home()` if no equivalent exists.
- `crates/niuma-core/src/platform/paths.rs`: add Claude managed registry path.
- `crates/niuma-core/src/lib.rs`: export `claude_code_managed_session`.
- `crates/niuma-cli/src/cli.rs`: add Claude commands and `ToolArg::ClaudeCode`.
- `crates/niuma-cli/src/main.rs`: route Claude commands.
- `crates/niuma-cli/src/tools/mod.rs`: export Claude CLI module.
- `src-tauri/src/tools/plugin_runtime.rs`: ensure managed builtin provider runtime includes Claude Code.
- Packaging/build files that list workspace binaries: include `niuma-claude-code-plugin` where Codex plugin is listed.

## Task 1: Register The Builtin Claude Code Plugin

**Files:**

- Create: `builtin-plugins/claude-code/plugin.json`
- Create: `builtin-plugins/claude-code-runtime/Cargo.toml`
- Create: `builtin-plugins/claude-code-runtime/src/main.rs`
- Create: `builtin-plugins/claude-code-runtime/src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `crates/niuma-core/src/plugin/builtin.rs`
- Modify: `crates/niuma-core/src/plugin/mod.rs`
- Modify: `crates/niuma-core/src/plugin/registry.rs`
- Test: `crates/niuma-core/src/plugin/tests.rs`

- [ ] **Step 1: Write failing builtin plugin tests**

Add tests to `crates/niuma-core/src/plugin/tests.rs`:

```rust
#[test]
fn builtin_claude_code_manifest_runs_as_independent_plugin_process() {
    let manifest = builtin_claude_code_manifest();

    assert_eq!(manifest.id, "builtin-claude-code");
    assert_eq!(manifest.source, PluginSource::Builtin);
    assert_eq!(manifest.tool_id, Some(ToolKind::ClaudeCode));
    assert_eq!(manifest.command.as_deref(), Some("niuma-claude-code-plugin"));
    assert!(manifest
        .capabilities
        .contains(&PluginCapability::EventWatcher));
    assert!(manifest
        .capabilities
        .contains(&PluginCapability::ToolSessionListProvider));
    assert!(manifest
        .capabilities
        .contains(&PluginCapability::ToolSessionDetailProvider));
}

#[test]
fn builtin_claude_code_manifest_uses_command_override_from_env() {
    std::env::set_var(CLAUDE_CODE_PLUGIN_COMMAND_ENV, "/tmp/niuma-claude-code-plugin");

    let manifest = builtin_claude_code_manifest();

    std::env::remove_var(CLAUDE_CODE_PLUGIN_COMMAND_ENV);
    assert_eq!(
        manifest.command.as_deref(),
        Some("/tmp/niuma-claude-code-plugin")
    );
}

#[test]
fn builtin_registry_contains_claude_code_plugin() {
    let registry = PluginRegistry::with_builtin_plugins();
    let plugin = registry.plugin_for_tool(&ToolKind::ClaudeCode).unwrap();

    assert_eq!(plugin.id, "builtin-claude-code");
    assert_eq!(plugin.source, PluginSource::Builtin);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p niuma-core builtin_claude_code
```

Expected: fails because `builtin_claude_code_manifest` and `CLAUDE_CODE_PLUGIN_COMMAND_ENV` do not exist.

- [ ] **Step 3: Add plugin manifest**

Create `builtin-plugins/claude-code/plugin.json`:

```json
{
  "id": "builtin-claude-code",
  "kind": "tool",
  "tool_id": "claude_code",
  "display_name": "Claude Code",
  "version": "0.1.0",
  "command": "niuma-claude-code-plugin",
  "args": [],
  "platforms": ["macos", "windows", "linux"],
  "capabilities": [
    "event_watcher",
    "tool_session_list_provider",
    "tool_session_detail_provider"
  ],
  "source": "builtin"
}
```

- [ ] **Step 4: Add runtime crate skeleton**

Add workspace member to root `Cargo.toml`:

```toml
"builtin-plugins/claude-code-runtime",
```

Create `builtin-plugins/claude-code-runtime/Cargo.toml`:

```toml
[package]
name = "niuma-claude-code-plugin-runtime"
version.workspace = true
edition.workspace = true
description.workspace = true
homepage.workspace = true
license.workspace = true
readme.workspace = true
repository.workspace = true

[[bin]]
name = "niuma-claude-code-plugin"
path = "src/main.rs"

[dependencies]
chrono.workspace = true
libc = "0.2"
notify = "8"
serde.workspace = true
serde_json.workspace = true
niuma-core = { path = "../../crates/niuma-core" }
ureq = { version = "2", features = ["json"] }

[dev-dependencies]
tempfile = "3"
```

Create `src/main.rs`:

```rust
fn main() {
    // 合并后的 Claude Code 插件同时提供事件监听和 session provider RPC。
    niuma_claude_code_plugin_runtime::run_combined_from_env();
}
```

Create minimal `src/lib.rs`:

```rust
pub fn run_combined_from_env() {
    eprintln!("NiumaNotifier Claude Code plugin runtime is not implemented yet");
}
```

- [ ] **Step 5: Register builtin manifest**

In `crates/niuma-core/src/plugin/builtin.rs`, add:

```rust
pub const CLAUDE_CODE_PLUGIN_COMMAND_ENV: &str = "NIUMA_CLAUDE_CODE_PLUGIN_COMMAND";
pub const BUILTIN_CLAUDE_CODE_PLUGIN_ID: &str = "builtin-claude-code";

const CLAUDE_CODE_PLUGIN_COMMAND: &str = "niuma-claude-code-plugin";
const BUILTIN_CLAUDE_CODE_PLUGIN_MANIFEST_JSON: &str =
    include_str!("../../../../builtin-plugins/claude-code/plugin.json");

pub fn builtin_claude_code_manifest() -> PluginManifest {
    let mut manifest = parse_plugin_manifest(BUILTIN_CLAUDE_CODE_PLUGIN_MANIFEST_JSON)
        .expect("内置 Claude Code 插件 manifest 必须是有效 plugin.json");
    manifest.source = PluginSource::Builtin;
    manifest.command = Some(builtin_plugin_command(
        CLAUDE_CODE_PLUGIN_COMMAND_ENV,
        manifest.command,
        CLAUDE_CODE_PLUGIN_COMMAND,
    ));
    manifest.base_dir = None;
    manifest
}
```

In `crates/niuma-core/src/plugin/mod.rs`, export the new items:

```rust
pub use builtin::{
    builtin_bark_manifest, builtin_claude_code_manifest, builtin_codex_manifest,
    builtin_ntfy_manifest, BARK_PLUGIN_COMMAND_ENV, BUILTIN_BARK_PLUGIN_ID,
    BUILTIN_CLAUDE_CODE_PLUGIN_ID, BUILTIN_CODEX_PLUGIN_ID, BUILTIN_NTFY_PLUGIN_ID,
    CLAUDE_CODE_PLUGIN_COMMAND_ENV, CODEX_PLUGIN_COMMAND_ENV, NTFY_PLUGIN_COMMAND_ENV,
};
```

In `crates/niuma-core/src/plugin/registry.rs`, include the manifest:

```rust
registry.register(builtin_claude_code_manifest());
```

- [ ] **Step 6: Run tests and commit**

Run:

```bash
cargo test -p niuma-core builtin_claude_code
cargo test -p niuma-core plugin
```

Expected: both commands pass.

Commit:

```bash
git add Cargo.toml builtin-plugins/claude-code builtin-plugins/claude-code-runtime crates/niuma-core/src/plugin
git commit -m "feat: 注册 Claude Code 内置插件" -m "修改内容：新增 Claude Code 内置插件 manifest、runtime crate 骨架和插件注册测试。" -m "修改原因：为 watcher/provider 实现提供可被宿主发现和启动的内置插件入口。"
```

## Task 2: Implement Claude JSONL Message Parsing

**Files:**

- Create: `builtin-plugins/claude-code-runtime/src/claude/mod.rs`
- Create: `builtin-plugins/claude-code-runtime/src/claude/session_protocol/current.rs`
- Create: `builtin-plugins/claude-code-runtime/src/session_messages.rs`
- Modify: `builtin-plugins/claude-code-runtime/src/lib.rs`
- Test: `builtin-plugins/claude-code-runtime/src/tests.rs`

- [ ] **Step 1: Add parser tests**

Create `builtin-plugins/claude-code-runtime/src/tests.rs` with tests for the verified transcript shapes:

```rust
use chrono::TimeZone;
use niuma_core::models::{RuntimeStateStatus, ToolKind};
use niuma_core::tool_session::ToolSessionMessageRole;

use crate::claude::session_protocol::current::ClaudeJsonlParser;
use crate::session_messages::parse_claude_message_line;

const SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";

#[test]
fn parser_emits_session_started_from_user_string_message() {
    let line = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:00.000Z","message":{"role":"user","content":"你好"}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser.parse_line(line, "/tmp/session.jsonl").unwrap().unwrap();

    assert_eq!(event.tool, ToolKind::ClaudeCode);
    assert_eq!(event.session_id, SESSION_ID);
    assert_eq!(event.status, RuntimeStateStatus::Running);
    assert!(event.summary.contains("你好"));
}

#[test]
fn parser_tracks_pending_tool_until_tool_result() {
    let tool_use = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"sleep 1"}}]}}"#;
    let tool_result = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok","is_error":false}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let first = parser.parse_line(tool_use, "/tmp/session.jsonl").unwrap().unwrap();
    let second = parser.parse_line(tool_result, "/tmp/session.jsonl").unwrap().unwrap();

    assert_eq!(first.summary, "Claude Code 正在调用工具：Bash");
    assert_eq!(second.summary, "Claude Code 工具执行完成：ok");
}

#[test]
fn parser_marks_failed_tool_result_as_error() {
    let line = r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"exit code 1","is_error":true}]}}"#;
    let mut parser = ClaudeJsonlParser::default();

    let event = parser.parse_line(line, "/tmp/session.jsonl").unwrap().unwrap();

    assert_eq!(event.status, RuntimeStateStatus::Error);
    assert!(event.summary.contains("exit code 1"));
}

#[test]
fn detail_message_parser_maps_assistant_text_and_tool_rows() {
    let assistant = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:04.000Z","message":{"role":"assistant","content":[{"type":"text","text":"完成了"}]}}"#;
    let tool = r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:05.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"pwd"}}]}}"#;

    let assistant_message = parse_claude_message_line(SESSION_ID, 0, assistant);
    let tool_message = parse_claude_message_line(SESSION_ID, 1, tool);

    assert_eq!(assistant_message.role, ToolSessionMessageRole::Assistant);
    assert_eq!(assistant_message.content, "完成了");
    assert_eq!(tool_message.role, ToolSessionMessageRole::ToolCall);
    assert_eq!(tool_message.content, "Bash");
}
```

- [ ] **Step 2: Run parser tests and verify failure**

Run:

```bash
cargo test -p niuma-claude-code-plugin-runtime parser_
```

Expected: fails because parser modules do not exist.

- [ ] **Step 3: Add Claude parser modules**

Create `src/claude/mod.rs`:

```rust
pub mod session_protocol;
```

Create `src/claude/session_protocol/mod.rs`:

```rust
pub mod current;
```

Create `src/claude/session_protocol/current.rs` with these public items:

```rust
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use niuma_core::models::{NiumaEvent, RuntimeStateStatus, ToolKind};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Default)]
pub(crate) struct ClaudeJsonlParser {
    pending_tools: HashMap<String, String>,
}

#[derive(Deserialize)]
struct ClaudeRow {
    #[serde(rename = "type")]
    row_type: String,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    #[serde(default)]
    message: Value,
}

impl ClaudeJsonlParser {
    pub(crate) fn parse_line(
        &mut self,
        line: &str,
        file_path: &str,
    ) -> Result<Option<NiumaEvent>, String> {
        let row: ClaudeRow = serde_json::from_str(line)
            .map_err(|error| format!("解析 Claude JSONL 失败：{error}"))?;
        let Some(session_id) = row.session_id.as_deref() else {
            return Ok(None);
        };
        let cwd = row.cwd.as_deref().unwrap_or("");
        let created_at = parse_timestamp(row.timestamp.as_deref()).unwrap_or_else(Utc::now);
        let project_name = cwd
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or("Claude Code")
            .to_string();
        let summary = summary_for_row(&row, &mut self.pending_tools);
        let Some((summary, status)) = summary else {
            return Ok(None);
        };

        Ok(Some(NiumaEvent {
            id: format!("claude-code:{session_id}:{}", created_at.timestamp_millis()),
            tool: ToolKind::ClaudeCode,
            session_id: session_id.to_string(),
            project_path: cwd.to_string(),
            project_name,
            status,
            summary,
            created_at,
            raw_event_type: Some(row.row_type),
            raw_event_id: Some(file_path.to_string()),
            attention_resolve_key: None,
            approval_request: None,
        }))
    }
}

fn summary_for_row(
    row: &ClaudeRow,
    pending_tools: &mut HashMap<String, String>,
) -> Option<(String, RuntimeStateStatus)> {
    match row.row_type.as_str() {
        "user" => user_summary(row, pending_tools),
        "assistant" => assistant_summary(row, pending_tools),
        _ => None,
    }
}

fn user_summary(
    row: &ClaudeRow,
    pending_tools: &mut HashMap<String, String>,
) -> Option<(String, RuntimeStateStatus)> {
    let content = row.message.get("content")?;
    if let Some(text) = content.as_str().and_then(trimmed_text) {
        return Some((text, RuntimeStateStatus::Running));
    }
    for item in content.as_array()? {
        if item.get("type").and_then(Value::as_str) == Some("tool_result") {
            let tool_use_id = item.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
            pending_tools.remove(tool_use_id);
            let text = item
                .get("content")
                .and_then(text_from_value)
                .unwrap_or_else(|| "工具结果已返回".to_string());
            let is_error = item
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let status = if is_error {
                RuntimeStateStatus::Error
            } else {
                RuntimeStateStatus::Running
            };
            return Some((format!("Claude Code 工具执行完成：{text}"), status));
        }
    }
    None
}

fn assistant_summary(
    row: &ClaudeRow,
    pending_tools: &mut HashMap<String, String>,
) -> Option<(String, RuntimeStateStatus)> {
    for item in row.message.get("content")?.as_array()? {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = item.get("text").and_then(Value::as_str).and_then(trimmed_text)?;
                return Some((text, RuntimeStateStatus::Completed));
            }
            Some("tool_use") => {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                pending_tools.insert(id.to_string(), name.to_string());
                return Some((
                    format!("Claude Code 正在调用工具：{name}"),
                    RuntimeStateStatus::Running,
                ));
            }
            Some("thinking") => {
                return Some(("Claude Code 正在思考".to_string(), RuntimeStateStatus::Running));
            }
            _ => {}
        }
    }
    None
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?).ok().map(|value| value.with_timezone(&Utc))
}

fn text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => trimmed_text(text),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| item.as_str().and_then(trimmed_text))
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

fn trimmed_text(value: &str) -> Option<String> {
    let text = value.trim();
    (!text.is_empty()).then(|| text.chars().take(240).collect())
}
```

- [ ] **Step 4: Add detail message normalization**

Create `src/session_messages.rs`:

```rust
use chrono::{DateTime, Utc};
use niuma_core::tool_session::{ToolSessionMessage, ToolSessionMessageRole};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct ClaudeRow {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    row_type: String,
    #[serde(default)]
    message: Value,
}

pub(crate) fn parse_claude_message_line(
    session_id: &str,
    line_index: usize,
    line: &str,
) -> ToolSessionMessage {
    let id = format!("claude_code:{session_id}:{line_index:020}");
    let Ok(row) = serde_json::from_str::<ClaudeRow>(line) else {
        return ToolSessionMessage {
            id,
            role: ToolSessionMessageRole::Unknown,
            content: String::new(),
            created_at: epoch(),
            metadata: json!({"reason": "invalid_json"}),
        };
    };
    let (role, content, content_type) = role_content_for_row(&row);
    ToolSessionMessage {
        id,
        role,
        content,
        created_at: parse_timestamp(row.timestamp.as_deref()).unwrap_or_else(epoch),
        metadata: json!({
            "raw_type": row.row_type,
            "content_type": content_type,
        }),
    }
}

fn role_content_for_row(row: &ClaudeRow) -> (ToolSessionMessageRole, String, &'static str) {
    let content = row.message.get("content").unwrap_or(&Value::Null);
    if row.row_type == "user" {
        if let Some(text) = content.as_str().and_then(trimmed_text) {
            return (ToolSessionMessageRole::User, text, "text");
        }
        if let Some((text, is_error)) = first_tool_result(content) {
            let role = ToolSessionMessageRole::ToolResult;
            let content_type = if is_error { "tool_result_error" } else { "tool_result" };
            return (role, text, content_type);
        }
    }
    if row.row_type == "assistant" {
        if let Some((role, text, content_type)) = first_assistant_item(content) {
            return (role, text, content_type);
        }
    }
    (ToolSessionMessageRole::Event, String::new(), "unmapped")
}

fn first_assistant_item(content: &Value) -> Option<(ToolSessionMessageRole, String, &'static str)> {
    for item in content.as_array()? {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = item.get("text").and_then(Value::as_str).and_then(trimmed_text)?;
                return Some((ToolSessionMessageRole::Assistant, text, "text"));
            }
            Some("thinking") => {
                return Some((
                    ToolSessionMessageRole::Assistant,
                    "Claude Code 正在思考".to_string(),
                    "thinking",
                ));
            }
            Some("tool_use") => {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                return Some((ToolSessionMessageRole::ToolCall, name.to_string(), "tool_use"));
            }
            _ => {}
        }
    }
    None
}

fn first_tool_result(content: &Value) -> Option<(String, bool)> {
    for item in content.as_array()? {
        if item.get("type").and_then(Value::as_str) == Some("tool_result") {
            let text = item
                .get("content")
                .and_then(text_from_value)
                .unwrap_or_else(|| "工具结果已返回".to_string());
            let is_error = item.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            return Some((text, is_error));
        }
    }
    None
}

fn text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => trimmed_text(text),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| item.as_str().and_then(trimmed_text))
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?).ok().map(|value| value.with_timezone(&Utc))
}

fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from(std::time::SystemTime::UNIX_EPOCH)
}

fn trimmed_text(value: &str) -> Option<String> {
    let text = value.trim();
    (!text.is_empty()).then(|| text.to_string())
}
```

Update `src/lib.rs`:

```rust
mod claude;
mod session_messages;

pub fn run_combined_from_env() {
    eprintln!("NiumaNotifier Claude Code plugin runtime is not implemented yet");
}
```

- [ ] **Step 5: Run tests and commit**

Run:

```bash
cargo test -p niuma-claude-code-plugin-runtime parser_
cargo test -p niuma-claude-code-plugin-runtime detail_message
```

Expected: parser and detail message tests pass.

Commit:

```bash
git add builtin-plugins/claude-code-runtime
git commit -m "feat: 解析 Claude Code 会话 JSONL" -m "修改内容：新增 Claude JSONL 增量事件解析和详情消息归一化测试。" -m "修改原因：让 watcher/provider 能从原始 transcript 中生成基础事件流。"
```

## Task 3: Build Claude Session Repository And Provider

**Files:**

- Create: `builtin-plugins/claude-code-runtime/src/claude/session_file_index.rs`
- Create: `builtin-plugins/claude-code-runtime/src/claude/discovery.rs`
- Create: `builtin-plugins/claude-code-runtime/src/claude/session_repository.rs`
- Create: `builtin-plugins/claude-code-runtime/src/session_provider.rs`
- Modify: `builtin-plugins/claude-code-runtime/src/claude/mod.rs`
- Modify: `builtin-plugins/claude-code-runtime/src/lib.rs`
- Test: `builtin-plugins/claude-code-runtime/src/tests.rs`

- [ ] **Step 1: Add repository/provider tests**

Add tests:

```rust
use std::fs;

use niuma_core::models::ToolKind;
use niuma_core::tool_session::ToolSessionStatus;
use niuma_core::tool_session_rpc::{ProviderRpcRequest, SessionDetailParams, SessionSnapshotParams};
use tempfile::tempdir;

use crate::claude::session_repository::ClaudeSessionRepository;
use crate::session_provider::ClaudeSessionProvider;

#[test]
fn repository_builds_snapshot_from_claude_project_files() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("projects").join("-repo");
    fs::create_dir_all(&project).unwrap();
    let file = project.join("11111111-1111-4111-8111-111111111111.jsonl");
    fs::write(
        &file,
        r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:00.000Z","message":{"role":"user","content":"你好"}}"#,
    )
    .unwrap();

    let mut repository = ClaudeSessionRepository::new(temp.path().to_path_buf());
    let sessions = repository.refresh_snapshot().unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].tool, ToolKind::ClaudeCode);
    assert_eq!(sessions[0].session_id, "11111111-1111-4111-8111-111111111111");
    assert_eq!(sessions[0].project_path, "/repo");
    assert_eq!(sessions[0].project_name, "repo");
    assert_eq!(sessions[0].status, ToolSessionStatus::Active);
    assert_eq!(sessions[0].first_user_message_preview.as_deref(), Some("你好"));
}

#[test]
fn provider_returns_snapshot_and_detail_for_claude_code_tool() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("projects").join("-repo");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("11111111-1111-4111-8111-111111111111.jsonl"),
        concat!(
            r#"{"type":"user","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:00.000Z","message":{"role":"user","content":"你好"}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/repo","timestamp":"2026-06-28T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"完成"}]}}"#,
            "\n"
        ),
    )
    .unwrap();
    let mut provider = ClaudeSessionProvider::with_claude_home(temp.path().to_path_buf());

    let snapshot_request = ProviderRpcRequest::new(
        "1",
        "session_snapshot",
        SessionSnapshotParams {
            tool: ToolKind::ClaudeCode,
        },
    )
    .unwrap();
    let snapshot = provider.handle_request(snapshot_request);
    assert!(snapshot.error.is_none());

    let detail_request = ProviderRpcRequest::new(
        "2",
        "session_detail",
        SessionDetailParams {
            tool: ToolKind::ClaudeCode,
            session_id: "11111111-1111-4111-8111-111111111111".to_string(),
            limit: 10,
            cursor: None,
        },
    )
    .unwrap();
    let detail = provider.handle_request(detail_request);
    assert!(detail.error.is_none());
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p niuma-claude-code-plugin-runtime repository_
cargo test -p niuma-claude-code-plugin-runtime provider_
```

Expected: fails because repository/provider modules do not exist.

- [ ] **Step 3: Implement discovery and file index**

Implement `discovery.rs`:

```rust
use std::path::{Path, PathBuf};

pub(crate) fn claude_projects_dir(claude_home: &Path) -> PathBuf {
    claude_home.join("projects")
}

pub(crate) fn recent_jsonl_files(claude_home: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_jsonl_files(&claude_projects_dir(claude_home), &mut files);
    files.sort_by(|left, right| modified_time(right).cmp(&modified_time(left)));
    files.into_iter().take(limit).collect()
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn modified_time(path: &Path) -> std::time::SystemTime {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}
```

Implement `session_file_index.rs` with file signature and indexed line helpers copied from Codex with names changed to Claude:

```rust
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClaudeSessionFileSignature {
    pub(crate) len: u64,
    pub(crate) modified: Option<std::time::SystemTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClaudeMessageLineIndex {
    pub(crate) line_index: usize,
    pub(crate) byte_start: u64,
    pub(crate) byte_end: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClaudeSessionFileIndex {
    pub(crate) path: String,
    pub(crate) signature: ClaudeSessionFileSignature,
    pub(crate) message_lines: Vec<ClaudeMessageLineIndex>,
}

pub(crate) fn session_file_signature(
    path: &Path,
) -> Result<ClaudeSessionFileSignature, std::io::Error> {
    let metadata = std::fs::metadata(path)?;
    Ok(ClaudeSessionFileSignature {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}
```

- [ ] **Step 4: Implement repository snapshot/detail**

Implement `session_repository.rs` with these public methods:

```rust
pub(crate) struct ClaudeSessionRepository {
    claude_home: PathBuf,
    index: HashMap<String, SessionIndex>,
}

impl ClaudeSessionRepository {
    pub(crate) fn new(claude_home: PathBuf) -> Self;
    pub(crate) fn refresh_snapshot(&mut self) -> Result<Vec<ToolSessionListItem>, String>;
    pub(crate) fn session_detail(
        &mut self,
        params: &SessionDetailParams,
    ) -> Result<ToolSessionDetail, ProviderError>;
    pub(crate) fn clear_runtime_indexes(&mut self);
}
```

Implementation rules:

- Scan at most 128 newest JSONL files.
- Parse session id from `sessionId`; fall back to file stem only when missing.
- Parse project path from `cwd`; project name is final path component.
- `status = Active` when mtime is within 60 seconds, otherwise `Inactive`.
- `is_subagent = file_path contains "/subagents/"`.
- `first_user_message_preview` comes from the first user string content, capped to 200 chars.
- Detail messages use `parse_claude_message_line`, newest first, with `next_cursor` as the next older line index.

- [ ] **Step 5: Implement provider RPC**

Implement `session_provider.rs` following Codex provider structure:

```rust
pub struct ClaudeSessionProvider {
    repository: Arc<Mutex<ClaudeSessionRepository>>,
}

impl ClaudeSessionProvider {
    pub fn from_config() -> Self;
    pub fn with_claude_home(claude_home: PathBuf) -> Self;
    pub fn handle_request(&mut self, request: ProviderRpcRequest) -> ProviderRpcResponse;
}
```

Provider behavior:

- `session_snapshot` only accepts `ToolKind::ClaudeCode`.
- `session_detail` only accepts `ToolKind::ClaudeCode`.
- Unsupported tool returns `unsupported_tool`.
- Unknown method returns `method_not_found`.
- Invalid params returns `invalid_params`.

- [ ] **Step 6: Run tests and commit**

Run:

```bash
cargo test -p niuma-claude-code-plugin-runtime repository_
cargo test -p niuma-claude-code-plugin-runtime provider_
```

Expected: repository/provider tests pass.

Commit:

```bash
git add builtin-plugins/claude-code-runtime
git commit -m "feat: 提供 Claude Code 会话列表和详情" -m "修改内容：新增 Claude session repository、provider RPC 和 snapshot/detail 测试。" -m "修改原因：让 NiuMaNotifier 不依赖 hooks 也能读取 Claude Code 会话。"
```

## Task 4: Implement Watcher Runtime Event Flow

**Files:**

- Create: `builtin-plugins/claude-code-runtime/src/claude/session_event_cursor.rs`
- Modify: `builtin-plugins/claude-code-runtime/src/lib.rs`
- Modify: `builtin-plugins/claude-code-runtime/src/claude/mod.rs`
- Test: `builtin-plugins/claude-code-runtime/src/tests.rs`

- [ ] **Step 1: Add incremental watcher tests**

Add tests:

```rust
use std::fs::OpenOptions;
use std::io::Write;

use tempfile::tempdir;

use crate::claude::session_event_cursor::ClaudeSessionScanner;
use crate::claude::session_repository::ClaudeSessionRepository;

#[test]
fn scanner_reads_appended_tool_use_before_tool_result() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("session.jsonl");
    std::fs::write(&file, "").unwrap();
    let repository = ClaudeSessionRepository::new(temp.path().to_path_buf());
    let mut scanner = ClaudeSessionScanner::with_repository(repository);

    {
        let mut handle = OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(handle, r#"{{"type":"assistant","sessionId":"44444444-4444-4444-8444-444444444444","cwd":"/repo","timestamp":"2026-06-28T01:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"toolu_1","name":"Bash","input":{{"command":"sleep 10"}}}}]}}}}"#).unwrap();
    }
    let first = scanner.scan_file(&file).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].summary, "Claude Code 正在调用工具：Bash");

    {
        let mut handle = OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(handle, r#"{{"type":"user","sessionId":"44444444-4444-4444-8444-444444444444","cwd":"/repo","timestamp":"2026-06-28T01:00:02.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_1","content":"done","is_error":false}}]}}}}"#).unwrap();
    }
    let second = scanner.scan_file(&file).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].summary, "Claude Code 工具执行完成：done");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p niuma-claude-code-plugin-runtime scanner_reads_appended
```

Expected: fails because scanner does not exist.

- [ ] **Step 3: Implement incremental scanner**

Implement `session_event_cursor.rs` using Codex scanner as the template:

```rust
#[derive(Clone, Default)]
pub(crate) struct ClaudeEventCursor {
    pub(crate) offset: u64,
    pub(crate) last_len: u64,
    pub(crate) parser: ClaudeJsonlParser,
}

pub(crate) struct ClaudeSessionScanner {
    repository: Arc<Mutex<ClaudeSessionRepository>>,
}

impl ClaudeSessionScanner {
    pub(crate) fn with_repository(repository: ClaudeSessionRepository) -> Self;
    pub(crate) fn scan_file(&mut self, path: &Path) -> Result<Vec<NiumaEvent>, String>;
    pub(crate) fn prime_file_to_end(&mut self, path: &Path) -> Result<(), String>;
}
```

Rules:

- Only parse complete newline-terminated lines.
- If file length shrinks, reset cursor and parser.
- Store cursor per path in repository.
- Prime discovered old files to end so historical transcripts do not emit duplicate notifications.

- [ ] **Step 4: Implement combined runtime loop**

Replace `run_combined_from_env()` in `src/lib.rs` with a Claude variant of Codex runtime:

```rust
pub fn run_combined_from_env() {
    start_parent_watchdog_from_env();
    let event_sink = match LocalApiClaudeEventSink::from_env() {
        Ok(event_sink) => event_sink,
        Err(error) => {
            eprintln!("NiumaNotifier Claude Code plugin process not started: {error}");
            std::process::exit(1);
        }
    };
    let claude_home = niuma_core::config::claude_home();
    let repository = Arc::new(Mutex::new(ClaudeSessionRepository::new(claude_home.clone())));
    let watcher_repository = repository.clone();
    if let Err(error) = thread::Builder::new()
        .name("claude-code-watcher-runtime".to_string())
        .spawn(move || run_runtime(Box::new(event_sink), watcher_repository, claude_home))
    {
        eprintln!("NiumaNotifier Claude Code watcher runtime not started: {error}");
        std::process::exit(1);
    }
    session_provider::run_stdio_session_provider_with_repository(repository);
}
```

Add concise comments only for non-obvious behavior:

- stdout is reserved for provider JSON Lines RPC.
- discovered historical files are primed to end.
- active files are polled because append events can be coalesced or missed.

- [ ] **Step 5: Add config helper**

If `crates/niuma-core/src/config.rs` lacks Claude home discovery, add:

```rust
pub fn claude_home() -> std::path::PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".claude")))
        .unwrap_or_else(|| std::path::PathBuf::from(".claude"))
}
```

- [ ] **Step 6: Run tests and commit**

Run:

```bash
cargo test -p niuma-claude-code-plugin-runtime scanner_reads_appended
cargo test -p niuma-claude-code-plugin-runtime
cargo build -p niuma-claude-code-plugin-runtime
```

Expected: tests and build pass.

Commit:

```bash
git add builtin-plugins/claude-code-runtime crates/niuma-core/src/config.rs
git commit -m "feat: 监听 Claude Code 会话文件事件" -m "修改内容：新增 Claude session watcher、增量扫描和合并 provider/runtime 入口。" -m "修改原因：保证无 hooks 场景下也能实时触发 Claude Code 基础事件流。"
```

## Task 5: Wire Desktop Runtime And Listener Enablement

**Files:**

- Modify: `src-tauri/src/tools/plugin_runtime.rs`
- Modify: `crates/niuma-core/src/listener_config.rs` if default enablement is changed
- Test: `src-tauri/src/tools/plugin_runtime.rs`
- Test: `crates/niuma-core/src/listener_config.rs`

- [ ] **Step 1: Add runtime integration tests**

Add or update tests in `src-tauri/src/tools/plugin_runtime.rs`:

```rust
#[test]
fn managed_runtime_manifests_include_builtin_claude_code_provider() {
    let registry = PluginRegistry::with_builtin_plugins();
    let manifests = managed_runtime_manifests(&registry);

    assert!(manifests.iter().any(|manifest| {
        manifest.id == "builtin-claude-code" && manifest.source == PluginSource::Builtin
    }));
}
```

- [ ] **Step 2: Run test and verify failure if runtime filtering excludes Claude**

Run:

```bash
cargo test -p niuma-desktop managed_runtime_manifests_include_builtin_claude_code_provider
```

Expected: fails only if runtime filtering is Codex-specific. If it already passes after Task 1, keep the test as regression coverage.

- [ ] **Step 3: Update runtime filtering**

In `managed_runtime_manifests`, ensure the predicate is capability-based:

```rust
manifest.source == PluginSource::Builtin
    && manifest
        .capabilities
        .contains(&PluginCapability::EventWatcher)
```

Do not special-case `builtin-codex`.

- [ ] **Step 4: Decide listener default**

Keep `claude_code_listening_enabled` default `false` for first release unless the product decision is to enable Claude by default. If enabling by default, update the existing listener config test:

```rust
#[test]
fn builtin_claude_code_listener_default_matches_product_setting() {
    assert!(!ListenerConfig::default().is_tool_enabled(&ToolKind::ClaudeCode));
}
```

- [ ] **Step 5: Run tests and commit**

Run:

```bash
cargo test -p niuma-desktop managed_runtime_manifests_include_builtin_claude_code_provider
cargo test -p niuma-core listener_config
```

Expected: tests pass.

Commit:

```bash
git add src-tauri/src/tools/plugin_runtime.rs crates/niuma-core/src/listener_config.rs
git commit -m "feat: 接入 Claude Code 插件运行时" -m "修改内容：让桌面插件 runtime 能按 capability 启动 Claude Code 内置 watcher/provider。" -m "修改原因：让新增内置插件在应用启动后进入正常事件流。"
```

## Task 6: Add Managed Claude Registry Helpers

**Files:**

- Create: `crates/niuma-core/src/claude_code_managed_session.rs`
- Modify: `crates/niuma-core/src/lib.rs`
- Modify: `crates/niuma-core/src/platform/paths.rs`
- Test: `crates/niuma-core/src/claude_code_managed_session.rs`

- [ ] **Step 1: Add registry tests**

Create tests in `claude_code_managed_session.rs`:

```rust
#[test]
fn managed_claude_channel_id_roundtrips_wrapper_session_id() {
    let channel_id = managed_claude_code_channel_id("niuma_claude_1");

    assert_eq!(channel_id, "niuma_claude_managed:niuma_claude_1");
    assert_eq!(
        wrapper_session_id_from_channel_id(&channel_id),
        Some("niuma_claude_1")
    );
}

#[test]
fn registry_roundtrips_managed_claude_session() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("claude-code.json");
    let session = ManagedClaudeCodeSession {
        wrapper_session_id: "niuma_claude_1".to_string(),
        state: ManagedClaudeCodeSessionState::Started,
        cwd: "/repo".to_string(),
        pid: Some(123),
        control_socket: Some("/tmp/niuma-claude/1/control.sock".to_string()),
        started_at: chrono::Utc::now(),
        claude_session_id: Some("11111111-1111-4111-8111-111111111111".to_string()),
        transcript_path: None,
        bound_at: None,
        binding_failure_reason: None,
    };

    update_registry(&path, |registry| registry.upsert(session.clone())).unwrap();
    let loaded = read_registry(&path).unwrap();

    assert_eq!(loaded.sessions.len(), 1);
    assert_eq!(loaded.sessions[0].wrapper_session_id, session.wrapper_session_id);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p niuma-core managed_claude
```

Expected: fails because registry helpers do not exist.

- [ ] **Step 3: Implement registry model**

Create `claude_code_managed_session.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MANAGED_CLAUDE_CODE_CHANNEL_PREFIX: &str = "niuma_claude_managed:";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ManagedClaudeCodeRegistry {
    pub version: u32,
    #[serde(default)]
    pub sessions: Vec<ManagedClaudeCodeSession>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedClaudeCodeSessionState {
    Started,
    BindingPending,
    Bound,
    Exited,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedClaudeCodeSession {
    pub wrapper_session_id: String,
    pub state: ManagedClaudeCodeSessionState,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_socket: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_failure_reason: Option<String>,
}

impl ManagedClaudeCodeRegistry {
    pub fn upsert(&mut self, session: ManagedClaudeCodeSession) {
        self.version = 1;
        self.sessions
            .retain(|item| item.wrapper_session_id != session.wrapper_session_id);
        self.sessions.push(session);
    }
}

pub fn managed_claude_code_channel_id(wrapper_session_id: &str) -> String {
    format!("{MANAGED_CLAUDE_CODE_CHANNEL_PREFIX}{wrapper_session_id}")
}

pub fn wrapper_session_id_from_channel_id(channel_id: &str) -> Option<&str> {
    channel_id.strip_prefix(MANAGED_CLAUDE_CODE_CHANNEL_PREFIX)
}

pub fn read_registry(path: &Path) -> Result<ManagedClaudeCodeRegistry, String> {
    if !path.exists() {
        return Ok(ManagedClaudeCodeRegistry::default());
    }
    let text = std::fs::read_to_string(path).map_err(|error| format!("读取 Claude Code managed registry 失败：{error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("解析 Claude Code managed registry 失败：{error}"))
}

pub fn update_registry(
    path: &Path,
    mutate: impl FnOnce(&mut ManagedClaudeCodeRegistry),
) -> Result<ManagedClaudeCodeRegistry, String> {
    let mut registry = read_registry(path)?;
    mutate(&mut registry);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建 Claude Code managed registry 目录失败：{error}"))?;
    }
    let text = serde_json::to_string_pretty(&registry).map_err(|error| format!("序列化 Claude Code managed registry 失败：{error}"))?;
    std::fs::write(path, text).map_err(|error| format!("写入 Claude Code managed registry 失败：{error}"))?;
    Ok(registry)
}
```

- [ ] **Step 4: Add managed registry path**

In `platform/paths.rs`, add:

```rust
pub fn claude_code_managed_registry_path() -> PathBuf {
    app_data_dir()
        .join("managed-sessions")
        .join("claude-code.json")
}
```

Export module in `lib.rs`:

```rust
pub mod claude_code_managed_session;
```

- [ ] **Step 5: Run tests and commit**

Run:

```bash
cargo test -p niuma-core managed_claude
```

Expected: tests pass.

Commit:

```bash
git add crates/niuma-core/src/claude_code_managed_session.rs crates/niuma-core/src/lib.rs crates/niuma-core/src/platform/paths.rs
git commit -m "feat: 新增 Claude Code 受管会话注册表" -m "修改内容：新增 Claude Code managed registry、channel id 辅助函数和注册表路径。" -m "修改原因：为 niuma claude 受管启动和 provider control channel 绑定提供持久状态。"
```

## Task 7: Add `niuma claude` CLI Commands

**Files:**

- Modify: `crates/niuma-cli/src/cli.rs`
- Modify: `crates/niuma-cli/src/main.rs`
- Modify: `crates/niuma-cli/src/tools/mod.rs`
- Create: `crates/niuma-cli/src/tools/claude_code/mod.rs`
- Create: `crates/niuma-cli/src/tools/claude_code/managed.rs`
- Create: `crates/niuma-cli/src/tools/claude_code/sessions.rs`
- Create: `crates/niuma-cli/src/tools/claude_code/send.rs`
- Create: `crates/niuma-cli/src/tools/claude_code/interrupt.rs`
- Test: `crates/niuma-cli/src/cli.rs`

- [ ] **Step 1: Add CLI parse tests**

Add to `crates/niuma-cli/src/cli.rs` tests:

```rust
fn parse_claude_args(argv: &[&str]) -> Vec<String> {
    let cli = Cli::try_parse_from(argv).unwrap();
    match cli.command.unwrap() {
        Command::Claude(command) => command.args,
        _ => panic!("expected claude command"),
    }
}

#[test]
fn parses_claude_flags_as_trailing_args() {
    assert_eq!(
        parse_claude_args(&["niuma", "claude", "--model", "sonnet"]),
        vec!["--model".to_string(), "sonnet".to_string()]
    );
}

#[test]
fn parses_claude_send_and_interrupt_commands() {
    let cli = Cli::try_parse_from(["niuma", "claude-send", "niuma_claude_1", "继续"]).unwrap();
    match cli.command.unwrap() {
        Command::ClaudeSend(command) => {
            assert_eq!(command.wrapper_session_id, "niuma_claude_1");
            assert_eq!(command.message, "继续");
        }
        _ => panic!("expected claude-send command"),
    }

    let cli = Cli::try_parse_from(["niuma", "claude-interrupt", "niuma_claude_1"]).unwrap();
    match cli.command.unwrap() {
        Command::ClaudeInterrupt(command) => {
            assert_eq!(command.wrapper_session_id, "niuma_claude_1");
        }
        _ => panic!("expected claude-interrupt command"),
    }
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p niuma-cli parses_claude
```

Expected: fails because Claude CLI variants do not exist.

- [ ] **Step 3: Add CLI variants**

In `cli.rs`, add:

```rust
Claude(ClaudeCommand),
ClaudeSessions,
ClaudeSend(ClaudeSendCommand),
ClaudeInterrupt(ClaudeInterruptCommand),
```

Add structs:

```rust
#[derive(Args)]
#[command(disable_help_flag = true, disable_version_flag = true)]
pub(crate) struct ClaudeCommand {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) args: Vec<String>,
}

#[derive(Args)]
pub(crate) struct ClaudeSendCommand {
    pub(crate) wrapper_session_id: String,
    #[arg(allow_hyphen_values = true)]
    pub(crate) message: String,
}

#[derive(Args)]
pub(crate) struct ClaudeInterruptCommand {
    pub(crate) wrapper_session_id: String,
}
```

Extend `ToolArg`:

```rust
ClaudeCode,
```

- [ ] **Step 4: Add command routing and module stubs**

In `tools/mod.rs`:

```rust
pub(crate) mod claude_code;
```

Create `tools/claude_code/mod.rs`:

```rust
pub(crate) mod interrupt;
pub(crate) mod managed;
pub(crate) mod send;
pub(crate) mod sessions;
```

In `main.rs`, route:

```rust
Command::Claude(command) => return tools::claude_code::managed::run_claude_command(command.args),
Command::ClaudeSessions => tools::claude_code::sessions::claude_sessions(),
Command::ClaudeSend(command) => {
    tools::claude_code::send::claude_send(command.wrapper_session_id, command.message)
}
Command::ClaudeInterrupt(command) => {
    tools::claude_code::interrupt::claude_interrupt(command.wrapper_session_id)
}
```

Stub modules return explicit unsupported responses until implementation steps fill them:

```rust
pub(crate) fn run_claude_command(_args: Vec<String>) -> ! {
    eprintln!("niuma claude 受管启动尚未实现");
    std::process::exit(2);
}
```

For `sessions/send/interrupt`, return the same `CommandOutput` type used by Codex commands with `success = false` and a clear Chinese message.

- [ ] **Step 5: Run tests and commit**

Run:

```bash
cargo test -p niuma-cli parses_claude
cargo test -p niuma-cli
```

Expected: CLI parse tests pass.

Commit:

```bash
git add crates/niuma-cli/src
git commit -m "feat: 新增 Claude Code CLI 命令入口" -m "修改内容：新增 niuma claude、claude-sessions、claude-send、claude-interrupt 的解析和路由入口。" -m "修改原因：为后续 Claude Code 受管启动和控制命令提供用户入口。"
```

## Task 8: Implement Managed Launch And Minimal Control

**Files:**

- Modify: `crates/niuma-cli/src/tools/claude_code/managed.rs`
- Modify: `crates/niuma-cli/src/tools/claude_code/sessions.rs`
- Modify: `crates/niuma-cli/src/tools/claude_code/send.rs`
- Modify: `crates/niuma-cli/src/tools/claude_code/interrupt.rs`
- Modify: `builtin-plugins/claude-code-runtime/src/claude/session_repository.rs`
- Test: `crates/niuma-cli/src/tools/claude_code/*.rs`
- Test: `builtin-plugins/claude-code-runtime/src/tests.rs`

- [ ] **Step 1: Add managed launch tests**

Add tests mirroring Codex app control at smaller scope:

```rust
#[test]
fn generated_wrapper_session_id_uses_claude_prefix() {
    let id = new_wrapper_session_id();

    assert!(id.starts_with("niuma_claude_"));
}

#[test]
fn real_claude_lookup_prefers_env_override() {
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("claude");
    std::fs::write(&fake, "#!/bin/sh\n").unwrap();
    std::env::set_var("NIUMA_REAL_CLAUDE", &fake);

    let resolved = resolve_real_claude().unwrap();

    std::env::remove_var("NIUMA_REAL_CLAUDE");
    assert_eq!(resolved, fake);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p niuma-cli claude
```

Expected: fails because managed helpers are still stubs.

- [ ] **Step 3: Implement managed launch**

Implement `run_claude_command(args)`:

- Generate `wrapper_session_id = niuma_claude_<uuid-simple>`.
- Generate `claude_session_id = Uuid::new_v4().to_string()`.
- Resolve real Claude with this order:
  1. `NIUMA_REAL_CLAUDE`
  2. first `claude` in `PATH` that is not the current executable
- Insert registry record with `state = Started`, cwd, pid after spawn, and generated session id.
- Spawn real Claude with original args plus `--session-id <uuid>` when args do not already contain `--session-id`.
- Wait for child exit and update registry state to `Exited`.

Add a comment near argument injection:

```rust
// 明确传入 session id，provider 绑定时不需要从目录名猜测会话身份。
```

- [ ] **Step 4: Implement sessions/send/interrupt**

Implement:

- `claude-sessions`: read `claude_code_managed_registry_path()` and print sessions like Codex sessions.
- `claude-send`: validate prefix `niuma_claude_`; first version supports exited/idle resume by running `claude --resume <session_id> <message>`.
- `claude-interrupt`: validate prefix and send interrupt only when a control socket or child pid exists; if unsupported, return a business failure message instead of pretending success.

Do not implement silent permission approval in this task.

- [ ] **Step 5: Add provider control binding**

In Claude repository snapshot building:

- Read managed registry.
- If list item session id equals `managed.claude_session_id`, attach control channel:

```rust
ToolSessionControlChannel {
    id: managed_claude_code_channel_id(&managed.wrapper_session_id),
    provider: "niuma_claude".to_string(),
    kind: "managed_process".to_string(),
    available: managed.state == ManagedClaudeCodeSessionState::Bound
        || managed.state == ManagedClaudeCodeSessionState::Started,
    capabilities: vec!["send_instruction".to_string(), "interrupt".to_string()],
    actions: vec![
        ToolSessionControlAction {
            action_type: "send_instruction".to_string(),
            transport: "local_api".to_string(),
            endpoint: Some("/api/v1/session-control/send-instruction".to_string()),
            debug_command: None,
        },
        ToolSessionControlAction {
            action_type: "interrupt".to_string(),
            transport: "local_api".to_string(),
            endpoint: Some("/api/v1/session-control/interrupt".to_string()),
            debug_command: None,
        },
    ],
    unavailable_reason: None,
    updated_at: Utc::now(),
}
```

- [ ] **Step 6: Run tests and commit**

Run:

```bash
cargo test -p niuma-cli claude
cargo test -p niuma-claude-code-plugin-runtime control
cargo test -p niuma-core managed_claude
```

Expected: tests pass.

Commit:

```bash
git add crates/niuma-cli/src/tools/claude_code builtin-plugins/claude-code-runtime crates/niuma-core/src/claude_code_managed_session.rs
git commit -m "feat: 支持 Claude Code 受管启动和基础控制" -m "修改内容：实现 niuma claude 受管启动、managed registry 更新、会话列表、resume 发送和中断能力边界。" -m "修改原因：在 watcher-first 基础上提供与 Codex 同级的受管控制入口。"
```

## Task 9: End-To-End Verification

**Files:**

- No required source changes unless verification exposes defects.

- [ ] **Step 1: Run unit tests**

Run:

```bash
cargo test -p niuma-core
cargo test -p niuma-cli
cargo test -p niuma-claude-code-plugin-runtime
```

Expected: all pass.

- [ ] **Step 2: Build runtime binaries**

Run:

```bash
cargo build -p niuma-claude-code-plugin-runtime
cargo build -p niuma-cli
```

Expected: both pass.

- [ ] **Step 3: Manual watcher verification without hooks**

Run:

```bash
NIUMA_CLAUDE_CODE_PLUGIN_COMMAND=target/debug/niuma-claude-code-plugin cargo run -p niuma-cli -- status claude-code
```

Expected: command does not crash. If status command does not yet support `claude-code`, update CLI status routing in a separate focused commit.

Then run a native Claude session without hooks:

```bash
claude --session-id 77777777-7777-4777-8777-777777777777 "只回复 ok"
```

Expected:

- A JSONL file appears under `~/.claude/projects/**/77777777-7777-4777-8777-777777777777.jsonl`.
- Claude Code provider snapshot contains that session.
- Detail provider returns at least one user and one assistant message.

- [ ] **Step 4: Manual pending tool verification**

Run a Claude command that uses Bash and takes at least 5 seconds:

```bash
claude --session-id 88888888-8888-4888-8888-888888888888 "运行 Bash: sleep 5 && echo done"
```

Expected:

- During execution, watcher sees `tool_use` before `tool_result`.
- UI/session detail shows running or pending tool, not actionable approval.
- After completion, pending state clears from detail/provider output.

- [ ] **Step 5: Final commit for verification fixes**

Only if verification required fixes:

```bash
git add <changed-files>
git commit -m "fix: 修复 Claude Code 集成验证问题" -m "修改内容：修复端到端验证中发现的 Claude Code watcher/provider 问题。" -m "修改原因：保证无 hooks 会话和工具调用事件流符合设计预期。"
```

If no fixes were needed, do not create an empty commit.

## Self-Review Checklist

- Spec coverage:
  - Watcher-first baseline: Task 2, Task 3, Task 4, Task 9.
  - No hooks dependency: Task 4 and Task 9 manual verification explicitly use native Claude without hooks.
  - Session list/detail provider: Task 3.
  - Builtin plugin: Task 1 and Task 5.
  - Managed control: Task 6, Task 7, Task 8.
  - API envelope constraints: no new backend route is introduced; existing provider RPC and session-control routes are reused.
- Placeholder scan:
  - This plan avoids open-ended implementation notes; each task has concrete files, test commands, expected results, and commit messages.
- Type consistency:
  - Public tool id is `ToolKind::ClaudeCode`, serialized as `claude_code`.
  - Plugin id is `builtin-claude-code`.
  - Binary name is `niuma-claude-code-plugin`.
  - Wrapper session id prefix is `niuma_claude_`.
  - Control channel prefix is `niuma_claude_managed:`.
