use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::listener_config::ListenerConfig;
use crate::models::{CompletionReason, EventType, NiumaEvent, SessionStatus, ToolKind};
use crate::notification_store::{
    NotificationChannel, NotificationRecord, NotificationRecordStatus, PluginNotificationResult,
};
use crate::store::SqliteStateStore;

#[test]
fn append_event_updates_session_status() {
    let store = SqliteStateStore::new(test_sqlite_path("append_event_updates_session_status"));
    let event = sample_event("dedupe-1", EventType::ApprovalRequested);

    let state = store.append_event(event).unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.sessions[0].status, SessionStatus::WaitingApproval);
    assert_eq!(state.sessions[0].project_name, "demo");
}

#[test]
fn sessions_returns_stored_sessions_ordered_by_activity() {
    let store = SqliteStateStore::new(test_sqlite_path("sessions_returns_stored"));
    store
        .append_event(sample_session_event(
            "dedupe-session-a",
            "session-a",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-session-b",
            "session-b",
            EventType::ApprovalRequested,
            2_000,
        ))
        .unwrap();

    let sessions = store.sessions().unwrap();

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, "session-a");
    assert_eq!(sessions[1].id, "session-b");
    assert_eq!(sessions[1].status, SessionStatus::WaitingApproval);
}

#[test]
fn sqlite_schema_does_not_create_events_table() {
    let path = test_sqlite_path("schema_without_events_table");
    let store = SqliteStateStore::new(path.clone());
    store.load().unwrap();

    let connection = rusqlite::Connection::open(path).unwrap();
    let exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'events'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(exists, 0);
}

#[test]
fn sqlite_schema_creates_first_release_columns_and_indexes() {
    let path = test_sqlite_path("schema_first_release_columns");
    let store = SqliteStateStore::new(path.clone());
    store.load().unwrap();

    let connection = rusqlite::Connection::open(path).unwrap();

    assert_table_has_columns(
        &connection,
        "sessions",
        &[
            "id",
            "tool",
            "project_path",
            "project_name",
            "status",
            "last_event_id",
            "last_activity_at",
            "payload",
        ],
    );
    assert_table_has_columns(
        &connection,
        "attention_items",
        &[
            "event_id",
            "session_id",
            "status",
            "attention_resolve_key",
            "created_at",
            "payload",
        ],
    );
    assert_table_has_columns(
        &connection,
        "latest_activity",
        &[
            "id",
            "event_id",
            "session_id",
            "status",
            "updated_at",
            "payload",
        ],
    );
    assert_table_has_columns(
        &connection,
        "public_events",
        &[
            "id",
            "dedupe_key",
            "source",
            "tool",
            "session_id",
            "project_path",
            "project_name",
            "event_type",
            "severity",
            "created_at",
            "payload",
        ],
    );
    assert_table_has_columns(
        &connection,
        "plugin_configs",
        &["plugin_id", "payload", "updated_at"],
    );

    assert_index_exists(&connection, "idx_sessions_last_activity_at");
    assert_index_exists(&connection, "idx_attention_items_created_at");
    assert_index_exists(&connection, "idx_public_events_created_at");
    assert_index_exists(&connection, "idx_notification_records_created_at");
}

#[test]
fn store_write_waits_for_temporary_sqlite_write_lock() {
    let path = test_sqlite_path("temporary_write_lock_wait");
    let store = SqliteStateStore::new(path.clone());
    store.load().unwrap();

    let mut blocking_connection = rusqlite::Connection::open(&path).unwrap();
    let tx = blocking_connection.transaction().unwrap();
    tx.execute(
        "INSERT INTO app_settings (key, payload, updated_at)
         VALUES ('lock-holder', '{}', '2026-06-16T00:00:00Z')",
        [],
    )
    .unwrap();

    let writer = std::thread::spawn({
        let store = store.clone();
        move || {
            store.save_listener_config(&ListenerConfig {
                codex_listening_enabled: true,
                ..ListenerConfig::default()
            })
        }
    });
    std::thread::sleep(Duration::from_millis(200));
    tx.commit().unwrap();

    writer.join().unwrap().unwrap();
    assert!(store.listener_config().unwrap().codex_listening_enabled);
}

#[test]
fn sqlite_store_uses_wal_journal_mode() {
    let path = test_sqlite_path("wal_journal_mode");
    let store = SqliteStateStore::new(path.clone());
    store.load().unwrap();

    let connection = rusqlite::Connection::open(path).unwrap();
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();

    assert_eq!(journal_mode.to_lowercase(), "wal");
}

#[test]
fn append_event_deduplicates_by_dedupe_key() {
    let store = SqliteStateStore::new(test_sqlite_path("append_event_deduplicates_by_dedupe_key"));
    let event = sample_event("same-dedupe", EventType::SessionStarted);

    store.append_event(event.clone()).unwrap();
    let state = store.append_event(event).unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.sessions.len(), 1);
}

