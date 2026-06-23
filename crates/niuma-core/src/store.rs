use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::listener_config::ListenerConfig;
use crate::models::{
    ApprovalDecisionKind, ApprovalRequest, AttentionItem, EventType, InternalStateSnapshot,
    LatestActivity, NiumaEvent, RuntimeStateItem, RuntimeStateStatus, ToolKind,
};
use crate::notification_store::{
    insert_record_if_absent, load_history_records, load_plugin_result, load_records,
    update_record_result, upsert_plugin_result, NotificationHistoryRecord, NotificationRecord,
    NotificationRecordStatus, PluginNotificationResult,
};
use crate::platform::locale::LanguagePreference;
use crate::plugin::PluginRuntimeState;
use crate::state::InternalStateEngine;

mod config_files;
mod schema;
mod transitions;

use config_files::ConfigFileStore;
use schema::init_schema;
use transitions::{
    already_applied, apply_attention_transition, is_late_terminal_activity, upsert_runtime_state,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredState {
    #[serde(default)]
    pub events: Vec<NiumaEvent>,
    #[serde(default)]
    pub runtime_states: Vec<RuntimeStateItem>,
    #[serde(default)]
    pub attention_items: Vec<AttentionItem>,
    #[serde(default)]
    pub latest_activity: Option<LatestActivity>,
    #[serde(default)]
    pub approval_requests: Vec<ApprovalRequest>,
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            runtime_states: Vec::new(),
            attention_items: Vec::new(),
            latest_activity: Some(LatestActivity::idle()),
            approval_requests: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct NiumaStore {
    path: PathBuf,
    runtime: Arc<Mutex<RuntimeState>>,
}

#[derive(Clone, Debug)]
struct RuntimeState {
    state: StoredState,
    public_events: Vec<NiumaEvent>,
    dedupe_keys: HashSet<String>,
    plugin_runtime_states: HashMap<String, PluginRuntimeState>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            state: StoredState::default(),
            public_events: Vec::new(),
            dedupe_keys: HashSet::new(),
            plugin_runtime_states: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DismissAttentionResult {
    pub dismissed_count: usize,
    pub event: NiumaEvent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendEventsResult {
    pub state: StoredState,
    pub applied_events: Vec<NiumaEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaleSweepResult {
    pub state: StoredState,
    pub staled_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MainStateInput {
    pub state: StoredState,
    pub public_events: Vec<NiumaEvent>,
}

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(10);
// 运行态只保留最近事件作为调试/API 快照，当前主状态引用的事件会额外保留。
const MAX_PUBLIC_EVENT_CACHE_SIZE: usize = 200;

impl NiumaStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            runtime: Arc::new(Mutex::new(RuntimeState::default())),
        }
    }

    pub fn default_path() -> PathBuf {
        crate::config::db_path()
    }

    fn config_files(&self) -> ConfigFileStore {
        ConfigFileStore::new(&self.path)
    }

    pub fn load(&self) -> Result<StoredState, String> {
        let _connection = self.open()?;
        Ok(self.runtime()?.state.clone())
    }

    pub fn append_event(&self, event: NiumaEvent) -> Result<StoredState, String> {
        self.append_events(vec![event])
    }

    pub fn append_events(&self, events: Vec<NiumaEvent>) -> Result<StoredState, String> {
        Ok(self.append_events_with_result(events)?.state)
    }

    pub fn append_events_with_result(
        &self,
        events: Vec<NiumaEvent>,
    ) -> Result<AppendEventsResult, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        let mut applied_events = Vec::new();
        for event in events {
            if self.event_already_applied(&runtime, &state, &event) {
                continue;
            }
            if is_late_terminal_activity(&state.runtime_states, &event) {
                continue;
            }
            runtime.dedupe_keys.insert(event.dedupe_key.clone());
            if is_public_event(&event) {
                runtime.public_events.push(event.clone());
            }
            upsert_runtime_state(&mut state.runtime_states, &event);
            apply_attention_transition(&mut state, &event);
            applied_events.push(event);
        }
        runtime.state = state.clone();
        trim_public_event_cache(&mut runtime);
        Ok(AppendEventsResult {
            state,
            applied_events,
        })
    }

    fn event_already_applied(
        &self,
        runtime: &RuntimeState,
        state: &StoredState,
        event: &NiumaEvent,
    ) -> bool {
        if already_applied(state, event) {
            return true;
        }
        // 不同事件源可能生成不同 id，但共享同一 dedupe_key；进程内缓存避免双路事件重复广播。
        runtime.dedupe_keys.contains(&event.dedupe_key)
    }

    pub fn mark_stale_running_sessions(
        &self,
        now: chrono::DateTime<Utc>,
        timeout: chrono::Duration,
    ) -> Result<StoredState, String> {
        Ok(self
            .mark_stale_running_sessions_with_result(now, timeout)?
            .state)
    }

    pub fn mark_stale_running_sessions_with_result(
        &self,
        now: chrono::DateTime<Utc>,
        timeout: chrono::Duration,
    ) -> Result<StaleSweepResult, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        let events = state
            .runtime_states
            .iter()
            .filter(|session| session.status == RuntimeStateStatus::Running)
            .filter(|session| now - session.last_activity_at >= timeout)
            .map(|session| {
                let tool_key = session.tool.as_str();
                NiumaEvent {
                    // 同一 session_id 可同时存在于多个工具，stale 事件也必须带 tool 防止去重冲突。
                    id: format!(
                        "event_stale_{}_{}_{}",
                        tool_key,
                        session.session_id,
                        now.timestamp_millis()
                    ),
                    dedupe_key: format!(
                        "stale:{}:{}:{}",
                        tool_key,
                        session.session_id,
                        now.timestamp()
                    ),
                    source: "niuma-session-stale-sweeper".to_string(),
                    tool: session.tool.clone(),
                    session_id: session.session_id.clone(),
                    parent_session_id: None,
                    project_path: session.project_path.clone(),
                    project_name: session.project_name.clone(),
                    event_type: EventType::SessionStaled,
                    severity: "info".to_string(),
                    // 摘要带上 tool，避免 ClaudeCode 等其他工具的 stale 事件显示成 Codex。
                    summary: format!("{} session became stale", tool_key),
                    content: None,
                    error_message: None,
                    attention_resolve_key: None,
                    completion_reason: None,
                    failure_reason: None,
                    payload_ref: None,
                    created_at: now,
                }
            })
            .collect::<Vec<_>>();

        let mut staled_count = 0;
        for event in events {
            if self.event_already_applied(&runtime, &state, &event) {
                continue;
            }
            runtime.dedupe_keys.insert(event.dedupe_key.clone());
            upsert_runtime_state(&mut state.runtime_states, &event);
            apply_attention_transition(&mut state, &event);
            staled_count += 1;
        }

        runtime.state = state.clone();
        Ok(StaleSweepResult {
            state,
            staled_count,
        })
    }

    pub fn internal_status_snapshot(&self) -> Result<InternalStateSnapshot, String> {
        let state = self.load()?;
        Ok(InternalStateEngine::aggregate(
            &state.attention_items,
            state.latest_activity.as_ref(),
            &state.events,
        ))
    }

    pub fn main_state_input(&self) -> Result<MainStateInput, String> {
        let runtime = self.runtime()?;
        let state = runtime.state.clone();
        let mut public_events = Vec::new();
        for event_id in main_state_event_ids(&state) {
            if let Some(event) = runtime
                .public_events
                .iter()
                .find(|event| event.id == event_id)
                .cloned()
            {
                public_events.push(event);
            }
        }
        Ok(MainStateInput {
            state,
            public_events,
        })
    }

    pub fn runtime_state_list(&self) -> Result<Vec<RuntimeStateItem>, String> {
        Ok(self.load()?.runtime_states)
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<NiumaEvent>, String> {
        // 保留旧方法名作为兼容门面，实际列表统一读取内存公开事件缓存。
        self.public_recent_events(limit)
    }

    pub fn public_recent_events(&self, limit: usize) -> Result<Vec<NiumaEvent>, String> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut events = self.runtime()?.public_events.clone();
        sort_events_newest_first(&mut events);
        events.truncate(limit);
        Ok(events)
    }

    pub fn public_event_by_id(&self, event_id: &str) -> Result<Option<NiumaEvent>, String> {
        Ok(self
            .runtime()?
            .public_events
            .iter()
            .find(|event| event.id == event_id)
            .cloned())
    }

    pub fn approval_request(&self, request_id: &str) -> Result<Option<ApprovalRequest>, String> {
        Ok(self
            .runtime()?
            .state
            .approval_requests
            .iter()
            .find(|request| request.id == request_id)
            .cloned())
    }

    pub fn approval_requests(&self) -> Result<Vec<ApprovalRequest>, String> {
        Ok(self.runtime()?.state.approval_requests.clone())
    }

    pub fn upsert_approval_request(&self, request: ApprovalRequest) -> Result<StoredState, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        crate::approval::upsert_request(&mut state.approval_requests, request);
        runtime.state = state.clone();
        Ok(state)
    }

    pub fn decide_approval(
        &self,
        request_id: &str,
        decision: ApprovalDecisionKind,
        decided_by: &str,
        decided_source: &str,
        reason: Option<String>,
        now: chrono::DateTime<Utc>,
    ) -> Result<crate::approval::ApprovalMutationResult, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        let result = crate::approval::decide(
            &mut state.approval_requests,
            request_id,
            decision,
            decided_by,
            decided_source,
            reason,
            now,
        )?;
        runtime.state = state;
        Ok(result)
    }

    pub fn return_approval_to_codex(
        &self,
        request_id: &str,
        returned_by: &str,
        returned_source: &str,
        reason: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<crate::approval::ApprovalMutationResult, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        let result = crate::approval::return_to_codex(
            &mut state.approval_requests,
            request_id,
            returned_by,
            returned_source,
            reason,
            now,
        )?;
        runtime.state = state;
        Ok(result)
    }

    pub fn heartbeat_approval_proxy(
        &self,
        request_id: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<crate::approval::ApprovalMutationResult, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        let result =
            crate::approval::heartbeat_proxy(&mut state.approval_requests, request_id, now)?;
        runtime.state = state;
        Ok(result)
    }

    pub fn return_stale_approval_proxies_to_codex(
        &self,
        now: chrono::DateTime<Utc>,
        stale_after: chrono::Duration,
    ) -> Result<Vec<crate::approval::ApprovalMutationResult>, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        let results = crate::approval::return_stale_proxies_to_codex(
            &mut state.approval_requests,
            now,
            stale_after,
        );
        runtime.state = state;
        Ok(results)
    }

    pub fn insert_notification_record_if_absent(
        &self,
        record: &NotificationRecord,
    ) -> Result<bool, String> {
        let connection = self.open()?;
        insert_record_if_absent(&connection, record)
    }

    pub fn update_notification_record_result(
        &self,
        record_id: &str,
        status: NotificationRecordStatus,
        error_message: Option<String>,
        sent_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), String> {
        let connection = self.open()?;
        update_record_result(
            &connection,
            record_id,
            &status,
            error_message.as_deref(),
            sent_at,
        )
    }

    pub fn notification_records(&self, limit: usize) -> Result<Vec<NotificationRecord>, String> {
        let connection = self.open()?;
        load_records(&connection, limit)
    }

    pub fn save_plugin_notification_result(
        &self,
        result: &PluginNotificationResult,
    ) -> Result<(), String> {
        let connection = self.open()?;
        upsert_plugin_result(&connection, result)
    }

    pub fn plugin_notification_result(
        &self,
        plugin_id: &str,
        event_id: &str,
    ) -> Result<Option<PluginNotificationResult>, String> {
        let connection = self.open()?;
        load_plugin_result(&connection, plugin_id, event_id)
    }

    pub fn notification_history_records(
        &self,
        limit: usize,
    ) -> Result<Vec<NotificationHistoryRecord>, String> {
        let connection = self.open()?;
        load_history_records(&connection, limit)
    }

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

    pub fn plugin_runtime_states(&self) -> Result<HashMap<String, PluginRuntimeState>, String> {
        Ok(self.runtime()?.plugin_runtime_states.clone())
    }

    pub fn save_plugin_runtime_state(
        &self,
        plugin_id: &str,
        runtime_state: PluginRuntimeState,
    ) -> Result<(), String> {
        self.runtime()?
            .plugin_runtime_states
            .insert(plugin_id.to_string(), runtime_state);
        Ok(())
    }

    pub fn remove_plugin_runtime_state(&self, plugin_id: &str) -> Result<(), String> {
        self.runtime()?.plugin_runtime_states.remove(plugin_id);
        Ok(())
    }

    pub fn clear_tool_state(&self, tool: &ToolKind) -> Result<StoredState, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        state.runtime_states.retain(|session| &session.tool != tool);
        state
            .approval_requests
            .retain(|request| &request.tool != tool);
        state.attention_items.retain(|item| &item.tool != tool);
        if state
            .latest_activity
            .as_ref()
            .and_then(|activity| activity.tool.as_ref())
            .map(|activity_tool| activity_tool == tool)
            .unwrap_or(false)
        {
            state.latest_activity = Some(LatestActivity::idle());
        }

        runtime.state = state.clone();
        Ok(state)
    }

    pub fn reset(&self) -> Result<StoredState, String> {
        let state = StoredState::default();
        *self.runtime()? = RuntimeState::default();
        Ok(state)
    }

    pub fn dismiss_active_blocker(&self) -> Result<Option<DismissAttentionResult>, String> {
        let mut runtime = self.runtime()?;
        let mut state = runtime.state.clone();
        let dismissed_count = state.attention_items.len();
        if dismissed_count == 0 {
            return Ok(None);
        }

        let now = Utc::now();
        let event = NiumaEvent {
            id: format!("event_manual_dismissed_{}", now.timestamp_millis()),
            dedupe_key: format!("manual_dismissed:{}", now.timestamp_millis()),
            source: "user".to_string(),
            tool: ToolKind::Codex,
            session_id: "all-attention-items".to_string(),
            parent_session_id: None,
            project_path: String::new(),
            project_name: "NiumaNotifier".to_string(),
            event_type: EventType::ManualDismissed,
            severity: "info".to_string(),
            summary: "User marked all attention items as handled".to_string(),
            content: None,
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            created_at: now,
        };
        state.attention_items.clear();
        runtime.state = state;
        Ok(Some(DismissAttentionResult {
            dismissed_count,
            event,
        }))
    }

    fn open(&self) -> Result<Connection, String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建应用数据目录失败：{error}"))?;
        }
        let connection = Connection::open(&self.path)
            .map_err(|error| format!("打开 SQLite 通知库失败：{error}"))?;
        configure_connection(&connection)?;
        init_schema(&connection)?;
        Ok(connection)
    }

    fn runtime(&self) -> Result<std::sync::MutexGuard<'_, RuntimeState>, String> {
        self.runtime
            .lock()
            .map_err(|_| "运行态内存锁已损坏".to_string())
    }
}

