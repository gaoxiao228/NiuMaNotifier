use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::listener_config::ListenerConfig;
use crate::models::{
    ApprovalChannel, ApprovalDecisionKind, ApprovalProxyStatus, ApprovalRequest, ApprovalStatus,
    CompletionReason, EventSessionScope, EventType, NiumaEvent, RuntimeStateStatus, ToolKind,
};
use crate::notification_store::{
    NotificationNotifierType, NotificationRecord, NotificationRecordStatus,
    PluginNotificationResult,
};
use crate::plugin::{PluginRuntimeState, PluginRuntimeStatus};
use crate::store::NiumaStore;

#[test]
fn approval_status_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_value(ApprovalStatus::ReturnedToCodex).unwrap(),
        serde_json::json!("returned_to_codex")
    );
}

#[test]
fn approval_decision_kind_serializes_as_allow_or_deny() {
    assert_eq!(
        serde_json::to_value(ApprovalDecisionKind::Allow).unwrap(),
        serde_json::json!("allow")
    );
    assert_eq!(
        serde_json::to_value(ApprovalDecisionKind::Deny).unwrap(),
        serde_json::json!("deny")
    );
}

#[test]
fn deciding_pending_approval_accepts_first_decision() {
    let store = NiumaStore::new(test_sqlite_path("approval_first_decision"));
    store
        .upsert_approval_request(sample_approval_request("approval-1"))
        .unwrap();

    let result = store
        .decide_approval(
            "approval-1",
            ApprovalDecisionKind::Allow,
            "desktop",
            "ui",
            Some("用户同意".to_string()),
            Utc.timestamp_opt(1_100, 0).single().unwrap(),
        )
        .unwrap();

    assert!(result.accepted);
    assert_eq!(result.request.status, ApprovalStatus::Allowed);
    assert_eq!(result.request.decided_by.as_deref(), Some("desktop"));
    assert_eq!(result.request.decided_source.as_deref(), Some("ui"));
}

#[test]
fn later_decision_does_not_override_first_decision() {
    let store = NiumaStore::new(test_sqlite_path("approval_first_wins"));
    store
        .upsert_approval_request(sample_approval_request("approval-1"))
        .unwrap();
    store
        .decide_approval(
            "approval-1",
            ApprovalDecisionKind::Allow,
            "desktop",
            "ui",
            None,
            Utc.timestamp_opt(1_100, 0).single().unwrap(),
        )
        .unwrap();

    let result = store
        .decide_approval(
            "approval-1",
            ApprovalDecisionKind::Deny,
            "builtin-bark",
            "notification",
            Some("晚到的拒绝".to_string()),
            Utc.timestamp_opt(1_101, 0).single().unwrap(),
        )
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(result.request.status, ApprovalStatus::Allowed);
    assert_eq!(result.request.decided_by.as_deref(), Some("desktop"));
    assert_eq!(result.request.reason, None);
}

#[test]
fn approval_heartbeat_updates_active_pending_request() {
    let store = NiumaStore::new(test_sqlite_path("approval_heartbeat"));
    let mut request = sample_approval_request("approval-1");
    request.proxy_status = ApprovalProxyStatus::Active;
    request.last_heartbeat_at = Some(Utc.timestamp_opt(100, 0).single().unwrap());
    store.upsert_approval_request(request).unwrap();

    let result = store
        .heartbeat_approval_proxy("approval-1", Utc.timestamp_opt(110, 0).single().unwrap())
        .unwrap();

    assert!(result.accepted);
    assert_eq!(result.request.proxy_status, ApprovalProxyStatus::Active);
    assert_eq!(
        result.request.last_heartbeat_at,
        Some(Utc.timestamp_opt(110, 0).single().unwrap())
    );
}

#[test]
fn stale_pending_proxy_returns_to_codex() {
    let store = NiumaStore::new(test_sqlite_path("stale_proxy"));
    let mut request = sample_approval_request("approval-1");
    request.proxy_status = ApprovalProxyStatus::Active;
    request.last_heartbeat_at = Some(Utc.timestamp_opt(100, 0).single().unwrap());
    store.upsert_approval_request(request).unwrap();

    let results = store
        .return_stale_approval_proxies_to_codex(
            Utc.timestamp_opt(109, 0).single().unwrap(),
            chrono::Duration::seconds(8),
        )
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].request.status, ApprovalStatus::ReturnedToCodex);
    assert_eq!(results[0].request.proxy_status, ApprovalProxyStatus::Lost);
    assert_eq!(
        results[0].request.decided_source.as_deref(),
        Some("proxy_lost")
    );
}

#[test]
fn return_to_codex_keeps_attention_item_waiting_approval() {
    let store = NiumaStore::new(test_sqlite_path("approval_return_keeps_attention"));
    store
        .upsert_approval_request(sample_approval_request("approval-1"))
        .unwrap();
    store
        .append_event(sample_approval_requested_event("approval-1"))
        .unwrap();

    let result = store
        .return_approval_to_codex(
            "approval-1",
            "hook-helper",
            "timeout",
            "10 分钟内未处理，请回到 Codex 中操作",
            Utc.timestamp_opt(1_600, 0).single().unwrap(),
        )
        .unwrap();

    assert!(result.accepted);
    assert_eq!(result.request.status, ApprovalStatus::ReturnedToCodex);
    let state = store.load().unwrap();
    assert_eq!(
        state.approval_requests[0].status,
        ApprovalStatus::ReturnedToCodex
    );
    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(
        state.attention_items[0].status,
        RuntimeStateStatus::WaitingApproval
    );
}

