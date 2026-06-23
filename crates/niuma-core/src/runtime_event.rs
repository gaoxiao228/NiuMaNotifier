use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::models::NiumaEvent;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const DEFAULT_CHANNEL_CAPACITY: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeEvent {
    NiumaEventsAppended {
        version: u64,
        events: Vec<NiumaEvent>,
    },
    AttentionDismissed {
        version: u64,
        dismissed_count: usize,
    },
    StateReset {
        version: u64,
    },
    StateChanged {
        version: u64,
        reason: StateChangeReason,
    },
    PluginNotificationTestRequested {
        version: u64,
        request: PluginNotificationTestRequest,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginNotificationTestRequest {
    pub test_id: String,
    pub plugin_id: String,
    pub title: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateChangeReason {
    StaleSweep,
    ListenerConfigChanged,
    PluginConfigChanged,
}

#[derive(Clone)]
pub struct RuntimeEventBus {
    sender: broadcast::Sender<RuntimeEvent>,
    version: Arc<AtomicU64>,
}

impl RuntimeEventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);
        Self {
            sender,
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.sender.subscribe()
    }

    pub fn publish_niuma_events(&self, events: Vec<NiumaEvent>) {
        if events.is_empty() {
            return;
        }
        self.publish(|version| RuntimeEvent::NiumaEventsAppended { version, events });
    }

    pub fn publish_attention_dismissed(&self, dismissed_count: usize) {
        self.publish(|version| RuntimeEvent::AttentionDismissed {
            version,
            dismissed_count,
        });
    }

    pub fn publish_state_reset(&self) {
        self.publish(|version| RuntimeEvent::StateReset { version });
    }

    pub fn publish_state_changed(&self, reason: StateChangeReason) {
        self.publish(|version| RuntimeEvent::StateChanged { version, reason });
    }

    pub fn publish_plugin_notification_test(&self, request: PluginNotificationTestRequest) {
        self.publish(|version| RuntimeEvent::PluginNotificationTestRequested { version, request });
    }

    fn publish(&self, build: impl FnOnce(u64) -> RuntimeEvent) {
        let version = self.version.fetch_add(1, Ordering::SeqCst) + 1;
        // 没有订阅者时发送失败是正常情况；状态已经写入 SQLite，后续订阅者会读快照补齐。
        let _ = self.sender.send(build(version));
    }
}

impl Default for RuntimeEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{EventType, ToolKind};
    use chrono::Utc;

    #[test]
    fn runtime_event_bus_publishes_niuma_events_with_incrementing_versions() {
        let bus = RuntimeEventBus::new();
        let mut receiver = bus.subscribe();
        let event = sample_event("event-runtime-1");

        bus.publish_niuma_events(vec![event.clone()]);
        bus.publish_state_changed(StateChangeReason::StaleSweep);

        assert_eq!(
            receiver.try_recv().unwrap(),
            RuntimeEvent::NiumaEventsAppended {
                version: 1,
                events: vec![event]
            }
        );
        assert_eq!(
            receiver.try_recv().unwrap(),
            RuntimeEvent::StateChanged {
                version: 2,
                reason: StateChangeReason::StaleSweep
            }
        );
    }

    fn sample_event(id: &str) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: format!("dedupe-{id}"),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: "session-runtime".to_string(),
            parent_session_id: None,
            project_path: "/tmp/runtime".to_string(),
            project_name: "runtime".to_string(),
            event_type: EventType::ApprovalRequested,
            severity: "urgent".to_string(),
            summary: "Runtime test".to_string(),
            content: Some("Runtime test".to_string()),
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            created_at: Utc::now(),
        }
    }
}
