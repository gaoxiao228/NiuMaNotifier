# NiuMa SQLite Notification-Only Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将默认数据库从 `state.sqlite` 改为 `niuma.sqlite`，SQLite 只保留统一通知历史表，配置改为 JSON 文件持久化，旧 `state.sqlite` 不迁移、不读取、不删除。

**Architecture:** 保留 `SqliteStateStore` 作为现有调用方门面，内部职责收缩为三类存储：SQLite 通知历史、JSON 配置、内存运行态。通知历史从内置/插件双表合并为单表 `notification_records`，API/Tauri 返回结构保持 `data.list` 兼容。配置读写落到应用数据目录下的 `config.json` 和 `plugin-configs/<plugin_id>.json`。

**Tech Stack:** Rust workspace、rusqlite、serde_json、chrono、Axum Local API、Tauri commands、Cargo tests、Markdown docs。

---

## Scope And Safety

- 不执行任何删除旧数据库文件的命令。
- 不写任何会自动删除、清空、覆盖旧 `state.sqlite` 的代码。
- 测试只使用现有测试辅助函数生成的隔离临时 SQLite/JSON 路径。
- 统一通知表里的内置通知器 ID 使用现有插件 ID：`builtin-bark`、`builtin-ntfy`。
- 工作区已有未提交改动，实施时每次提交只 `git add` 本任务明确涉及的文件。
- 当前 API 风格已经使用统一响应结构：`code`、`message`、`data`。通知历史接口是非分页列表，继续返回 `data.list`，符合 `backend-api-standard`。

## File Structure

- Modify: `crates/niuma-core/src/config.rs`
  - 负责数据库路径环境变量从 `NIUMA_STATE_PATH` 改为 `NIUMA_DB_PATH`，默认文件名改为 `niuma.sqlite`。
- Modify: `crates/niuma-core/src/store/schema.rs`
  - 负责创建 SQLite schema，只保留统一 `notification_records` 表和索引。
- Modify: `crates/niuma-core/src/notification_store.rs`
  - 负责统一通知记录模型、插入、更新、查询和历史兼容视图。
- Modify: `crates/niuma-core/src/store.rs`
  - 继续作为调用方门面，运行态保持内存化，配置读写改为 JSON 文件，通知读写委托给统一表。
- Create: `crates/niuma-core/src/store/config_files.rs`
  - 负责 `config.json` 与 `plugin-configs/<plugin_id>.json` 的 JSON 文件读写。
- Modify: `crates/niuma-core/src/store/tests.rs`
  - 覆盖路径、schema、通知单表、JSON 配置、旧库不参与新流程。
- Modify: `crates/niuma-api/src/handlers.rs`
  - 保持通知历史 API 响应结构，更新插件通知结果写入统一记录模型。
- Modify: `crates/niuma-api/src/tests.rs`
  - 覆盖 API 通知历史和插件通知结果仍符合统一响应结构。
- Modify: `src-tauri/src/commands.rs`
  - 保持 Tauri 通知历史命令响应结构，更新插件通知测试结果读取统一记录。
- Modify: `src-tauri/src/tools/plugin_runtime.rs`
  - 注入 `NIUMA_DB_PATH`，不再注入 `NIUMA_STATE_PATH`。
- Modify: `builtin-plugins/ntfy-runtime/src/lib.rs`
  - 使用 `NIUMA_DB_PATH` 作为诊断 seed，不再读取 `NIUMA_STATE_PATH`。
- Modify docs:
  - `docs/integration/plugin-development_zh.md`
  - `docs/integration/sse-external-integration.md`
  - `docs/integration/sse-external-integration_zh.md`
  - 已有旧 spec/plan 文档可以保留历史语义，不作为运行文档强制更新。

---

### Task 1: Database Path And Environment Variable

**Files:**
- Modify: `crates/niuma-core/src/config.rs`
- Modify: `src-tauri/src/tools/plugin_runtime.rs`
- Modify: `builtin-plugins/ntfy-runtime/src/lib.rs`
- Test: `crates/niuma-core/src/config.rs`
- Test: `src-tauri/src/tools/plugin_runtime.rs`

- [ ] **Step 1: Write failing config tests**

In `crates/niuma-core/src/config.rs`, replace the existing `state_path_uses_override_or_default_sqlite_path` test with:

```rust
#[test]
fn db_path_uses_niuma_db_path_or_default_sqlite_path() {
    assert_eq!(
        db_path_from_env(Some("/tmp/custom-niuma.sqlite")),
        PathBuf::from("/tmp/custom-niuma.sqlite")
    );
    assert_eq!(
        db_path_from_env(None),
        crate::platform::paths::app_data_dir().join("niuma.sqlite")
    );
}

#[test]
fn old_state_path_env_is_not_used_for_database_path() {
    // 旧 NIUMA_STATE_PATH 已废弃；数据库路径只接受 NIUMA_DB_PATH。
    assert_eq!(
        db_path_from_env(None),
        crate::platform::paths::app_data_dir().join("niuma.sqlite")
    );
}
```