#[test]
fn append_event_updates_session_status() {
    let store = NiumaStore::new(test_sqlite_path("append_event_updates_session_status"));
    let event = sample_event("dedupe-1", EventType::ApprovalRequested);

    let state = store.append_event(event).unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.runtime_states.len(), 1);
    assert_eq!(
        state.runtime_states[0].status,
        RuntimeStateStatus::WaitingApproval
    );
    assert_eq!(state.runtime_states[0].project_name, "demo");
}

#[test]
fn sessions_returns_stored_sessions_ordered_by_activity() {
    let store = NiumaStore::new(test_sqlite_path("sessions_returns_stored"));
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

    let sessions = store.runtime_state_list().unwrap();

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].session_id, "session-a");
    assert_eq!(sessions[1].session_id, "session-b");
    assert_eq!(sessions[1].status, RuntimeStateStatus::WaitingApproval);
}

#[test]
fn sqlite_schema_does_not_create_events_table() {
    let path = test_sqlite_path("schema_without_events_table");
    let store = NiumaStore::new(path.clone());
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
fn runtime_state_is_not_persisted_to_sqlite_tables() {
    let path = test_sqlite_path("runtime_state_is_not_persisted");
    let store = NiumaStore::new(path.clone());
    store
        .save_listener_config(&ListenerConfig::default())
        .unwrap();

    store
        .append_event(sample_event(
            "dedupe-memory-only",
            EventType::ApprovalRequested,
        ))
        .unwrap();

    let connection = rusqlite::Connection::open(path).unwrap();
    assert_table_missing(&connection, "sessions");
    assert_table_missing(&connection, "attention_items");
    assert_table_missing(&connection, "latest_activity");
    assert_table_missing(&connection, "public_events");
}

#[test]
fn new_store_with_same_path_starts_with_empty_runtime_state() {
    let path = test_sqlite_path("new_store_same_path_empty_runtime");
    let store = NiumaStore::new(path.clone());
    store
        .append_event(sample_event(
            "dedupe-runtime-reset",
            EventType::ApprovalRequested,
        ))
        .unwrap();

    let reloaded = NiumaStore::new(path);

    assert!(reloaded.runtime_state_list().unwrap().is_empty());
    assert!(reloaded.public_recent_events(10).unwrap().is_empty());
    assert_eq!(
        reloaded.internal_status_snapshot().unwrap().status,
        RuntimeStateStatus::Idle
    );
}

#[test]
fn schema_initializes_only_notification_records_table() {
    let path = test_sqlite_path("schema_notification_only");
    let store = NiumaStore::new(path.clone());
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

    assert_table_has_columns(
        &connection,
        "notification_records",
        &[
            "id",
            "notifier_id",
            "notifier_type",
            "event_id",
            "event_type",
            "status",
            "title",
            "body",
            "reason",
            "error_message",
            "created_at",
            "sent_at",
        ],
    );

    assert_index_exists(&connection, "idx_notification_records_created_at");
    assert_index_exists(&connection, "idx_notification_records_notifier_created_at");
}

#[test]
fn store_write_waits_for_temporary_sqlite_write_lock() {
    let path = test_sqlite_path("temporary_write_lock_wait");
    let store = NiumaStore::new(path.clone());
    store.load().unwrap();

    let mut blocking_connection = rusqlite::Connection::open(&path).unwrap();
    let tx = blocking_connection.transaction().unwrap();
    tx.execute(
        "INSERT INTO notification_records
         (id, notifier_id, notifier_type, event_id, event_type, status, created_at)
         VALUES ('lock-holder', 'builtin-ntfy', 'builtin', 'event-lock-holder', '\"task_failed\"', '\"sent\"', '2026-06-16T00:00:00Z')",
        [],
    )
    .unwrap();

    let writer = std::thread::spawn({
        let store = store.clone();
        move || {
            store.insert_notification_record_if_absent(&sample_notification_record(
                "record-waiting-writer",
                "builtin-bark",
                "event-waiting-writer",
            ))
        }
    });
    std::thread::sleep(Duration::from_millis(200));
    tx.commit().unwrap();

    assert!(writer.join().unwrap().unwrap());
    assert_eq!(store.notification_records(20).unwrap().len(), 2);
}

#[test]
fn sqlite_store_uses_wal_journal_mode() {
    let path = test_sqlite_path("wal_journal_mode");
    let store = NiumaStore::new(path.clone());
    store.load().unwrap();

    let connection = rusqlite::Connection::open(path).unwrap();
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();

    assert_eq!(journal_mode.to_lowercase(), "wal");
}

#[test]
fn append_event_deduplicates_by_dedupe_key() {
    let store = NiumaStore::new(test_sqlite_path("append_event_deduplicates_by_dedupe_key"));
    let event = sample_event("same-dedupe", EventType::SessionStarted);

    store.append_event(event.clone()).unwrap();
    let state = store.append_event(event).unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.runtime_states.len(), 1);
}

