# Tool Session Reader Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build independent tool session list/detail provider support while renaming Niuma runtime state away from session terminology.

**Architecture:** Keep reader plugins isolated from provider plugins: readers call only the host Local API, while provider plugins communicate only with the host over stdio JSON Lines RPC. The host stores session snapshots, routes detail requests to the single registered provider for a tool, and returns the standard API envelope.

**Tech Stack:** Rust workspace (`niuma-core`, `niuma-api`, `src-tauri`, `builtin-plugins/codex-runtime`), Axum Local API, Tauri command fallback, stdio JSON Lines provider protocol, TypeScript frontend.

---

## Scope And Sequencing

This plan implements the spec in testable slices:

1. Rename existing runtime state types and API.
2. Extend plugin capability parsing and uniqueness validation.
3. Add shared tool session models and host in-memory registry.
4. Add Local API routes for `session_list` and `session_detail`.
5. Add stdio JSON Lines provider runtime support.
6. Add independent Codex session provider binary and parser.
7. Update frontend labels, docs, and integration tests.

Each task includes a verification command and a commit point. Do not start a later task until the current task's tests pass.

## File Structure

- `crates/niuma-core/src/models.rs`: rename `NiumaSession` to `RuntimeStateItem`, rename `SessionStatus` to `RuntimeStateStatus`, keep serialized status values unchanged.
- `crates/niuma-core/src/store.rs`: rename public runtime-state accessors while keeping the internal vector field migration explicit.
- `crates/niuma-core/src/store/transitions.rs`: update transition functions to operate on `RuntimeStateItem`.
- `crates/niuma-core/src/main_state.rs`: update main-state derivation to use runtime-state naming.
- `crates/niuma-core/src/event_display.rs`: update status helper names.
- `crates/niuma-core/src/dashboard.rs`: expose `runtime_state_list()`.
- `crates/niuma-core/src/tool_session.rs`: create shared session snapshot, detail, message, and provider RPC DTOs.
- `crates/niuma-core/src/plugin.rs`: add new capabilities and global provider uniqueness validation.
- `crates/niuma-api/src/handlers.rs`: rename runtime-state handler and add `session_list`/`session_detail`.
- `crates/niuma-api/src/routes.rs`: replace `/api/v1/sessions` with `/api/v1/runtime_state_list`, add `/api/v1/session_list`, `/api/v1/session_detail`.
- `crates/niuma-api/src/state.rs`: add shared `ToolSessionRegistry`.
- `crates/niuma-api/src/tool_sessions.rs`: create API-facing registry/query/filter/detail logic.
- `src-tauri/src/tools/plugin_runtime.rs`: include session provider plugins in managed runtimes and route stdio RPC.
- `src-tauri/src/main.rs`: configure the new Codex session provider binary and pass the shared registry into Local API startup.
- `src/api.ts`, `src/statusView.ts`, `src/i18n.ts`, `src/settingsView.ts`: update frontend types, calls, labels, and capability text.
- `builtin-plugins/codex/plugin.json`: rename watcher manifest to watcher semantics or keep id stable with watcher-only capability during migration.
- `builtin-plugins/codex-session-provider/plugin.json`: create independent session provider manifest.
- `builtin-plugins/codex-runtime/src/session_provider.rs`: create Codex session provider runtime.
- `builtin-plugins/codex-runtime/src/session_messages.rs`: create Codex JSONL-to-message parser and indexer.
- `builtin-plugins/codex-runtime/Cargo.toml`: add `niuma-codex-session-provider` binary.
- `docs/integration/plugin-development.md`, `docs/integration/plugin-development_zh.md`: update public API and capability docs.

---

### Task 1: Rename Runtime State Backend Types And API

**Files:**
- Modify: `crates/niuma-core/src/models.rs`
- Modify: `crates/niuma-core/src/store.rs`
- Modify: `crates/niuma-core/src/store/transitions.rs`
- Modify: `crates/niuma-core/src/main_state.rs`
- Modify: `crates/niuma-core/src/event_display.rs`
- Modify: `crates/niuma-core/src/dashboard.rs`
- Modify: `crates/niuma-api/src/handlers.rs`
- Modify: `crates/niuma-api/src/routes.rs`
- Modify: `crates/niuma-api/src/tests.rs`

- [ ] **Step 1: Write failing API test for the renamed route**

Add this test in `crates/niuma-api/src/tests.rs` next to the existing session list test:

```rust
#[tokio::test]
async fn runtime_state_list_returns_standard_list_envelope() {
    let store = NiumaStore::new(test_path("runtime_state_list"));
    store
        .append_events(vec![event_with_session("runtime-session")])
        .unwrap();
    let router = app(store);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/runtime_state_list")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = read_json(response).await;
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["list"][0]["session_id"], "runtime-session");
    assert!(body["data"]["list"][0].get("id").is_none());
}

#[tokio::test]
async fn old_sessions_route_is_removed() {
    let router = app(NiumaStore::new(test_path("old_sessions_route_removed")));

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_json(response).await;
    assert_eq!(body["code"], 900005);
}
```

- [ ] **Step 2: Run the failing API tests**

Run:

```bash
cargo test -p niuma-api runtime_state_list_returns_standard_list_envelope old_sessions_route_is_removed
```

Expected: `runtime_state_list_returns_standard_list_envelope` fails because the route does not exist or still returns `id`; `old_sessions_route_is_removed` fails while `/api/v1/sessions` still exists.

- [ ] **Step 3: Rename the core model types**