#[test]
fn append_event_deduplicates_different_ids_by_public_dedupe_key() {
    let store = SqliteStateStore::new(test_sqlite_path(
        "append_event_deduplicates_different_ids_by_public_dedupe_key",
    ));
    let first = sample_session_event(
        "same-public-dedupe",
        "session-public-dedupe",
        EventType::AssistantMessageCompleted,
        1_000,
    );
    let mut second = sample_session_event(
        "same-public-dedupe",
        "session-public-dedupe",
        EventType::AssistantMessageCompleted,
        2_000,
    );
    second.id = "event_different_id_same_dedupe".to_string();

    store.append_event(first).unwrap();
    let result = store.append_events_with_result(vec![second]).unwrap();

    assert!(result.applied_events.is_empty());
    assert_eq!(store.public_recent_events(10).unwrap().len(), 1);
}

#[test]
fn append_events_deduplicates_same_batch_by_public_dedupe_key() {
    let store = SqliteStateStore::new(test_sqlite_path(
        "append_events_deduplicates_same_batch_by_public_dedupe_key",
    ));
    let first = sample_session_event(
        "same-batch-public-dedupe",
        "session-batch-dedupe",
        EventType::AssistantMessageCompleted,
        1_000,
    );
    let mut second = sample_session_event(
        "same-batch-public-dedupe",
        "session-batch-dedupe",
        EventType::AssistantMessageCompleted,
        2_000,
    );
    second.id = "event_different_batch_id_same_dedupe".to_string();

    let result = store
        .append_events_with_result(vec![first.clone(), second])
        .unwrap();

    assert_eq!(result.applied_events, vec![first]);
    assert_eq!(store.public_recent_events(10).unwrap().len(), 1);
}

#[test]
fn append_events_writes_multiple_events_once() {
    let store = SqliteStateStore::new(test_sqlite_path("append_events_writes_multiple"));
    let first = sample_session_event("dedupe-a", "manual-a", EventType::SessionStarted, 1_000);
    let second = sample_session_event("dedupe-b", "manual-b", EventType::ApprovalRequested, 2_000);

    let state = store.append_events(vec![first, second]).unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.sessions.len(), 2);
    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == "manual-b")
            .unwrap()
            .status,
        SessionStatus::WaitingApproval
    );
}

#[test]
fn append_events_deduplicates_existing_events() {
    let store = SqliteStateStore::new(test_sqlite_path("append_events_deduplicates"));
    let first = sample_session_event("same-dedupe", "manual-a", EventType::SessionStarted, 1_000);
    let second = sample_session_event("same-dedupe", "manual-a", EventType::TaskFailed, 2_000);

    let state = store.append_events(vec![first, second]).unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.sessions[0].status, SessionStatus::Running);
}

#[test]
fn append_events_with_result_returns_only_applied_events() {
    let store = SqliteStateStore::new(test_sqlite_path("append_events_with_result_applied"));
    let first = sample_session_event(
        "dedupe-applied-first",
        "session-applied",
        EventType::SessionStarted,
        1_000,
    );
    let duplicate = first.clone();
    let second = sample_session_event(
        "dedupe-applied-second",
        "session-applied",
        EventType::ApprovalRequested,
        2_000,
    );

    let result = store
        .append_events_with_result(vec![first.clone(), duplicate, second.clone()])
        .unwrap();

    assert_eq!(result.applied_events, vec![first, second]);
    assert_eq!(result.state.sessions.len(), 1);
    assert_eq!(result.state.attention_items.len(), 1);
}

#[test]
fn completed_from_other_session_does_not_hide_existing_attention_item() {
    let store = SqliteStateStore::new(test_sqlite_path("completed_does_not_hide_attention"));
    store
        .append_event(sample_session_event(
            "dedupe-a-waiting",
            "session-a",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-b-completed",
            "session-b",
            EventType::AssistantMessageCompleted,
            2_000,
        ))
        .unwrap();

    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(snapshot.status, SessionStatus::WaitingApproval);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-a"));
}

#[test]
fn running_from_same_session_clears_all_attention_items_for_that_session() {
    let store = SqliteStateStore::new(test_sqlite_path("running_clears_same_session_attention"));
    store
        .append_event(sample_session_event(
            "dedupe-a-approval",
            "session-a",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-a-input",
            "session-a",
            EventType::InputRequested,
            2_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-b-error",
            "session-b",
            EventType::TaskFailed,
            3_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-a-running",
            "session-a",
            EventType::SessionStarted,
            4_000,
        ))
        .unwrap();

    let state = store.load().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].session_id, "session-b");
    assert_eq!(snapshot.status, SessionStatus::Error);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-b"));
}

#[test]
fn activity_from_same_session_clears_waiting_input() {
    let store = SqliteStateStore::new(test_sqlite_path("activity_clears_waiting_input"));
    store
        .append_event(sample_session_event(
            "dedupe-a-input",
            "session-a",
            EventType::InputRequested,
            1_000,
        ))
        .unwrap();

    let state = store
        .append_event(sample_session_event(
            "dedupe-a-activity",
            "session-a",
            EventType::SessionActivity,
            2_000,
        ))
        .unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert!(state.attention_items.is_empty());
    assert_eq!(state.sessions[0].status, SessionStatus::Running);
    assert_eq!(snapshot.status, SessionStatus::Running);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-a"));
}