#[test]
fn append_event_deduplicates_different_ids_by_public_dedupe_key() {
    let store = NiumaStore::new(test_sqlite_path(
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
    let store = NiumaStore::new(test_sqlite_path(
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
    let store = NiumaStore::new(test_sqlite_path("append_events_writes_multiple"));
    let first = sample_session_event("dedupe-a", "manual-a", EventType::SessionStarted, 1_000);
    let second = sample_session_event("dedupe-b", "manual-b", EventType::ApprovalRequested, 2_000);

    let state = store.append_events(vec![first, second]).unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.runtime_states.len(), 2);
    assert_eq!(
        state
            .runtime_states
            .iter()
            .find(|session| session.session_id == "manual-b")
            .unwrap()
            .status,
        RuntimeStateStatus::WaitingApproval
    );
}

#[test]
fn append_events_deduplicates_existing_events() {
    let store = NiumaStore::new(test_sqlite_path("append_events_deduplicates"));
    let first = sample_session_event("same-dedupe", "manual-a", EventType::SessionStarted, 1_000);
    let second = sample_session_event("same-dedupe", "manual-a", EventType::TaskFailed, 2_000);

    let state = store.append_events(vec![first, second]).unwrap();

    assert!(state.events.is_empty());
    assert_eq!(state.runtime_states[0].status, RuntimeStateStatus::Running);
}

#[test]
fn append_events_with_result_returns_only_applied_events() {
    let store = NiumaStore::new(test_sqlite_path("append_events_with_result_applied"));
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
    assert_eq!(result.state.runtime_states.len(), 1);
    assert_eq!(result.state.attention_items.len(), 1);
}

#[test]
fn completed_from_other_session_does_not_hide_existing_attention_item() {
    let store = NiumaStore::new(test_sqlite_path("completed_does_not_hide_attention"));
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

    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingApproval);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-a"));
}

#[test]
fn running_from_same_session_clears_all_attention_items_for_that_session() {
    let store = NiumaStore::new(test_sqlite_path("running_clears_same_session_attention"));
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
    assert_eq!(snapshot.status, RuntimeStateStatus::Error);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-b"));
}

#[test]
fn running_from_other_tool_same_session_does_not_clear_attention() {
    let store = NiumaStore::new(test_sqlite_path(
        "running_other_tool_same_session_keeps_attention",
    ));
    store
        .append_event(sample_session_event(
            "dedupe-codex-approval-same",
            "shared-session",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();

    let state = store
        .append_event(sample_tool_event(
            ToolKind::ClaudeCode,
            "dedupe-claude-running-same",
            "shared-session",
            EventType::SessionStarted,
            2_000,
        ))
        .unwrap();

    // 相同 session_id 只在同一工具内代表同一个运行态，跨工具事件不能清掉 Codex 的关注项。
    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].tool, ToolKind::Codex);
    assert_eq!(
        state.attention_items[0].status,
        RuntimeStateStatus::WaitingApproval
    );
    assert_eq!(
        state
            .runtime_states
            .iter()
            .find(
                |session| session.tool == ToolKind::Codex && session.session_id == "shared-session"
            )
            .unwrap()
            .status,
        RuntimeStateStatus::WaitingApproval
    );
}

#[test]
fn activity_from_same_session_clears_waiting_input() {
    let store = NiumaStore::new(test_sqlite_path("activity_clears_waiting_input"));
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
    assert_eq!(state.runtime_states[0].status, RuntimeStateStatus::Running);
    assert_eq!(snapshot.status, RuntimeStateStatus::Running);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-a"));
}

#[test]
fn resolved_activity_from_other_tool_same_session_keeps_unkeyed_approval() {
    let store = NiumaStore::new(test_sqlite_path(
        "resolved_other_tool_same_session_keeps_approval",
    ));
    store
        .append_event(sample_session_event(
            "dedupe-codex-unkeyed-approval",
            "shared-session",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();

    let state = store
        .append_event(
            sample_tool_event(
                ToolKind::ClaudeCode,
                "dedupe-claude-resolved-same",
                "shared-session",
                EventType::SessionActivity,
                2_000,
            )
            .with_attention_resolve_key("claude_permission:shared-session:call-1"),
        )
        .unwrap();

    // ClaudeCode 的恢复事件不能按 session_id 误清理 Codex 的未带 key 审批项。
    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].tool, ToolKind::Codex);
    assert_eq!(
        state.attention_items[0].status,
        RuntimeStateStatus::WaitingApproval
    );
}

#[test]
fn unkeyed_activity_keeps_keyed_approval_waiting() {
    let store = NiumaStore::new(test_sqlite_path("activity_keeps_keyed_approval"));
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
        .runtime_states
        .iter()
        .find(|session| session.session_id == "session-a")
        .unwrap();

    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(
        state.attention_items[0].status,
        RuntimeStateStatus::WaitingApproval
    );
    assert_eq!(session.status, RuntimeStateStatus::WaitingApproval);
    assert_eq!(
        session.last_activity_at,
        Utc.timestamp_opt(2_000, 0).single().unwrap()
    );
    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingApproval);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-a"));
}

#[test]
fn resolved_approval_activity_clears_only_matching_attention_item() {
    let store = NiumaStore::new(test_sqlite_path("resolved_approval_clears_matching"));
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
    assert_eq!(
        state.attention_items[0].status,
        RuntimeStateStatus::WaitingInput
    );
    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingInput);
}