In `crates/niuma-core/src/models.rs`, replace the old runtime-state types with:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStateStatus {
    Idle,
    Running,
    WaitingApproval,
    WaitingInput,
    Completed,
    Error,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeStateItem {
    pub tool: ToolKind,
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub status: RuntimeStateStatus,
    pub last_event_id: Option<String>,
    pub last_activity_at: DateTime<Utc>,
}
```

Update `AttentionItem`, `LatestActivity`, and `InternalStateSnapshot` to use `RuntimeStateStatus`. Keep their field names unchanged where they already describe status, not a session entity.

- [ ] **Step 4: Update state transition construction**

In `crates/niuma-core/src/store/transitions.rs`, rename `upsert_session` to `upsert_runtime_state` and construct `RuntimeStateItem` with `session_id`:

```rust
pub(super) fn upsert_runtime_state(states: &mut Vec<RuntimeStateItem>, event: &NiumaEvent) {
    let status = status_from_event(&event.event_type);
    if let Some(state) = states
        .iter_mut()
        .find(|item| item.tool == event.tool && item.session_id == event.session_id)
    {
        apply_runtime_state_update(state, event, status);
        return;
    }

    states.push(RuntimeStateItem {
        tool: event.tool.clone(),
        session_id: event.session_id.clone(),
        project_path: event.project_path.clone(),
        project_name: event.project_name.clone(),
        status,
        last_event_id: Some(event.id.clone()),
        last_activity_at: event.created_at,
    });
}
```

Rename helper arguments from `sessions` to `runtime_states` in this file. Keep behavior the same.

- [ ] **Step 5: Rename store and dashboard accessors**

In `crates/niuma-core/src/store.rs`, rename the public accessor:

```rust
pub fn runtime_state_list(&self) -> Result<Vec<RuntimeStateItem>, String> {
    Ok(self.load()?.runtime_states)
}
```

Update `NiumaStoreState` to use:

```rust
pub runtime_states: Vec<RuntimeStateItem>,
```

Do not add a serde alias for the old `sessions` field. This is a hard cut: old runtime-state snapshots can be rebuilt from new events.

In `crates/niuma-core/src/dashboard.rs`, expose:

```rust
pub fn runtime_state_list(&self) -> Result<Vec<RuntimeStateItem>, String> {
    self.store.runtime_state_list()
}
```

- [ ] **Step 6: Rename API handler and route**

In `crates/niuma-api/src/handlers.rs`, replace `get_sessions` with:

```rust
pub(crate) async fn get_runtime_state_list(State(state): State<AppState>) -> Response {
    match DashboardService::new(state.store).runtime_state_list() {
        Ok(items) => json_response(200, ApiResponse::ok(json!({ "list": items }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}
```

In `crates/niuma-api/src/routes.rs`, replace:

```rust
.route("/api/v1/sessions", get(get_sessions).options(preflight))
```

with:

```rust
.route(
    "/api/v1/runtime_state_list",
    get(get_runtime_state_list).options(preflight),
)
```

- [ ] **Step 7: Update compile errors from renamed Rust types**

Use targeted replacements:

```bash
rg -n "NiumaSession|SessionStatus|\\.sessions\\b|sessions\\(" crates/niuma-core crates/niuma-api builtin-plugins
```

Apply these naming conversions:

```text
NiumaSession -> RuntimeStateItem
SessionStatus -> RuntimeStateStatus
state.sessions -> state.runtime_states
store.sessions() -> store.runtime_state_list()
```

For user-facing event names such as `EventType::SessionStarted`, keep existing names because they describe tool events, not the Niuma runtime-state model.

- [ ] **Step 8: Run Rust tests for backend rename**

Run:

```bash
cargo test -p niuma-core -p niuma-api
```

Expected: all tests pass.

- [ ] **Step 9: Commit runtime-state backend rename**

```bash
git add crates/niuma-core crates/niuma-api
git commit -m "refactor: 重命名运行态模型和接口" -m "修改内容：将 NiumaSession/SessionStatus 改为 RuntimeStateItem/RuntimeStateStatus，并将 /api/v1/sessions 改为 /api/v1/runtime_state_list。" -m "修改原因：运行态不是工具 session，重命名可以避免与后续工具会话列表和详情接口混淆。"
```

---

### Task 2: Rename Runtime State Frontend Types And Calls

**Files:**
- Modify: `src/api.ts`
- Modify: `src/statusView.ts`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: frontend tests under `tests/`

- [ ] **Step 1: Write failing frontend API expectation**

In `tests/statusSummaryRender.test.ts`, add this assertion:

```ts
import { describe, expect, it } from 'vitest'
import type { RuntimeStateItem } from '../src/api'

describe('RuntimeStateItem API type', () => {
  it('uses session_id instead of id for runtime state correlation', () => {
    const item: RuntimeStateItem = {
      tool: 'codex',
      session_id: 'session-1',
      project_path: '/repo',
      project_name: 'repo',
      status: 'running',
      last_event_id: 'event-1',
      last_activity_at: '2026-06-22T08:00:00Z'
    }

    expect(item.session_id).toBe('session-1')
    expect('id' in item).toBe(false)
  })
})
```

- [ ] **Step 2: Run the failing frontend test**

Run:

```bash
npm test -- statusSummaryRender.test.ts
```

Expected: fails because `RuntimeStateItem` is not exported yet or code still uses `id`.

- [ ] **Step 3: Update frontend API types and fetch path**

In `src/api.ts`, replace `NiumaSession`/`SessionsPayload` with:

```ts
export type RuntimeStateItem = {
  tool: string
  session_id: string
  project_path: string
  project_name: string
  status: string
  last_event_id: string | null
  last_activity_at: string
}

export type RuntimeStateListPayload = {
  list: RuntimeStateItem[]
}
```

Update `refreshSupplementaryData()` and rename the returned field to `runtimeStates`:

```ts
const [runtimeStates, events] = await Promise.all([
  requestLocalApi<RuntimeStateListPayload>('/api/v1/runtime_state_list'),
  requestLocalApi<RecentEvents>('/api/v1/events?limit=10')
])
return {
  runtimeStates: runtimeStates.list,
  events: events.list
}
```

- [ ] **Step 4: Update status view selection logic**

In `src/statusView.ts`, change imports and local names:

```ts
import type { ListenerToolConfig, MainStatePayload, NiumaEvent, RuntimeStateItem } from './api'
```

Replace the session selector id logic with `session_id`:

```ts
function preferredRuntimeStateId(
  runtimeStates: RuntimeStateItem[],
  currentId: string | null,
  primarySessionId: string | null
) {
  if (currentId && runtimeStates.some((state) => state.session_id === currentId)) {
    return currentId
  }
  if (primarySessionId && runtimeStates.some((state) => state.session_id === primarySessionId)) {
    return primarySessionId
  }
  return sortedRuntimeStatesByLatestActivity(runtimeStates)[0]?.session_id ?? null
}
```

Update lookup:

```ts
const state = options.runtimeStates.find((item) => item.session_id === selectedSessionId)
```

- [ ] **Step 5: Rename Tauri command fallback**

In `src-tauri/src/commands.rs`, rename `get_sessions` to `get_runtime_state_list` and return the renamed payload. In `src-tauri/src/main.rs`, update `tauri::generate_handler!`:

```rust
commands::get_runtime_state_list,
```

In `src/api.ts`, update the fallback invoke call:

```ts
invoke<ApiResponse<RuntimeStateListPayload>>('get_runtime_state_list')
```

- [ ] **Step 6: Run frontend and Tauri compile checks**

Run:

```bash
npm test
cargo test -p niuma-core -p niuma-api
```

Expected: all tests pass. If Tauri commands are covered only by compile, run:

```bash
cargo check -p niuma-notifier
```

Expected: check passes.

- [ ] **Step 7: Commit frontend runtime-state rename**

```bash
git add src src-tauri tests
git commit -m "refactor: 前端改用运行态命名" -m "修改内容：将前端 NiumaSession 类型、Local API 路径和 Tauri fallback 调用改为 RuntimeStateItem 与 runtime_state_list。" -m "修改原因：前端展示的是 Niuma 运行态，不应继续使用 session 命名。"
```

---

### Task 3: Add Plugin Capabilities And Provider Uniqueness Validation

**Files:**
- Modify: `crates/niuma-core/src/plugin.rs`
- Modify: `builtin-plugins/codex/plugin.json`
- Add: `builtin-plugins/codex-session-provider/plugin.json`
- Modify: `crates/niuma-api/src/tests.rs`
- Modify: `src/i18n.ts`
- Modify: `src/settingsView.ts`
- Modify: `tests/settingsViewRender.test.ts`

- [ ] **Step 1: Write failing manifest tests**

Add tests in `crates/niuma-core/src/plugin.rs` test module:

```rust
#[test]
fn parses_tool_session_provider_capabilities() {
    let manifest = parse_plugin_manifest(
        r#"{
            "id": "codex-session-provider",
            "kind": "tool",
            "tool_id": "codex",
            "display_name": "Codex Session Provider",
            "version": "0.1.0",
            "command": "niuma-codex-session-provider",
            "capabilities": ["tool_session_list_provider", "tool_session_detail_provider"]
        }"#,
    )
    .unwrap();

    assert_eq!(
        manifest.capabilities,
        vec![
            PluginCapability::ToolSessionListProvider,
            PluginCapability::ToolSessionDetailProvider
        ]
    );
}

#[test]
fn rejects_detail_provider_without_list_provider() {
    let error = parse_plugin_manifest(
        r#"{
            "id": "broken-session-provider",
            "kind": "tool",
            "tool_id": "codex",
            "display_name": "Broken",
            "version": "0.1.0",
            "command": "broken",
            "capabilities": ["tool_session_detail_provider"]
        }"#,
    )
    .unwrap_err();

    assert!(error.contains("tool_session_detail_provider 必须同时声明 tool_session_list_provider"));
}

#[test]
fn rejects_provider_capability_on_non_tool_plugin() {
    let error = parse_plugin_manifest(
        r#"{
            "id": "broken-reader",
            "kind": "notification",
            "display_name": "Broken Reader",
            "version": "0.1.0",
            "command": "broken",
            "capabilities": ["tool_session_list_provider"]
        }"#,
    )
    .unwrap_err();

    assert!(error.contains("非工具插件不能声明 provider capability"));
}
```

- [ ] **Step 2: Run failing manifest tests**

Run:

```bash
cargo test -p niuma-core parses_tool_session_provider_capabilities rejects_detail_provider_without_list_provider rejects_provider_capability_on_non_tool_plugin
```

Expected: fails because enum variants and validation do not exist.

- [ ] **Step 3: Add capability enum variants**

In `crates/niuma-core/src/plugin.rs`, extend `PluginCapability`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    EventWatcher,
    EventConsumer,
    ApprovalHandler,
    NotificationTest,
    StateConsumer,
    ToolSessionListProvider,
    ToolSessionDetailProvider,
    ToolSessionListReader,
    ToolSessionDetailReader,
}
```

Add helpers:

```rust
fn is_provider_capability(capability: &PluginCapability) -> bool {
    matches!(
        capability,
        PluginCapability::EventWatcher
            | PluginCapability::ToolSessionListProvider
            | PluginCapability::ToolSessionDetailProvider
    )
}
```

- [ ] **Step 4: Add manifest validation**

In `validate_plugin_manifest`, replace the event-watcher-only non-tool check with:

```rust
if manifest.kind != PluginKind::Tool
    && manifest.capabilities.iter().any(is_provider_capability)
{
    return Err(format!(
        "非工具插件不能声明 provider capability：{}",
        manifest.id
    ));
}
if manifest
    .capabilities
    .contains(&PluginCapability::ToolSessionDetailProvider)
    && !manifest
        .capabilities
        .contains(&PluginCapability::ToolSessionListProvider)
{
    return Err(format!(
        "tool_session_detail_provider 必须同时声明 tool_session_list_provider：{}",
        manifest.id
    ));
}
```

- [ ] **Step 5: Add registry-level uniqueness validation**

Add this method to `PluginRegistry`:

```rust
pub fn validate_provider_uniqueness(&self) -> Result<(), String> {
    let mut seen = BTreeMap::<(ToolKind, PluginCapability), String>::new();
    for manifest in &self.manifests {
        let Some(tool_id) = manifest.tool_id.clone() else {
            continue;
        };
        for capability in manifest.capabilities.iter().filter(|item| is_provider_capability(item))
        {
            let key = (tool_id.clone(), capability.clone());
            if let Some(existing_id) = seen.get(&key) {
                return Err(format!(
                    "同一工具 {} 的 provider capability {:?} 已由插件 {} 声明，不能再由插件 {} 声明",
                    tool_id.as_str(),
                    capability,
                    existing_id,
                    manifest.id
                ));
            }
            seen.insert(key, manifest.id.clone());
        }
    }
    Ok(())
}
```

Validate each candidate before registration by checking it against the registry's current provider index. Built-in conflicts must panic during development. External plugin conflicts must skip that external plugin and log a clear error that names the existing plugin id, rejected plugin id, `tool_id`, and capability.

- [ ] **Step 6: Add session provider manifest**

Create `builtin-plugins/codex-session-provider/plugin.json`:

```json
{
  "id": "builtin-codex-session-provider",
  "kind": "tool",
  "tool_id": "codex",
  "display_name": "Codex Session Provider",
  "version": "0.1.0",
  "command": "niuma-codex-session-provider",
  "args": [],
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["tool_session_list_provider", "tool_session_detail_provider"],
  "icon_url": "/assets/codex-icon.png",
  "source": "builtin"
}
```

Add an include string and `builtin_codex_session_provider_manifest()` in `plugin.rs`. Register it in `PluginRegistry::with_builtin_plugins()`.

- [ ] **Step 7: Update plugin capability UI labels**

In `src/i18n.ts`, add translation keys for all supported languages. For `zh-CN` use:

```ts
pluginCapabilityToolSessionListProvider: '提供 AI 会话列表',
pluginCapabilityToolSessionDetailProvider: '提供 AI 会话解析',
pluginCapabilityToolSessionListReader: '读取 AI 会话列表',
pluginCapabilityToolSessionDetailReader: '可读取 AI 会话内容'
```

In `src/settingsView.ts`, extend `translatePluginCapability`:

```ts
if (capability === 'tool_session_list_provider') {
  return t.pluginCapabilityToolSessionListProvider
}
if (capability === 'tool_session_detail_provider') {
  return t.pluginCapabilityToolSessionDetailProvider
}
if (capability === 'tool_session_list_reader') {
  return t.pluginCapabilityToolSessionListReader
}
if (capability === 'tool_session_detail_reader') {
  return t.pluginCapabilityToolSessionDetailReader
}
```

Keep the first implementation text-only. Do not add a new sensitive badge style in this task because the existing plugin capability UI has no separate badge pattern.

- [ ] **Step 8: Run capability tests**

Run:

```bash
cargo test -p niuma-core plugin
npm test -- settingsViewRender.test.ts
```

Expected: all tests pass.

- [ ] **Step 9: Commit capability changes**

```bash
git add crates/niuma-core builtin-plugins src tests
git commit -m "feat: 新增工具会话插件能力" -m "修改内容：新增 tool_session provider/reader capability，增加 provider 唯一性校验，并补充插件管理页能力文案。" -m "修改原因：工具会话读取需要独立于 event_watcher 的 provider 能力，并且同一工具不能存在多个同类上报能力。"
```

---

### Task 4: Add Shared Tool Session Models And Registry

**Files:**
- Add: `crates/niuma-core/src/tool_session.rs`
- Modify: `crates/niuma-core/src/lib.rs`
- Add: `crates/niuma-api/src/tool_sessions.rs`
- Modify: `crates/niuma-api/src/lib.rs`
- Modify: `crates/niuma-api/src/state.rs`
- Modify: `crates/niuma-api/src/tests.rs`

- [ ] **Step 1: Write failing registry tests**

Add tests in `crates/niuma-api/src/tests.rs`:

```rust
#[tokio::test]
async fn session_list_filters_snapshot_items() {
    let registry = ToolSessionRegistry::default();
    registry.replace_snapshot(
        "codex",
        vec![
            test_tool_session_item("codex", "main", false, true),
            test_tool_session_item("codex", "sub", true, true),
            test_tool_session_item("codex", "old", false, false),
        ],
    );

    let result = registry
        .list(ToolSessionListQuery {
            tool: Some("codex".to_string()),
            include_subagents: Some(false),
            active_only: Some(true),
            limit: Some(100),
        })
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].session_id, "main");
}
```

Expected helper signatures:

```rust
fn test_tool_session_item(
    tool: &str,
    session_id: &str,
    is_subagent: bool,
    is_active: bool,
) -> ToolSessionListItem
```

- [ ] **Step 2: Run failing registry test**

Run:

```bash
cargo test -p niuma-api session_list_filters_snapshot_items
```

Expected: fails because models and registry do not exist.

- [ ] **Step 3: Add shared models**

Create `crates/niuma-core/src/tool_session.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::ToolKind;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSessionStatus {
    Active,
    Inactive,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolSessionListItem {
    pub id: String,
    pub tool: ToolKind,
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub file_path: String,
    pub modified_at: DateTime<Utc>,
    pub discovered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub is_active: bool,
    pub is_subagent: bool,
    pub parent_session_id: Option<String>,
    pub status: ToolSessionStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSessionMessageRole {
    User,
    Assistant,
    System,
    ToolCall,
    ToolResult,
    Event,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSessionMessage {
    pub id: String,
    pub role: ToolSessionMessageRole,
    pub content: String,
    pub created_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSessionDetail {
    pub tool: ToolKind,
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub is_subagent: bool,
    pub parent_session_id: Option<String>,
    pub messages: Vec<ToolSessionMessage>,
    pub next_cursor: Option<String>,
}
```

Expose it in `crates/niuma-core/src/lib.rs`:

```rust
pub mod tool_session;
```

- [ ] **Step 4: Add API registry**

Create `crates/niuma-api/src/tool_sessions.rs`:

```rust
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use niuma_core::models::ToolKind;
use niuma_core::tool_session::{ToolSessionDetail, ToolSessionListItem};

#[derive(Clone, Debug, Default)]
pub struct ToolSessionRegistry {
    snapshots: Arc<Mutex<BTreeMap<String, Vec<ToolSessionListItem>>>>,
}

#[derive(Clone, Debug, Default)]
pub struct ToolSessionListQuery {
    pub tool: Option<String>,
    pub include_subagents: Option<bool>,
    pub active_only: Option<bool>,
    pub limit: Option<usize>,
}

impl ToolSessionRegistry {
    pub fn replace_snapshot(&self, tool: &str, sessions: Vec<ToolSessionListItem>) {
        self.snapshots
            .lock()
            .expect("tool session registry mutex poisoned")
            .insert(tool.to_string(), sessions);
    }

    pub fn list(&self, query: ToolSessionListQuery) -> Result<Vec<ToolSessionListItem>, String> {
        let tool = query.tool.as_deref().unwrap_or("all").trim();
        let include_subagents = query.include_subagents.unwrap_or(false);
        let active_only = query.active_only.unwrap_or(false);
        let limit = query.limit.unwrap_or(100).min(500);
        if limit == 0 {
            return Err("limit 必须大于 0".to_string());
        }

        let snapshots = self
            .snapshots
            .lock()
            .expect("tool session registry mutex poisoned");
        let mut list = Vec::new();
        for (snapshot_tool, sessions) in snapshots.iter() {
            if tool != "all" && snapshot_tool != tool {
                continue;
            }
            list.extend(sessions.iter().cloned());
        }
        list.retain(|item| include_subagents || !item.is_subagent);
        if active_only {
            list.retain(|item| item.is_active);
        }
        list.sort_by(|left, right| {
            right
                .last_seen_at
                .cmp(&left.last_seen_at)
                .then_with(|| right.modified_at.cmp(&left.modified_at))
        });
        list.truncate(limit);
        Ok(list)
    }

    pub fn find_session(&self, tool: &ToolKind, session_id: &str) -> Option<ToolSessionListItem> {
        self.snapshots
            .lock()
            .expect("tool session registry mutex poisoned")
            .get(tool.as_str())
            .and_then(|items| items.iter().find(|item| item.session_id == session_id))
            .cloned()
    }

    pub fn cache_detail_page(&self, _detail: &ToolSessionDetail) {
        // Keep the initial registry focused on snapshots. Page cache is added with provider RPC.
    }
}
```

- [ ] **Step 5: Wire registry into API state**

In `crates/niuma-api/src/lib.rs`, expose the module:

```rust
mod tool_sessions;
pub use tool_sessions::ToolSessionRegistry;
```

In `crates/niuma-api/src/state.rs`, add:

```rust
pub(crate) tool_sessions: crate::tool_sessions::ToolSessionRegistry,
```

Update router constructors in `routes.rs` to accept or create a default registry. Add an overload:

```rust
pub fn app_with_bus_plugin_dir_and_tool_sessions(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    plugin_dir: PathBuf,
    tool_sessions: ToolSessionRegistry,
) -> Router
```

Have existing constructors call this with `ToolSessionRegistry::default()`.

- [ ] **Step 6: Run registry tests**

Run:

```bash
cargo test -p niuma-api session_list_filters_snapshot_items
```

Expected: pass.

- [ ] **Step 7: Commit shared model and registry**

```bash
git add crates/niuma-core crates/niuma-api
git commit -m "feat: 新增工具会话共享模型和注册表" -m "修改内容：新增工具 session 列表、详情和消息模型，并在 Local API 状态中加入工具会话 snapshot 注册表。" -m "修改原因：session_list 和 session_detail 需要宿主保存 provider 上报的统一会话视图。"
```

---

### Task 5: Add Session List And Detail Local API Routes With Fake Provider

**Files:**
- Modify: `crates/niuma-api/src/tool_sessions.rs`
- Modify: `crates/niuma-api/src/handlers.rs`
- Modify: `crates/niuma-api/src/routes.rs`
- Modify: `crates/niuma-api/src/response.rs`
- Modify: `crates/niuma-api/src/tests.rs`

- [ ] **Step 1: Write failing API route tests**

Add tests in `crates/niuma-api/src/tests.rs`:

```rust
#[tokio::test]
async fn session_list_returns_snapshot_with_filters() {
    let registry = ToolSessionRegistry::default();
    registry.replace_snapshot(
        "codex",
        vec![
            test_tool_session_item("codex", "main", false, true),
            test_tool_session_item("codex", "sub", true, true),
        ],
    );
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_list_filters")),
        registry,
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_list?tool=codex&include_subagents=false&active_only=true&limit=100")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = read_json(response).await;
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["list"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["list"][0]["session_id"], "main");
}

#[tokio::test]
async fn session_detail_requires_existing_snapshot_session() {
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_missing")),
        ToolSessionRegistry::default(),
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_detail?tool=codex&session_id=missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = read_json(response).await;
    assert_eq!(body["code"], 100101);
    assert!(body["message"].as_str().unwrap().contains("session_id 不存在"));
}
```

- [ ] **Step 2: Run failing API route tests**

Run:

```bash
cargo test -p niuma-api session_list_returns_snapshot_with_filters session_detail_requires_existing_snapshot_session
```

Expected: fails because routes and helper constructor do not exist.

- [ ] **Step 3: Add query structs and handlers**

In `crates/niuma-api/src/handlers.rs`, add:

```rust
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionListQuery {
    tool: Option<String>,
    include_subagents: Option<bool>,
    active_only: Option<bool>,
    limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionDetailQuery {
    tool: Option<String>,
    session_id: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

pub(crate) async fn get_session_list(
    State(state): State<AppState>,
    Query(query): Query<SessionListQuery>,
) -> Response {
    let result = state.tool_sessions.list(ToolSessionListQuery {
        tool: query.tool,
        include_subagents: query.include_subagents,
        active_only: query.active_only,
        limit: query.limit,
    });
    match result {
        Ok(list) => json_response(200, ApiResponse::ok(json!({ "list": list }))),
        Err(message) => json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, message)),
    }
}

pub(crate) async fn get_session_detail(
    State(state): State<AppState>,
    Query(query): Query<SessionDetailQuery>,
) -> Response {
    let Some(tool_text) = query.tool.as_deref().map(str::trim).filter(|value| !value.is_empty()) else {
        return json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, "tool 不能为空"));
    };
    let Some(session_id) = query.session_id.as_deref().map(str::trim).filter(|value| !value.is_empty()) else {
        return json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, "session_id 不能为空"));
    };
    let tool = ToolKind::from_id(tool_text);
    if state.tool_sessions.find_session(&tool, session_id).is_none() {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("session_id 不存在：{session_id}"),
            ),
        );
    }
    json_response(
        200,
        ApiResponse::fail(ApiErrorCode::BusinessValidation, "session detail provider 尚未就绪"),
    )
}
```

The final detail implementation is added after provider RPC. This task establishes parameter and snapshot semantics.

- [ ] **Step 4: Add routes**

In `crates/niuma-api/src/routes.rs`, import and route:

```rust
get_session_detail, get_session_list,
```

```rust
.route("/api/v1/session_list", get(get_session_list).options(preflight))
.route("/api/v1/session_detail", get(get_session_detail).options(preflight))
```

- [ ] **Step 5: Ensure query parsing returns envelope**

If invalid booleans or invalid `limit` produce Axum default responses, add a custom extractor or route-level rejection handler. The expected envelope for `limit=abc` is:

```json
{
  "code": 100003,
  "message": "参数类型错误",
  "data": null
}
```

Add a focused test:

```rust
#[tokio::test]
async fn session_list_invalid_limit_returns_standard_400() {
    let router = app(NiumaStore::new(test_path("session_list_invalid_limit")));

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_list?limit=abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = read_json(response).await;
    assert_ne!(body["code"], 0);
    assert!(body.get("data").is_some());
}
```

- [ ] **Step 6: Run API tests**

Run:

```bash
cargo test -p niuma-api session_list session_detail
```

Expected: all session list/detail route tests pass.

- [ ] **Step 7: Commit API route skeleton**

```bash
git add crates/niuma-api
git commit -m "feat: 新增工具会话读取接口骨架" -m "修改内容：新增 /api/v1/session_list 和 /api/v1/session_detail，列表读取宿主 snapshot，详情先完成参数和 snapshot 校验。" -m "修改原因：第三方插件需要通过宿主 Local API 读取工具会话视图，后续 provider RPC 将接入详情数据来源。"
```

---

### Task 6: Add Stdio JSON Lines Provider Runtime

**Files:**
- Add: `crates/niuma-core/src/tool_session_rpc.rs`
- Modify: `crates/niuma-core/src/lib.rs`
- Modify: `src-tauri/src/tools/plugin_runtime.rs`
- Modify: `src-tauri/src/tools/mod.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `crates/niuma-api/src/tool_sessions.rs`
- Modify: `crates/niuma-api/src/state.rs`
- Add or modify tests in `src-tauri/src/tools/plugin_runtime.rs`

- [ ] **Step 1: Add shared RPC DTOs**

Create `crates/niuma-core/src/tool_session_rpc.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tool_session::{ToolSessionDetail, ToolSessionListItem};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderRpcRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRpcResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProviderRpcError>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderRpcError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRpcNotification {
    #[serde(rename = "type")]
    pub message_type: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshotParams {
    pub tool: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshotResult {
    pub tool: String,
    pub sessions: Vec<ToolSessionListItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionDetailParams {
    pub tool: String,
    pub session_id: String,
    pub limit: usize,
    pub cursor: Option<String>,
}

pub type SessionDetailResult = ToolSessionDetail;
```

Expose it in `crates/niuma-core/src/lib.rs`:

```rust
pub mod tool_session_rpc;
```

- [ ] **Step 2: Add provider client trait to registry**

In `crates/niuma-api/src/tool_sessions.rs`, add:

```rust
pub trait ToolSessionDetailProvider: Send + Sync {
    fn session_detail(
        &self,
        tool: &str,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<ToolSessionDetail, String>;
}
```

Extend `ToolSessionRegistry` with:

```rust
providers: Arc<Mutex<BTreeMap<String, Arc<dyn ToolSessionDetailProvider>>>>,
```

Add:

```rust
pub fn register_detail_provider(
    &self,
    tool: &str,
    provider: Arc<dyn ToolSessionDetailProvider>,
) {
    self.providers
        .lock()
        .expect("tool session provider mutex poisoned")
        .insert(tool.to_string(), provider);
}

pub fn detail(
    &self,
    tool: &str,
    session_id: &str,
    limit: usize,
    cursor: Option<String>,
) -> Result<ToolSessionDetail, String> {
    let provider = self
        .providers
        .lock()
        .expect("tool session provider mutex poisoned")
        .get(tool)
        .cloned()
        .ok_or_else(|| format!("工具不支持 session detail：{tool}"))?;
    provider.session_detail(tool, session_id, limit, cursor)
}
```

- [ ] **Step 3: Update `get_session_detail` to call registry detail**

In `crates/niuma-api/src/handlers.rs`, after snapshot existence check:

```rust
let limit = query.limit.unwrap_or(100).min(500);
if limit == 0 {
    return json_response(
        200,
        ApiResponse::fail(ApiErrorCode::BusinessValidation, "limit 必须大于 0"),
    );
}
match state
    .tool_sessions
    .detail(tool.as_str(), session_id, limit, query.cursor)
{
    Ok(detail) => json_response(200, ApiResponse::ok(detail)),
    Err(message) => json_response(200, ApiResponse::fail(ApiErrorCode::BusinessValidation, message)),
}
```

- [ ] **Step 4: Add stdio provider process wrapper**

In `src-tauri/src/tools/plugin_runtime.rs`, split managed process handling:

```rust
enum ManagedPluginRuntime {
    Plain(Child),
    SessionProvider(SessionProviderProcess),
}

struct SessionProviderProcess {
    child: Child,
    stdin: std::process::ChildStdin,
    pending: Arc<Mutex<HashMap<String, std::sync::mpsc::Sender<ProviderRpcResponse>>>>,
}
```

When spawning a manifest that has `ToolSessionListProvider`, use `stdout(Stdio::piped())` and `stdin(Stdio::piped())`. Keep logs on stderr.

- [ ] **Step 5: Read provider stdout lines**

Add a thread per session provider process:

```rust
fn spawn_provider_stdout_reader(
    plugin_id: String,
    tool: String,
    stdout: std::process::ChildStdout,
    registry: ToolSessionRegistry,
    pending: Arc<Mutex<HashMap<String, std::sync::mpsc::Sender<ProviderRpcResponse>>>>,
) {
    thread::Builder::new()
        .name(format!("session-provider-stdout-{plugin_id}"))
        .spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().flatten() {
                if let Ok(response) = serde_json::from_str::<ProviderRpcResponse>(&line) {
                    if let Some(sender) = pending
                        .lock()
                        .expect("provider pending mutex poisoned")
                        .remove(&response.id)
                    {
                        let _ = sender.send(response);
                    }
                    continue;
                }
                if let Ok(notification) =
                    serde_json::from_str::<ProviderRpcNotification>(&line)
                {
                    handle_provider_notification(&tool, notification, &registry);
                    continue;
                }
                eprintln!("NiumaNotifier session provider {plugin_id} emitted invalid JSON");
            }
        })
        .map_err(|error| {
            eprintln!("NiumaNotifier provider stdout reader not started: {error}");
        })
        .ok();
}
```

Implement `handle_provider_notification` to parse `session_snapshot_updated` params as `SessionSnapshotResult` and call `registry.replace_snapshot(&result.tool, result.sessions)`.

- [ ] **Step 6: Register detail provider adapter**

Create an adapter that implements `ToolSessionDetailProvider` by writing requests to provider stdin and waiting for a response with timeout:

```rust
impl ToolSessionDetailProvider for SessionProviderHandle {
    fn session_detail(
        &self,
        tool: &str,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<ToolSessionDetail, String> {
        let params = SessionDetailParams {
            tool: tool.to_string(),
            session_id: session_id.to_string(),
            limit,
            cursor,
        };
        let value = self.call("session_detail", serde_json::to_value(params).unwrap(), Duration::from_secs(10))?;
        serde_json::from_value(value).map_err(|error| format!("provider 返回详情格式无效：{error}"))
    }
}
```

The `call` method writes one JSON line:

```rust
let line = serde_json::to_string(&request)
    .map_err(|error| format!("序列化 provider 请求失败：{error}"))?;
writeln!(stdin, "{line}")
    .map_err(|error| format!("写入 provider 请求失败：{error}"))?;
```

- [ ] **Step 7: Pull snapshot after provider startup**

After starting a session provider, call:

```rust
handle.session_snapshot(tool.as_str(), Duration::from_secs(5))
```

On success, call `registry.replace_snapshot(tool.as_str(), result.sessions)`. On failure, save plugin runtime state as failed with the provider error.

- [ ] **Step 8: Run runtime checks**

Run:

```bash
cargo test -p niuma-core -p niuma-api
cargo check -p niuma-notifier
```

Expected: all tests and checks pass.

- [ ] **Step 9: Commit provider runtime**

```bash
git add crates/niuma-core crates/niuma-api src-tauri
git commit -m "feat: 新增 session provider 进程协议" -m "修改内容：新增 stdio JSON Lines provider RPC DTO、工具会话详情 provider 注册表，并让插件管理器支持 session provider 进程。" -m "修改原因：session_detail 需要由独立 provider 插件按需返回归一化消息，同时保持 reader 插件只调用宿主 API。"
```

---

### Task 7: Implement Independent Codex Session Provider

**Files:**
- Modify: `builtin-plugins/codex-runtime/Cargo.toml`
- Add: `builtin-plugins/codex-runtime/src/session_provider.rs`
- Add: `builtin-plugins/codex-runtime/src/session_messages.rs`
- Modify: `builtin-plugins/codex-runtime/src/lib.rs`
- Add: `builtin-plugins/codex-runtime/src/session_provider_tests.rs` or extend `src/tests.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `crates/niuma-core/src/plugin.rs`

- [ ] **Step 1: Write failing Codex message parser test**

Add a test in `builtin-plugins/codex-runtime/src/session_messages.rs`:

```rust
#[test]
fn codex_messages_are_returned_newest_first_without_raw_payload() {
    let lines = vec![
        r#"{"timestamp":"2026-06-22T08:00:00Z","type":"session_meta","payload":{"id":"session-1","cwd":"/repo"}}"#,
        r#"{"timestamp":"2026-06-22T08:01:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"第一条"}]}}"#,
        r#"{"timestamp":"2026-06-22T08:02:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"第二条"}]}}"#,
    ];

    let messages = parse_codex_messages_from_lines("session-1", &lines).unwrap();

    assert_eq!(messages[0].role, ToolSessionMessageRole::Assistant);
    assert_eq!(messages[0].content, "第二条");
    assert_eq!(messages[1].role, ToolSessionMessageRole::User);
    assert_eq!(messages[1].content, "第一条");
    let serialized = serde_json::to_string(&messages).unwrap();
    assert!(!serialized.contains("\"payload\""));
    assert!(!serialized.contains("raw_line"));
}
```

- [ ] **Step 2: Run failing parser test**

Run:

```bash
cargo test -p niuma-codex-plugin-runtime codex_messages_are_returned_newest_first_without_raw_payload
```

Expected: fails because parser does not exist.

- [ ] **Step 3: Implement message parser**

Create `builtin-plugins/codex-runtime/src/session_messages.rs`:

```rust
use chrono::{DateTime, Utc};
use niuma_core::tool_session::{ToolSessionMessage, ToolSessionMessageRole};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct CodexRow {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    row_type: String,
    payload: Value,
}

pub fn parse_codex_messages_from_lines(
    session_id: &str,
    lines: &[&str],
) -> Result<Vec<ToolSessionMessage>, String> {
    let mut messages = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let row: CodexRow = serde_json::from_str(line)
            .map_err(|error| format!("解析 Codex JSONL 失败：{error}"))?;
        if row.row_type == "session_meta" {
            continue;
        }
        if let Some(message) = message_from_row(session_id, line_index as u64, row)? {
            messages.push(message);
        }
    }
    messages.sort_by(|left, right| right.id.cmp(&left.id));
    Ok(messages)
}

fn message_from_row(
    session_id: &str,
    line_index: u64,
    row: CodexRow,
) -> Result<Option<ToolSessionMessage>, String> {
    let kind = row.payload.get("type").and_then(Value::as_str).unwrap_or("unknown");
    let created_at = row
        .timestamp
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    let (role, content) = match (row.row_type.as_str(), kind) {
        ("response_item", "message") => message_role_and_content(&row.payload),
        ("response_item", "function_call") => (
            ToolSessionMessageRole::ToolCall,
            function_call_summary(&row.payload),
        ),
        ("response_item", "function_call_output") => (
            ToolSessionMessageRole::ToolResult,
            text_from_payload(&row.payload).unwrap_or_default(),
        ),
        ("event_msg", "task_started") => (ToolSessionMessageRole::Event, "任务开始".to_string()),
        ("event_msg", "thread_rolled_back") => {
            (ToolSessionMessageRole::Event, "会话已回滚".to_string())
        }
        ("event_msg", _) => (
            ToolSessionMessageRole::Event,
            text_from_payload(&row.payload).unwrap_or_default(),
        ),
        _ => (
            ToolSessionMessageRole::Unknown,
            text_from_payload(&row.payload).unwrap_or_default(),
        ),
    };
    Ok(Some(ToolSessionMessage {
        id: format!("codex:{session_id}:{line_index:020}"),
        role,
        content,
        created_at,
        metadata: json!({
            "source": "codex_session_file",
            "codex_row_type": row.row_type,
            "codex_item_type": kind
        }),
    }))
}
```

Add helper functions `message_role_and_content`, `text_from_payload`, and `function_call_summary` in the same file. Each helper must extract only text or short summaries, not return the raw payload.

- [ ] **Step 4: Implement provider stdin/stdout loop**

Create `builtin-plugins/codex-runtime/src/session_provider.rs` with:

```rust
pub fn run_from_env() {
    let tool = std::env::var("NIUMA_TOOL_ID").unwrap_or_else(|_| "codex".to_string());
    let mut provider = CodexSessionProvider::new(tool);
    provider.run_stdio();
}
```

Implement `run_stdio()`:

```rust
fn run_stdio(&mut self) {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().flatten() {
        let response = self.handle_rpc_line(&line);
        println!("{}", serde_json::to_string(&response).unwrap());
    }
}
```

Implement `session_snapshot` by scanning Codex session directories using existing discovery helpers where possible. Implement `session_detail` through the provider-owned message index: refresh the index for the requested `session_id`, select the requested newest-first page, read only the indexed JSONL ranges for that page, parse those ranges, and return normalized messages. Keep the host free of Codex file reads and keep full-file parsing out of the request path.

- [ ] **Step 5: Add provider binary**

In `builtin-plugins/codex-runtime/Cargo.toml`, add:

```toml
[[bin]]
name = "niuma-codex-session-provider"
path = "src/session_provider_main.rs"
```

Create `builtin-plugins/codex-runtime/src/session_provider_main.rs`:

```rust
fn main() {
    niuma_codex_plugin_runtime::session_provider::run_from_env();
}
```

Export modules in `builtin-plugins/codex-runtime/src/lib.rs`:

```rust
pub mod session_messages;
pub mod session_provider;
```

- [ ] **Step 6: Configure packaged provider command**

In `src-tauri/src/main.rs`, add:

```rust
const CODEX_SESSION_PROVIDER_BINARY_NAME: &str = "niuma-codex-session-provider";
```

Add a configure function and call it during setup:

```rust
fn configure_builtin_codex_session_provider_command(app: &tauri::App) {
    configure_builtin_plugin_command(
        app,
        niuma_core::plugin::CODEX_SESSION_PROVIDER_COMMAND_ENV,
        CODEX_SESSION_PROVIDER_BINARY_NAME,
    );
}
```

In `crates/niuma-core/src/plugin.rs`, define `CODEX_SESSION_PROVIDER_COMMAND_ENV` and resolve it in `builtin_codex_session_provider_manifest()`.

- [ ] **Step 7: Run provider tests**

Run:

```bash
cargo test -p niuma-codex-plugin-runtime
cargo check -p niuma-notifier
```

Expected: tests and checks pass.

- [ ] **Step 8: Commit Codex session provider**

```bash
git add builtin-plugins src-tauri crates/niuma-core
git commit -m "feat: 新增 Codex 会话 provider 插件" -m "修改内容：新增独立 Codex session provider 二进制、消息解析器、provider manifest 和打包命令解析。" -m "修改原因：工具会话列表和详情需要独立于 event_watcher 的 provider 插件提供。"
```

---

### Task 8: Documentation And Full Verification

**Files:**
- Modify: `docs/integration/plugin-development.md`
- Modify: `docs/integration/plugin-development_zh.md`
- Modify: `README.md`
- Modify: `README_zh.md`
- Modify: `docs/superpowers/specs/2026-06-23-tool-session-reader-design.md` only if implementation diverges from the accepted design.

- [ ] **Step 1: Update integration docs with new APIs**

In `docs/integration/plugin-development_zh.md`, add a section:

```markdown
## 工具会话读取

第三方 reader 插件通过宿主 Local API 读取工具会话，不直接读取工具目录，也不直接调用 provider 插件。

```http
GET /api/v1/session_list?tool=codex&include_subagents=false&active_only=false&limit=100
GET /api/v1/session_detail?tool=codex&session_id=session-1&limit=100&cursor=cursor-1
```

`session_detail` 返回倒序消息，`messages[0]` 是本页最新消息。`next_cursor` 用于继续读取更旧消息。

第一版不做 token 鉴权，`tool_session_detail_reader` 是敏感能力声明和 UI 展示标记，不是服务端强鉴权边界。
```
```

Add the equivalent English section in `docs/integration/plugin-development.md`.

- [ ] **Step 2: Update capability table**

Update both integration docs capability tables to include:

```markdown
| `tool_session_list_provider` | tool | Provides discovered tool session list to the host. |
| `tool_session_detail_provider` | tool | Provides normalized tool session messages to the host. |
| `tool_session_list_reader` | any business plugin | Reads the host `session_list` API. |
| `tool_session_detail_reader` | any business plugin | Reads AI conversation content through the host `session_detail` API. Sensitive. |
```

- [ ] **Step 3: Run full verification**

Run:

```bash
cargo test
npm test
cargo check -p niuma-notifier
```

Expected: all pass.

- [ ] **Step 4: Confirm old API removal and new API docs**

Run:

```bash
rg -n "/api/v1/sessions|NiumaSession|SessionStatus" crates src docs README.md README_zh.md
```

Expected: only migration notes or historical design references remain. Runtime code and public integration docs should use `runtime_state_list`, `RuntimeStateItem`, and `RuntimeStateStatus`.

- [ ] **Step 5: Commit docs and final verification updates**

```bash
git add docs README.md README_zh.md
git commit -m "docs: 更新工具会话读取集成文档" -m "修改内容：补充 session_list/session_detail 接口、工具会话 provider/reader capability 和 runtime_state_list 迁移说明。" -m "修改原因：第三方插件需要明确通过宿主 Local API 读取工具会话的使用方式和能力声明边界。"
```

---

## Self-Review

- Spec coverage: The plan covers runtime-state rename, `session_list`, `session_detail`, provider capability uniqueness, independent Codex session provider, stdio JSON Lines RPC, newest-first detail pagination, metadata restrictions, and docs.
- Placeholder scan: The plan contains no `TBD`, unresolved implementation slots, or open-ended validation instructions. Where the implementation can choose small internal helpers, the expected behavior and file location are specified.
- Type consistency: Public names are consistently `RuntimeStateItem`, `RuntimeStateStatus`, `ToolSessionListItem`, `ToolSessionDetail`, `ToolSessionMessage`, `/api/v1/runtime_state_list`, `/api/v1/session_list`, and `/api/v1/session_detail`.