#[test]
fn unkeyed_activity_keeps_keyed_approval_waiting() {
    let store = SqliteStateStore::new(test_sqlite_path("activity_keeps_keyed_approval"));
    let approval = sample_session_event(
        "approval-dedupe",
        "session-a",
        EventType::ApprovalRequested,
        1_000,
    )
    .with_attention_resolve_key("codex_permission:session-a:call-1");
    let activity = sample_session_event(
        "activity-dedupe",
        "session-a",
        EventType::SessionActivity,
        2_000,
    );

    store.append_event(approval).unwrap();
    let state = store.append_event(activity).unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == "session-a")
        .unwrap();

    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(
        state.attention_items[0].status,
        SessionStatus::WaitingApproval
    );
    assert_eq!(session.status, SessionStatus::WaitingApproval);
    assert_eq!(
        session.last_activity_at,
        Utc.timestamp_opt(2_000, 0).single().unwrap()
    );
    assert_eq!(snapshot.status, SessionStatus::WaitingApproval);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-a"));
}

#[test]
fn resolved_approval_activity_clears_only_matching_attention_item() {
    let store = SqliteStateStore::new(test_sqlite_path("resolved_approval_clears_matching"));
    let approval = sample_session_event(
        "approval-dedupe",
        "session-a",
        EventType::ApprovalRequested,
        1_000,
    )
    .with_attention_resolve_key("codex_permission:session-a:call-1");
    let input = sample_session_event(
        "input-dedupe",
        "session-a",
        EventType::InputRequested,
        2_000,
    );
    let resolved = sample_session_event(
        "resolved-dedupe",
        "session-a",
        EventType::SessionActivity,
        3_000,
    )
    .with_attention_resolve_key("codex_permission:session-a:call-1");

    store.append_event(approval).unwrap();
    store.append_event(input).unwrap();
    let state = store.append_event(resolved).unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].status, SessionStatus::WaitingInput);
    assert_eq!(snapshot.status, SessionStatus::WaitingInput);
}

#[test]
fn resolved_approval_activity_clears_unkeyed_hook_approval_without_hiding_input() {
    let store = SqliteStateStore::new(test_sqlite_path("resolved_approval_clears_hook_approval"));
    let approval = sample_session_event(
        "hook-approval-dedupe",
        "session-a",
        EventType::ApprovalRequested,
        1_000,
    );
    let input = sample_session_event(
        "input-dedupe",
        "session-a",
        EventType::InputRequested,
        2_000,
    );
    let resolved = sample_session_event(
        "resolved-dedupe",
        "session-a",
        EventType::SessionActivity,
        3_000,
    )
    .with_attention_resolve_key("codex_permission:session-a:call-1");

    store.append_event(approval).unwrap();
    store.append_event(input).unwrap();
    let state = store.append_event(resolved).unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].status, SessionStatus::WaitingInput);
    assert_eq!(snapshot.status, SessionStatus::WaitingInput);
}

#[test]
fn idle_from_same_session_clears_attention_without_hiding_other_attention() {
    let store = SqliteStateStore::new(test_sqlite_path("idle_clears_same_session_attention"));
    store
        .append_event(sample_session_event(
            "dedupe-a-approval",
            "session-a",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-b-input",
            "session-b",
            EventType::InputRequested,
            2_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-a-idle",
            "session-a",
            EventType::SessionIdled,
            3_000,
        ))
        .unwrap();

    let state = store.load().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].session_id, "session-b");
    assert_eq!(snapshot.status, SessionStatus::WaitingInput);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-b"));
}

#[test]
fn stale_clears_same_session_running_activity_without_becoming_primary_status() {
    let store = SqliteStateStore::new(test_sqlite_path("stale_clears_running_activity"));
    store
        .append_event(sample_session_event(
            "dedupe-running-a",
            "session-a",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-stale-a",
            "session-a",
            EventType::SessionStaled,
            2_000,
        ))
        .unwrap();

    let state = store.load().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == "session-a")
            .unwrap()
            .status,
        SessionStatus::Stale
    );
    assert_eq!(snapshot.status, SessionStatus::Idle);
    assert_eq!(snapshot.primary_session_id, None);
}

#[test]
fn stale_does_not_hide_attention_from_other_sessions() {
    let store = SqliteStateStore::new(test_sqlite_path("stale_keeps_other_attention"));
    store
        .append_event(sample_session_event(
            "dedupe-approval-a",
            "session-a",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-input-b",
            "session-b",
            EventType::InputRequested,
            2_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-stale-a",
            "session-a",
            EventType::SessionStaled,
            3_000,
        ))
        .unwrap();

    let state = store.load().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].session_id, "session-b");
    assert_eq!(snapshot.status, SessionStatus::WaitingInput);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-b"));
}

#[test]
fn mark_stale_running_sessions_only_stales_old_running_sessions() {
    let store = SqliteStateStore::new(test_sqlite_path("mark_stale_running_sessions"));
    store
        .append_event(sample_session_event(
            "dedupe-old-running",
            "old-running",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-new-running",
            "new-running",
            EventType::SessionStarted,
            2_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-old-completed",
            "old-completed",
            EventType::AssistantMessageCompleted,
            1_000,
        ))
        .unwrap();

    let now = Utc.timestamp_opt(2_000 + 599, 0).single().unwrap();
    let state = store
        .mark_stale_running_sessions(now, chrono::Duration::minutes(10))
        .unwrap();

    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == "old-running")
            .unwrap()
            .status,
        SessionStatus::Stale
    );
    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == "new-running")
            .unwrap()
            .status,
        SessionStatus::Running
    );
    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == "old-completed")
            .unwrap()
            .status,
        SessionStatus::Completed
    );
}