fn configure_connection(connection: &Connection) -> Result<(), String> {
    // WAL 允许读连接和写连接更好地并发；busy_timeout 让短暂写锁等待释放，而不是立刻报 database is locked。
    connection
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
        .map_err(|error| format!("配置 SQLite busy timeout 失败：{error}"))?;
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(|error| format!("启用 SQLite WAL 失败：{error}"))?;
    if journal_mode.to_lowercase() != "wal" {
        return Err(format!(
            "启用 SQLite WAL 失败：当前 journal_mode={journal_mode}"
        ));
    }
    connection
        .execute_batch("PRAGMA synchronous = NORMAL;")
        .map_err(|error| format!("配置 SQLite synchronous 失败：{error}"))?;
    Ok(())
}

fn main_state_event_ids(state: &StoredState) -> Vec<String> {
    let mut ids = Vec::new();
    for item in &state.attention_items {
        push_unique(&mut ids, item.event_id.clone());
    }
    if let Some(event_id) = state
        .latest_activity
        .as_ref()
        .and_then(|activity| activity.event_id.as_ref())
    {
        push_unique(&mut ids, event_id.clone());
    }
    ids
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn is_public_event(event: &NiumaEvent) -> bool {
    !matches!(
        event.event_type,
        EventType::SessionActivity | EventType::SessionStaled
    )
}

fn sort_events_newest_first(events: &mut [NiumaEvent]) {
    events.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
}

fn trim_public_event_cache(runtime: &mut RuntimeState) {
    if runtime.public_events.len() <= MAX_PUBLIC_EVENT_CACHE_SIZE {
        return;
    }
    let active_event_ids = main_state_event_ids(&runtime.state)
        .into_iter()
        .collect::<HashSet<_>>();
    sort_events_newest_first(&mut runtime.public_events);
    let mut kept = Vec::with_capacity(MAX_PUBLIC_EVENT_CACHE_SIZE);
    for event in runtime.public_events.drain(..) {
        if kept.len() < MAX_PUBLIC_EVENT_CACHE_SIZE || active_event_ids.contains(&event.id) {
            kept.push(event);
        }
    }
    runtime.public_events = kept;
}

#[cfg(test)]
mod tests;
