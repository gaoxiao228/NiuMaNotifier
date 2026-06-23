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
    ApprovalRequest, ApprovalStatus, AttentionItem, LatestActivity, NiumaEvent, RuntimeStateItem,
    RuntimeStateStatus, ToolKind,
};
use crate::runtime_event::RuntimeEventBus;
use crate::store::NiumaStore;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<StateApprovalDetail>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StateApprovalDetail {
    pub request_id: String,
    pub status: ApprovalStatus,
    pub can_decide: bool,
    pub message: Option<String>,
    pub decided_by: Option<String>,
    pub decided_source: Option<String>,
}

#[derive(Clone)]
pub struct MainStateService {
    store: NiumaStore,
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
    pub fn new(store: NiumaStore) -> Self {
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
                &state.runtime_states,
                &public_events,
                &state.approval_requests,
            ));
        }
        if let Some(item) = first_error_item(&state.attention_items) {
            return Ok(state_from_attention_item(
                item,
                MainStateStatus::Error,
                &state.runtime_states,
                &public_events,
                &state.approval_requests,
            ));
        }
        if let Some(activity) = state.latest_activity.as_ref() {
            return Ok(state_from_latest_activity(
                activity,
                &state.runtime_states,
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
    runtime_states: &[RuntimeStateItem],
    public_events: &HashMap<String, NiumaEvent>,
    approval_requests: &[ApprovalRequest],
) -> MainStatePayload {
    let event = public_events.get(&item.event_id);
    let session = runtime_states
        .iter()
        .find(|state| state.tool == item.tool && state.session_id == item.session_id)
        .map(StateSession::from);
    MainStatePayload {
        version: 0,
        status,
        updated_at: Some(item.created_at),
        session,
        detail: Some(detail_from_attention_item(item, event, approval_requests)),
    }
}

fn state_from_latest_activity(
    activity: &LatestActivity,
    runtime_states: &[RuntimeStateItem],
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
        .tool
        .as_ref()
        .zip(activity.session_id.as_deref())
        // latest activity 来源于具体事件，必须用 tool + session_id 找回同一个运行态。
        .and_then(|(tool, session_id)| {
            runtime_states
                .iter()
                .find(|state| &state.tool == tool && state.session_id == session_id)
        })
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

impl From<&RuntimeStateItem> for StateSession {
    fn from(session: &RuntimeStateItem) -> Self {
        Self {
            id: session.session_id.clone(),
            tool: session.tool.clone(),
            project_name: session.project_name.clone(),
            project_path: session.project_path.clone(),
        }
    }
}

impl From<&RuntimeStateStatus> for MainStateStatus {
    fn from(status: &RuntimeStateStatus) -> Self {
        match status {
            RuntimeStateStatus::WaitingApproval => MainStateStatus::WaitingApproval,
            RuntimeStateStatus::WaitingInput => MainStateStatus::WaitingInput,
            RuntimeStateStatus::Running => MainStateStatus::Running,
            RuntimeStateStatus::Completed => MainStateStatus::Completed,
            RuntimeStateStatus::Error => MainStateStatus::Error,
            RuntimeStateStatus::Idle | RuntimeStateStatus::Stale => MainStateStatus::Idle,
        }
    }
}

fn first_waiting_item(items: &[AttentionItem]) -> Option<&AttentionItem> {
    items.iter().find(|item| {
        matches!(
            item.status,
            RuntimeStateStatus::WaitingApproval | RuntimeStateStatus::WaitingInput
        )
    })
}

fn first_error_item(items: &[AttentionItem]) -> Option<&AttentionItem> {
    items
        .iter()
        .find(|item| item.status == RuntimeStateStatus::Error)
}

fn detail_from_attention_item(
    item: &AttentionItem,
    event: Option<&NiumaEvent>,
    approval_requests: &[ApprovalRequest],
) -> StateDetail {
    match event {
        Some(event) => {
            let mut detail = detail_from_event(event);
            detail.approval = approval_detail_for_refs(
                event.payload_ref.as_deref(),
                item.attention_resolve_key.as_deref(),
                approval_requests,
            );
            detail
        }
        None => StateDetail {
            event_id: item.event_id.clone(),
            event_type: event_type_name_for_status(&item.status).to_string(),
            severity: if item.status == RuntimeStateStatus::Error {
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
            approval: approval_detail_for_refs(
                None,
                item.attention_resolve_key.as_deref(),
                approval_requests,
            ),
        },
    }
}

fn activity_detail(activity: &LatestActivity, event: Option<&NiumaEvent>) -> Option<StateDetail> {
    event
        .map(detail_from_event)
        .or_else(|| match activity.status {
            RuntimeStateStatus::Idle | RuntimeStateStatus::Stale => None,
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
                approval: None,
            }),
        })
}

fn detail_from_event(event: &NiumaEvent) -> StateDetail {
    display_detail_from_event(event).into()
}

fn approval_detail_for_refs(
    payload_ref: Option<&str>,
    attention_resolve_key: Option<&str>,
    requests: &[ApprovalRequest],
) -> Option<StateApprovalDetail> {
    let request_id = approval_request_id_from_ref(payload_ref)
        .or_else(|| approval_request_id_from_ref(attention_resolve_key))?;
    requests
        .iter()
        .find(|request| request.id == request_id)
        .map(StateApprovalDetail::from)
}

fn approval_request_id_from_ref(value: Option<&str>) -> Option<&str> {
    value?.strip_prefix("approval:")
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
            approval: None,
        }
    }
}