- [ ] **Step 2: Run config test to verify it fails**

Run:

```bash
cargo test -p niuma-core config::tests::db_path_uses_niuma_db_path_or_default_sqlite_path
```

Expected: FAIL because `db_path_from_env` does not exist yet.

- [ ] **Step 3: Rename path helpers minimally**

In `crates/niuma-core/src/config.rs`, replace the state path helpers with:

```rust
pub fn db_path() -> PathBuf {
    db_path_from_env(std::env::var("NIUMA_DB_PATH").ok().as_deref())
}

pub fn db_path_from_env(value: Option<&str>) -> PathBuf {
    value.map(PathBuf::from).unwrap_or_else(default_db_path)
}

fn default_db_path() -> PathBuf {
    crate::platform::paths::app_data_dir().join("niuma.sqlite")
}
```

In `crates/niuma-core/src/store.rs`, update:

```rust
pub fn default_path() -> PathBuf {
    crate::config::db_path()
}
```

- [ ] **Step 4: Update plugin runtime env injection**

In `src-tauri/src/tools/plugin_runtime.rs`, replace the environment variable injection with:

```rust
.env(
    "NIUMA_DB_PATH",
    SqliteStateStore::default_path()
        .to_string_lossy()
        .to_string(),
)
```

Remove the `NIUMA_STATE_PATH` injection from that command builder.

- [ ] **Step 5: Update ntfy runtime diagnostic env read**

In `builtin-plugins/ntfy-runtime/src/lib.rs`, replace:

```rust
let seed = std::env::var("NIUMA_STATE_PATH")
```

with:

```rust
let seed = std::env::var("NIUMA_DB_PATH")
```

If the nearby error/log text mentions state path, change it to database path.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p niuma-core config::tests
```

Expected: PASS.

Run:

```bash
cargo test --workspace plugin_runtime
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/niuma-core/src/config.rs crates/niuma-core/src/store.rs src-tauri/src/tools/plugin_runtime.rs builtin-plugins/ntfy-runtime/src/lib.rs
git commit -m "refactor: 调整数据库路径命名" -m "修改内容：默认数据库改为 niuma.sqlite，并将环境变量改为 NIUMA_DB_PATH。" -m "修改原因：数据库不再承担完整状态持久化职责，旧 state 命名已经不准确。"
```

---

### Task 2: SQLite Schema Keeps Only Unified Notification Table

**Files:**
- Modify: `crates/niuma-core/src/store/schema.rs`
- Modify: `crates/niuma-core/src/store/tests.rs`

- [ ] **Step 1: Write failing schema test**

In `crates/niuma-core/src/store/tests.rs`, update the schema initialization test so it asserts only the new table and indexes exist:

```rust
#[test]
fn schema_initializes_only_notification_records_table() {
    let path = test_sqlite_path("schema_notification_only");
    let store = SqliteStateStore::new(&path);
    store.load().unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();

    assert_table_exists(&connection, "notification_records");
    assert_table_missing(&connection, "sessions");
    assert_table_missing(&connection, "attention_items");
    assert_table_missing(&connection, "latest_activity");
    assert_table_missing(&connection, "public_events");
    assert_table_missing(&connection, "app_settings");
    assert_table_missing(&connection, "plugin_configs");
    assert_table_missing(&connection, "plugin_notification_results");

    assert_index_exists(&connection, "idx_notification_records_created_at");
    assert_index_exists(&connection, "idx_notification_records_notifier_created_at");
}
```

Add this helper near the existing table/index helpers:

```rust
fn assert_table_missing(connection: &rusqlite::Connection, table: &str) {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS (
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = ?1
            )",
            [table],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!exists, "table should not exist: {table}");
}
```

- [ ] **Step 2: Run schema test to verify it fails**

Run:

```bash
cargo test -p niuma-core schema_initializes_only_notification_records_table
```

Expected: FAIL because old tables are still created.

- [ ] **Step 3: Shrink schema**

In `crates/niuma-core/src/store/schema.rs`, replace `execute_batch` SQL with:

```rust
// 新库只持久化通知历史；事件、会话、关注项和配置分别由内存/JSON 负责。
connection
    .execute_batch(
        "
        CREATE TABLE IF NOT EXISTS notification_records (
            id TEXT PRIMARY KEY,
            notifier_id TEXT NOT NULL,
            notifier_type TEXT NOT NULL,
            event_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            status TEXT NOT NULL,
            title TEXT,
            body TEXT,
            reason TEXT,
            error_message TEXT,
            created_at TEXT NOT NULL,
            sent_at TEXT,
            UNIQUE(notifier_id, event_id)
        );

        CREATE INDEX IF NOT EXISTS idx_notification_records_created_at
            ON notification_records(created_at);
        CREATE INDEX IF NOT EXISTS idx_notification_records_notifier_created_at
            ON notification_records(notifier_id, created_at);
        ",
    )
    .map_err(|error| format!("初始化 SQLite 通知库失败：{error}"))
