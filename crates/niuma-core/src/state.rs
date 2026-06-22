use crate::models::{
    AttentionItem, InternalStateSnapshot, LatestActivity, NiumaEvent, RuntimeStateStatus,
};

pub struct InternalStateEngine;

impl InternalStateEngine {
    // 仅聚合 SQLite 内部状态，不承诺对外展示规则。
    // completed 过期、stale 隐藏和 detail 兜底由 MainStateService 负责。
    pub fn aggregate(
        attention_items: &[AttentionItem],
        latest_activity: Option<&LatestActivity>,
        events: &[NiumaEvent],
    ) -> InternalStateSnapshot {
        if let Some(item) = attention_items.first() {
            return InternalStateSnapshot {
                status: item.status.clone(),
                primary_session_id: Some(item.session_id.clone()),
                updated_at: Some(item.created_at),
                primary_event: event_by_id(events, &item.event_id),
            };
        }

        if let Some(activity) = latest_activity {
            return InternalStateSnapshot {
                status: activity.status.clone(),
                primary_session_id: activity.session_id.clone(),
                updated_at: activity.updated_at,
                primary_event: activity
                    .event_id
                    .as_deref()
                    .and_then(|event_id| event_by_id(events, event_id)),
            };
        }

        InternalStateSnapshot {
            status: RuntimeStateStatus::Idle,
            primary_session_id: None,
            updated_at: None,
            primary_event: None,
        }
    }
}

fn event_by_id(events: &[NiumaEvent], event_id: &str) -> Option<NiumaEvent> {
    events.iter().find(|event| event.id == event_id).cloned()
}

#[cfg(test)]
mod tests;