#[test]
fn resolved_approval_activity_clears_unkeyed_hook_approval_without_hiding_input() {
    let store = NiumaStore::new(test_sqlite_path("resolved_approval_clears_hook_approval"));
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
    assert_eq!(
        state.attention_items[0].status,
        RuntimeStateStatus::WaitingInput
    );
    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingInput);
}

#[test]
fn idle_from_same_session_clears_attention_without_hiding_other_attention() {
    let store = NiumaStore::new(test_sqlite_path("idle_clears_same_session_attention"));
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
    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingInput);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-b"));
}

#[test]
fn idle_from_same_session_id_other_tool_does_not_clear_latest_activity() {
    let store = NiumaStore::new(test_sqlite_path("idle_other_tool_keeps_latest"));
    store
        .append_event(sample_session_event(
            "dedupe-codex-running-shared",
            "shared-session",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();

    let state = store
        .append_event(sample_tool_event(
            ToolKind::ClaudeCode,
            "dedupe-claude-idle-shared",
            "shared-session",
            EventType::SessionIdled,
            2_000,
        ))
        .unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    // latest_activity 的运行态身份必须同时匹配 tool 和 session_id，避免同名 session 跨工具串扰。
    let latest_activity = state.latest_activity.unwrap();
    assert_eq!(latest_activity.tool, Some(ToolKind::Codex));
    assert_eq!(
        latest_activity.session_id.as_deref(),
        Some("shared-session")
    );
    assert_eq!(latest_activity.status, RuntimeStateStatus::Running);
    assert_eq!(snapshot.status, RuntimeStateStatus::Running);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("shared-session")
    );
    // runtime_states 也要保留两个工具各自的状态，idle 只影响 ClaudeCode 自己。
    assert_eq!(
        state
            .runtime_states
            .iter()
            .find(
                |session| session.tool == ToolKind::Codex && session.session_id == "shared-session"
            )
            .unwrap()
            .status,
        RuntimeStateStatus::Running
    );
    assert_eq!(
        state
            .runtime_states
            .iter()
            .find(|session| session.tool == ToolKind::ClaudeCode
                && session.session_id == "shared-session")
            .unwrap()
            .status,
        RuntimeStateStatus::Idle
    );
}

#[test]
fn stale_clears_same_session_running_activity_without_becoming_primary_status() {
    let store = NiumaStore::new(test_sqlite_path("stale_clears_running_activity"));
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
            .runtime_states
            .iter()
            .find(|session| session.session_id == "session-a")
            .unwrap()
            .status,
        RuntimeStateStatus::Stale
    );
    assert_eq!(snapshot.status, RuntimeStateStatus::Idle);
    assert_eq!(snapshot.primary_session_id, None);
}

#[test]
fn stale_does_not_hide_attention_from_other_sessions() {
    let store = NiumaStore::new(test_sqlite_path("stale_keeps_other_attention"));
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
    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingInput);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("session-b"));
}

#[test]
fn stale_from_other_tool_same_session_does_not_hide_attention() {
    let store = NiumaStore::new(test_sqlite_path(
        "stale_other_tool_same_session_keeps_attention",
    ));
    store
        .append_event(sample_session_event(
            "dedupe-codex-input-same",
            "shared-session",
            EventType::InputRequested,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_tool_event(
            ToolKind::ClaudeCode,
            "dedupe-claude-stale-same",
            "shared-session",
            EventType::SessionStaled,
            2_000,
        ))
        .unwrap();

    let state = store.load().unwrap();

    // stale 清理范围必须限定到同一 tool + session_id，不能隐藏另一个工具的阻塞项。
    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].tool, ToolKind::Codex);
    assert_eq!(
        store.internal_status_snapshot().unwrap().status,
        RuntimeStateStatus::WaitingInput
    );
}

#[test]
fn mark_stale_running_sessions_only_stales_old_running_sessions() {
    let store = NiumaStore::new(test_sqlite_path("mark_stale_running_sessions"));
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
            .runtime_states
            .iter()
            .find(|session| session.session_id == "old-running")
            .unwrap()
            .status,
        RuntimeStateStatus::Stale
    );
    assert_eq!(
        state
            .runtime_states
            .iter()
            .find(|session| session.session_id == "new-running")
            .unwrap()
            .status,
        RuntimeStateStatus::Running
    );
    assert_eq!(
        state
            .runtime_states
            .iter()
            .find(|session| session.session_id == "old-completed")
            .unwrap()
            .status,
        RuntimeStateStatus::Completed
    );
}