```

- [ ] **Step 4: Run schema test**

Run:

```bash
cargo test -p niuma-core schema_initializes_only_notification_records_table
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/niuma-core/src/store/schema.rs crates/niuma-core/src/store/tests.rs
git commit -m "refactor: 收缩 SQLite 通知库表结构" -m "修改内容：SQLite schema 只创建统一 notification_records 表和通知索引。" -m "修改原因：事件、会话、配置和运行态状态不再写入数据库。"
```

---

### Task 3: Merge Builtin And Plugin Notifications Into One Model

**Files:**
- Modify: `crates/niuma-core/src/notification_store.rs`
- Modify: `crates/niuma-core/src/store.rs`
- Modify: `crates/niuma-core/src/store/tests.rs`

- [ ] **Step 1: Write failing unified notification tests**

In `crates/niuma-core/src/store/tests.rs`, replace plugin notification table tests with unified-table tests:

```rust
#[test]
fn notification_records_dedupe_by_notifier_and_event() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_records_notifier_dedupe"));
    let record = sample_notification_record("record-1", "builtin-bark", "event-1");

    assert!(store.insert_notification_record_if_absent(&record).unwrap());
    assert!(!store.insert_notification_record_if_absent(&record).unwrap());

    let records = store.notification_records(20).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].notifier_id, "builtin-bark");
}

#[test]
fn notification_records_allow_same_event_for_different_notifiers() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_records_multi_notifier"));
    let bark = sample_notification_record("record-bark", "builtin-bark", "event-1");
    let ntfy = sample_notification_record("record-ntfy", "builtin-ntfy", "event-1");

    assert!(store.insert_notification_record_if_absent(&bark).unwrap());
    assert!(store.insert_notification_record_if_absent(&ntfy).unwrap());

    let records = store.notification_records(20).unwrap();
    assert_eq!(records.len(), 2);
}

#[test]
fn notification_history_records_marks_plugin_id_for_plugin_notifier() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_history_unified"));
    let builtin = sample_notification_record("builtin-record", "builtin-bark", "event-builtin");
    let plugin = sample_plugin_notification_record("plugin-record", "external-slack", "event-plugin");

    store.insert_notification_record_if_absent(&builtin).unwrap();
    store.save_plugin_notification_result(&plugin).unwrap();

    let records = store.notification_history_records(20).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().any(|record| record.channel == "builtin-bark" && record.plugin_id.is_none()));
    assert!(records.iter().any(|record| record.channel == "external-slack" && record.plugin_id == Some("external-slack".to_string())));
}
```

Update test helpers:

```rust
fn sample_notification_record(id: &str, notifier_id: &str, event_id: &str) -> NotificationRecord {
    NotificationRecord {
        id: id.to_string(),
        notifier_id: notifier_id.to_string(),
        notifier_type: NotificationNotifierType::Builtin,
        event_id: event_id.to_string(),
        event_type: EventType::SessionStopped,
        status: NotificationRecordStatus::Sent,
        title: Some("Done".to_string()),
        body: Some("Finished".to_string()),
        reason: None,
        error_message: None,
        created_at: Utc.timestamp_opt(1_000, 0).unwrap(),
        sent_at: Some(Utc.timestamp_opt(1_001, 0).unwrap()),
    }
}