impl From<&ApprovalRequest> for StateApprovalDetail {
    fn from(request: &ApprovalRequest) -> Self {
        let (can_decide, message) = match request.status {
            ApprovalStatus::Pending => (true, None),
            ApprovalStatus::Allowed => (false, Some("已同意，等待 Codex 继续".to_string())),
            ApprovalStatus::Denied => (false, Some("已拒绝，等待 Codex 继续".to_string())),
            ApprovalStatus::ReturnedToCodex => (
                false,
                Some("Niuma 已停止代处理，请回到 Codex 中同意或拒绝".to_string()),
            ),
        };

        Self {
            request_id: request.id.clone(),
            status: request.status.clone(),
            can_decide,
            message,
            decided_by: request.decided_by.clone(),
            decided_source: request.decided_source.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::listener_config::ListenerConfig;
    use crate::main_state::{MainStateService, MainStateStatus};
    use crate::models::{
        ApprovalProxyStatus, ApprovalRequest, ApprovalStatus, CompletionReason, EventType,
        FailureReason, NiumaEvent, ToolKind,
    };
    use crate::store::NiumaStore;

    #[test]
    fn waiting_approval_uses_event_content_with_summary_fallback() {
        let store = NiumaStore::new(test_sqlite_path("waiting_approval_detail"));
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
    fn waiting_approval_exposes_pending_approval_actions() {
        let store = NiumaStore::new(test_sqlite_path("pending_approval_actions"));
        enable_codex_listener(&store);
        store
            .upsert_approval_request(sample_approval_request(
                "approval-1",
                ApprovalStatus::Pending,
            ))
            .unwrap();
        let mut event = sample_event(
            "approval-event-1",
            EventType::ApprovalRequested,
            "Bash: cargo test",
            1_000,
        );
        event.payload_ref = Some("approval:approval-1".to_string());
        event.attention_resolve_key = Some("approval:approval-1".to_string());
        store.append_event(event).unwrap();

        let state = MainStateService::new(store)
            .current_state(at(2_000))
            .unwrap();

        assert_eq!(state.status, MainStateStatus::WaitingApproval);
        let approval = state.detail.unwrap().approval.unwrap();
        assert_eq!(approval.request_id, "approval-1");
        assert_eq!(approval.status, ApprovalStatus::Pending);
        assert!(approval.can_decide);
        assert!(approval.message.is_none());
    }

    #[test]
    fn returned_to_codex_keeps_waiting_approval_without_actions() {
        let store = NiumaStore::new(test_sqlite_path("returned_approval_actions"));
        enable_codex_listener(&store);
        store
            .upsert_approval_request(sample_approval_request(
                "approval-1",
                ApprovalStatus::Pending,
            ))
            .unwrap();
        let mut event = sample_event(
            "approval-event-1",
            EventType::ApprovalRequested,
            "Bash: cargo test",
            1_000,
        );
        event.payload_ref = Some("approval:approval-1".to_string());
        event.attention_resolve_key = Some("approval:approval-1".to_string());
        store.append_event(event).unwrap();
        store
            .return_approval_to_codex(
                "approval-1",
                "hook-helper",
                "timeout",
                "10 分钟内未处理，请回到 Codex 中操作",
                at(1_600),
            )
            .unwrap();

        let state = MainStateService::new(store)
            .current_state(at(1_601))
            .unwrap();

        assert_eq!(state.status, MainStateStatus::WaitingApproval);
        let approval = state.detail.unwrap().approval.unwrap();
        assert_eq!(approval.status, ApprovalStatus::ReturnedToCodex);
        assert!(!approval.can_decide);
        assert_eq!(
            approval.message.as_deref(),
            Some("Niuma 已停止代处理，请回到 Codex 中同意或拒绝")
        );
    }

    #[test]
    fn completed_expires_to_idle_after_one_minute() {
        let store = NiumaStore::new(test_sqlite_path("completed_expiry"));
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
        let store = NiumaStore::new(test_sqlite_path("error_no_expiry"));
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
        let store = NiumaStore::new(test_sqlite_path("stale_hidden"));
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
        let store = NiumaStore::new(test_sqlite_path("listeners_disabled_idle"));
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: false,
                ..ListenerConfig::default()
            })
            .unwrap();
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
        let store = NiumaStore::new(test_sqlite_path("listener_enabled_priority"));
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

    #[test]
    fn attention_main_state_uses_matching_tool_for_same_session_id() {
        let store = NiumaStore::new(test_sqlite_path("main_state_attention_same_session_tool"));
        enable_codex_listener(&store);
        store
            .append_event(sample_event(
                "codex-running-same-session",
                EventType::SessionStarted,
                "Codex started",
                1_000,
            ))
            .unwrap();
        store
            .append_event(sample_tool_event(
                ToolKind::ClaudeCode,
                "claude-approval-same-session",
                EventType::ApprovalRequested,
                "ClaudeCode waits for approval",
                2_000,
            ))
            .unwrap();

        let state = MainStateService::new(store)
            .current_state(at(2_100))
            .unwrap();

        // attention 回查运行态时必须同时匹配 tool 和 session_id，避免拿到先写入的 Codex session。
        assert_eq!(state.status, MainStateStatus::WaitingApproval);
        assert_eq!(state.session.unwrap().tool, ToolKind::ClaudeCode);
    }

    #[test]
    fn latest_activity_main_state_uses_matching_tool_for_same_session_id() {
        let store = NiumaStore::new(test_sqlite_path("main_state_latest_same_session_tool"));
        enable_codex_listener(&store);
        store
            .append_event(sample_event(
                "codex-running-same-session",
                EventType::SessionStarted,
                "Codex started",
                1_000,
            ))
            .unwrap();
        store
            .append_event(sample_tool_event(
                ToolKind::ClaudeCode,
                "claude-completed-same-session",
                EventType::AssistantMessageCompleted,
                "ClaudeCode completed",
                2_000,
            ))
            .unwrap();

        let state = MainStateService::new(store)
            .current_state(at(2_010))
            .unwrap();

        // latest activity 也要携带 tool，否则同 session_id 下会展示成另一个工具的 session。
        assert_eq!(state.status, MainStateStatus::Completed);
        assert_eq!(state.session.unwrap().tool, ToolKind::ClaudeCode);
    }

    fn sample_event(id: &str, event_type: EventType, summary: &str, timestamp: i64) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: format!("dedupe-{id}"),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: "s1".to_string(),
            parent_session_id: None,
            normalized_session_id: None,
            session_scope: None,
            agent_nickname: None,
            agent_role: None,
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

    fn sample_tool_event(
        tool: ToolKind,
        id: &str,
        event_type: EventType,
        summary: &str,
        timestamp: i64,
    ) -> NiumaEvent {
        let mut event = sample_event(id, event_type, summary, timestamp);
        event.tool = tool;
        event
    }

    fn sample_approval_request(id: &str, status: ApprovalStatus) -> ApprovalRequest {
        ApprovalRequest {
            id: id.to_string(),
            tool: ToolKind::Codex,
            session_id: "s1".to_string(),
            turn_id: "turn-1".to_string(),
            tool_name: "Bash".to_string(),
            command: Some("cargo test".to_string()),
            description: Some("运行测试".to_string()),
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            status,
            decided_by: None,
            decided_source: None,
            reason: None,
            created_at: at(1_000),
            updated_at: at(1_000),
            proxy_timeout_seconds: 600,
            proxy_status: ApprovalProxyStatus::Active,
            last_heartbeat_at: Some(at(1_000)),
            proxy_lost_at: None,
        }
    }

    fn at(timestamp: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(timestamp, 0).single().unwrap()
    }

    fn enable_codex_listener(store: &NiumaStore) {
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: true,
                ..ListenerConfig::default()
            })
            .unwrap();
    }

    fn test_sqlite_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niuma-main-state-{name}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("niuma.sqlite")
    }
}
