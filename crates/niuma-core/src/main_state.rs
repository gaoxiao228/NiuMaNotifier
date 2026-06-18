use std::collections::HashMap;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;

use crate::event_display::{
    detail_from_event as display_detail_from_event, event_type_name_for_status,
    fallback_content_for_status, fallback_error_for_status, status_summary, EventDisplayDetail,
};
use crate::models::{
    AttentionItem, LatestActivity, NiumaEvent, NiumaSession, SessionStatus, ToolKind,
};
use crate::runtime_event::RuntimeEventBus;
use crate::store::SqliteStateStore;

const COMPLETED_RETENTION: chrono::Duration = chrono::Duration::minutes(1);
pub const MAIN_STATE_REFRESH_INTERVAL: StdDuration = StdDuration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MainStatePayload {
    pub version: u64,
    pub status: MainStateStatus,
    pub updated_at: Option<DateTime<Utc>>,
    pub session: Option<StateSession>,
    pub detail: Option<StateDetail>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MainStateStatus {
    WaitingApproval,
    WaitingInput,
    Running,
    Completed,
    Error,
    Idle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StateSession {
    pub id: String,
    pub tool: ToolKind,
    pub project_name: String,
    pub project_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StateDetail {
    pub event_id: String,
    pub event_type: String,
    pub severity: String,
    pub summary: String,
    pub content: Option<String>,
    pub error_message: Option<String>,
    pub payload_ref: Option<String>,
    pub completion_reason: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Clone)]
pub struct MainStateService {
    store: SqliteStateStore,
}

pub struct MainStateWatcher {
    runtime_events: tokio::sync::broadcast::Receiver<crate::runtime_event::RuntimeEvent>,
    refresh_interval: StdDuration,
}

impl MainStateWatcher {
    pub fn new(runtime_events: &RuntimeEventBus) -> Self {
        Self {
            runtime_events: runtime_events.subscribe(),
            refresh_interval: MAIN_STATE_REFRESH_INTERVAL,
        }
    }

    pub async fn wait_for_refresh(&mut self) -> bool {
        match tokio::time::timeout(self.refresh_interval, self.runtime_events.recv()).await {
            Ok(Ok(_)) | Ok(Err(RecvError::Lagged(_))) | Err(_) => true,
            Ok(Err(RecvError::Closed)) => false,
        }
    }
}

impl MainStateService {
    pub fn new(store: SqliteStateStore) -> Self {
        Self { store }
    }

    pub fn current_state(&self, now: DateTime<Utc>) -> Result<MainStatePayload, String> {
        if !self.store.listener_config()?.is_any_ai_listening_enabled() {
            return Ok(idle_payload());
        }

        let input = self.store.main_state_input()?;
        let state = input.state;
        let public_events = event_map(input.public_events);
        if let Some(item) = first_waiting_item(&state.attention_items) {
            return Ok(state_from_attention_item(
                item,
                MainStateStatus::from(&item.status),
                &state.sessions,
                &public_events,
            ));
        }
        if let Some(item) = first_error_item(&state.attention_items) {
            return Ok(state_from_attention_item(
                item,
                MainStateStatus::Error,
                &state.sessions,
                &public_events,
            ));
        }
        if let Some(activity) = state.latest_activity.as_ref() {
            return Ok(state_from_latest_activity(
                activity,
                &state.sessions,
                &public_events,
                now,
            ));
        }
        Ok(idle_payload())
    }
}

fn state_from_attention_item(
    item: &AttentionItem,
    status: MainStateStatus,
    sessions: &[NiumaSession],
    public_events: &HashMap<String, NiumaEvent>,
) -> MainStatePayload {
    let event = public_events.get(&item.event_id);
    let session = sessions
        .iter()
        .find(|session| session.id == item.session_id)
        .map(StateSession::from);
    MainStatePayload {
        version: 0,
        status,
        updated_at: Some(item.created_at),
        session,
        detail: Some(detail_from_attention_item(item, event)),
    }
}

fn state_from_latest_activity(
    activity: &LatestActivity,
    sessions: &[NiumaSession],
    public_events: &HashMap<String, NiumaEvent>,
    now: DateTime<Utc>,
) -> MainStatePayload {
    let status = MainStateStatus::from(&activity.status);
    if status == MainStateStatus::Idle {
        return idle_payload();
    }
    if status == MainStateStatus::Completed
        && activity
            .updated_at
            .map(|updated_at| now - updated_at > COMPLETED_RETENTION)
            .unwrap_or(false)
    {
        return idle_payload();
    }

    let event = activity
        .event_id
        .as_deref()
        .and_then(|event_id| public_events.get(event_id));
    let session = activity
        .session_id
        .as_deref()
        .and_then(|session_id| sessions.iter().find(|session| session.id == session_id))
        .map(StateSession::from);
    MainStatePayload {
        version: 0,
        status,
        updated_at: activity.updated_at,
        session,
        detail: activity_detail(activity, event),
    }
}

fn event_map(events: Vec<NiumaEvent>) -> HashMap<String, NiumaEvent> {
    events
        .into_iter()
        .map(|event| (event.id.clone(), event))
        .collect()
}

impl From<&NiumaSession> for StateSession {
    fn from(session: &NiumaSession) -> Self {
        Self {
            id: session.id.clone(),
            tool: session.tool.clone(),
            project_name: session.project_name.clone(),
            project_path: session.project_path.clone(),
        }
    }
}

impl From<&SessionStatus> for MainStateStatus {
    fn from(status: &SessionStatus) -> Self {
        match status {
            SessionStatus::WaitingApproval => MainStateStatus::WaitingApproval,
            SessionStatus::WaitingInput => MainStateStatus::WaitingInput,
            SessionStatus::Running => MainStateStatus::Running,
            SessionStatus::Completed => MainStateStatus::Completed,
            SessionStatus::Error => MainStateStatus::Error,
            SessionStatus::Idle | SessionStatus::Stale => MainStateStatus::Idle,
        }
    }
}

fn first_waiting_item(items: &[AttentionItem]) -> Option<&AttentionItem> {
    items.iter().find(|item| {
        matches!(
            item.status,
            SessionStatus::WaitingApproval | SessionStatus::WaitingInput
        )
    })
}

fn first_error_item(items: &[AttentionItem]) -> Option<&AttentionItem> {
    items
        .iter()
        .find(|item| item.status == SessionStatus::Error)
}

fn detail_from_attention_item(item: &AttentionItem, event: Option<&NiumaEvent>) -> StateDetail {
    match event {
        Some(event) => detail_from_event(event),
        None => StateDetail {
            event_id: item.event_id.clone(),
            event_type: event_type_name_for_status(&item.status).to_string(),
            severity: if item.status == SessionStatus::Error {
                "error"
            } else {
                "urgent"
            }
            .to_string(),
            summary: truncate(&item.summary, 200),
            content: fallback_content_for_status(&item.status, None, &item.summary),
            error_message: fallback_error_for_status(&item.status, None, &item.summary),
            payload_ref: None,
            completion_reason: None,
            failure_reason: None,
        },
    }
}

fn activity_detail(activity: &LatestActivity, event: Option<&NiumaEvent>) -> Option<StateDetail> {
    event
        .map(detail_from_event)
        .or_else(|| match activity.status {
            SessionStatus::Idle | SessionStatus::Stale => None,
            _ => activity.event_id.as_ref().map(|event_id| StateDetail {
                event_id: event_id.clone(),
                event_type: event_type_name_for_status(&activity.status).to_string(),
                severity: "info".to_string(),
                summary: status_summary(&activity.status).to_string(),
                content: fallback_content_for_status(
                    &activity.status,
                    None,
                    status_summary(&activity.status),
                ),
                error_message: fallback_error_for_status(
                    &activity.status,
                    None,
                    status_summary(&activity.status),
                ),
                payload_ref: None,
                completion_reason: None,
                failure_reason: None,
            }),
        })
}

fn detail_from_event(event: &NiumaEvent) -> StateDetail {
    display_detail_from_event(event).into()
}

fn idle_payload() -> MainStatePayload {
    MainStatePayload {
        version: 0,
        status: MainStateStatus::Idle,
        updated_at: None,
        session: None,
        detail: None,
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

impl From<EventDisplayDetail> for StateDetail {
    fn from(detail: EventDisplayDetail) -> Self {
        Self {
            event_id: detail.event_id,
            event_type: detail.event_type,
            severity: detail.severity,
            summary: detail.summary,
            content: detail.content,
            error_message: detail.error_message,
            payload_ref: detail.payload_ref,
            completion_reason: detail.completion_reason,
            failure_reason: detail.failure_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::listener_config::ListenerConfig;
    use crate::main_state::{MainStateService, MainStateStatus};
    use crate::models::{CompletionReason, EventType, FailureReason, NiumaEvent, ToolKind};
    use crate::store::SqliteStateStore;

    #[test]
    fn waiting_approval_uses_event_content_with_summary_fallback() {
        let store = SqliteStateStore::new(test_sqlite_path("waiting_approval_detail"));
        enable_codex_listener(&store);
        let mut event = sample_event(
            "approval-1",
            EventType::ApprovalRequested,
            "Bash: cargo test",
            1_000,
        );
        event.content = None;
        store.append_event(event).unwrap();

        let service = MainStateService::new(store);
        let state = service.current_state(at(2_000)).unwrap();

        assert_eq!(state.status, MainStateStatus::WaitingApproval);
        assert_eq!(state.version, 0);
        assert_eq!(state.session.unwrap().id, "s1");
        let detail = state.detail.unwrap();
        assert_eq!(detail.event_id, "approval-1");
        assert_eq!(detail.content.as_deref(), Some("Bash: cargo test"));
        assert_eq!(detail.summary, "Bash: cargo test");
    }

    #[test]
    fn completed_expires_to_idle_after_one_minute() {
        let store = SqliteStateStore::new(test_sqlite_path("completed_expiry"));
        enable_codex_listener(&store);
        let mut event = sample_event(
            "completed-1",
            EventType::AssistantMessageCompleted,
            "Codex task completed",
            1_000,
        );
        event.content = Some("最终回复正文".to_string());
        event.completion_reason = Some(CompletionReason::Normal);
        store.append_event(event).unwrap();

        let service = MainStateService::new(store);
        let fresh = service.current_state(at(1_000 + 59)).unwrap();
        assert_eq!(fresh.status, MainStateStatus::Completed);
        assert_eq!(
            fresh.detail.unwrap().content.as_deref(),
            Some("最终回复正文")
        );

        let expired = service.current_state(at(1_000 + 61)).unwrap();
        assert_eq!(expired.status, MainStateStatus::Idle);
        assert!(expired.session.is_none());
        assert!(expired.detail.is_none());
    }

    #[test]
    fn error_does_not_expire_and_uses_error_message_fallback() {
        let store = SqliteStateStore::new(test_sqlite_path("error_no_expiry"));
        enable_codex_listener(&store);
        let mut event = sample_event("failed-1", EventType::TaskFailed, "请求失败", 1_000);
        event.error_message = None;
        event.failure_reason = Some(FailureReason::ResponseStreamFailed);
        store.append_event(event).unwrap();

        let service = MainStateService::new(store);
        let state = service.current_state(at(1_000 + 600)).unwrap();

        assert_eq!(state.status, MainStateStatus::Error);
        let detail = state.detail.unwrap();
        assert_eq!(detail.error_message.as_deref(), Some("请求失败"));
        assert_eq!(
            detail.failure_reason.as_deref(),
            Some("response_stream_failed")
        );
    }

    #[test]
    fn stale_is_not_exposed_as_public_status() {
        let store = SqliteStateStore::new(test_sqlite_path("stale_hidden"));
        enable_codex_listener(&store);
        store
            .append_event(sample_event(
                "running-1",
                EventType::SessionStarted,
                "Codex started",
                1_000,
            ))
            .unwrap();
        store
            .append_event(sample_event(
                "stale-1",
                EventType::SessionStaled,
                "Codex session became stale",
                2_000,
            ))
            .unwrap();

        let service = MainStateService::new(store);
        let state = service.current_state(at(3_000)).unwrap();

        assert_eq!(state.status, MainStateStatus::Idle);
        assert!(state.session.is_none());
        assert!(state.detail.is_none());
    }

    #[test]
    fn disabled_ai_listeners_force_main_state_to_idle() {
        let store = SqliteStateStore::new(test_sqlite_path("listeners_disabled_idle"));
        store
            .append_event(sample_event(
                "approval-disabled",
                EventType::ApprovalRequested,
                "Bash: cargo test",
                1_000,
            ))
            .unwrap();

        let service = MainStateService::new(store);
        let state = service.current_state(at(2_000)).unwrap();

        assert_eq!(state.status, MainStateStatus::Idle);
        assert!(state.session.is_none());
        assert!(state.detail.is_none());
    }

    #[test]
    fn enabled_ai_listener_uses_existing_main_state_priority() {
        let store = SqliteStateStore::new(test_sqlite_path("listener_enabled_priority"));
        enable_codex_listener(&store);
        store
            .append_event(sample_event(
                "approval-enabled",
                EventType::ApprovalRequested,
                "Bash: cargo test",
                1_000,
            ))
            .unwrap();

        let service = MainStateService::new(store);
        let state = service.current_state(at(2_000)).unwrap();

        assert_eq!(state.status, MainStateStatus::WaitingApproval);
        assert_eq!(
            state.detail.unwrap().event_id,
            "approval-enabled".to_string()
        );
    }

    fn sample_event(id: &str, event_type: EventType, summary: &str, timestamp: i64) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: format!("dedupe-{id}"),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: "s1".to_string(),
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            event_type,
            severity: "urgent".to_string(),
            summary: summary.to_string(),
            content: Some(format!("detail: {summary}")),
            error_message: Some(format!("error: {summary}")),
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            created_at: at(timestamp),
        }
    }

    fn at(timestamp: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(timestamp, 0).single().unwrap()
    }

    fn enable_codex_listener(store: &SqliteStateStore) {
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: true,
                ..ListenerConfig::default()
            })
            .unwrap();
    }

    fn test_sqlite_path(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "niuma-main-state-{name}-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }
}