#[test]
fn mark_stale_running_sessions_stales_at_exact_timeout_boundary() {
    let store = SqliteStateStore::new(test_sqlite_path("mark_stale_running_boundary"));
    store
        .append_event(sample_session_event(
            "dedupe-boundary-running",
            "boundary-running",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();

    let now = Utc.timestamp_opt(1_600, 0).single().unwrap();
    let state = store
        .mark_stale_running_sessions(now, chrono::Duration::minutes(10))
        .unwrap();

    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == "boundary-running")
            .unwrap()
            .status,
        SessionStatus::Stale
    );
}

#[test]
fn mark_stale_running_sessions_is_idempotent_for_same_now() {
    let store = SqliteStateStore::new(test_sqlite_path("mark_stale_running_idempotent"));
    store
        .append_event(sample_session_event(
            "dedupe-old-running",
            "old-running",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();

    let now = Utc.timestamp_opt(2_000, 0).single().unwrap();
    let first_state = store
        .mark_stale_running_sessions(now, chrono::Duration::minutes(10))
        .unwrap();
    let state = store
        .mark_stale_running_sessions(now, chrono::Duration::minutes(10))
        .unwrap();

    assert_eq!(state, first_state);
    assert!(state.events.is_empty());
    assert_eq!(
        state
            .sessions
            .iter()
            .find(|session| session.id == "old-running")
            .unwrap()
            .status,
        SessionStatus::Stale
    );
}

#[test]
fn duplicate_attention_events_are_kept_when_dedupe_keys_are_different() {
    let store = SqliteStateStore::new(test_sqlite_path("duplicate_attention_kept"));
    store
        .append_event(sample_session_event(
            "dedupe-a-approval-1",
            "session-a",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-a-approval-2",
            "session-a",
            EventType::ApprovalRequested,
            2_000,
        ))
        .unwrap();

    let state = store.load().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(state.attention_items.len(), 2);
    assert_eq!(snapshot.status, SessionStatus::WaitingApproval);
    assert_eq!(
        snapshot.updated_at,
        Some(Utc.timestamp_opt(1_000, 0).single().unwrap())
    );
    assert!(snapshot.primary_event.is_none());
}

#[test]
fn input_requested_updates_session_to_waiting_input() {
    let store = SqliteStateStore::new(test_sqlite_path("input_requested_updates_session"));
    let event = sample_event("dedupe-input", EventType::InputRequested);

    let state = store.append_event(event).unwrap();

    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.sessions[0].status, SessionStatus::WaitingInput);
}

#[test]
fn task_failed_updates_session_to_error() {
    let store = SqliteStateStore::new(test_sqlite_path("task_failed_updates_session"));
    let event = sample_event("dedupe-error", EventType::TaskFailed);

    let state = store.append_event(event).unwrap();

    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.sessions[0].status, SessionStatus::Error);
}

#[test]
fn reset_clears_events_and_sessions() {
    let store = SqliteStateStore::new(test_sqlite_path("reset_clears_events_and_sessions"));
    store
        .append_event(sample_event("dedupe-reset", EventType::ApprovalRequested))
        .unwrap();

    let state = store.reset().unwrap();

    assert!(state.events.is_empty());
    assert!(state.sessions.is_empty());
    assert!(store.load().unwrap().events.is_empty());
}

#[test]
fn notification_records_dedupe_by_event_and_channel() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_records_dedupe"));
    let record = NotificationRecord {
        id: "record-1".to_string(),
        event_id: "event-1".to_string(),
        event_type: EventType::TaskFailed,
        channel: NotificationChannel::Ntfy,
        status: NotificationRecordStatus::Sent,
        title: Some("任务失败".to_string()),
        body: Some("项目：demo\n任务失败详情".to_string()),
        reason: Some("task_failed".to_string()),
        error_message: None,
        created_at: chrono::Utc::now(),
        sent_at: Some(chrono::Utc::now()),
    };

    assert!(store.insert_notification_record_if_absent(&record).unwrap());
    assert!(!store.insert_notification_record_if_absent(&record).unwrap());

    let records = store.notification_records(20).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].title.as_deref(), Some("任务失败"));
    assert_eq!(records[0].body.as_deref(), Some("项目：demo\n任务失败详情"));
}

#[test]
fn notification_records_allow_same_event_on_different_channels() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_records_same_event"));
    let bark_record =
        sample_notification_record("record-bark", "event-1", NotificationChannel::Bark);
    let ntfy_record =
        sample_notification_record("record-ntfy", "event-1", NotificationChannel::Ntfy);

    assert!(store
        .insert_notification_record_if_absent(&bark_record)
        .unwrap());
    assert!(store
        .insert_notification_record_if_absent(&ntfy_record)
        .unwrap());

    assert_eq!(store.notification_records(20).unwrap().len(), 2);
}