#[test]
fn mark_stale_running_sessions_stales_same_session_id_for_each_tool() {
    let store = NiumaStore::new(test_sqlite_path("mark_stale_same_session_each_tool"));
    store
        .append_event(sample_session_event(
            "dedupe-codex-running-shared",
            "shared-session",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_tool_event(
            ToolKind::ClaudeCode,
            "dedupe-claude-running-shared",
            "shared-session",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();

    let now = Utc.timestamp_opt(2_000, 0).single().unwrap();
    let result = store
        .mark_stale_running_sessions_with_result(now, chrono::Duration::minutes(10))
        .unwrap();

    // stale 事件的 id/dedupe_key 必须带 tool，否则第二个同 session_id 运行态会被去重跳过。
    assert_eq!(result.staled_count, 2);
    for tool in [ToolKind::Codex, ToolKind::ClaudeCode] {
        assert_eq!(
            result
                .state
                .runtime_states
                .iter()
                .find(|session| session.tool == tool && session.session_id == "shared-session")
                .unwrap()
                .status,
            RuntimeStateStatus::Stale
        );
    }
}

#[test]
fn mark_stale_running_sessions_stales_at_exact_timeout_boundary() {
    let store = NiumaStore::new(test_sqlite_path("mark_stale_running_boundary"));
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
            .runtime_states
            .iter()
            .find(|session| session.session_id == "boundary-running")
            .unwrap()
            .status,
        RuntimeStateStatus::Stale
    );
}

#[test]
fn mark_stale_running_sessions_is_idempotent_for_same_now() {
    let store = NiumaStore::new(test_sqlite_path("mark_stale_running_idempotent"));
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
            .runtime_states
            .iter()
            .find(|session| session.session_id == "old-running")
            .unwrap()
            .status,
        RuntimeStateStatus::Stale
    );
}

#[test]
fn duplicate_attention_events_are_kept_when_dedupe_keys_are_different() {
    let store = NiumaStore::new(test_sqlite_path("duplicate_attention_kept"));
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
    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingApproval);
    assert_eq!(
        snapshot.updated_at,
        Some(Utc.timestamp_opt(1_000, 0).single().unwrap())
    );
    assert!(snapshot.primary_event.is_none());
}

#[test]
fn input_requested_updates_session_to_waiting_input() {
    let store = NiumaStore::new(test_sqlite_path("input_requested_updates_session"));
    let event = sample_event("dedupe-input", EventType::InputRequested);

    let state = store.append_event(event).unwrap();

    assert_eq!(state.runtime_states.len(), 1);
    assert_eq!(
        state.runtime_states[0].status,
        RuntimeStateStatus::WaitingInput
    );
}

#[test]
fn task_failed_updates_session_to_error() {
    let store = NiumaStore::new(test_sqlite_path("task_failed_updates_session"));
    let event = sample_event("dedupe-error", EventType::TaskFailed);

    let state = store.append_event(event).unwrap();

    assert_eq!(state.runtime_states.len(), 1);
    assert_eq!(state.runtime_states[0].status, RuntimeStateStatus::Error);
}

#[test]
fn reset_clears_events_and_sessions() {
    let store = NiumaStore::new(test_sqlite_path("reset_clears_events_and_sessions"));
    store
        .append_event(sample_event("dedupe-reset", EventType::ApprovalRequested))
        .unwrap();

    let state = store.reset().unwrap();

    assert!(state.events.is_empty());
    assert!(state.runtime_states.is_empty());
    assert!(store.load().unwrap().events.is_empty());
}

#[test]
fn notification_records_dedupe_by_notifier_and_event() {
    let store = NiumaStore::new(test_sqlite_path("notification_records_dedupe"));
    let record = sample_notification_record("record-1", "builtin-ntfy", "event-1");

    assert!(store.insert_notification_record_if_absent(&record).unwrap());
    assert!(!store.insert_notification_record_if_absent(&record).unwrap());

    let records = store.notification_records(20).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].notifier_id, "builtin-ntfy");
    assert_eq!(records[0].notifier_type, NotificationNotifierType::Builtin);
    assert_eq!(records[0].title.as_deref(), Some("任务失败"));
    assert_eq!(records[0].body.as_deref(), Some("项目：demo\n任务失败详情"));
}

#[test]
fn notification_records_allow_same_event_on_different_notifiers() {
    let store = NiumaStore::new(test_sqlite_path("notification_records_same_event"));
    let bark_record = sample_notification_record("record-bark", "builtin-bark", "event-1");
    let ntfy_record = sample_notification_record("record-ntfy", "builtin-ntfy", "event-1");

    assert!(store
        .insert_notification_record_if_absent(&bark_record)
        .unwrap());
    assert!(store
        .insert_notification_record_if_absent(&ntfy_record)
        .unwrap());

    assert_eq!(store.notification_records(20).unwrap().len(), 2);
}

#[test]
fn notification_records_allow_same_notifier_on_different_events() {
    let store = NiumaStore::new(test_sqlite_path("notification_records_same_notifier"));
    let first = sample_notification_record("record-1", "builtin-ntfy", "event-1");
    let second = sample_notification_record("record-2", "builtin-ntfy", "event-2");

    assert!(store.insert_notification_record_if_absent(&first).unwrap());
    assert!(store.insert_notification_record_if_absent(&second).unwrap());

    assert_eq!(store.notification_records(20).unwrap().len(), 2);
}

#[test]
fn notification_record_result_can_be_updated_after_reservation() {
    let store = NiumaStore::new(test_sqlite_path("notification_record_update_result"));
    let mut record = sample_notification_record("record-pending", "builtin-ntfy", "event-1");
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
    let store = NiumaStore::new(test_sqlite_path("plugin_notification_result_upsert"));
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
fn notification_history_records_marks_plugin_id_for_plugin_notifier() {
    let store = NiumaStore::new(test_sqlite_path("notification_history_merge"));
    let builtin = sample_notification_record("builtin-record", "builtin-ntfy", "event-builtin");
    let plugin = sample_plugin_notification_result("plugin-record", "builtin-ntfy", "event-plugin");

    store
        .insert_notification_record_if_absent(&builtin)
        .unwrap();
    store.save_plugin_notification_result(&plugin).unwrap();

    let records = store.notification_history_records(20).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .any(|record| record.channel == "builtin-ntfy" && record.plugin_id.is_none()));
    assert!(records.iter().any(|record| record.channel == "builtin-ntfy"
        && record.plugin_id.as_deref() == Some("builtin-ntfy")));
}