fn sample_plugin_notification_record(
    id: &str,
    plugin_id: &str,
    event_id: &str,
) -> PluginNotificationResult {
    PluginNotificationResult {
        id: id.to_string(),
        plugin_id: plugin_id.to_string(),
        event_id: event_id.to_string(),
        event_type: EventType::SessionStopped,
        status: NotificationRecordStatus::Sent,
        title: Some("Plugin Done".to_string()),
        body: Some("Plugin Finished".to_string()),
        reason: None,
        error_message: None,
        created_at: Utc.timestamp_opt(1_002, 0).unwrap(),
        sent_at: Some(Utc.timestamp_opt(1_003, 0).unwrap()),
    }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p niuma-core notification_records_dedupe_by_notifier_and_event
cargo test -p niuma-core notification_history_records_marks_plugin_id_for_plugin_notifier
```

Expected: FAIL because `NotificationNotifierType` and new fields do not exist.

- [ ] **Step 3: Update notification models**

In `crates/niuma-core/src/notification_store.rs`, add:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationNotifierType {
    Builtin,
    Plugin,
}
```

Change `NotificationRecord` to:

```rust
pub struct NotificationRecord {
    pub id: String,
    pub notifier_id: String,
    pub notifier_type: NotificationNotifierType,
    pub event_id: String,
    pub event_type: EventType,
    pub status: NotificationRecordStatus,
    pub title: Option<String>,
    pub body: Option<String>,
    pub reason: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
}
```

Keep `NotificationChannel` only if existing Bark/Ntfy sending code still constructs builtin records through it. If kept, add a conversion helper:

```rust
pub fn builtin_notifier_id(channel: &NotificationChannel) -> &'static str {
    match channel {
        NotificationChannel::Bark => "builtin-bark",
        NotificationChannel::Ntfy => "builtin-ntfy",
    }
}
```

- [ ] **Step 4: Update SQL insert/update/load functions**

Update `insert_record_if_absent` SQL:

```rust
"INSERT INTO notification_records
 (id, notifier_id, notifier_type, event_id, event_type, status, title, body, reason, error_message, created_at, sent_at)
 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
 ON CONFLICT(notifier_id, event_id) DO NOTHING"
```

Use params in this order:

```rust
params![
    &record.id,
    &record.notifier_id,
            notifier_type_id(&record.notifier_type),
    &record.event_id,
    serde_json::to_string(&record.event_type)
        .map_err(|error| format!("序列化通知事件类型失败：{error}"))?,
    serde_json::to_string(&record.status)
        .map_err(|error| format!("序列化通知记录状态失败：{error}"))?,
    &record.title,
    &record.body,
    &record.reason,
    &record.error_message,
    record.created_at.to_rfc3339(),
    record.sent_at.map(|value| value.to_rfc3339()),
]
```

Update `load_records` SELECT:

```rust
"SELECT id, notifier_id, notifier_type, event_id, event_type, status, title, body, reason, error_message, created_at, sent_at
 FROM notification_records
 ORDER BY created_at DESC
 LIMIT ?1"
```

Map columns consistently:

```rust
let notifier_type_text: String = row.get(2)?;
let event_type_text: String = row.get(4)?;
let status_text: String = row.get(5)?;
let created_at_text: String = row.get(10)?;
let sent_at_text: Option<String> = row.get(11)?;
```

Add stable type helpers near the existing channel helpers:

```rust
fn notifier_type_id(value: &NotificationNotifierType) -> &'static str {
    match value {
        NotificationNotifierType::Builtin => "builtin",
        NotificationNotifierType::Plugin => "plugin",
    }
}

fn parse_notifier_type(value: &str) -> Result<NotificationNotifierType, String> {
    match value {
        "builtin" => Ok(NotificationNotifierType::Builtin),
        "plugin" => Ok(NotificationNotifierType::Plugin),
        _ => Err(format!("未知通知器类型：{value}")),
    }
}
```

- [ ] **Step 5: Collapse plugin notification persistence into the unified table**

Change `upsert_plugin_result` to convert into `NotificationRecord` and upsert `notification_records`:

```rust
let record = NotificationRecord {
    id: result.id.clone(),
    notifier_id: result.plugin_id.clone(),
    notifier_type: NotificationNotifierType::Plugin,
    event_id: result.event_id.clone(),
    event_type: result.event_type.clone(),
    status: result.status.clone(),
    title: result.title.clone(),
    body: result.body.clone(),
    reason: result.reason.clone(),
    error_message: result.error_message.clone(),
    created_at: result.created_at,
    sent_at: result.sent_at,
};
```

Use SQL:

```rust
"INSERT INTO notification_records
 (id, notifier_id, notifier_type, event_id, event_type, status, title, body, reason, error_message, created_at, sent_at)
 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
 ON CONFLICT(notifier_id, event_id) DO UPDATE SET
   status = excluded.status,
   title = excluded.title,
   body = excluded.body,
   reason = excluded.reason,
   error_message = excluded.error_message,
   created_at = excluded.created_at,
   sent_at = excluded.sent_at"
```

Update `load_plugin_result` to query:

```sql
SELECT id, notifier_id, event_id, event_type, status, title, body, reason, error_message, created_at, sent_at
FROM notification_records
WHERE notifier_type = 'plugin' AND notifier_id = ?1 AND event_id = ?2
```

Then convert the row back to `PluginNotificationResult`.

- [ ] **Step 6: Simplify history loading**

Change `load_history_records` to load from one table only:

```rust
pub(crate) fn load_history_records(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<NotificationHistoryRecord>, String> {
    Ok(load_records(connection, limit)?
        .into_iter()
        .map(NotificationHistoryRecord::from)
        .collect())
}
```

Change `impl From<NotificationRecord> for NotificationHistoryRecord`:

```rust
plugin_id: match record.notifier_type {
    NotificationNotifierType::Plugin => Some(record.notifier_id.clone()),
    NotificationNotifierType::Builtin => None,
},
channel: record.notifier_id,
```

- [ ] **Step 7: Run focused notification tests**

Run:

```bash
cargo test -p niuma-core notification_records
cargo test -p niuma-core plugin_notification
cargo test -p niuma-core notification_history
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/niuma-core/src/notification_store.rs crates/niuma-core/src/store.rs crates/niuma-core/src/store/tests.rs
git commit -m "refactor: 合并通知历史存储表" -m "修改内容：内置通知和插件通知统一写入 notification_records 表。" -m "修改原因：通知历史只有一个业务列表，不需要维护 legacy 和 plugin 双表。"
```

---

### Task 4: Move Settings And Plugin Configs To JSON Files

**Files:**
- Create: `crates/niuma-core/src/store/config_files.rs`
- Modify: `crates/niuma-core/src/store.rs`
- Modify: `crates/niuma-core/src/store/tests.rs`

- [ ] **Step 1: Write failing JSON config tests**

In `crates/niuma-core/src/store/tests.rs`, add:

```rust
#[test]
fn listener_config_persists_to_json_config_file() {
    let root = test_data_dir("json_listener_config");
    let store = SqliteStateStore::new(root.join("niuma.sqlite"));
    let config = ListenerConfig {
        codex_listening_enabled: true,
        tool_listening_enabled: BTreeMap::new(),
    };

    store.save_listener_config(&config).unwrap();

    let config_path = root.join("config.json");
    assert!(config_path.exists());
    let reloaded = SqliteStateStore::new(root.join("niuma.sqlite"))
        .listener_config()
        .unwrap();
    assert!(reloaded.codex_listening_enabled);
}

#[test]
fn plugin_config_persists_to_plugin_config_json_file() {
    let root = test_data_dir("json_plugin_config");
    let store = SqliteStateStore::new(root.join("niuma.sqlite"));
    let config = serde_json::json!({ "server": "https://example.com" })
        .as_object()
        .unwrap()
        .clone();

    store.save_plugin_config("external-demo", &config).unwrap();

    assert!(root.join("plugin-configs").join("external-demo.json").exists());
    let reloaded = SqliteStateStore::new(root.join("niuma.sqlite"))
        .plugin_config("external-demo")
        .unwrap()
        .unwrap();
    assert_eq!(reloaded["server"], "https://example.com");
}

#[test]
fn plugin_runtime_states_are_memory_only() {
    let path = test_sqlite_path("runtime_states_memory_only");
    let store = SqliteStateStore::new(&path);
    store
        .save_plugin_runtime_state(
            "external-demo",
            PluginRuntimeState {
                status: PluginRuntimeStatus::Running,
                last_error: Some("boom".to_string()),
            },
        )
        .unwrap();

    let reloaded = SqliteStateStore::new(path);
    assert!(reloaded.plugin_runtime_states().unwrap().is_empty());
}
```

Import `PluginRuntimeStatus` beside `PluginRuntimeState` in the test module.

Add a unique temporary directory helper for JSON tests:

```rust
fn test_data_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "niuma-notifier-{name}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p niuma-core listener_config_persists_to_json_config_file
cargo test -p niuma-core plugin_config_persists_to_plugin_config_json_file
cargo test -p niuma-core plugin_runtime_states_are_memory_only
```

Expected: FAIL because config is still read/written through SQLite.

- [ ] **Step 3: Create JSON config storage module**

Create `crates/niuma-core/src/store/config_files.rs`:

```rust
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::listener_config::ListenerConfig;
use crate::platform::locale::LanguagePreference;

#[derive(Clone, Debug)]
pub(super) struct ConfigFileStore {
    root: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppConfigFile {
    #[serde(default)]
    listener_config: ListenerConfig,
    #[serde(default = "default_language_preference")]
    language_preference: String,
    #[serde(default)]
    plugin_enabled_map: BTreeMap<String, bool>,
}

impl Default for AppConfigFile {
    fn default() -> Self {
        Self {
            listener_config: ListenerConfig::default(),
            language_preference: default_language_preference(),
            plugin_enabled_map: BTreeMap::new(),
        }
    }
}

impl ConfigFileStore {
    pub(super) fn new(db_path: &Path) -> Self {
        let root = db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self { root }
    }

    pub(super) fn listener_config(&self) -> Result<ListenerConfig, String> {
        Ok(self.read_app_config()?.listener_config)
    }

    pub(super) fn save_listener_config(&self, config: &ListenerConfig) -> Result<(), String> {
        let mut app_config = self.read_app_config()?;
        app_config.listener_config = config.clone();
        self.write_app_config(&app_config)
    }

    pub(super) fn language_preference(&self) -> Result<LanguagePreference, String> {
        let preference = self.read_app_config()?.language_preference;
        LanguagePreference::from_storage_id(&preference)
            .ok_or_else(|| format!("未知语言偏好：{preference}"))
    }

    pub(super) fn save_language_preference(
        &self,
        preference: LanguagePreference,
    ) -> Result<(), String> {
        let mut app_config = self.read_app_config()?;
        app_config.language_preference = preference.storage_id().to_string();
        self.write_app_config(&app_config)
    }

    pub(super) fn plugin_enabled_map(&self) -> Result<BTreeMap<String, bool>, String> {
        Ok(self.read_app_config()?.plugin_enabled_map)
    }

    pub(super) fn save_plugin_enabled_map(
        &self,
        map: &BTreeMap<String, bool>,
    ) -> Result<(), String> {
        let mut app_config = self.read_app_config()?;
        app_config.plugin_enabled_map = map.clone();
        self.write_app_config(&app_config)
    }

    pub(super) fn plugin_config(
        &self,
        plugin_id: &str,
    ) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
        let path = self.plugin_config_path(plugin_id);
        if !path.exists() {
            return Ok(None);
        }
        let value = read_json_file(&path)?;
        let Some(object) = value.as_object() else {
            return Err(format!("插件配置格式无效：{plugin_id}"));
        };
        Ok(Some(object.clone()))
    }

    pub(super) fn save_plugin_config(
        &self,
        plugin_id: &str,
        config: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        let path = self.plugin_config_path(plugin_id);
        write_json_file(&path, &serde_json::Value::Object(config.clone()))
    }

    pub(super) fn remove_plugin_config(&self, plugin_id: &str) -> Result<(), String> {
        let path = self.plugin_config_path(plugin_id);
        if path.exists() {
            fs::remove_file(&path).map_err(|error| format!("移除插件配置失败：{error}"))?;
        }
        Ok(())
    }

    fn app_config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    fn plugin_config_path(&self, plugin_id: &str) -> PathBuf {
        self.root.join("plugin-configs").join(format!("{plugin_id}.json"))
    }

    fn read_app_config(&self) -> Result<AppConfigFile, String> {
        let path = self.app_config_path();
        if !path.exists() {
            return Ok(AppConfigFile::default());
        }
        serde_json::from_value(read_json_file(&path)?)
            .map_err(|error| format!("解析应用配置失败：{error}"))
    }

    fn write_app_config(&self, config: &AppConfigFile) -> Result<(), String> {
        let value = serde_json::to_value(config)
            .map_err(|error| format!("序列化应用配置失败：{error}"))?;
        write_json_file(&self.app_config_path(), &value)
    }
}

fn default_language_preference() -> String {
    LanguagePreference::System.storage_id().to_string()
}

fn read_json_file(path: &Path) -> Result<serde_json::Value, String> {
    let content = fs::read_to_string(path).map_err(|error| format!("读取配置文件失败：{error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析配置文件失败：{error}"))
}

fn write_json_file(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败：{error}"))?;
    }
    let content = serde_json::to_string_pretty(value)
        .map_err(|error| format!("序列化配置文件失败：{error}"))?;
    fs::write(path, content).map_err(|error| format!("写入配置文件失败：{error}"))
}
```

- [ ] **Step 4: Wire JSON config store into SqliteStateStore**

In `crates/niuma-core/src/store.rs`, add:

```rust
mod config_files;
use config_files::ConfigFileStore;
```

Add method:

```rust
fn config_files(&self) -> ConfigFileStore {
    ConfigFileStore::new(&self.path)
}
```

Replace SQLite-backed config methods with delegates:

```rust
pub fn listener_config(&self) -> Result<ListenerConfig, String> {
    self.config_files().listener_config()
}

pub fn save_listener_config(&self, config: &ListenerConfig) -> Result<(), String> {
    self.config_files().save_listener_config(config)
}

pub fn language_preference(&self) -> Result<LanguagePreference, String> {
    self.config_files().language_preference()
}

pub fn save_language_preference(&self, preference: LanguagePreference) -> Result<(), String> {
    self.config_files().save_language_preference(preference)
}

pub fn plugin_enabled_map(&self) -> Result<BTreeMap<String, bool>, String> {
    self.config_files().plugin_enabled_map()
}

pub fn save_plugin_enabled_map(&self, map: &BTreeMap<String, bool>) -> Result<(), String> {
    self.config_files().save_plugin_enabled_map(map)
}

pub fn plugin_config(
    &self,
    plugin_id: &str,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    self.config_files().plugin_config(plugin_id)
}

pub fn save_plugin_config(
    &self,
    plugin_id: &str,
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    self.config_files().save_plugin_config(plugin_id, config)
}

pub fn remove_plugin_config(&self, plugin_id: &str) -> Result<(), String> {
    self.config_files().remove_plugin_config(plugin_id)
}
```

Change runtime state methods to memory only:

```rust
pub fn plugin_runtime_states(
    &self,
) -> Result<std::collections::HashMap<String, PluginRuntimeState>, String> {
    Ok(self.runtime()?.plugin_runtime_states.clone())
}
```

Add `plugin_runtime_states` to `RuntimeState`:

```rust
plugin_runtime_states: std::collections::HashMap<String, PluginRuntimeState>,
```

Update `save_plugin_runtime_state` and `remove_plugin_runtime_state` to mutate `runtime.plugin_runtime_states` instead of SQLite.

- [ ] **Step 5: Remove unused SQLite config constants/imports**

In `crates/niuma-core/src/store.rs`, remove these if no longer used:

```rust
use rusqlite::{params, Connection, OptionalExtension};
const LISTENER_CONFIG_KEY: &str = "listener_config";
const LANGUAGE_PREFERENCE_KEY: &str = "language_preference";
const PLUGIN_ENABLED_MAP_KEY: &str = "plugin_enabled_map";
const PLUGIN_RUNTIME_STATES_KEY: &str = "plugin_runtime_states";
```

Keep `Connection` if `open()` still uses it.

- [ ] **Step 6: Run focused config tests**

Run:

```bash
cargo test -p niuma-core listener_config_persists_to_json_config_file
cargo test -p niuma-core plugin_config_persists_to_plugin_config_json_file
cargo test -p niuma-core plugin_runtime_states_are_memory_only
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/niuma-core/src/store.rs crates/niuma-core/src/store/config_files.rs crates/niuma-core/src/store/tests.rs
git commit -m "refactor: 使用 JSON 文件保存配置" -m "修改内容：全局配置和插件配置从 SQLite 迁移到 config.json 与 plugin-configs JSON 文件。" -m "修改原因：配置需要持久化，但不需要占用通知历史数据库。"
```

---

### Task 5: Keep API And Tauri Notification History Compatible

**Files:**
- Modify: `crates/niuma-api/src/handlers.rs`
- Modify: `crates/niuma-api/src/tests.rs`
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Update API tests to assert unified history shape**

In `crates/niuma-api/src/tests.rs`, update `notification_records_returns_standard_list_envelope` to create a unified record:

```rust
let store = SqliteStateStore::new(test_path("notification_records_list"));
store
    .insert_notification_record_if_absent(&NotificationRecord {
        id: "record-1".to_string(),
        notifier_id: "builtin-bark".to_string(),
        notifier_type: NotificationNotifierType::Builtin,
        event_id: "event-1".to_string(),
        event_type: EventType::SessionStopped,
        status: NotificationRecordStatus::Sent,
        title: Some("Done".to_string()),
        body: Some("Finished".to_string()),
        reason: None,
        error_message: None,
        created_at: Utc.timestamp_opt(1_000, 0).unwrap(),
        sent_at: Some(Utc.timestamp_opt(1_001, 0).unwrap()),
    })
    .unwrap();
```

Assert:

```rust
assert_eq!(value["code"], 0);
assert_eq!(value["data"]["list"][0]["channel"], "builtin-bark");
assert!(value["data"]["list"][0]["plugin_id"].is_null());
```

- [ ] **Step 2: Run API notification history test**

Run:

```bash
cargo test -p niuma-api notification_records_returns_standard_list_envelope
```

Expected: PASS after Task 3 model updates are complete.

- [ ] **Step 3: Update plugin notification result handler construction**

In `crates/niuma-api/src/handlers.rs`, keep request/response behavior unchanged. Ensure `save_plugin_notification_result` still constructs `PluginNotificationResult` and calls:

```rust
state
    .store
    .save_plugin_notification_result(&result)
    .map_err(PluginNotificationResultError::System)?;
```

The storage layer now writes this into `notification_records`; handler should not know about table details.

- [ ] **Step 4: Update Tauri plugin notification test result helper**

In `src-tauri/src/commands.rs`, keep `plugin_notification_result(plugin_id, test_id)` call behavior. Verify returned `PluginNotificationResult` still has:

```rust
plugin_id
event_id
status
error_message
sent_at
```

No response-shape change should be introduced.

- [ ] **Step 5: Run API plugin notification tests**

Run:

```bash
cargo test -p niuma-api plugin_notification_results
```

Expected: PASS. Business failures continue returning `HTTP 200 + non-zero code`; system failures remain `HTTP 500 + 900001/900002` through existing response helpers.

- [ ] **Step 6: Commit**

```bash
git add crates/niuma-api/src/handlers.rs crates/niuma-api/src/tests.rs src-tauri/src/commands.rs
git commit -m "test: 保持通知历史接口兼容" -m "修改内容：更新 Local API 和 Tauri 通知历史相关测试与模型调用。" -m "修改原因：底层表合并后接口仍需保持现有 data.list 响应结构。"
```

---

### Task 6: Update Runtime Docs And Integration References

**Files:**
- Modify: `docs/integration/plugin-development_zh.md`
- Modify: `docs/integration/sse-external-integration.md`
- Modify: `docs/integration/sse-external-integration_zh.md`
- Optional Modify: `docs/superpowers/specs/2026-06-19-event-consumer-notification-plugins.md`
- Optional Modify: `docs/superpowers/plans/2026-06-18-plugin-parent-pid-watchdog.md`

- [ ] **Step 1: Update active integration docs**

Replace runtime docs references:

```text
NIUMA_STATE_PATH
```

with:

```text
NIUMA_DB_PATH
```

Use wording:

```markdown
| `NIUMA_DB_PATH` | 当前实例使用的 SQLite 通知历史数据库路径，仅用于诊断，不应直接写入。 |
```

For SSE docs, update troubleshooting text to say database path only affects notification history storage and diagnostics, not current event/session state.

- [ ] **Step 2: Decide whether to touch old superpowers docs**

Historical specs/plans can keep `NIUMA_STATE_PATH` as old-context records. Only update them if current project documentation lint requires no occurrences. Do not rewrite historical docs solely for aesthetics.

- [ ] **Step 3: Search for remaining runtime references**

Run:

```bash
rg "NIUMA_STATE_PATH|state.sqlite|state_path\\(" docs crates src-tauri builtin-plugins -n
```

Expected:

- No occurrences in runtime code.
- No occurrences in active integration docs except explicit historical notes.
- The new design spec may still mention old names as deprecated context.

- [ ] **Step 4: Commit**

```bash
git add docs/integration/plugin-development_zh.md docs/integration/sse-external-integration.md docs/integration/sse-external-integration_zh.md
git commit -m "docs: 更新数据库路径环境变量说明" -m "修改内容：运行文档从 NIUMA_STATE_PATH 更新为 NIUMA_DB_PATH。" -m "修改原因：SQLite 现在只表示通知历史数据库，不再表示完整状态库。"
```

---

### Task 7: Full Verification And Cleanup

**Files:**
- Review all files touched in Tasks 1-6.

- [ ] **Step 1: Run core tests**

Run:

```bash
cargo test -p niuma-core
```

Expected: PASS.

- [ ] **Step 2: Run API tests**

Run:

```bash
cargo test -p niuma-api
```

Expected: PASS.

- [ ] **Step 3: Run workspace tests**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 4: Verify old database is not touched by code**

Run:

```bash
rg "state.sqlite|NIUMA_STATE_PATH|app_settings|plugin_configs|plugin_notification_results|public_events|latest_activity|attention_items|CREATE TABLE IF NOT EXISTS sessions" crates src-tauri builtin-plugins -n
```

Expected:

- No runtime code creates old tables.
- No runtime code reads `NIUMA_STATE_PATH`.
- No notification code queries `plugin_notification_results`.
- Test names may mention old concepts only when explicitly asserting old behavior is gone.

- [ ] **Step 5: Inspect staged diff before any final commit**

Run:

```bash
git status --short
git diff --stat
```

Expected: only intended backend/docs files are changed. Existing unrelated frontend changes remain unstaged unless they belong to the current implementation.

- [ ] **Step 6: Final commit if needed**

If Tasks 1-6 were committed individually, do not create an extra commit. If some final cleanup files remain, commit only those:

Use explicit `git add` paths for any cleanup files shown by `git status --short`, then commit:

```bash
git commit -m "refactor: 完成通知数据库持久化收敛" -m "修改内容：补齐数据库路径、通知单表、JSON 配置和文档更新后的收尾调整。" -m "修改原因：确保实现与 niuma.sqlite 只保存通知历史的设计一致。"
```

---

## Self-Review

- Spec coverage:
  - `niuma.sqlite` 默认路径：Task 1。
  - `NIUMA_DB_PATH` 替代 `NIUMA_STATE_PATH`：Task 1、Task 6。
  - SQLite 只保留通知表：Task 2。
  - 内置和插件通知单表合并：Task 3。
  - 配置 JSON 持久化：Task 4。
  - 运行态内存化：Task 4 和已有内存运行态实现。
  - 旧 `state.sqlite` 不迁移、不删除、不读取：Task 1、Task 6、Task 7。
  - API/Tauri 响应兼容：Task 5。
- Placeholder scan:
  - 无 `TBD`、`TODO`、`implement later`。
  - 每个代码修改步骤都给出目标文件、关键代码或准确行为。
- Type consistency:
  - 统一通知模型使用 `NotificationNotifierType`、`notifier_id`、`notifier_type`。
  - API 历史返回继续使用 `channel` 和可选 `plugin_id`。
  - 配置文件模块只在 `SqliteStateStore` 门面内使用，调用方方法名保持兼容。
