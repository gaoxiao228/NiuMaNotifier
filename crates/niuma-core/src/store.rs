use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::listener_config::ListenerConfig;
use crate::models::{
    AttentionItem, EventType, InternalStateSnapshot, LatestActivity, NiumaEvent, NiumaSession,
    SessionStatus, ToolKind,
};
use crate::notification_store::{
    insert_record_if_absent, load_channels, load_records, save_channels, update_record_result,
    NotificationChannelConfig, NotificationRecord, NotificationRecordStatus,
};
use crate::platform::locale::LanguagePreference;
use crate::state::InternalStateEngine;

mod persistence;
mod public_events;
mod schema;
mod transitions;

use persistence::{load_json_rows, save_state};
use public_events::{
    append_public_event, clear_public_events, load_public_event_by_id, load_public_events,
};
use schema::init_schema;
use transitions::{
    already_applied, apply_attention_transition, is_late_terminal_activity, upsert_session,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredState {
    #[serde(default)]
    pub events: Vec<NiumaEvent>,
    #[serde(default)]
    pub sessions: Vec<NiumaSession>,
    #[serde(default)]
    pub attention_items: Vec<AttentionItem>,
    #[serde(default)]
    pub latest_activity: Option<LatestActivity>,
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            sessions: Vec::new(),
            attention_items: Vec::new(),
            latest_activity: Some(LatestActivity::idle()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SqliteStateStore {
    path: PathBuf,
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

const LISTENER_CONFIG_KEY: &str = "listener_config";
const LANGUAGE_PREFERENCE_KEY: &str = "language_preference";
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(10);

impl SqliteStateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        crate::config::state_path()
    }

    pub fn load(&self) -> Result<StoredState, String> {
        let connection = self.open()?;
        self.load_with_connection(&connection)
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
        let mut connection = self.open()?;
        let tx = connection
            .transaction()
            .map_err(|error| format!("开启 SQLite 事务失败：{error}"))?;
        let mut state = self.load_with_connection(&tx)?;
        let mut applied_events = Vec::new();
        for event in events {
            if already_applied(&state, &event) {
                continue;
            }
            if is_late_terminal_activity(&state.sessions, &event) {
                continue;
            }
            append_public_event(&tx, &event)?;
            upsert_session(&mut state.sessions, &event);
            apply_attention_transition(&mut state, &event);
            applied_events.push(event);
        }
        save_state(&tx, &state)?;
        tx.commit()
            .map_err(|error| format!("提交 SQLite 事务失败：{error}"))?;
        Ok(AppendEventsResult {
            state,
            applied_events,
        })
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
        let mut connection = self.open()?;
        let tx = connection
            .transaction()
            .map_err(|error| format!("开启 SQLite 事务失败：{error}"))?;
        let mut state = self.load_with_connection(&tx)?;
        let events = state
            .sessions
            .iter()
            .filter(|session| session.status == SessionStatus::Running)
            .filter(|session| now - session.last_activity_at >= timeout)
            .map(|session| NiumaEvent {
                id: format!("event_stale_{}_{}", session.id, now.timestamp_millis()),
                dedupe_key: format!("stale:{}:{}", session.id, now.timestamp()),
                source: "codex-session-stale-sweeper".to_string(),
                tool: session.tool.clone(),
                session_id: session.id.clone(),
                project_path: session.project_path.clone(),
                project_name: session.project_name.clone(),
                event_type: EventType::SessionStaled,
                severity: "info".to_string(),
                summary: "Codex session became stale".to_string(),
                content: None,
                error_message: None,
                attention_resolve_key: None,
                completion_reason: None,
                failure_reason: None,
                payload_ref: None,
                created_at: now,
            })
            .collect::<Vec<_>>();

        let mut staled_count = 0;
        for event in events {
            if already_applied(&state, &event) {
                continue;
            }
            upsert_session(&mut state.sessions, &event);
            apply_attention_transition(&mut state, &event);
            staled_count += 1;
        }

        save_state(&tx, &state)?;
        tx.commit()
            .map_err(|error| format!("提交 SQLite 事务失败：{error}"))?;
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
        let connection = self.open()?;
        let state = self.load_with_connection(&connection)?;
        let mut public_events = Vec::new();
        for event_id in main_state_event_ids(&state) {
            if let Some(event) = load_public_event_by_id(&connection, &event_id)? {
                public_events.push(event);
            }
        }
        Ok(MainStateInput {
            state,
            public_events,
        })
    }

    pub fn sessions(&self) -> Result<Vec<NiumaSession>, String> {
        Ok(self.load()?.sessions)
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<NiumaEvent>, String> {
        // 保留旧方法名作为兼容门面，实际列表统一读取公开事件表。
        self.public_recent_events(limit)
    }

    pub fn public_recent_events(&self, limit: usize) -> Result<Vec<NiumaEvent>, String> {
        let connection = self.open()?;
        load_public_events(&connection, limit)
    }

    pub fn public_event_by_id(&self, event_id: &str) -> Result<Option<NiumaEvent>, String> {
        let connection = self.open()?;
        load_public_event_by_id(&connection, event_id)
    }

    pub fn notification_channels(&self) -> Result<Vec<NotificationChannelConfig>, String> {
        let connection = self.open()?;
        load_channels(&connection)
    }

    pub fn save_notification_channels(
        &self,
        configs: Vec<NotificationChannelConfig>,
    ) -> Result<(), String> {
        let connection = self.open()?;
        // 按 channel id upsert 传入配置；未传入的渠道配置会保留。
        save_channels(&connection, &configs)
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

    pub fn listener_config(&self) -> Result<ListenerConfig, String> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT payload FROM app_settings WHERE key = ?1",
                [LISTENER_CONFIG_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("读取监听配置失败：{error}"))?;
        match payload {
            Some(payload) => {
                serde_json::from_str(&payload).map_err(|error| format!("解析监听配置失败：{error}"))
            }
            None => Ok(ListenerConfig::default()),
        }
    }

    pub fn save_listener_config(&self, config: &ListenerConfig) -> Result<(), String> {
        let connection = self.open()?;
        let payload = serde_json::to_string(config)
            .map_err(|error| format!("序列化监听配置失败：{error}"))?;
        connection
            .execute(
                "INSERT INTO app_settings (key, payload, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    payload = excluded.payload,
                    updated_at = excluded.updated_at",
                params![LISTENER_CONFIG_KEY, payload, Utc::now().to_rfc3339()],
            )
            .map_err(|error| format!("保存监听配置失败：{error}"))?;
        Ok(())
    }

    pub fn language_preference(&self) -> Result<LanguagePreference, String> {
        let connection = self.open()?;
        let payload = connection
            .query_row(
                "SELECT payload FROM app_settings WHERE key = ?1",
                [LANGUAGE_PREFERENCE_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("读取语言偏好失败：{error}"))?;
        let Some(payload) = payload else {
            return Ok(LanguagePreference::System);
        };
        let value = serde_json::from_str::<serde_json::Value>(&payload)
            .map_err(|error| format!("解析语言偏好失败：{error}"))?;
        let preference = value
            .get("preference")
            .and_then(serde_json::Value::as_str)
            .or_else(|| value.as_str())
            .ok_or_else(|| "语言偏好格式无效".to_string())?;
        LanguagePreference::from_storage_id(preference)
            .ok_or_else(|| format!("未知语言偏好：{preference}"))
    }

    pub fn save_language_preference(&self, preference: LanguagePreference) -> Result<(), String> {
        let connection = self.open()?;
        let payload = serde_json::json!({
            "preference": preference.storage_id()
        })
        .to_string();
        connection
            .execute(
                "INSERT INTO app_settings (key, payload, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    payload = excluded.payload,
                    updated_at = excluded.updated_at",
                params![LANGUAGE_PREFERENCE_KEY, payload, Utc::now().to_rfc3339()],
            )
            .map_err(|error| format!("保存语言偏好失败：{error}"))?;
        Ok(())
    }

    pub fn clear_tool_state(&self, tool: &ToolKind) -> Result<StoredState, String> {
        let mut connection = self.open()?;
        let tx = connection
            .transaction()
            .map_err(|error| format!("开启 SQLite 事务失败：{error}"))?;
        let mut state = self.load_with_connection(&tx)?;
        let removed_session_ids = state
            .sessions
            .iter()
            .filter(|session| &session.tool == tool)
            .map(|session| session.id.clone())
            .collect::<std::collections::HashSet<_>>();

        state.sessions.retain(|session| &session.tool != tool);
        state
            .attention_items
            .retain(|item| !removed_session_ids.contains(&item.session_id));
        if state
            .latest_activity
            .as_ref()
            .and_then(|activity| activity.session_id.as_deref())
            .map(|session_id| removed_session_ids.contains(session_id))
            .unwrap_or(false)
        {
            state.latest_activity = Some(LatestActivity::idle());
        }

        save_state(&tx, &state)?;
        tx.commit()
            .map_err(|error| format!("提交 SQLite 事务失败：{error}"))?;
        Ok(state)
    }

    pub fn reset(&self) -> Result<StoredState, String> {
        let state = StoredState::default();
        let mut connection = self.open()?;
        let tx = connection
            .transaction()
            .map_err(|error| format!("开启 SQLite 事务失败：{error}"))?;
        save_state(&tx, &state)?;
        clear_public_events(&tx)?;
        tx.commit()
            .map_err(|error| format!("提交 SQLite 事务失败：{error}"))?;
        Ok(state)
    }

    pub fn dismiss_active_blocker(&self) -> Result<Option<DismissAttentionResult>, String> {
        let mut connection = self.open()?;
        let tx = connection
            .transaction()
            .map_err(|error| format!("开启 SQLite 事务失败：{error}"))?;
        let mut state = self.load_with_connection(&tx)?;
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
        save_state(&tx, &state)?;
        tx.commit()
            .map_err(|error| format!("提交 SQLite 事务失败：{error}"))?;
        Ok(Some(DismissAttentionResult {
            dismissed_count,
            event,
        }))
    }

    fn open(&self) -> Result<Connection, String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建状态目录失败：{error}"))?;
        }
        let connection = Connection::open(&self.path)
            .map_err(|error| format!("打开 SQLite 状态库失败：{error}"))?;
        configure_connection(&connection)?;
        init_schema(&connection)?;
        Ok(connection)
    }

    fn load_with_connection(&self, connection: &Connection) -> Result<StoredState, String> {
        let sessions =
            load_json_rows::<NiumaSession>(connection, "sessions", "last_activity_at ASC")?;
        let attention_items =
            load_json_rows::<AttentionItem>(connection, "attention_items", "created_at ASC")?;
        let latest_activity = connection
            .query_row(
                "SELECT payload FROM latest_activity WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("读取 latest_activity 失败：{error}"))?
            .map(|payload| serde_json::from_str::<LatestActivity>(&payload))
            .transpose()
            .map_err(|error| format!("解析 latest_activity 失败：{error}"))?;

        Ok(StoredState {
            events: Vec::new(),
            sessions,
            attention_items,
            latest_activity: latest_activity.or_else(|| Some(LatestActivity::idle())),
        })
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

#[cfg(test)]
mod tests;