#[test]
fn notification_records_return_error_on_duplicate_id_for_different_event_and_notifier() {
    let store = NiumaStore::new(test_sqlite_path("notification_records_duplicate_id"));
    let first = sample_notification_record("record-1", "builtin-bark", "event-1");
    let duplicate_id = sample_notification_record("record-1", "builtin-ntfy", "event-2");

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
        let store = NiumaStore::new(path.clone());
        store.load().unwrap();

        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .execute(
                "INSERT INTO notification_records
                 (id, notifier_id, notifier_type, event_id, event_type, status, reason, error_message, created_at, sent_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7, ?8)",
                rusqlite::params![
                    format!("record-{name}"),
                    "builtin-bark",
                    "builtin",
                    format!("event-{name}"),
                    event_type,
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
fn notification_records_return_error_for_unknown_notifier_type() {
    let path = test_sqlite_path("notification_records_unknown_notifier_type");
    let store = NiumaStore::new(path.clone());
    store.load().unwrap();

    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO notification_records
             (id, notifier_id, notifier_type, event_id, event_type, status, reason, error_message, created_at, sent_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7, NULL)",
            rusqlite::params![
                "record-unknown-notifier-type",
                "builtin-bark",
                "sms",
                "event-unknown-channel",
                "\"task_failed\"",
                "\"sent\"",
                "2026-06-12T00:00:00Z",
            ],
        )
        .unwrap();

    assert!(store.notification_records(20).is_err());
}

#[test]
fn dismiss_active_blocker_clears_all_attention_items_without_changing_latest_activity() {
    let store = NiumaStore::new(test_sqlite_path("dismiss_clears_all_attention"));
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
    assert_eq!(snapshot.status, RuntimeStateStatus::Running);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("session-running")
    );
}

#[test]
fn dismiss_active_blocker_returns_none_without_waiting_session() {
    let store = NiumaStore::new(test_sqlite_path("dismiss_active_blocker_none"));
    store
        .append_event(sample_event("dedupe-running", EventType::SessionStarted))
        .unwrap();

    let event = store.dismiss_active_blocker().unwrap();

    assert!(event.is_none());
}

#[test]
fn sqlite_store_matches_core_status_flow() {
    let store = NiumaStore::new(test_sqlite_path("sqlite_core_status_flow"));
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
    assert_eq!(state.runtime_states.len(), 2);
    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingApproval);
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
fn listener_config_persists_to_json_config_file() {
    let root = test_data_dir("json_listener_config");
    let path = root.join("niuma.sqlite");
    let store = NiumaStore::new(path.clone());

    let default_config = store.listener_config().unwrap();
    store
        .save_listener_config(&ListenerConfig {
            codex_listening_enabled: false,
            ..ListenerConfig::default()
        })
        .unwrap();
    let reloaded = NiumaStore::new(path).listener_config().unwrap();

    assert!(root.join("config.json").exists());
    assert!(default_config.codex_listening_enabled);
    assert!(!reloaded.codex_listening_enabled);
}

#[test]
fn plugin_enabled_map_defaults_empty_and_persists() {
    let root = test_data_dir("json_plugin_enabled_map");
    let path = root.join("niuma.sqlite");
    let store = NiumaStore::new(path.clone());
    let mut enabled = BTreeMap::new();
    enabled.insert("builtin-bark".to_string(), true);

    let default_map = store.plugin_enabled_map().unwrap();
    store.save_plugin_enabled_map(&enabled).unwrap();
    let reloaded = NiumaStore::new(path).plugin_enabled_map().unwrap();

    assert!(root.join("config.json").exists());
    assert!(default_map.is_empty());
    assert_eq!(reloaded.get("builtin-bark"), Some(&true));
}

#[test]
fn plugin_config_persists_to_plugin_config_json_file() {
    let root = test_data_dir("json_plugin_config");
    let path = root.join("niuma.sqlite");
    let store = NiumaStore::new(path.clone());
    let mut config = serde_json::Map::new();
    config.insert("device_key".to_string(), serde_json::json!("device-1"));

    assert!(store.plugin_config("builtin-bark").unwrap().is_none());
    store.save_plugin_config("builtin-bark", &config).unwrap();
    let reloaded = NiumaStore::new(path.clone())
        .plugin_config("builtin-bark")
        .unwrap()
        .unwrap();

    assert!(root
        .join("plugin-configs")
        .join("builtin-bark.json")
        .exists());
    NiumaStore::new(path)
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
    let root = test_data_dir("json_language_preference");
    let path = root.join("niuma.sqlite");
    let store = NiumaStore::new(path.clone());

    let default_preference = store.language_preference().unwrap();
    store
        .save_language_preference(crate::platform::locale::LanguagePreference::Fixed(
            crate::platform::locale::SystemLanguage::Ja,
        ))
        .unwrap();
    let reloaded = NiumaStore::new(path).language_preference().unwrap();

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
fn plugin_runtime_states_are_memory_only() {
    let path = test_sqlite_path("runtime_states_memory_only");
    let store = NiumaStore::new(&path);
    store
        .save_plugin_runtime_state(
            "external-demo",
            PluginRuntimeState {
                status: PluginRuntimeStatus::Running,
                last_error: Some("boom".to_string()),
            },
        )
        .unwrap();

    let states = store.plugin_runtime_states().unwrap();
    assert_eq!(
        states.get("external-demo").map(|state| &state.status),
        Some(&PluginRuntimeStatus::Running)
    );

    let reloaded = NiumaStore::new(path);
    assert!(reloaded.plugin_runtime_states().unwrap().is_empty());
}

#[test]
fn clear_tool_state_removes_only_codex_aggregation() {
    let store = NiumaStore::new(test_sqlite_path("clear_codex_state_only"));
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

    assert_eq!(state.runtime_states.len(), 1);
    assert_eq!(state.runtime_states[0].tool, ToolKind::ClaudeCode);
    assert_eq!(state.runtime_states[0].session_id, "claude-session");
    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].session_id, "claude-session");
    let snapshot = store.internal_status_snapshot().unwrap();
    assert_eq!(snapshot.status, RuntimeStateStatus::WaitingApproval);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("claude-session")
    );
}