#[test]
fn notification_records_allow_same_channel_on_different_events() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_records_same_channel"));
    let first = sample_notification_record("record-1", "event-1", NotificationChannel::Ntfy);
    let second = sample_notification_record("record-2", "event-2", NotificationChannel::Ntfy);

    assert!(store.insert_notification_record_if_absent(&first).unwrap());
    assert!(store.insert_notification_record_if_absent(&second).unwrap());

    assert_eq!(store.notification_records(20).unwrap().len(), 2);
}

#[test]
fn notification_record_result_can_be_updated_after_reservation() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_record_update_result"));
    let mut record =
        sample_notification_record("record-pending", "event-1", NotificationChannel::Ntfy);
    record.status = NotificationRecordStatus::Pending;
    record.sent_at = None;

    assert!(store.insert_notification_record_if_absent(&record).unwrap());
    store
        .update_notification_record_result(
            "record-pending",
            NotificationRecordStatus::Sent,
            None,
            Some(chrono::Utc::now()),
        )
        .unwrap();

    let records = store.notification_records(20).unwrap();
    assert_eq!(records[0].status, NotificationRecordStatus::Sent);
    assert!(records[0].sent_at.is_some());
}

#[test]
fn plugin_notification_result_upserts_by_plugin_and_event() {
    let store = SqliteStateStore::new(test_sqlite_path("plugin_notification_result_upsert"));
    let mut result =
        sample_plugin_notification_result("plugin-record-1", "builtin-bark", "event-1");

    store.save_plugin_notification_result(&result).unwrap();
    result.status = NotificationRecordStatus::Failed;
    result.error_message = Some("network failed".to_string());
    result.sent_at = None;
    store.save_plugin_notification_result(&result).unwrap();

    let records = store.notification_history_records(20).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].plugin_id.as_deref(), Some("builtin-bark"));
    assert_eq!(records[0].channel, "builtin-bark");
    assert_eq!(records[0].status, NotificationRecordStatus::Failed);
    assert_eq!(records[0].error_message.as_deref(), Some("network failed"));
}

#[test]
fn notification_history_records_merge_legacy_and_plugin_results() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_history_merge"));
    let legacy =
        sample_notification_record("legacy-record", "event-legacy", NotificationChannel::Ntfy);
    let plugin = sample_plugin_notification_result("plugin-record", "builtin-ntfy", "event-plugin");

    store.insert_notification_record_if_absent(&legacy).unwrap();
    store.save_plugin_notification_result(&plugin).unwrap();

    let records = store.notification_history_records(20).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .any(|record| record.channel == "ntfy" && record.plugin_id.is_none()));
    assert!(records.iter().any(|record| record.channel == "builtin-ntfy"
        && record.plugin_id.as_deref() == Some("builtin-ntfy")));
}

#[test]
fn notification_records_return_error_on_duplicate_id_for_different_event_and_channel() {
    let store = SqliteStateStore::new(test_sqlite_path("notification_records_duplicate_id"));
    let first = sample_notification_record("record-1", "event-1", NotificationChannel::Bark);
    let duplicate_id = sample_notification_record("record-1", "event-2", NotificationChannel::Ntfy);

    assert!(store.insert_notification_record_if_absent(&first).unwrap());

    assert!(store
        .insert_notification_record_if_absent(&duplicate_id)
        .is_err());
}

#[test]
fn notification_records_return_error_for_corrupted_stored_values() {
    for (name, event_type, status, created_at, sent_at) in [
        (
            "corrupt_event_type",
            "not-json",
            "\"sent\"",
            "2026-06-12T00:00:00Z",
            None,
        ),
        (
            "corrupt_status",
            "\"task_failed\"",
            "not-json",
            "2026-06-12T00:00:00Z",
            None,
        ),
        (
            "corrupt_created_at",
            "\"task_failed\"",
            "\"sent\"",
            "not-a-date",
            None,
        ),
        (
            "corrupt_sent_at",
            "\"task_failed\"",
            "\"sent\"",
            "2026-06-12T00:00:00Z",
            Some("not-a-date"),
        ),
    ] {
        let path = test_sqlite_path(name);
        let store = SqliteStateStore::new(path.clone());
        store.load().unwrap();

        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .execute(
                "INSERT INTO notification_records
                 (id, event_id, event_type, channel, status, reason, error_message, created_at, sent_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7)",
                rusqlite::params![
                    format!("record-{name}"),
                    format!("event-{name}"),
                    event_type,
                    "bark",
                    status,
                    created_at,
                    sent_at,
                ],
            )
            .unwrap();

        assert!(store.notification_records(20).is_err());
    }
}

#[test]
fn notification_records_return_error_for_unknown_channel() {
    let path = test_sqlite_path("notification_records_unknown_channel");
    let store = SqliteStateStore::new(path.clone());
    store.load().unwrap();

    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO notification_records
             (id, event_id, event_type, channel, status, reason, error_message, created_at, sent_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, NULL)",
            rusqlite::params![
                "record-unknown-channel",
                "event-unknown-channel",
                "\"task_failed\"",
                "sms",
                "\"sent\"",
                "2026-06-12T00:00:00Z",
            ],
        )
        .unwrap();

    assert!(store.notification_records(20).is_err());
}

