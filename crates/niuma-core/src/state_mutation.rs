use chrono::{DateTime, Duration, Utc};

use crate::listener_config::ListenerConfig;
use crate::models::NiumaEvent;
use crate::models::ToolKind;
use crate::runtime_event::{RuntimeEventBus, StateChangeReason};
use crate::store::{
    AppendEventsResult, DismissAttentionResult, NiumaStore, StaleSweepResult, StoredState,
};

#[derive(Clone)]
pub struct StateMutationService {
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListenerConfigUpdateResult {
    pub config: ListenerConfig,
    pub state: Option<StoredState>,
}

impl StateMutationService {
    pub fn new(store: NiumaStore, runtime_events: RuntimeEventBus) -> Self {
        Self {
            store,
            runtime_events,
        }
    }

    pub fn append_events(&self, events: Vec<NiumaEvent>) -> Result<AppendEventsResult, String> {
        let result = self.store.append_events_with_result(events)?;
        // 只发布实际进入状态机的事件，避免重复扫描触发 UI 刷新和通知。
        self.runtime_events
            .publish_niuma_events(result.applied_events.clone());
        Ok(result)
    }

    pub fn dismiss_active_blocker(&self) -> Result<Option<DismissAttentionResult>, String> {
        let result = self.store.dismiss_active_blocker()?;
        if let Some(result) = &result {
            self.runtime_events
                .publish_attention_dismissed(result.dismissed_count);
        }
        Ok(result)
    }

    pub fn reset(&self) -> Result<StoredState, String> {
        let state = self.store.reset()?;
        self.runtime_events.publish_state_reset();
        Ok(state)
    }

    pub fn mark_stale_running_sessions(
        &self,
        now: DateTime<Utc>,
        timeout: Duration,
    ) -> Result<StaleSweepResult, String> {
        let result = self
            .store
            .mark_stale_running_sessions_with_result(now, timeout)?;
        if result.staled_count > 0 {
            self.runtime_events
                .publish_state_changed(StateChangeReason::StaleSweep);
        }
        Ok(result)
    }

    pub fn set_codex_listening_enabled(
        &self,
        enabled: bool,
    ) -> Result<ListenerConfigUpdateResult, String> {
        self.set_tool_listening_enabled(ToolKind::Codex, enabled)
    }

    pub fn set_tool_listening_enabled(
        &self,
        tool: ToolKind,
        enabled: bool,
    ) -> Result<ListenerConfigUpdateResult, String> {
        let previous = self.store.listener_config()?;
        let config = previous.clone().with_tool_enabled(&tool, enabled);
        let disabled_tools = if enabled { Vec::new() } else { vec![tool] };
        self.apply_listener_config_update(previous, config, disabled_tools)
    }

    pub fn set_listener_config(
        &self,
        config: ListenerConfig,
    ) -> Result<ListenerConfigUpdateResult, String> {
        let previous = self.store.listener_config()?;
        let disabled_tools = previous
            .tool_enabled_map()
            .into_iter()
            .filter_map(|(tool_id, was_enabled)| {
                let tool = ToolKind::from_id(tool_id);
                if was_enabled && !config.is_tool_enabled(&tool) {
                    Some(tool)
                } else {
                    None
                }
            })
            .collect();
        self.apply_listener_config_update(previous, config, disabled_tools)
    }