#[test]
fn clear_tool_state_keeps_other_tool_with_same_session_id() {
    let store = NiumaStore::new(test_sqlite_path(
        "clear_tool_state_same_session_keeps_other_tool",
    ));
    store
        .append_event(sample_session_event(
            "dedupe-codex-approval-shared-clear",
            "shared-session",
            EventType::ApprovalRequested,
            1_000,
        ))
        .unwrap();
    store
        .append_event(sample_tool_event(
            ToolKind::ClaudeCode,
            "dedupe-claude-input-shared-clear",
            "shared-session",
            EventType::InputRequested,
            2_000,
        ))
        .unwrap();

    let state = store.clear_tool_state(&ToolKind::Codex).unwrap();

    // 清理指定工具时要按 tool 区分运行态身份，保留另一个工具同 session_id 的关注项。
    assert_eq!(state.runtime_states.len(), 1);
    assert_eq!(state.runtime_states[0].tool, ToolKind::ClaudeCode);
    assert_eq!(state.runtime_states[0].session_id, "shared-session");
    assert_eq!(state.attention_items.len(), 1);
    assert_eq!(state.attention_items[0].tool, ToolKind::ClaudeCode);
    assert_eq!(
        store.internal_status_snapshot().unwrap().status,
        RuntimeStateStatus::WaitingInput
    );
}

#[test]
fn clear_tool_state_resets_codex_latest_activity_to_idle() {
    let store = NiumaStore::new(test_sqlite_path("clear_codex_latest_idle"));
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

    assert!(state.runtime_states.is_empty());
    assert!(state.attention_items.is_empty());
    assert_eq!(snapshot.status, RuntimeStateStatus::Idle);
}

#[test]
fn public_recent_events_filters_stale_and_respects_limit() {
    let store = NiumaStore::new(test_sqlite_path("public_recent_filters_stale"));
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
fn public_recent_events_keeps_only_recent_memory_cache() {
    let store = NiumaStore::new(test_sqlite_path("public_recent_memory_cache"));
    for index in 0..250 {
        store
            .append_event(sample_session_event(
                &format!("dedupe-cache-{index}"),
                &format!("session-cache-{index}"),
                EventType::AssistantMessageCompleted,
                index,
            ))
            .unwrap();
    }

    let events = store.public_recent_events(500).unwrap();

    assert_eq!(events.len(), 200);
    assert_eq!(events[0].id, "event_dedupe-cache-249");
    assert_eq!(events[199].id, "event_dedupe-cache-50");
}

#[test]
fn session_activity_keeps_session_running_and_updates_last_activity() {
    let store = NiumaStore::new(test_sqlite_path("session_activity_keeps_running"));
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
        .runtime_states
        .iter()
        .find(|session| session.session_id == "session-a")
        .unwrap();
    assert_eq!(session.status, RuntimeStateStatus::Running);
    assert_eq!(
        session.last_activity_at,
        Utc.timestamp_opt(1_200, 0).single().unwrap()
    );
    assert_eq!(
        store.internal_status_snapshot().unwrap().status,
        RuntimeStateStatus::Running
    );
}

#[test]
fn session_activity_after_completion_does_not_reopen_session() {
    let store = NiumaStore::new(test_sqlite_path("activity_after_completion_ignored"));
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
        .runtime_states
        .iter()
        .find(|session| session.session_id == "session-a")
        .unwrap();
    assert_eq!(session.status, RuntimeStateStatus::Completed);
    assert_eq!(
        store.internal_status_snapshot().unwrap().status,
        RuntimeStateStatus::Completed
    );
}

#[test]
fn subagent_completed_does_not_override_main_latest_activity() {
    let store = NiumaStore::new(test_sqlite_path(
        "subagent_completed_does_not_override_main",
    ));
    store
        .append_event(sample_session_event(
            "dedupe-main-running",
            "main-session",
            EventType::SessionStarted,
            1_000,
        ))
        .unwrap();
    let mut completed = sample_session_event(
        "dedupe-subagent-completed",
        "subagent-session",
        EventType::AssistantMessageCompleted,
        1_100,
    );
    completed.parent_session_id = Some("main-session".to_string());
    completed.normalized_session_id = Some("main-session".to_string());
    completed.session_scope = Some(EventSessionScope::Subagent);

    let state = store.append_event(completed).unwrap();

    let subagent = state
        .runtime_states
        .iter()
        .find(|session| session.session_id == "subagent-session")
        .unwrap();
    assert_eq!(subagent.status, RuntimeStateStatus::Completed);
    let snapshot = store.internal_status_snapshot().unwrap();
    assert_eq!(snapshot.status, RuntimeStateStatus::Running);
    assert_eq!(snapshot.primary_session_id.as_deref(), Some("main-session"));
    let events = store.public_recent_events(10).unwrap();
    assert!(events
        .iter()
        .any(|event| event.session_id == "subagent-session"
            && event.event_type == EventType::AssistantMessageCompleted));
}

#[test]
fn codex_rollback_sequence_finishes_even_with_late_token_count() {
    let store = NiumaStore::new(test_sqlite_path("rollback_sequence_late_token_count"));
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
        .runtime_states
        .iter()
        .find(|session| session.session_id == "session-rollback")
        .unwrap();
    assert_eq!(session.status, RuntimeStateStatus::Completed);
    assert_eq!(snapshot.status, RuntimeStateStatus::Completed);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("session-rollback")
    );
}