#[test]
fn dismiss_active_blocker_clears_all_attention_items_without_changing_latest_activity() {
    let store = SqliteStateStore::new(test_sqlite_path("dismiss_clears_all_attention"));
    store
        .append_event(sample_session_event(
            "dedupe-running",
            "session-running",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-a-approval",
            "session-a",
            EventType::ApprovalRequested,
            2_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-b-input",
            "session-b",
            EventType::InputRequested,
            3_000,
        ))
        .unwrap();

    let result = store.dismiss_active_blocker().unwrap().unwrap();
    let state = store.load().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(result.dismissed_count, 2);
    assert!(state.attention_items.is_empty());
    assert_eq!(snapshot.status, SessionStatus::Running);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("session-running")
    );
}

#[test]
fn dismiss_active_blocker_returns_none_without_waiting_session() {
    let store = SqliteStateStore::new(test_sqlite_path("dismiss_active_blocker_none"));
    store
        .append_event(sample_event("dedupe-running", EventType::SessionStarted))
        .unwrap();

    let event = store.dismiss_active_blocker().unwrap();

    assert!(event.is_none());
}

#[test]
fn sqlite_store_matches_core_status_flow() {
    let store = SqliteStateStore::new(test_sqlite_path("sqlite_core_status_flow"));
    store
        .append_event(sample_session_event(
            "dedupe-running",
            "session-running",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-approval",
            "session-approval",
            EventType::ApprovalRequested,
            2_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-task-failed",
            "session-approval",
            EventType::TaskFailed,
            3_000,
        ))
        .unwrap();

    let state = store.load().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.sessions.len(), 2);
    assert_eq!(snapshot.status, SessionStatus::WaitingApproval);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("session-approval")
    );
    let recent_events = store.recent_events(2).unwrap();
    assert_eq!(recent_events.len(), 2);
    assert_eq!(recent_events[0].event_type, EventType::TaskFailed);
    assert_eq!(recent_events[1].event_type, EventType::ApprovalRequested);
}

#[test]
fn listener_config_defaults_to_codex_disabled_and_persists() {
    let path = test_sqlite_path("listener_config_default_persist");
    let store = SqliteStateStore::new(path.clone());

    let default_config = store.listener_config().unwrap();
    store
        .save_listener_config(&ListenerConfig {
            codex_listening_enabled: true,
            ..ListenerConfig::default()
        })
        .unwrap();
    let reloaded = SqliteStateStore::new(path).listener_config().unwrap();

    assert!(!default_config.codex_listening_enabled);
    assert!(reloaded.codex_listening_enabled);
}

#[test]
fn plugin_enabled_map_defaults_empty_and_persists() {
    let path = test_sqlite_path("plugin_enabled_map_default_persist");
    let store = SqliteStateStore::new(path.clone());
    let mut enabled = BTreeMap::new();
    enabled.insert("builtin-bark".to_string(), true);

    let default_map = store.plugin_enabled_map().unwrap();
    store.save_plugin_enabled_map(&enabled).unwrap();
    let reloaded = SqliteStateStore::new(path).plugin_enabled_map().unwrap();

    assert!(default_map.is_empty());
    assert_eq!(reloaded.get("builtin-bark"), Some(&true));
}

#[test]
fn plugin_config_defaults_empty_persists_and_removes() {
    let path = test_sqlite_path("plugin_config_persist");
    let store = SqliteStateStore::new(path.clone());
    let mut config = serde_json::Map::new();
    config.insert("device_key".to_string(), serde_json::json!("device-1"));

    assert!(store.plugin_config("builtin-bark").unwrap().is_none());
    store.save_plugin_config("builtin-bark", &config).unwrap();
    let reloaded = SqliteStateStore::new(path.clone())
        .plugin_config("builtin-bark")
        .unwrap()
        .unwrap();
    SqliteStateStore::new(path)
        .remove_plugin_config("builtin-bark")
        .unwrap();

    assert_eq!(
        reloaded.get("device_key"),
        Some(&serde_json::json!("device-1"))
    );
    assert!(store.plugin_config("builtin-bark").unwrap().is_none());
}

#[test]
fn language_preference_defaults_to_system_and_persists() {
    let path = test_sqlite_path("language_preference_default_persist");
    let store = SqliteStateStore::new(path.clone());

    let default_preference = store.language_preference().unwrap();
    store
        .save_language_preference(crate::platform::locale::LanguagePreference::Fixed(
            crate::platform::locale::SystemLanguage::Ja,
        ))
        .unwrap();
    let reloaded = SqliteStateStore::new(path).language_preference().unwrap();

    assert_eq!(
        default_preference,
        crate::platform::locale::LanguagePreference::System
    );
    assert_eq!(
        reloaded,
        crate::platform::locale::LanguagePreference::Fixed(
            crate::platform::locale::SystemLanguage::Ja
        )
    );
}