    fn apply_listener_config_update(
        &self,
        previous: ListenerConfig,
        config: ListenerConfig,
        disabled_tools: Vec<ToolKind>,
    ) -> Result<ListenerConfigUpdateResult, String> {
        self.store.save_listener_config(&config)?;

        let mut changed = previous != config;
        let mut state = None;
        for tool in disabled_tools {
            let before = self.store.load()?;
            let after = self.store.clear_tool_state(&tool)?;
            if before != after {
                changed = true;
            }
            state = Some(after);
        }

        if changed {
            self.runtime_events
                .publish_state_changed(StateChangeReason::ListenerConfigChanged);
        }
        Ok(ListenerConfigUpdateResult { config, state })
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::listener_config::ListenerConfig;
    use crate::models::{EventType, NiumaEvent, ToolKind};
    use crate::runtime_event::RuntimeEvent;

    #[test]
    fn append_events_publishes_only_applied_events() {
        let store = NiumaStore::new(test_sqlite_path("append_events_publish_applied"));
        let runtime_events = RuntimeEventBus::new();
        let mut receiver = runtime_events.subscribe();
        let service = StateMutationService::new(store, runtime_events);
        let first = sample_event("event-1", "dedupe-1", EventType::SessionStarted, 1_000);
        let duplicate = first.clone();
        let second = sample_event("event-2", "dedupe-2", EventType::ApprovalRequested, 1_002);

        let result = service
            .append_events(vec![first.clone(), duplicate, second.clone()])
            .unwrap();

        assert_eq!(result.applied_events, vec![first.clone(), second.clone()]);
        assert_eq!(
            receiver.try_recv().unwrap(),
            RuntimeEvent::NiumaEventsAppended {
                version: 1,
                events: vec![first, second]
            }
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn append_events_skips_publish_when_all_events_are_duplicates() {
        let store = NiumaStore::new(test_sqlite_path("append_events_no_publish_duplicate"));
        let runtime_events = RuntimeEventBus::new();
        let mut receiver = runtime_events.subscribe();
        let service = StateMutationService::new(store, runtime_events);
        let event = sample_event("event-1", "dedupe-1", EventType::SessionStarted, 1_000);

        service.append_events(vec![event.clone()]).unwrap();
        receiver.try_recv().unwrap();
        let result = service.append_events(vec![event]).unwrap();

        assert!(result.applied_events.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn stale_sweep_publishes_only_when_session_changed() {
        let store = NiumaStore::new(test_sqlite_path("stale_sweep_publish_changed"));
        let runtime_events = RuntimeEventBus::new();
        let mut receiver = runtime_events.subscribe();
        let service = StateMutationService::new(store.clone(), runtime_events);
        store
            .append_event(sample_event(
                "event-running",
                "dedupe-running",
                EventType::SessionStarted,
                1_000,
            ))
            .unwrap();

        let first = service
            .mark_stale_running_sessions(
                Utc.timestamp_opt(1_600, 0).single().unwrap(),
                Duration::minutes(10),
            )
            .unwrap();
        let second = service
            .mark_stale_running_sessions(
                Utc.timestamp_opt(1_600, 0).single().unwrap(),
                Duration::minutes(10),
            )
            .unwrap();

        assert_eq!(first.staled_count, 1);
        assert_eq!(second.staled_count, 0);
        assert_eq!(
            receiver.try_recv().unwrap(),
            RuntimeEvent::StateChanged {
                version: 1,
                reason: StateChangeReason::StaleSweep
            }
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn disabling_codex_listener_saves_config_clears_state_and_publishes_change() {
        let store = NiumaStore::new(test_sqlite_path("disable_codex_listener"));
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: true,
                ..ListenerConfig::default()
            })
            .unwrap();
        store
            .append_event(sample_event(
                "event-running",
                "dedupe-running",
                EventType::SessionStarted,
                1_000,
            ))
            .unwrap();
        let runtime_events = RuntimeEventBus::new();
        let mut receiver = runtime_events.subscribe();
        let service = StateMutationService::new(store.clone(), runtime_events);

        let result = service.set_codex_listening_enabled(false).unwrap();

        assert!(!result.config.codex_listening_enabled);
        assert!(store.load().unwrap().runtime_states.is_empty());
        assert_eq!(
            receiver.try_recv().unwrap(),
            RuntimeEvent::StateChanged {
                version: 1,
                reason: StateChangeReason::ListenerConfigChanged
            }
        );
    }

    #[test]
    fn disabling_tool_listener_clears_only_that_tool_state() {
        let store = NiumaStore::new(test_sqlite_path("disable_one_tool_listener"));
        store
            .save_listener_config(&ListenerConfig {
                codex_listening_enabled: true,
                claude_code_listening_enabled: true,
                ..ListenerConfig::default()
            })
            .unwrap();
        store
            .append_event(sample_tool_event(
                ToolKind::Codex,
                "event-codex-running",
                "dedupe-codex-running",
                "codex-session",
                EventType::SessionStarted,
                1_000,
            ))
            .unwrap();
        store
            .append_event(sample_tool_event(
                ToolKind::ClaudeCode,
                "event-claude-running",
                "dedupe-claude-running",
                "claude-session",
                EventType::SessionStarted,
                1_001,
            ))
            .unwrap();
        let runtime_events = RuntimeEventBus::new();
        let service = StateMutationService::new(store.clone(), runtime_events);

        let result = service
            .set_tool_listening_enabled(ToolKind::ClaudeCode, false)
            .unwrap();

        assert!(result.config.codex_listening_enabled);
        assert!(!result.config.claude_code_listening_enabled);
        let state = store.load().unwrap();
        assert_eq!(state.runtime_states.len(), 1);
        assert_eq!(state.runtime_states[0].tool, ToolKind::Codex);
        assert_eq!(state.runtime_states[0].session_id, "codex-session");
    }

    fn sample_event(
        id: &str,
        dedupe_key: &str,
        event_type: EventType,
        timestamp: i64,
    ) -> NiumaEvent {
        sample_tool_event(
            ToolKind::Codex,
            id,
            dedupe_key,
            "session-mutation",
            event_type,
            timestamp,
        )
    }

    fn sample_tool_event(
        tool: ToolKind,
        id: &str,
        dedupe_key: &str,
        session_id: &str,
        event_type: EventType,
        timestamp: i64,
    ) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: dedupe_key.to_string(),
            source: "test".to_string(),
            tool,
            session_id: session_id.to_string(),
            project_path: "/tmp/mutation".to_string(),
            project_name: "mutation".to_string(),
            event_type,
            severity: "info".to_string(),
            summary: "Mutation test".to_string(),
            content: Some("Mutation test".to_string()),
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            created_at: Utc.timestamp_opt(timestamp, 0).single().unwrap(),
        }
    }

    fn test_sqlite_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niuma-state-mutation-{name}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("niuma.sqlite")
    }
}