#[test]
fn public_recent_events_filters_internal_session_activity() {
    let store = NiumaStore::new(test_sqlite_path("public_recent_filters_activity"));
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
    let store = NiumaStore::new(test_sqlite_path("empty_path_preserves_existing"));
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
        .runtime_states
        .iter()
        .find(|session| session.session_id == "session-a")
        .unwrap();

    assert_eq!(session.project_path, "/tmp/demo");
    assert_eq!(session.project_name, "demo");
}

#[test]
fn sqlite_store_reset_and_dismiss_preserve_activity_behavior() {
    let store = NiumaStore::new(test_sqlite_path("sqlite_reset_and_dismiss"));
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
    assert_eq!(snapshot.status, RuntimeStateStatus::Running);
    assert_eq!(
        snapshot.primary_session_id.as_deref(),
        Some("session-running")
    );

    let state = store.reset().unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert!(state.events.is_empty());
    assert!(state.runtime_states.is_empty());
    assert_eq!(snapshot.status, RuntimeStateStatus::Idle);
}

fn sample_event(dedupe_key: &str, event_type: EventType) -> NiumaEvent {
    sample_session_event(dedupe_key, "s1", event_type, 1_000)
}

fn sample_approval_request(id: &str) -> ApprovalRequest {
    ApprovalRequest {
        id: id.to_string(),
        tool: ToolKind::Codex,
        session_id: "session-approval".to_string(),
        turn_id: "turn-approval".to_string(),
        tool_name: "Bash".to_string(),
        command: Some("cargo test".to_string()),
        description: Some("运行测试".to_string()),
        project_path: "/tmp/demo".to_string(),
        project_name: "demo".to_string(),
        status: ApprovalStatus::Pending,
        decided_by: None,
        decided_source: None,
        reason: None,
        created_at: Utc.timestamp_opt(1_000, 0).single().unwrap(),
        updated_at: Utc.timestamp_opt(1_000, 0).single().unwrap(),
        proxy_timeout_seconds: 600,
        proxy_status: ApprovalProxyStatus::Active,
        last_heartbeat_at: Some(Utc.timestamp_opt(1_000, 0).single().unwrap()),
        proxy_lost_at: None,
        channel: ApprovalChannel::HookProxy,
        control_ref: None,
    }
}

fn sample_approval_requested_event(request_id: &str) -> NiumaEvent {
    sample_session_event(
        &format!("approval-requested-{request_id}"),
        "session-approval",
        EventType::ApprovalRequested,
        1_000,
    )
    .with_attention_resolve_key(&format!("approval:{request_id}"))
    .with_payload_ref(&format!("approval:{request_id}"))
}

fn sample_notification_record(id: &str, notifier_id: &str, event_id: &str) -> NotificationRecord {
    NotificationRecord {
        id: id.to_string(),
        notifier_id: notifier_id.to_string(),
        notifier_type: NotificationNotifierType::Builtin,
        event_id: event_id.to_string(),
        event_type: EventType::TaskFailed,
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
        parent_session_id: None,
        normalized_session_id: None,
        session_scope: None,
        agent_nickname: None,
        agent_role: None,
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
    fn with_payload_ref(self, payload_ref: &str) -> Self;
}

impl EventTestExt for NiumaEvent {
    fn with_attention_resolve_key(mut self, key: &str) -> Self {
        self.attention_resolve_key = Some(key.to_string());
        self
    }

    fn with_payload_ref(mut self, payload_ref: &str) -> Self {
        self.payload_ref = Some(payload_ref.to_string());
        self
    }
}

fn test_sqlite_path(name: &str) -> std::path::PathBuf {
    test_data_dir(name).join("niuma.sqlite")
}

fn test_data_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "niuma-notifier-{name}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

fn assert_table_exists(connection: &rusqlite::Connection, table: &str) {
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
    assert!(exists, "table should exist: {table}");
}

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
