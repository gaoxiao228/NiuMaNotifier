use chrono::{TimeZone, Utc};

use crate::models::{AttentionItem, LatestActivity, RuntimeStateStatus, ToolKind};
use crate::state::InternalStateEngine;

#[test]
fn earliest_attention_item_wins_over_later_attention_item() {
    let first_time = Utc.timestamp_opt(1_000, 0).single().unwrap();
    let second_time = Utc.timestamp_opt(2_000, 0).single().unwrap();
    let first_event = sample_event(
        "event-input",
        "session-a",
        crate::models::EventType::InputRequested,
        first_time,
    );
    let second_event = sample_event(
        "event-approval",
        "session-b",
        crate::models::EventType::ApprovalRequested,
        second_time,
    );
    let attention_items = vec![
        AttentionItem::from_event(&first_event, RuntimeStateStatus::WaitingInput),
        AttentionItem::from_event(&second_event, RuntimeStateStatus::WaitingApproval),
    ];

    let snapshot =
        InternalStateEngine::aggregate(&attention_items, None, &[first_event, second_event]);

    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingInput);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-a"));
    assert_eq!(snapshot.primary_event.unwrap().summary, "event-input");
}

#[test]
fn latest_activity_is_used_when_no_attention_items_exist() {
    let now = Utc.timestamp_opt(1_000, 0).single().unwrap();
    let event = sample_event(
        "event-running",
        "session-running",
        crate::models::EventType::SessionStarted,
        now,
    );
    let activity = LatestActivity::from_event(&event, RuntimeStateStatus::Running);

    let snapshot = InternalStateEngine::aggregate(&[], Some(&activity), &[event]);

    assert_eq!(snapshot.status, RuntimeStateStatus::Running);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("session-running")
    );
    assert_eq!(
        snapshot.primary_event.unwrap().event_type,
        crate::models::EventType::SessionStarted
    );
}

#[test]
fn idle_is_used_when_no_attention_or_latest_activity_exists() {
    let snapshot = InternalStateEngine::aggregate(&[], None, &[]);

    assert_eq!(snapshot.status, RuntimeStateStatus::Idle);
    assert_eq!(snapshot.primary_session_id, None);
    assert_eq!(snapshot.primary_event, None);
}

fn sample_event(
    id: &str,
    session_id: &str,
    event_type: crate::models::EventType,
    created_at: chrono::DateTime<Utc>,
) -> crate::models::NiumaEvent {
    crate::models::NiumaEvent {
        id: id.to_string(),
        dedupe_key: format!("dedupe-{id}"),
        source: "test".to_string(),
        tool: ToolKind::Codex,
        session_id: session_id.to_string(),
        project_path: "/tmp/demo".to_string(),
        project_name: "demo".to_string(),
        event_type,
        severity: "info".to_string(),
        summary: id.to_string(),
        content: None,
        error_message: None,
        attention_resolve_key: None,
        completion_reason: None,
        failure_reason: None,
        payload_ref: None,
        created_at,
    }
}