#[test]
fn clear_tool_state_removes_only_codex_aggregation() {
    let store = SqliteStateStore::new(test_sqlite_path("clear_codex_state_only"));
    store
        .append_event(sample_session_event(
            "dedupe-codex-running",
            "codex-session",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-codex-approval",
            "codex-session",
            EventType::ApprovalRequested,
            2_000,
        ))
        .unwrap();
    store
        .append_event(sample_tool_event(
            ToolKind::ClaudeCode,
            "dedupe-claude-approval",
            "claude-session",
            EventType::ApprovalRequested,
            3_000,
        ))
        .unwrap();

    let state = store.clear_tool_state(&ToolKind::Codex).unwrap();

    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.sessions[0].tool, ToolKind::ClaudeCode);
    assert_eq!(state.sessions[0].id, "claude-session");
    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].session_id, "claude-session");
    let snapshot = store.internal_status_snapshot().unwrap();
    assert_eq!(snapshot.status, SessionStatus::WaitingApproval);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("claude-session")
    );
}

#[test]
fn clear_tool_state_resets_codex_latest_activity_to_idle() {
    let store = SqliteStateStore::new(test_sqlite_path("clear_codex_latest_idle"));
    store
        .append_event(sample_session_event(
            "dedupe-codex-running",
            "codex-session",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();

    let state = store.clear_tool_state(&ToolKind::Codex).unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert!(state.sessions.is_empty());
    assert!(state.attention_items.is_empty());
    assert_eq!(snapshot.status, SessionStatus::Idle);
}

#[test]
fn public_recent_events_filters_stale_and_respects_limit() {
    let store = SqliteStateStore::new(test_sqlite_path("public_recent_filters_stale"));
    store
        .append_event(sample_session_event(
            "dedupe-approval-a",
            "session-a",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-stale-a",
            "session-a",
            EventType::SessionStaled,
            2_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-input-b",
            "session-b",
            EventType::InputRequested,
            3_000,
        ))
        .unwrap();

    let events = store.public_recent_events(1).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, "session-b");
    assert_eq!(events[0].event_type, EventType::InputRequested);
}

#[test]
fn session_activity_keeps_session_running_and_updates_last_activity() {
    let store = SqliteStateStore::new(test_sqlite_path("session_activity_keeps_running"));
    store
        .append_event(sample_session_event(
            "dedupe-running",
            "session-a",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();

    let state = store
        .append_event(sample_session_event(
            "dedupe-activity",
            "session-a",
            EventType::SessionActivity,
            1_200,
        ))
        .unwrap();

    let session = state
        .sessions
        .iter()
        .find(|session| session.id == "session-a")
        .unwrap();
    assert_eq!(session.status, SessionStatus::Running);
    assert_eq!(
        session.last_activity_at,
        Utc.timestamp_opt(1_200, 0).single().unwrap()
    );
    assert_eq!(
        store.internal_status_snapshot().unwrap().status,
        SessionStatus::Running
    );
}

#[test]
fn session_activity_after_completion_does_not_reopen_session() {
    let store = SqliteStateStore::new(test_sqlite_path("activity_after_completion_ignored"));
    store
        .append_event(sample_session_event(
            "dedupe-running",
            "session-a",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-completed",
            "session-a",
            EventType::AssistantMessageCompleted,
            1_100,
        ))
        .unwrap();

    let state = store
        .append_event(sample_session_event(
            "dedupe-late-activity",
            "session-a",
            EventType::SessionActivity,
            1_200,
        ))
        .unwrap();

    let session = state
        .sessions
        .iter()
        .find(|session| session.id == "session-a")
        .unwrap();
    assert_eq!(session.status, SessionStatus::Completed);
    assert_eq!(
        store.internal_status_snapshot().unwrap().status,
        SessionStatus::Completed
    );
}

#[test]
fn codex_rollback_sequence_finishes_even_with_late_token_count() {
    let store = SqliteStateStore::new(test_sqlite_path("rollback_sequence_late_token_count"));
    store
        .append_event(sample_session_event(
            "rollback-started",
            "session-rollback",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    let mut aborted = sample_session_event(
        "rollback-aborted",
        "session-rollback",
        EventType::AssistantMessageCompleted,
        1_001,
    );
    aborted.completion_reason = Some(CompletionReason::Interrupted);
    store.append_event(aborted).unwrap();
    store
        .append_event(sample_session_event(
            "rollback-late-activity",
            "session-rollback",
            EventType::SessionActivity,
            1_002,
        ))
        .unwrap();
    let mut rolled_back = sample_session_event(
        "rollback-finished",
        "session-rollback",
        EventType::AssistantMessageCompleted,
        1_003,
    );
    rolled_back.completion_reason = Some(CompletionReason::RolledBack);
    store.append_event(rolled_back).unwrap();

    let snapshot = store.internal_status_snapshot().unwrap();
    let state = store.load().unwrap();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == "session-rollback")
        .unwrap();
    assert_eq!(session.status, SessionStatus::Completed);
    assert_eq!(snapshot.status, SessionStatus::Completed);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("session-rollback")
    );
}

#[test]
fn public_recent_events_filters_internal_session_activity() {
    let store = SqliteStateStore::new(test_sqlite_path("public_recent_filters_activity"));
    store
        .append_event(sample_session_event(
            "dedupe-running",
            "session-a",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-activity",
            "session-a",
            EventType::SessionActivity,
            1_200,
        ))
        .unwrap();

    let events = store.public_recent_events(10).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, EventType::SessionStarted);
}

#[test]
fn empty_project_path_does_not_overwrite_existing_session_path() {
    let store = SqliteStateStore::new(test_sqlite_path("empty_path_preserves_existing"));
    store
        .append_event(sample_session_event(
            "dedupe-with-path",
            "session-a",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    let mut empty_path_event = sample_session_event(
        "dedupe-empty-path",
        "session-a",
        EventType::ApprovalRequested,
        2_000,
    );
    empty_path_event.project_path = String::new();
    empty_path_event.project_name = "Codex".to_string();

    let state = store.append_event(empty_path_event).unwrap();
    let session = state
        .sessions
        .iter()
        .find(|session| session.id == "session-a")
        .unwrap();

    assert_eq!(session.project_path, "/tmp/demo");
    assert_eq!(session.project_name, "demo");
}

#[test]
fn sqlite_store_reset_and_dismiss_preserve_activity_behavior() {
    let store = SqliteStateStore::new(test_sqlite_path("sqlite_reset_and_dismiss"));
    store
        .append_event(sample_session_event(
            "dedupe-running",
            "session-running",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_session_event(
            "dedupe-input",
            "session-input",
            EventType::InputRequested,
            2_000,
        ))
        .unwrap();

    let result = store.dismiss_active_blocker().unwrap().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(result.dismissed_count, 1);
    assert_eq!(snapshot.status, SessionStatus::Running);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("session-running")
    );

    let state = store.reset().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert!(state.events.is_empty());
    assert!(state.sessions.is_empty());
    assert_eq!(snapshot.status, SessionStatus::Idle);
}

fn sample_event(dedupe_key: &str, event_type: EventType) -> NiumaEvent {
    sample_session_event(dedupe_key, "s1", event_type, 1_000)
}

fn sample_notification_record(
    id: &str,
    event_id: &str,
    channel: NotificationChannel,
) -> NotificationRecord {
    NotificationRecord {
        id: id.to_string(),
        event_id: event_id.to_string(),
        event_type: EventType::TaskFailed,
        channel,
        status: NotificationRecordStatus::Sent,
        title: Some("任务失败".to_string()),
        body: Some("项目：demo\n任务失败详情".to_string()),
        reason: Some("task_failed".to_string()),
        error_message: None,
        created_at: Utc.timestamp_opt(1_000, 0).single().unwrap(),
        sent_at: Some(Utc.timestamp_opt(1_001, 0).single().unwrap()),
    }
}

fn sample_plugin_notification_result(
    id: &str,
    plugin_id: &str,
    event_id: &str,
) -> PluginNotificationResult {
    PluginNotificationResult {
        id: id.to_string(),
        plugin_id: plugin_id.to_string(),
        event_id: event_id.to_string(),
        event_type: EventType::TaskFailed,
        status: NotificationRecordStatus::Sent,
        title: Some("任务失败".to_string()),
        body: Some("项目：demo\n插件通知详情".to_string()),
        reason: Some("task_failed".to_string()),
        error_message: None,
        created_at: Utc.timestamp_opt(1_002, 0).single().unwrap(),
        sent_at: Some(Utc.timestamp_opt(1_003, 0).single().unwrap()),
    }
}

fn sample_session_event(
    dedupe_key: &str,
    session_id: &str,
    event_type: EventType,
    timestamp: i64,
) -> NiumaEvent {
    NiumaEvent {
        id: format!("event_{dedupe_key}"),
        dedupe_key: dedupe_key.to_string(),
        source: "test".to_string(),
        tool: ToolKind::Codex,
        session_id: session_id.to_string(),
        project_path: "/tmp/demo".to_string(),
        project_name: "demo".to_string(),
        event_type,
        severity: "urgent".to_string(),
        summary: "Bash: cargo test".to_string(),
        content: Some("Bash: cargo test".to_string()),
        error_message: None,
        attention_resolve_key: None,
        completion_reason: None,
        failure_reason: None,
        payload_ref: None,
        created_at: Utc.timestamp_opt(timestamp, 0).single().unwrap(),
    }
}

fn sample_tool_event(
    tool: ToolKind,
    dedupe_key: &str,
    session_id: &str,
    event_type: EventType,
    timestamp: i64,
) -> NiumaEvent {
    let mut event = sample_session_event(dedupe_key, session_id, event_type, timestamp);
    event.tool = tool;
    event
}

trait EventTestExt {
    fn with_attention_resolve_key(self, key: &str) -> Self;
}

impl EventTestExt for NiumaEvent {
    fn with_attention_resolve_key(mut self, key: &str) -> Self {
        self.attention_resolve_key = Some(key.to_string());
        self
    }
}

fn test_sqlite_path(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "niuma-notifier-{name}-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn assert_table_has_columns(connection: &rusqlite::Connection, table: &str, columns: &[&str]) {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    let actual = statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for column in columns {
        assert!(
            actual.iter().any(|actual_column| actual_column == column),
            "{table} should contain column {column}; actual columns: {actual:?}"
        );
    }
}

fn assert_index_exists(connection: &rusqlite::Connection, index: &str) {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
            [index],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "index should exist: {index}");
}
