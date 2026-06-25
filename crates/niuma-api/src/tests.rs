use axum::body::{to_bytes, Body};
use axum::http::Request;
use chrono::{TimeZone, Utc};
use http_body_util::BodyExt;
use niuma_core::listener_config::ListenerConfig;
use niuma_core::models::{
    ApprovalProxyStatus, ApprovalRequest, ApprovalStatus, EventSessionScope, EventType, NiumaEvent,
    ToolKind,
};
use niuma_core::notification_store::{
    NotificationNotifierType, NotificationRecord, NotificationRecordStatus,
};
use niuma_core::runtime_event::{PluginNotificationTestRequest, RuntimeEvent, RuntimeEventBus};
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;
use niuma_core::tool_session::{
    ToolSessionDetail, ToolSessionListItem, ToolSessionMessage, ToolSessionMessageRole,
    ToolSessionStatus,
};
use serde_json::Value;
use std::sync::{Arc, Mutex as StdMutex};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tower::ServiceExt;

use crate::tool_sessions::{ToolSessionDetailProvider, ToolSessionListQuery, ToolSessionRegistry};
use crate::{
    app, app_with_bus, app_with_bus_and_plugin_dir, app_with_bus_and_tool_sessions,
    app_with_tool_sessions,
};

#[tokio::test]
async fn post_event_then_get_main_state_returns_waiting_approval() {
    let store = NiumaStore::new(test_path("post_event_then_get_main_state"));
    enable_codex_listener(&store);
    let router = app(store);
    let event = sample_event();

    let post = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&event).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), 200);

    let get = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/main-state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(get).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["state"]["status"], "waiting_approval");
    assert_eq!(value["data"]["state"]["session"]["id"], "s1");
    assert_eq!(value["data"]["state"]["updated_at"], "1970-01-01T00:16:40Z");
    assert_eq!(value["data"]["state"]["detail"]["event_id"], "event-1");
    assert_eq!(
        value["data"]["state"]["detail"]["content"],
        "Bash: cargo test"
    );
}

#[tokio::test]
async fn post_plugin_events_accepts_builtin_codex_events() {
    let store = NiumaStore::new(test_path("post_plugin_events"));
    enable_codex_listener(&store);
    let router = app(store);
    let event = sample_event();
    let body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "events": [event]
    });

    let post = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugin-events")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(post).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["plugin_id"], "builtin-codex");
    assert_eq!(value["data"]["applied_count"], 1);

    let get = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/main-state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(get).await;
    assert_eq!(value["data"]["state"]["status"], "waiting_approval");
}

#[tokio::test]
async fn post_plugin_events_delays_codex_watcher_approval() {
    let store = NiumaStore::new(test_path("post_plugin_events_watcher_approval"));
    enable_codex_listener(&store);
    let router = app(store);
    let mut event = sample_event_with_type(
        "watcher-approval",
        "watcher-dedupe",
        EventType::ApprovalRequested,
        1_000,
    );
    event.source = "codex-session-file".to_string();
    event.summary = "exec_command: cargo test".to_string();
    event.content = Some("exec_command: cargo test".to_string());
    event.payload_ref = Some("codex_watcher_approval:pending".to_string());
    let body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "events": [event]
    });

    let post = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugin-events")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(post).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["applied_count"], 0);
    assert_eq!(value["data"]["delayed_count"], 1);

    tokio::time::sleep(Duration::from_millis(2_100)).await;

    let state = get_json(&router, "/api/v1/main-state").await;
    assert_eq!(state["data"]["state"]["status"], "waiting_approval");
}

#[tokio::test]
async fn post_plugin_events_cancels_codex_watcher_approval_when_output_arrives() {
    let store = NiumaStore::new(test_path("post_plugin_events_watcher_output_cancels"));
    enable_codex_listener(&store);
    let router = app(store);
    let resolve_key = "codex_permission:s1:call-output-1";
    let mut approval = sample_event_with_type(
        "watcher-approval-output",
        "watcher-dedupe-output",
        EventType::ApprovalRequested,
        1_000,
    );
    approval.source = "codex-session-file".to_string();
    approval.summary = "exec_command: npm test".to_string();
    approval.content = Some("exec_command: npm test".to_string());
    approval.attention_resolve_key = Some(resolve_key.to_string());
    approval.payload_ref = Some("codex_watcher_approval:output".to_string());
    let approval_body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "events": [approval]
    });

    let approval_post = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugin-events")
                .header("content-type", "application/json")
                .body(Body::from(approval_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let approval_value = response_json(approval_post).await;
    assert_eq!(approval_value["code"], 0);
    assert_eq!(approval_value["data"]["applied_count"], 0);
    assert_eq!(approval_value["data"]["delayed_count"], 1);

    let mut output = sample_event_with_type(
        "watcher-output",
        "watcher-output-dedupe",
        EventType::SessionActivity,
        1_001,
    );
    output.source = "codex-session-file".to_string();
    output.summary = "Codex session activity".to_string();
    output.attention_resolve_key = Some(resolve_key.to_string());
    output.payload_ref = Some("/tmp/rollout.jsonl".to_string());
    let output_body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "events": [output]
    });

    let output_post = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugin-events")
                .header("content-type", "application/json")
                .body(Body::from(output_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let output_value = response_json(output_post).await;
    assert_eq!(output_value["code"], 0);
    assert_eq!(output_value["data"]["applied_count"], 1);

    tokio::time::sleep(Duration::from_millis(2_100)).await;

    let state = get_json(&router, "/api/v1/main-state").await;
    assert_eq!(state["data"]["state"]["status"], "running");
}

#[tokio::test]
async fn post_plugin_events_dedupes_repeated_events() {
    let router = app(NiumaStore::new(test_path("post_plugin_events_dedupe")));
    let event = sample_event();
    let body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "events": [event]
    })
    .to_string();

    for expected_applied in [1, 0] {
        let post = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/plugin-events")
                    .header("content-type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let value = response_json(post).await;
        assert_eq!(value["code"], 0);
        assert_eq!(value["data"]["applied_count"], expected_applied);
    }
}

#[tokio::test]
async fn post_plugin_events_rejects_tool_mismatch() {
    let router = app(NiumaStore::new(test_path("post_plugin_events_mismatch")));
    let mut event = sample_event();
    event.tool = ToolKind::ClaudeCode;
    let body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "events": [event]
    });

    let post = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugin-events")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(post).await;

    assert_eq!(value["code"], 100101);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("只能上报 codex"));
}

#[tokio::test]
async fn runtime_state_list_returns_standard_list_envelope() {
    let store = NiumaStore::new(test_path("runtime_state_list"));
    store.append_event(sample_event()).unwrap();
    let router = app(store);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/runtime_state_list")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["list"][0]["session_id"], "s1");
    // 运行态只保留工具 session_id 作为关联字段，不再暴露旧的 id 字段。
    assert!(value["data"]["list"][0].get("id").is_none());
}

#[tokio::test]
async fn old_sessions_route_is_removed() {
    let router = app(NiumaStore::new(test_path("old_sessions_route_removed")));

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 404);
    assert_eq!(value["code"], 900005);
}

#[tokio::test]
async fn approval_request_create_lists_pending_and_updates_main_state() {
    let store = NiumaStore::new(test_path("api_approval_request_create"));
    enable_codex_listener(&store);
    let router = app(store);

    let created = post_json(
        &router,
        "/api/v1/approval-requests",
        sample_approval_request_body("approval-1"),
    )
    .await;
    assert_eq!(created["code"], 0);
    assert_eq!(created["data"]["request_id"], "approval-1");
    assert_eq!(created["data"]["status"], "pending");

    let list = get_json(&router, "/api/v1/approval-requests?status=pending").await;
    assert_eq!(list["code"], 0);
    assert_eq!(list["data"]["list"][0]["id"], "approval-1");
    assert_eq!(list["data"]["list"][0]["status"], "pending");

    let main_state = get_json(&router, "/api/v1/main-state").await;
    assert_eq!(main_state["data"]["state"]["status"], "waiting_approval");
    assert_eq!(
        main_state["data"]["state"]["detail"]["approval"]["request_id"],
        "approval-1"
    );
    assert_eq!(
        main_state["data"]["state"]["detail"]["approval"]["can_decide"],
        true
    );
}

#[tokio::test]
async fn approval_decision_accepts_first_consumer_only() {
    let router = app(NiumaStore::new(test_path("api_approval_first")));
    post_json(
        &router,
        "/api/v1/approval-requests",
        sample_approval_request_body("approval-1"),
    )
    .await;

    let first = post_json(
        &router,
        "/api/v1/approval-decisions",
        serde_json::json!({
            "request_id": "approval-1",
            "decision": "allow",
            "decided_by": "desktop",
            "decided_source": "ui",
            "reason": "用户同意"
        }),
    )
    .await;
    assert_eq!(first["code"], 0);
    assert_eq!(first["data"]["accepted"], true);
    assert_eq!(first["data"]["status"], "allowed");
    assert_eq!(first["data"]["decision"], "allow");

    let second = post_json(
        &router,
        "/api/v1/approval-decisions",
        serde_json::json!({
            "request_id": "approval-1",
            "decision": "deny",
            "decided_by": "builtin-bark",
            "decided_source": "notification"
        }),
    )
    .await;
    assert_eq!(second["code"], 0);
    assert_eq!(second["data"]["accepted"], false);
    assert_eq!(second["data"]["status"], "allowed");
    assert_eq!(second["data"]["decided_by"], "desktop");

    let decision = get_json(&router, "/api/v1/approval-decisions?request_id=approval-1").await;
    assert_eq!(decision["code"], 0);
    assert_eq!(decision["data"]["status"], "allowed");
    assert_eq!(decision["data"]["decision"], "allow");
}

#[tokio::test]
async fn approval_heartbeat_keeps_pending_request_active() {
    let store = NiumaStore::new(test_path("api_approval_heartbeat"));
    enable_codex_listener(&store);
    let router = app(store);

    post_json(
        &router,
        "/api/v1/approval-requests",
        sample_approval_request_body("approval-1"),
    )
    .await;

    let response = post_json(
        &router,
        "/api/v1/approval-requests/heartbeat",
        serde_json::json!({
            "request_id": "approval-1",
            "source": "hook-helper"
        }),
    )
    .await;

    assert_eq!(response["code"], 0);
    assert_eq!(response["data"]["request_id"], "approval-1");
    assert_eq!(response["data"]["proxy_status"], "active");
}

#[tokio::test]
async fn approval_return_to_codex_keeps_main_state_without_actions() {
    let store = NiumaStore::new(test_path("api_approval_return"));
    enable_codex_listener(&store);
    let router = app(store);
    post_json(
        &router,
        "/api/v1/approval-requests",
        sample_approval_request_body("approval-1"),
    )
    .await;

    let returned = post_json(
        &router,
        "/api/v1/approval-requests/return",
        serde_json::json!({
            "request_id": "approval-1",
            "returned_by": "hook-helper",
            "reason": "10 分钟内未处理，请回到 Codex 中操作"
        }),
    )
    .await;
    assert_eq!(returned["code"], 0);
    assert_eq!(returned["data"]["accepted"], true);
    assert_eq!(returned["data"]["status"], "returned_to_codex");
    assert!(returned["data"]["decision"].is_null());

    let main_state = get_json(&router, "/api/v1/main-state").await;
    assert_eq!(main_state["data"]["state"]["status"], "waiting_approval");
    assert_eq!(
        main_state["data"]["state"]["detail"]["approval"]["can_decide"],
        false
    );
    assert_eq!(
        main_state["data"]["state"]["detail"]["approval"]["message"],
        "Niuma 已停止代处理，请回到 Codex 中同意或拒绝"
    );
}

#[tokio::test]
async fn approval_proxy_watchdog_returns_stale_request_to_codex() {
    let store = NiumaStore::new(test_path("api_approval_watchdog"));
    enable_codex_listener(&store);
    let heartbeat_at = Utc.timestamp_opt(100, 0).single().unwrap();
    let request = ApprovalRequest {
        id: "approval-1".to_string(),
        tool: ToolKind::Codex,
        session_id: "s1".to_string(),
        turn_id: "turn-1".to_string(),
        tool_name: "Bash".to_string(),
        command: Some("cargo test".to_string()),
        description: Some("运行测试".to_string()),
        project_path: "/tmp/demo".to_string(),
        project_name: "demo".to_string(),
        status: ApprovalStatus::Pending,
        decided_by: None,
        decided_source: None,
        reason: None,
        created_at: heartbeat_at,
        updated_at: heartbeat_at,
        proxy_timeout_seconds: 600,
        proxy_status: ApprovalProxyStatus::Active,
        last_heartbeat_at: Some(heartbeat_at),
        proxy_lost_at: None,
    };
    store.upsert_approval_request(request.clone()).unwrap();
    store
        .append_event(crate::handlers::approval_event_for_internal(
            &request,
            EventType::ApprovalRequested,
            "urgent",
            "approval-api",
        ))
        .unwrap();

    let bus = RuntimeEventBus::new();
    let mutation_service = StateMutationService::new(store.clone(), bus.clone());
    let swept = crate::approval_proxy_watchdog::sweep_approval_proxy_watchdog_at(
        &store,
        &mutation_service,
        Utc.timestamp_opt(109, 0).single().unwrap(),
        chrono::Duration::seconds(8),
    )
    .unwrap();

    assert_eq!(swept, 1);
    let router = app_with_bus(store, bus);
    let main_state = get_json(&router, "/api/v1/main-state").await;
    assert_eq!(main_state["data"]["state"]["status"], "waiting_approval");
    assert_eq!(
        main_state["data"]["state"]["detail"]["approval"]["status"],
        "returned_to_codex"
    );
    assert_eq!(
        main_state["data"]["state"]["detail"]["approval"]["can_decide"],
        false
    );
}

#[tokio::test]
async fn watcher_approval_unknown_is_delayed_then_fallback_applied() {
    let store = NiumaStore::new(test_path("api_watcher_delayed_fallback"));
    enable_codex_listener(&store);
    let router = app(store);
    let mut watcher = sample_event_with_type(
        "watcher-approval",
        "watcher-dedupe",
        EventType::ApprovalRequested,
        1_000,
    );
    watcher.source = "codex-session-file".to_string();
    watcher.summary = "exec_command: cargo test".to_string();
    watcher.content = Some("exec_command: cargo test".to_string());
    watcher.payload_ref = Some("codex_watcher_approval:pending".to_string());

    let response = post_json(
        &router,
        "/api/v1/events",
        serde_json::to_value(watcher).unwrap(),
    )
    .await;

    assert_eq!(response["code"], 0);
    assert_eq!(response["data"]["accepted"], true);
    assert_eq!(response["data"]["delayed"], true);
    assert_eq!(response["data"]["reason"], "waiting_for_hook_approval");

    let state_before = get_json(&router, "/api/v1/main-state").await;
    assert_ne!(state_before["data"]["state"]["status"], "waiting_approval");

    tokio::time::sleep(Duration::from_millis(2_100)).await;

    let state_after = get_json(&router, "/api/v1/main-state").await;
    assert_eq!(state_after["data"]["state"]["status"], "waiting_approval");
}

#[tokio::test]
async fn watcher_approval_after_hook_is_suppressed() {
    let store = NiumaStore::new(test_path("api_hook_first_suppresses_watcher"));
    enable_codex_listener(&store);
    let router = app(store);

    let created = post_json(
        &router,
        "/api/v1/approval-requests",
        sample_approval_request_body("approval-hook-first"),
    )
    .await;
    assert_eq!(created["code"], 0);
    assert_eq!(created["data"]["accepted"], true);
    assert_eq!(created["data"]["ownership"], "hook");

    let mut watcher = sample_event_with_type(
        "watcher-approval-after-hook",
        "watcher-dedupe-after-hook",
        EventType::ApprovalRequested,
        1_001,
    );
    watcher.source = "codex-session-file".to_string();
    watcher.summary = "exec_command: cargo test".to_string();
    watcher.content = Some("exec_command: cargo test".to_string());
    watcher.payload_ref = Some("codex_watcher_approval:late".to_string());

    let response = post_json(
        &router,
        "/api/v1/events",
        serde_json::to_value(watcher).unwrap(),
    )
    .await;

    assert_eq!(response["code"], 0);
    assert_eq!(response["data"]["accepted"], true);
    assert_eq!(response["data"]["suppressed"], true);
    assert_eq!(response["data"]["reason"], "hook_approval_already_emitted");

    tokio::time::sleep(Duration::from_millis(2_100)).await;

    let state_after = get_json(&router, "/api/v1/main-state").await;
    assert_eq!(state_after["data"]["state"]["status"], "waiting_approval");
    assert_eq!(
        state_after["data"]["state"]["detail"]["approval"]["request_id"],
        "approval-hook-first"
    );
    assert_eq!(
        state_after["data"]["state"]["detail"]["approval"]["can_decide"],
        true
    );
}

#[tokio::test]
async fn watcher_subagent_approval_after_hook_is_suppressed_by_normalized_session() {
    let store = NiumaStore::new(test_path("api_hook_first_suppresses_normalized_watcher"));
    enable_codex_listener(&store);
    let router = app(store);

    let created = post_json(
        &router,
        "/api/v1/approval-requests",
        sample_approval_request_body("approval-parent-session"),
    )
    .await;
    assert_eq!(created["code"], 0);
    assert_eq!(created["data"]["accepted"], true);

    let mut watcher = sample_event_with_type(
        "watcher-subagent-approval-after-hook",
        "watcher-subagent-dedupe-after-hook",
        EventType::ApprovalRequested,
        1_001,
    );
    watcher.source = "codex-session-file".to_string();
    watcher.session_id = "subagent-session".to_string();
    watcher.parent_session_id = Some("intermediate-parent".to_string());
    watcher.normalized_session_id = Some("s1".to_string());
    watcher.session_scope = Some(EventSessionScope::Subagent);
    watcher.summary = "exec_command: cargo test".to_string();
    watcher.content = Some("exec_command: cargo test".to_string());
    watcher.payload_ref = Some("codex_watcher_approval:subagent-late".to_string());

    let response = post_json(
        &router,
        "/api/v1/events",
        serde_json::to_value(watcher).unwrap(),
    )
    .await;

    assert_eq!(response["code"], 0);
    assert_eq!(response["data"]["accepted"], true);
    assert_eq!(response["data"]["suppressed"], true);
    assert_eq!(response["data"]["reason"], "hook_approval_already_emitted");
}

#[tokio::test]
async fn watcher_approval_after_hook_matches_approval_description() {
    let store = NiumaStore::new(test_path("api_hook_description_suppresses_watcher"));
    enable_codex_listener(&store);
    let router = app(store);
    let description =
        "是否允许我再次发起一次网络请求到 https://example.com，用来模拟真实的网络访问授权弹框？";

    let created = post_json(
        &router,
        "/api/v1/approval-requests",
        serde_json::json!({
            "request_id": "approval-description-first",
            "tool": "codex",
            "session_id": "s1",
            "turn_id": "turn-1",
            "tool_name": "Bash",
            "command": "curl --location --head --max-time 10 https://example.com",
            "description": description,
            "project_path": "/tmp/demo",
            "project_name": "demo",
            "timeout_seconds": 600
        }),
    )
    .await;
    assert_eq!(created["code"], 0);
    assert_eq!(created["data"]["accepted"], true);
    assert_eq!(created["data"]["ownership"], "hook");

    let mut watcher = sample_event_with_type(
        "watcher-approval-description-after-hook",
        "watcher-dedupe-description-after-hook",
        EventType::ApprovalRequested,
        1_001,
    );
    watcher.source = "codex-session-file".to_string();
    watcher.summary = format!("exec_command: {description}");
    watcher.content = Some(format!("exec_command: {description}"));
    watcher.payload_ref = Some("codex_watcher_approval:late-description".to_string());

    let response = post_json(
        &router,
        "/api/v1/events",
        serde_json::to_value(watcher).unwrap(),
    )
    .await;

    assert_eq!(response["code"], 0);
    assert_eq!(response["data"]["accepted"], true);
    assert_eq!(response["data"]["suppressed"], true);
    assert_eq!(response["data"]["reason"], "hook_approval_already_emitted");
}

#[tokio::test]
async fn late_hook_approval_after_watcher_fallback_returns_to_codex() {
    let store = NiumaStore::new(test_path("api_late_hook_fallback"));
    enable_codex_listener(&store);
    let router = app(store);
    let mut watcher = sample_event_with_type(
        "watcher-approval",
        "watcher-dedupe",
        EventType::ApprovalRequested,
        1_000,
    );
    watcher.source = "codex-session-file".to_string();
    watcher.summary = "exec_command: cargo test".to_string();
    watcher.content = Some("exec_command: cargo test".to_string());
    watcher.payload_ref = Some("codex_watcher_approval:pending".to_string());

    let response = post_json(
        &router,
        "/api/v1/events",
        serde_json::to_value(watcher).unwrap(),
    )
    .await;
    assert_eq!(response["code"], 0);
    assert_eq!(response["data"]["delayed"], true);

    tokio::time::sleep(Duration::from_millis(2_100)).await;

    let created = post_json(
        &router,
        "/api/v1/approval-requests",
        sample_approval_request_body("approval-late"),
    )
    .await;

    assert_eq!(created["code"], 0);
    assert_eq!(created["data"]["accepted"], false);
    assert_eq!(created["data"]["ownership"], "watcher_fallback");
    assert_eq!(created["data"]["hook_action"], "return_to_codex");
}

#[tokio::test]
async fn approval_decision_missing_request_is_business_failure() {
    let router = app(NiumaStore::new(test_path("api_approval_missing")));

    let response = post_json(
        &router,
        "/api/v1/approval-decisions",
        serde_json::json!({
            "request_id": "missing",
            "decision": "allow",
            "decided_by": "desktop",
            "decided_source": "ui"
        }),
    )
    .await;

    assert_eq!(response["code"], 100101);
    assert!(response["message"]
        .as_str()
        .unwrap()
        .contains("授权请求不存在"));
}

#[tokio::test]
async fn old_status_endpoint_is_removed() {
    let router = app(NiumaStore::new(test_path("old_status_removed")));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 404);
    assert_eq!(value["code"], 900005);
}

#[tokio::test]
async fn old_manual_test_reset_endpoint_is_removed() {
    // 正式 /api/v1/state/reset 是唯一清空状态入口，避免测试路由分叉。
    let router = app(NiumaStore::new(test_path("old_manual_test_reset_removed")));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/manual-test/reset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 404);
    assert_eq!(value["code"], 900005);
    assert_eq!(value["message"], "接口不存在");
}

#[tokio::test]
async fn post_event_publishes_appended_runtime_event() {
    let store = NiumaStore::new(test_path("post_event_runtime_event"));
    let bus = RuntimeEventBus::new();
    let mut receiver = bus.subscribe();
    let router = app_with_bus(store, bus);
    let event = sample_event();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&event).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(
        receiver.try_recv().unwrap(),
        RuntimeEvent::NiumaEventsAppended {
            version: 1,
            events: vec![event]
        }
    );
}

#[tokio::test]
async fn state_reset_requires_explicit_confirmation() {
    let router = app(NiumaStore::new(test_path("state_reset_requires_confirm")));

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/state/reset")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"confirm":"wrong","reason":"state_stuck"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["data"].is_null());
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("confirm 必须为 RESET_NIUMA_STATE"));
}

#[tokio::test]
async fn state_reset_clears_state_and_publishes_runtime_event() {
    let store = NiumaStore::new(test_path("state_reset_clears_state"));
    store.append_event(sample_event()).unwrap();
    let bus = RuntimeEventBus::new();
    let mut receiver = bus.subscribe();
    let router = app_with_bus(store, bus);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/state/reset")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"confirm":"RESET_NIUMA_STATE","reason":"state_stuck"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["reset"], true);
    assert_eq!(value["data"]["event_count"], 0);
    assert_eq!(value["data"]["session_count"], 0);
    assert_eq!(value["data"]["state"]["status"], "idle");
    assert!(value["data"]["reset_at"].as_str().is_some());
    assert_eq!(
        receiver.try_recv().unwrap(),
        RuntimeEvent::StateReset { version: 1 }
    );

    let get = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/main-state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let main_state = response_json(get).await;
    assert_eq!(main_state["data"]["state"]["status"], "idle");
}

#[tokio::test]
async fn invalid_json_returns_protocol_error_envelope() {
    let router = app(NiumaStore::new(test_path("invalid_json")));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/events")
                .header("content-type", "application/json")
                .body(Body::from("{bad"))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 400);
    assert_eq!(value["code"], 100004);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("请求体无法解析"));
}

#[tokio::test]
async fn route_not_found_returns_standard_envelope() {
    let router = app(NiumaStore::new(test_path("not_found")));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 404);
    assert_eq!(value["code"], 900005);
    assert_eq!(value["message"], "接口不存在");
}

#[tokio::test]
async fn manual_test_empty_sessions_is_business_failure() {
    let router = app(NiumaStore::new(test_path("manual_empty_sessions")));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/manual-test/scenario")
                .header("content-type", "application/json")
                .body(Body::from(r#"{ "scenario": "empty", "sessions": [] }"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("至少启用一个 session"));
}

#[tokio::test]
async fn notification_config_routes_are_removed() {
    let router = app(NiumaStore::new(test_path(
        "notification_config_routes_removed",
    )));

    for (method, uri) in [
        ("GET", "/api/v1/notification-config"),
        ("POST", "/api/v1/notification-config/save"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let value = response_json(response).await;

        assert_eq!(status, 404);
        assert_eq!(value["code"], 900005);
    }
}

#[tokio::test]
async fn listener_config_defaults_to_enabled_and_saves_disabled() {
    let router = app(NiumaStore::new(test_path("listener_config_round_trip")));

    let get = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/listener-config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let get_value = response_json(get).await;
    assert_eq!(get_value["code"], 0);
    assert_eq!(get_value["data"]["codex_listening_enabled"], true);
    assert_eq!(get_value["data"]["tool_listening_enabled"]["codex"], true);
    assert_eq!(get_value["data"]["tools"][0]["id"], "codex");
    assert_eq!(get_value["data"]["tools"][0]["plugin_id"], "builtin-codex");

    let save = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/listener-config/save")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"codex_listening_enabled":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(save.status(), 200);
    let save_value = response_json(save).await;
    assert_eq!(save_value["code"], 0);
    assert_eq!(save_value["data"]["saved"], true);
    assert_eq!(save_value["data"]["codex_listening_enabled"], false);
    assert_eq!(save_value["data"]["tool_listening_enabled"]["codex"], false);

    let get = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/listener-config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let get_value = response_json(get).await;
    assert_eq!(get_value["data"]["codex_listening_enabled"], false);
    assert_eq!(get_value["data"]["tools"][0]["enabled"], false);
}

#[tokio::test(flavor = "current_thread")]
async fn plugins_list_returns_builtin_plugin_status() {
    let _guard = env_lock().lock().unwrap();
    let codex_home = test_dir("plugins_list_codex_home");
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("CODEX_HOME", &codex_home);
    let store = NiumaStore::new(test_path("plugins_list"));
    store
        .save_plugin_runtime_state(
            "builtin-codex",
            niuma_core::plugin::PluginRuntimeState::running(),
        )
        .unwrap();
    let router = app_with_bus_and_plugin_dir(
        store,
        RuntimeEventBus::new(),
        test_dir("plugins_list_plugin_dir"),
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/plugins")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["list"][0]["id"], "builtin-codex");
    assert_eq!(value["data"]["list"][0]["runtime_status"], "running");
    assert_eq!(value["data"]["list"][0]["enabled"], true);
    assert!(value["data"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|plugin| plugin["id"] == "builtin-bark"
            && plugin["capabilities"]
                .as_array()
                .is_some_and(|capabilities| capabilities
                    .iter()
                    .any(|capability| capability == "notification_test"))
            && plugin["config_schema"]
                .as_array()
                .is_some_and(|schema| !schema.is_empty())));
    assert!(value["data"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|plugin| plugin["id"] == "builtin-ntfy"
            && plugin["capabilities"]
                .as_array()
                .is_some_and(|capabilities| capabilities
                    .iter()
                    .any(|capability| capability == "notification_test"))
            && plugin["config_schema"]
                .as_array()
                .is_some_and(|schema| !schema.is_empty())));
    let codex = value["data"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["id"] == "builtin-codex")
        .unwrap();
    assert!(codex["capabilities"]
        .as_array()
        .is_some_and(|capabilities| capabilities
            .iter()
            .any(|capability| capability == "event_watcher")));
    assert!(codex["capabilities"]
        .as_array()
        .is_some_and(|capabilities| capabilities
            .iter()
            .any(|capability| capability == "tool_session_list_provider")));
    assert!(codex["capabilities"]
        .as_array()
        .is_some_and(|capabilities| capabilities
            .iter()
            .any(|capability| capability == "tool_session_detail_provider")));
    assert!(codex["management_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["id"] == "codex_hook_install"));
    assert!(codex["management_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["label"] == "安装 Hook"));
    assert!(!codex["management_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["id"] == "codex_hook_uninstall"));
    restore_codex_home(previous_codex_home);
}

#[tokio::test]
async fn plugin_action_rejects_unknown_action_for_plugin() {
    let router = app_with_bus_and_plugin_dir(
        NiumaStore::new(test_path("plugin_action_unknown")),
        RuntimeEventBus::new(),
        test_dir("plugin_action_unknown_dir"),
    );

    let body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "action_id": "unknown_action"
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/actions")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 100101);
    assert!(value["message"].as_str().unwrap().contains("未知插件动作"));
}

#[tokio::test]
async fn plugin_config_save_validates_required_fields_and_publishes_event() {
    let store = NiumaStore::new(test_path("plugin_config_save"));
    let bus = RuntimeEventBus::new();
    let mut receiver = bus.subscribe();
    let router =
        app_with_bus_and_plugin_dir(store.clone(), bus, test_dir("plugin_config_save_dir"));

    let invalid_body = serde_json::json!({
        "plugin_id": "builtin-bark",
        "config": {
            "device_key": ""
        }
    });
    let invalid = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/config")
                .header("content-type", "application/json")
                .body(Body::from(invalid_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let invalid_value = response_json(invalid).await;
    assert_eq!(invalid_value["code"], 100101);

    let valid_body = serde_json::json!({
        "plugin_id": "builtin-bark",
        "config": {
            "device_key": "device-1"
        }
    });
    let valid = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/config")
                .header("content-type", "application/json")
                .body(Body::from(valid_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let valid_value = response_json(valid).await;

    assert_eq!(valid_value["code"], 0);
    assert_eq!(valid_value["data"]["saved"], true);
    assert_eq!(valid_value["data"]["config"]["device_key"], "device-1");
    assert_eq!(
        store
            .plugin_config("builtin-bark")
            .unwrap()
            .unwrap()
            .get("device_key"),
        Some(&serde_json::json!("device-1"))
    );
    assert_eq!(
        receiver.try_recv().unwrap(),
        RuntimeEvent::StateChanged {
            version: 1,
            reason: niuma_core::runtime_event::StateChangeReason::PluginConfigChanged
        }
    );
}

#[tokio::test]
async fn plugin_import_copies_folder_and_returns_plugin_list() {
    let source_dir = test_dir("plugin_import_source");
    let plugin_dir = test_dir("plugin_import_destination");
    write_demo_plugin(&source_dir, "niuma-plugin-import-test");
    std::fs::create_dir_all(source_dir.join("bin")).unwrap();
    std::fs::write(source_dir.join("bin/demo.mjs"), "console.log('demo')").unwrap();
    let store = NiumaStore::new(test_path("plugin_import"));
    let router =
        app_with_bus_and_plugin_dir(store.clone(), RuntimeEventBus::new(), plugin_dir.clone());

    let body = serde_json::json!({
        "source_dir": source_dir.to_string_lossy()
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/import")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["imported"], true);
    assert_eq!(value["data"]["plugin"]["id"], "niuma-plugin-import-test");
    assert_eq!(value["data"]["plugin"]["enabled"], true);
    assert_eq!(
        store
            .listener_config()
            .unwrap()
            .is_tool_enabled(&ToolKind::Custom("demo_tool".to_string())),
        true
    );
    assert!(plugin_dir
        .join("niuma-plugin-import-test/bin/demo.mjs")
        .exists());
}

#[tokio::test]
async fn plugin_import_rejects_builtin_plugin_id() {
    let source_dir = test_dir("plugin_import_builtin_source");
    let plugin_dir = test_dir("plugin_import_builtin_destination");
    write_demo_plugin(&source_dir, "builtin-codex");
    let router = app_with_bus_and_plugin_dir(
        NiumaStore::new(test_path("plugin_import_builtin")),
        RuntimeEventBus::new(),
        plugin_dir,
    );

    let body = serde_json::json!({
        "source_dir": source_dir.to_string_lossy()
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/import")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 100101);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("不能覆盖内置插件"));
}

#[tokio::test]
async fn plugin_enabled_updates_notification_plugin_map_and_publishes_event() {
    let store = NiumaStore::new(test_path("plugin_enabled_notification"));
    let bus = RuntimeEventBus::new();
    let mut receiver = bus.subscribe();
    let router = app_with_bus_and_plugin_dir(
        store.clone(),
        bus,
        test_dir("plugin_enabled_notification_dir"),
    );

    let body = serde_json::json!({
        "plugin_id": "builtin-bark",
        "enabled": true
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/enabled")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["saved"], true);
    assert_eq!(value["data"]["plugin_id"], "builtin-bark");
    assert_eq!(value["data"]["enabled"], true);
    assert_eq!(
        store.plugin_enabled_map().unwrap().get("builtin-bark"),
        Some(&true)
    );
    assert!(value["data"]["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .any(|plugin| plugin["id"] == "builtin-bark" && plugin["enabled"] == true));
    assert_eq!(
        receiver.try_recv().unwrap(),
        RuntimeEvent::StateChanged {
            version: 1,
            reason: niuma_core::runtime_event::StateChangeReason::PluginConfigChanged
        }
    );
}

#[tokio::test]
async fn plugin_enabled_updates_tool_listener_config() {
    let store = NiumaStore::new(test_path("plugin_enabled_tool"));
    let bus = RuntimeEventBus::new();
    let mut receiver = bus.subscribe();
    let router =
        app_with_bus_and_plugin_dir(store.clone(), bus, test_dir("plugin_enabled_tool_dir"));

    let body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "enabled": true
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/enabled")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["plugin_id"], "builtin-codex");
    assert_eq!(value["data"]["enabled"], true);
    assert!(store
        .listener_config()
        .unwrap()
        .is_tool_enabled(&ToolKind::Codex));
    assert_eq!(
        receiver.try_recv().unwrap(),
        RuntimeEvent::StateChanged {
            version: 1,
            reason: niuma_core::runtime_event::StateChangeReason::ListenerConfigChanged
        }
    );
}

#[tokio::test]
async fn plugin_enabled_updates_external_session_provider_map_without_touching_listener_config() {
    let store = NiumaStore::new(test_path("plugin_enabled_session_provider"));
    store
        .save_listener_config(&ListenerConfig::default().with_tool_enabled(&ToolKind::Codex, true))
        .unwrap();
    let bus = RuntimeEventBus::new();
    let mut receiver = bus.subscribe();
    let plugin_root = test_dir("plugin_enabled_session_provider_dir");
    let installed_dir = plugin_root.join("external-demo-session-provider");
    std::fs::create_dir_all(&installed_dir).unwrap();
    write_session_provider_plugin(
        &installed_dir,
        "external-demo-session-provider",
        "demo_tool",
    );
    let router = app_with_bus_and_plugin_dir(store.clone(), bus, plugin_root);

    let body = serde_json::json!({
        "plugin_id": "external-demo-session-provider",
        "enabled": false
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/enabled")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    // session provider 的启用状态独立存储，不能连带关闭 Codex event_watcher。
    assert_eq!(value["code"], 0);
    assert!(store
        .listener_config()
        .unwrap()
        .is_tool_enabled(&ToolKind::Codex));
    assert_eq!(
        store
            .plugin_enabled_map()
            .unwrap()
            .get("external-demo-session-provider"),
        Some(&false)
    );
    assert!(value["data"]["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |plugin| plugin["id"] == "external-demo-session-provider" && plugin["enabled"] == false
        ));
    assert_eq!(
        receiver.try_recv().unwrap(),
        RuntimeEvent::StateChanged {
            version: 1,
            reason: niuma_core::runtime_event::StateChangeReason::PluginConfigChanged
        }
    );
}

#[tokio::test]
async fn plugin_remove_deletes_external_plugin_and_disables_tool() {
    let plugin_dir = test_dir("plugin_remove_destination");
    let installed_dir = plugin_dir.join("niuma-plugin-remove-test");
    std::fs::create_dir_all(&installed_dir).unwrap();
    write_demo_plugin(&installed_dir, "niuma-plugin-remove-test");
    let store = NiumaStore::new(test_path("plugin_remove"));
    store
        .save_listener_config(
            &ListenerConfig::default()
                .with_tool_enabled(&ToolKind::Custom("demo_tool".to_string()), true),
        )
        .unwrap();
    store
        .save_plugin_runtime_state(
            "niuma-plugin-remove-test",
            niuma_core::plugin::PluginRuntimeState::running(),
        )
        .unwrap();
    let router =
        app_with_bus_and_plugin_dir(store.clone(), RuntimeEventBus::new(), plugin_dir.clone());

    let body = serde_json::json!({
        "plugin_id": "niuma-plugin-remove-test"
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/remove")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["removed"], true);
    assert!(!installed_dir.exists());
    assert!(value["data"]["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .all(|plugin| plugin["id"] != "niuma-plugin-remove-test"));
    assert!(!store
        .listener_config()
        .unwrap()
        .is_tool_enabled(&ToolKind::Custom("demo_tool".to_string())));
    assert!(!store
        .plugin_runtime_states()
        .unwrap()
        .contains_key("niuma-plugin-remove-test"));
}

#[tokio::test]
async fn plugin_remove_session_provider_does_not_disable_tool_listener() {
    let plugin_dir = test_dir("plugin_remove_session_provider_destination");
    let installed_dir = plugin_dir.join("demo-session-provider");
    std::fs::create_dir_all(&installed_dir).unwrap();
    write_session_provider_plugin(&installed_dir, "demo-session-provider", "demo_tool");
    let store = NiumaStore::new(test_path("plugin_remove_session_provider"));
    store
        .save_listener_config(
            &ListenerConfig::default()
                .with_tool_enabled(&ToolKind::Custom("demo_tool".to_string()), true),
        )
        .unwrap();
    let router =
        app_with_bus_and_plugin_dir(store.clone(), RuntimeEventBus::new(), plugin_dir.clone());

    let body = serde_json::json!({
        "plugin_id": "demo-session-provider"
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/remove")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["removed"], true);
    assert!(!installed_dir.exists());
    // session provider 有 tool_id，但没有 event_watcher，删除时不能复用工具监听开关。
    assert!(store
        .listener_config()
        .unwrap()
        .is_tool_enabled(&ToolKind::Custom("demo_tool".to_string())));
}

#[tokio::test]
async fn plugin_remove_rejects_builtin_plugin_id() {
    let router = app_with_bus_and_plugin_dir(
        NiumaStore::new(test_path("plugin_remove_builtin")),
        RuntimeEventBus::new(),
        test_dir("plugin_remove_builtin_destination"),
    );

    let body = serde_json::json!({
        "plugin_id": "builtin-codex"
    });
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/remove")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 100101);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("不能移除内置插件"));
}

#[tokio::test]
async fn listener_config_accepts_dynamic_tool_map() {
    let router = app(NiumaStore::new(test_path("listener_config_dynamic_map")));

    let save = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/listener-config/save")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"tool_listening_enabled":{"codex":true,"claude_code":false}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(save).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["tool_listening_enabled"]["codex"], true);
    assert_eq!(
        value["data"]["tool_listening_enabled"]["claude_code"],
        false
    );
}

#[tokio::test]
async fn listener_config_rejects_string_enabled_as_business_failure() {
    let router = app(NiumaStore::new(test_path(
        "listener_config_invalid_enabled",
    )));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/listener-config/save")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"codex_listening_enabled":"true"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["data"].is_null());
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("codex_listening_enabled 必须是布尔值"));
}

#[tokio::test]
async fn notification_records_returns_standard_list_envelope() {
    let store = NiumaStore::new(test_path("notification_records_list"));
    store
        .insert_notification_record_if_absent(&NotificationRecord {
            id: "record-api-title-body".to_string(),
            notifier_id: "builtin-ntfy".to_string(),
            notifier_type: NotificationNotifierType::Builtin,
            event_id: "event-api-title-body".to_string(),
            event_type: EventType::InputRequested,
            status: NotificationRecordStatus::Sent,
            title: Some("需要输入".to_string()),
            body: Some("项目：demo\n请选择运行方式".to_string()),
            reason: Some("input_requested".to_string()),
            error_message: None,
            created_at: Utc.timestamp_opt(1_000, 0).single().unwrap(),
            sent_at: Some(Utc.timestamp_opt(1_001, 0).single().unwrap()),
        })
        .unwrap();
    let router = app(store);
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/notification-records")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 0);
    assert!(value["data"]["list"].is_array());
    assert_eq!(value["data"]["list"][0]["title"], "需要输入");
    assert_eq!(
        value["data"]["list"][0]["body"],
        "项目：demo\n请选择运行方式"
    );
}

#[tokio::test]
async fn cors_preflight_allows_json_post() {
    let router = app(NiumaStore::new(test_path("cors")));
    let response = router
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/v1/manual-test/scenario")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-methods")
            .unwrap(),
        "GET, POST, OPTIONS"
    );
}

#[tokio::test]
async fn sse_stream_allows_cross_origin_event_source() {
    let router = app(NiumaStore::new(test_path("sse_cors")));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/state/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "*"
    );
}

#[tokio::test]
async fn sse_stream_emits_state_after_runtime_event() {
    let store = NiumaStore::new(test_path("sse_runtime_event"));
    enable_codex_listener(&store);
    let bus = RuntimeEventBus::new();
    let router = app_with_bus(store, bus);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/state/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();

    let initial = next_sse_chunk(&mut body).await;
    assert!(initial.contains("event: state"));
    assert!(initial.contains("id: 1"));
    assert!(initial.contains("\"status\":\"idle\""));
    assert!(initial.contains("\"session\":null"));
    assert!(initial.contains("\"detail\":null"));
    assert!(!initial.contains("\"snapshot\""));

    let event = sample_event();
    let post = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&event).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), 200);

    let updated = next_sse_chunk(&mut body).await;
    assert!(updated.contains("event: state"));
    assert!(updated.contains("id: 2"));
    assert!(updated.contains("\"status\":\"waiting_approval\""));
    assert!(updated.contains("\"event_id\":\"event-1\""));
    assert!(updated.contains("\"content\":\"Bash: cargo test\""));
    assert!(!updated.contains("NiumaEventsAppended"));
}

#[tokio::test]
async fn sse_stream_emits_state_updates_to_each_connected_client() {
    let store = NiumaStore::new(test_path("sse_multiple_clients"));
    enable_codex_listener(&store);
    let bus = RuntimeEventBus::new();
    let router = app_with_bus(store, bus);

    let first_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/state/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let second_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/state/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_response.status(), 200);
    assert_eq!(second_response.status(), 200);

    let mut first_body = first_response.into_body();
    let mut second_body = second_response.into_body();
    let first_initial = next_sse_chunk(&mut first_body).await;
    let second_initial = next_sse_chunk(&mut second_body).await;
    assert!(first_initial.contains("\"status\":\"idle\""));
    assert!(second_initial.contains("\"status\":\"idle\""));

    let event = sample_event();
    let post = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&event).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), 200);

    // 每个 SSE 连接都有自己的发送游标，同一次状态变化必须广播给所有在线客户端。
    let first_updated = next_sse_chunk(&mut first_body).await;
    let second_updated = next_sse_chunk(&mut second_body).await;
    assert!(first_updated.contains("\"status\":\"waiting_approval\""));
    assert!(second_updated.contains("\"status\":\"waiting_approval\""));
}

#[tokio::test]
async fn session_project_groups_stream_overlays_runtime_status() {
    let store = NiumaStore::new(test_path("session_project_groups_stream_runtime_status"));
    enable_codex_listener(&store);
    let bus = RuntimeEventBus::new();
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![tool_session_item(
            "s1",
            ToolKind::Codex,
            30,
            20,
            true,
            false,
        )],
    );
    let router = app_with_bus_and_tool_sessions(store, bus, registry);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();

    let initial = next_sse_chunk(&mut body).await;
    assert!(initial.contains("event: session_project_groups"));
    assert!(initial.contains("\"primary_session_id\":\"s1\""));
    assert!(initial.contains("\"status\":\"active\""));
    assert!(initial.contains("\"runtime_status\":null"));

    let mut event = sample_event();
    event.session_id = "s1".to_string();
    let posted = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&event).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(posted.status(), 200);

    let updated = next_sse_chunk(&mut body).await;
    assert!(updated.contains("event: session_project_groups"));
    assert!(updated.contains("\"primary_session_id\":\"s1\""));
    assert!(updated.contains("\"status\":\"active\""));
    assert!(updated.contains("\"runtime_status\":\"waiting_approval\""));
    assert!(updated.contains("\"runtime_last_event_id\":\"event-1\""));
}

#[tokio::test]
async fn session_detail_stream_emits_initial_and_updated_snapshot() {
    let store = NiumaStore::new(test_path("session_detail_stream"));
    enable_codex_listener(&store);
    let bus = RuntimeEventBus::new();
    let detail_provider = Arc::new(MutableDetailProvider::new(sample_tool_session_detail(
        "existing-session",
    )));
    let registry = session_detail_registry_with_provider(detail_provider.clone());
    let router = app_with_bus_and_tool_sessions(store, bus, registry);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_detail/stream?tool=codex&session_id=existing-session&limit=100")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();

    let initial = next_sse_chunk(&mut body).await;
    assert!(initial.contains("event: session_detail"));
    assert!(initial.contains("\"session_id\":\"existing-session\""));
    assert!(initial.contains("\"id\":\"m2\""));
    assert!(initial.contains("\"next_cursor\":\"next-1\""));

    let mut changed_detail = sample_tool_session_detail("existing-session");
    changed_detail.messages.insert(
        0,
        ToolSessionMessage {
            id: "m3".to_string(),
            role: ToolSessionMessageRole::Assistant,
            content: "updated".to_string(),
            created_at: Utc.timestamp_opt(30, 0).single().unwrap(),
            metadata: Value::Null,
        },
    );
    detail_provider.replace(changed_detail);

    let mut event = sample_event();
    event.session_id = "existing-session".to_string();
    event.normalized_session_id = Some("existing-session".to_string());
    let posted = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&event).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(posted.status(), 200);

    let updated = next_sse_chunk(&mut body).await;
    assert!(updated.contains("event: session_detail"));
    assert!(updated.contains("\"id\":\"m3\""));
    assert!(updated.contains("\"content\":\"updated\""));
}

#[tokio::test]
async fn session_detail_stream_requires_tool_and_session_id() {
    let registry = session_detail_registry_with_provider(Arc::new(FakeDetailProvider {
        detail: sample_tool_session_detail("existing-session"),
        calls: Arc::new(StdMutex::new(Vec::new())),
    }));
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_stream_missing")),
        registry,
    );

    for uri in [
        "/api/v1/session_detail/stream",
        "/api/v1/session_detail/stream?tool=codex",
        "/api/v1/session_detail/stream?session_id=existing-session",
        "/api/v1/session_detail/stream?tool=&session_id=existing-session",
        "/api/v1/session_detail/stream?tool=codex&session_id=",
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let value = response_json(response).await;
        assert_ne!(value["code"], 0, "{uri} 应返回业务失败");
    }
}

#[tokio::test]
async fn events_stream_allows_cross_origin_event_source() {
    let router = app(NiumaStore::new(test_path("events_sse_cors")));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/events/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "*"
    );
}

#[tokio::test]
async fn events_stream_emits_applied_event_after_post_event() {
    let store = NiumaStore::new(test_path("events_sse_post_event"));
    let router = app(store);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/events/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();

    let event = sample_event();
    let post = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&event).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), 200);

    let chunk = next_sse_chunk(&mut body).await;
    assert!(chunk.contains("event: event"));
    assert!(chunk.contains("id: event-1"));
    assert!(chunk.contains("\"id\":\"event-1\""));
    assert!(chunk.contains("\"event_type\":\"approval_requested\""));
    assert!(!chunk.contains("NiumaEventsAppended"));
}

#[tokio::test]
async fn events_stream_filters_events_by_session_and_event_type() {
    let store = NiumaStore::new(test_path("events_sse_filtered"));
    let router = app(store);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/events/stream?session_id=s1&event_type=approval_requested")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();

    let mut wrong_session =
        sample_event_with_type("event-2", "event-2", EventType::ApprovalRequested, 1_001);
    wrong_session.session_id = "s2".to_string();
    let wrong_type = sample_event_with_type("event-3", "event-3", EventType::TaskFailed, 1_002);
    let expected =
        sample_event_with_type("event-4", "event-4", EventType::ApprovalRequested, 1_003);

    for event in [wrong_session, wrong_type, expected] {
        let post = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&event).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post.status(), 200);
    }

    let chunk = next_sse_chunk(&mut body).await;
    assert!(chunk.contains("event: event"));
    assert!(chunk.contains("id: event-4"));
    assert!(chunk.contains("\"session_id\":\"s1\""));
    assert!(chunk.contains("\"event_type\":\"approval_requested\""));
    assert!(!chunk.contains("event-2"));
    assert!(!chunk.contains("event-3"));
}

#[tokio::test]
async fn events_stream_skips_duplicate_events() {
    let store = NiumaStore::new(test_path("events_sse_duplicate"));
    let router = app(store);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/events/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();

    let event = sample_event();
    for _ in 0..2 {
        let post = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&event).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post.status(), 200);
    }

    let chunk = next_sse_chunk(&mut body).await;
    assert!(chunk.contains("id: event-1"));
    assert!(
        no_sse_chunk_within(&mut body, Duration::from_millis(250)).await,
        "重复事件不应再次广播给事件消费者"
    );
}

#[tokio::test]
async fn events_stream_emits_notification_test_requests() {
    let store = NiumaStore::new(test_path("events_sse_notification_test"));
    let runtime_events = RuntimeEventBus::new();
    let router = app_with_bus(store, runtime_events.clone());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/events/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();
    let request = PluginNotificationTestRequest {
        test_id: "manual-test:builtin-ntfy:1".to_string(),
        plugin_id: "builtin-ntfy".to_string(),
        title: "NiuMa 测试通知".to_string(),
        body: "测试正文".to_string(),
        created_at: Utc::now(),
    };

    runtime_events.publish_plugin_notification_test(request);

    let chunk = next_sse_chunk(&mut body).await;
    assert!(chunk.contains("event: notification_test"));
    assert!(chunk.contains("id: manual-test:builtin-ntfy:1"));
    assert!(chunk.contains("\"plugin_id\":\"builtin-ntfy\""));
    assert!(!chunk.contains("NiumaEventsAppended"));
}

#[tokio::test]
async fn plugin_notification_records_save_sent_result() {
    let store = NiumaStore::new(test_path("plugin_notification_result_sent"));
    store.append_event(sample_event()).unwrap();
    let router = app_with_bus_and_plugin_dir(
        store.clone(),
        RuntimeEventBus::new(),
        test_dir("plugin_notification_result_sent_dir"),
    );
    let body = serde_json::json!({
        "plugin_id": "builtin-bark",
        "event_id": "event-1",
        "status": "sent",
        "title": "需要处理",
        "body": "项目：demo",
        "reason": "approval_requested",
        "sent_at": "2026-06-19T12:00:00Z"
    });

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/notification-results")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["saved"], true);
    assert_eq!(
        value["data"]["record_id"],
        "plugin_notification:builtin-bark:event-1"
    );
    let records = store.notification_history_records(20).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].plugin_id.as_deref(), Some("builtin-bark"));
    assert_eq!(records[0].channel, "builtin-bark");
}

#[tokio::test]
async fn plugin_notification_records_rejects_non_notification_plugin() {
    let store = NiumaStore::new(test_path("plugin_notification_result_non_notification"));
    store.append_event(sample_event()).unwrap();
    let router = app(store);
    let body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "event_id": "event-1",
        "status": "sent"
    });

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/notification-results")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 100101);
    assert!(value["message"].as_str().unwrap().contains("不是通知插件"));
}

#[tokio::test]
async fn plugin_notification_records_rejects_unknown_event() {
    let router = app(NiumaStore::new(test_path(
        "plugin_notification_result_unknown_event",
    )));
    let body = serde_json::json!({
        "plugin_id": "builtin-ntfy",
        "event_id": "missing-event",
        "status": "failed",
        "error_message": "network failed"
    });

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/notification-results")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 100101);
    assert!(value["message"].as_str().unwrap().contains("事件不存在"));
}

#[tokio::test]
async fn plugin_notification_test_results_save_sent_result() {
    let store = NiumaStore::new(test_path("plugin_notification_test_result_sent"));
    let router = app_with_bus_and_plugin_dir(
        store.clone(),
        RuntimeEventBus::new(),
        test_dir("plugin_notification_test_result_sent_dir"),
    );
    let body = serde_json::json!({
        "plugin_id": "builtin-ntfy",
        "test_id": "manual-test:builtin-ntfy:1",
        "status": "sent",
        "title": "NiuMa 测试通知",
        "body": "测试正文",
        "sent_at": "2026-06-19T12:00:00Z"
    });

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/notification-test-results")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["saved"], true);
    assert_eq!(
        value["data"]["record_id"],
        "plugin_notification_test:builtin-ntfy:manual-test:builtin-ntfy:1"
    );
    let records = store.notification_history_records(20).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].plugin_id.as_deref(), Some("builtin-ntfy"));
    assert_eq!(records[0].reason.as_deref(), Some("manual_test"));
    assert_eq!(records[0].event_type, EventType::SessionActivity);
}

#[tokio::test]
async fn plugin_notification_test_results_rejects_non_notification_plugin() {
    let router = app(NiumaStore::new(test_path(
        "plugin_notification_test_result_non_notification",
    )));
    let body = serde_json::json!({
        "plugin_id": "builtin-codex",
        "test_id": "manual-test:builtin-codex:1",
        "status": "sent"
    });

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/notification-test-results")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let value = response_json(response).await;

    assert_eq!(value["code"], 100101);
    assert!(value["message"].as_str().unwrap().contains("不是通知插件"));
}

#[tokio::test]
async fn session_list_returns_snapshot_with_filters() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![
            tool_session_item("codex-active", ToolKind::Codex, 30, 20, true, false),
            tool_session_item("codex-inactive", ToolKind::Codex, 40, 30, false, false),
            tool_session_item("codex-subagent", ToolKind::Codex, 50, 40, true, true),
        ],
    );
    registry.replace_snapshot(
        ToolKind::ClaudeCode,
        vec![tool_session_item(
            "claude-active",
            ToolKind::ClaudeCode,
            60,
            50,
            true,
            false,
        )],
    );
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_list_api_filters")),
        registry,
    );

    let value = get_json(
        &router,
        "/api/v1/session_list?tool=codex&include_subagents=true&active_only=true&limit=10",
    )
    .await;

    assert_eq!(value["code"], 0);
    assert_eq!(
        value["data"]["list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["session_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["codex-subagent", "codex-active"]
    );
    assert_eq!(value["data"]["list"][0]["tool"], "codex");
}

#[tokio::test]
async fn session_project_groups_returns_project_normalized_sessions() {
    let registry = ToolSessionRegistry::new();
    let mut main = tool_session_item("parent-session", ToolKind::Codex, 30, 20, false, false);
    main.first_user_message_preview = Some("主会话第一条用户消息".to_string());
    main.first_user_message_at = Some(Utc.timestamp_opt(10, 0).single().unwrap());
    let mut subagent = tool_session_item("child-session", ToolKind::Codex, 50, 50, true, true);
    subagent.agent_nickname = Some("Jason".to_string());
    subagent.agent_role = Some("default".to_string());
    subagent.first_user_message_preview = Some("子代理第一条用户消息".to_string());
    subagent.first_user_message_at = Some(Utc.timestamp_opt(40, 0).single().unwrap());
    let other_project = tool_session_item_with_project(
        "other-session",
        ToolKind::Codex,
        80,
        80,
        true,
        false,
        "/tmp/other",
        "other",
    );
    registry.replace_snapshot(ToolKind::Codex, vec![main, subagent, other_project]);
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_project_groups_api")),
        registry,
    );

    let value = get_json(
        &router,
        "/api/v1/session_project_groups?tool=codex&project_path=/tmp/demo&include_subagents=true&page=1&page_size=10",
    )
    .await;

    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["page"], 1);
    assert_eq!(value["data"]["page_size"], 10);
    assert_eq!(value["data"]["total"], 1);
    assert_eq!(value["data"]["list"][0]["project_path"], "/tmp/demo");
    assert_eq!(value["data"]["list"][0]["normalized_session_count"], 1);
    assert_eq!(value["data"]["list"][0]["raw_session_count"], 2);
    assert_eq!(value["data"]["list"][0]["subagent_count"], 1);
    assert_eq!(
        value["data"]["list"][0]["sessions"][0]["normalized_session_id"],
        "parent-session"
    );
    assert_eq!(
        value["data"]["list"][0]["sessions"][0]["primary_session_id"],
        "parent-session"
    );
    assert_eq!(value["data"]["list"][0]["sessions"][0]["status"], "active");
    assert_eq!(
        value["data"]["list"][0]["sessions"][0]["first_user_message_preview"],
        "主会话第一条用户消息"
    );
    assert_eq!(
        value["data"]["list"][0]["sessions"][0]["first_user_message_at"],
        "1970-01-01T00:00:10Z"
    );
    assert_eq!(
        value["data"]["list"][0]["sessions"][0]["raw_sessions"][1]["agent_nickname"],
        "Jason"
    );
}

#[tokio::test]
async fn session_project_groups_zero_page_returns_business_failure_envelope() {
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_project_groups_zero_page")),
        ToolSessionRegistry::new(),
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_project_groups?page=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["message"].as_str().unwrap().contains("page"));
}

#[tokio::test]
async fn session_list_invalid_limit_returns_standard_400() {
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_list_invalid_limit")),
        ToolSessionRegistry::new(),
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_list?limit=abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 400);
    assert_ne!(value["code"], 0);
    assert!(value["data"].is_null());
    assert!(value["message"].as_str().unwrap().contains("limit"));
}

#[tokio::test]
async fn session_list_zero_limit_returns_business_failure_envelope() {
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_list_zero_limit")),
        ToolSessionRegistry::new(),
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_list?limit=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["message"].as_str().unwrap().contains("limit"));
    // 业务失败仍必须保留统一 envelope 的 data 字段，允许为空或上下文对象。
    assert!(value.get("data").is_some());
    assert!(value["data"].is_null() || value["data"].is_object());
}

#[tokio::test]
async fn session_detail_missing_tool_session_id_business_failure() {
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_missing")),
        ToolSessionRegistry::new(),
    );

    for uri in [
        "/api/v1/session_detail?session_id=s1",
        "/api/v1/session_detail?tool=codex",
        "/api/v1/session_detail?tool=&session_id=s1",
        "/api/v1/session_detail?tool=codex&session_id=",
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let value = response_json(response).await;

        assert_eq!(status, 200);
        assert_eq!(value["code"], 100101);
        assert!(value["data"].is_null());
    }
}

#[tokio::test]
async fn session_detail_requires_existing_snapshot_session() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![tool_session_item(
            "existing-session",
            ToolKind::Codex,
            30,
            20,
            true,
            false,
        )],
    );
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_missing_snapshot")),
        registry,
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_detail?tool=codex&session_id=missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["message"]
        .as_str()
        .unwrap()
        .contains("session_id 不存在"));
}

#[tokio::test]
async fn session_detail_existing_snapshot_provider_not_ready_business_failure() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![tool_session_item(
            "existing-session",
            ToolKind::Codex,
            30,
            20,
            true,
            false,
        )],
    );
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_provider_not_ready")),
        registry,
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_detail?tool=codex&session_id=existing-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert_eq!(value["message"], "session detail provider 尚未就绪");
}

#[tokio::test]
async fn session_detail_existing_snapshot_with_fake_provider_returns_detail() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![tool_session_item(
            "existing-session",
            ToolKind::Codex,
            30,
            20,
            true,
            false,
        )],
    );
    registry.register_detail_provider(
        ToolKind::Codex,
        Arc::new(FakeDetailProvider {
            detail: sample_tool_session_detail("existing-session"),
            calls: Arc::new(StdMutex::new(Vec::new())),
        }),
    );
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_fake_provider")),
        registry,
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_detail?tool=codex&session_id=existing-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["session_id"], "existing-session");
    assert_eq!(value["data"]["messages"][0]["id"], "m2");
    assert_eq!(value["data"]["messages"][1]["id"], "m1");
}

#[tokio::test]
async fn session_detail_default_limit_passes_100_to_provider() {
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let registry = session_detail_registry_with_provider(Arc::new(FakeDetailProvider {
        detail: sample_tool_session_detail("existing-session"),
        calls: Arc::clone(&calls),
    }));
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_default_limit")),
        registry,
    );

    let value = get_json(
        &router,
        "/api/v1/session_detail?tool=codex&session_id=existing-session",
    )
    .await;

    assert_eq!(value["code"], 0);
    // limit 缺省值由宿主 API 统一归一化，provider 不再感知 None。
    assert_eq!(*calls.lock().unwrap(), vec![100]);
}

#[tokio::test]
async fn session_detail_caps_large_limit_before_provider_call() {
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let registry = session_detail_registry_with_provider(Arc::new(FakeDetailProvider {
        detail: sample_tool_session_detail("existing-session"),
        calls: Arc::clone(&calls),
    }));
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_large_limit")),
        registry,
    );

    let value = get_json(
        &router,
        "/api/v1/session_detail?tool=codex&session_id=existing-session&limit=900",
    )
    .await;

    assert_eq!(value["code"], 0);
    // provider 只接收封顶后的 limit，避免各 provider 重复实现同一规则。
    assert_eq!(*calls.lock().unwrap(), vec![500]);
}

#[tokio::test]
async fn session_detail_zero_limit_fails_without_provider_call() {
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let registry = session_detail_registry_with_provider(Arc::new(FakeDetailProvider {
        detail: sample_tool_session_detail("existing-session"),
        calls: Arc::clone(&calls),
    }));
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_zero_limit")),
        registry,
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_detail?tool=codex&session_id=existing-session&limit=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["message"].as_str().unwrap().contains("limit"));
    // limit=0 是宿主层业务校验失败，不能再调用 provider。
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn session_detail_rejects_provider_detail_for_other_session() {
    let registry = session_detail_registry_with_provider(Arc::new(FakeDetailProvider {
        detail: sample_tool_session_detail("other-session"),
        calls: Arc::new(StdMutex::new(Vec::new())),
    }));
    let router = app_with_tool_sessions(
        NiumaStore::new(test_path("session_detail_mismatched_provider_detail")),
        registry,
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/session_detail?tool=codex&session_id=existing-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["message"].as_str().unwrap().contains("归属不匹配"));
}

#[test]
fn tool_session_unregister_detail_provider_returns_not_ready() {
    let registry = ToolSessionRegistry::new();
    registry.register_detail_provider(
        ToolKind::Codex,
        Arc::new(FakeDetailProvider {
            detail: sample_tool_session_detail("existing-session"),
            calls: Arc::new(StdMutex::new(Vec::new())),
        }),
    );

    // provider 进程退出或被禁用时，宿主必须移除旧 client，避免请求打到失效 stdin。
    registry.unregister_detail_provider(&ToolKind::Codex);

    let error = registry
        .detail(&ToolKind::Codex, "existing-session", 100, None)
        .unwrap_err();
    assert_eq!(error, "session detail provider 尚未就绪");
}

#[test]
fn session_list_filters_snapshot_items() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![
            tool_session_item("codex-active", ToolKind::Codex, 30, 20, true, false),
            tool_session_item("codex-inactive", ToolKind::Codex, 40, 30, false, false),
            tool_session_item("codex-subagent", ToolKind::Codex, 50, 40, true, true),
        ],
    );
    registry.replace_snapshot(
        ToolKind::ClaudeCode,
        vec![tool_session_item(
            "claude-active",
            ToolKind::ClaudeCode,
            60,
            50,
            true,
            false,
        )],
    );

    let items = registry
        .list(ToolSessionListQuery {
            tool: Some("codex".to_string()),
            active_only: true,
            ..ToolSessionListQuery::default()
        })
        .unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].session_id, "codex-active");
    assert_eq!(items[0].tool, ToolKind::Codex);
}

#[test]
fn tool_session_clear_snapshot_removes_provider_sessions() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![tool_session_item(
            "existing-session",
            ToolKind::Codex,
            30,
            20,
            true,
            false,
        )],
    );

    // provider 生命周期结束后应清掉该 tool 的列表缓存，但不能影响其他 tool。
    registry.clear_snapshot(&ToolKind::Codex);

    let items = registry
        .list(ToolSessionListQuery {
            tool: Some("codex".to_string()),
            include_subagents: true,
            ..ToolSessionListQuery::default()
        })
        .unwrap();
    assert!(items.is_empty());
    assert!(registry
        .find_session(&ToolKind::Codex, "existing-session")
        .is_none());
}

#[test]
fn tool_session_list_all_merges_snapshots() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![tool_session_item(
            "codex-newer",
            ToolKind::Codex,
            100,
            80,
            true,
            false,
        )],
    );
    registry.replace_snapshot(
        ToolKind::ClaudeCode,
        vec![tool_session_item(
            "claude-older",
            ToolKind::ClaudeCode,
            90,
            85,
            true,
            false,
        )],
    );

    let items = registry
        .list(ToolSessionListQuery {
            tool: Some("all".to_string()),
            include_subagents: true,
            ..ToolSessionListQuery::default()
        })
        .unwrap();

    assert_eq!(
        items
            .iter()
            .map(|item| item.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["codex-newer", "claude-older"]
    );
}

#[test]
fn tool_session_replace_snapshot_normalizes_provider_tool_and_id() {
    let registry = ToolSessionRegistry::new();
    let mut item = tool_session_item(
        "provider-session",
        ToolKind::ClaudeCode,
        100,
        80,
        true,
        false,
    );
    item.id = "provider-supplied-id".to_string();

    registry.replace_snapshot(ToolKind::Codex, vec![item]);

    let items = registry
        .list(ToolSessionListQuery {
            tool: Some("codex".to_string()),
            include_subagents: true,
            ..ToolSessionListQuery::default()
        })
        .unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].tool, ToolKind::Codex);
    assert_eq!(items[0].id, "codex:provider-session");
}

#[test]
fn tool_session_list_uses_deterministic_tie_breakers() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![
            tool_session_item("codex-b", ToolKind::Codex, 100, 100, true, false),
            tool_session_item("codex-a", ToolKind::Codex, 100, 100, true, false),
        ],
    );
    registry.replace_snapshot(
        ToolKind::ClaudeCode,
        vec![tool_session_item(
            "claude-a",
            ToolKind::ClaudeCode,
            100,
            100,
            true,
            false,
        )],
    );

    let items = registry
        .list(ToolSessionListQuery {
            tool: Some("all".to_string()),
            include_subagents: true,
            limit: Some(2),
            ..ToolSessionListQuery::default()
        })
        .unwrap();

    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["claude_code:claude-a", "codex:codex-a"]
    );
}

#[test]
fn tool_session_list_limit_is_capped_at_500() {
    let registry = ToolSessionRegistry::new();
    let sessions = (0..520)
        .map(|index| {
            tool_session_item(
                &format!("codex-{index}"),
                ToolKind::Codex,
                index,
                index,
                true,
                false,
            )
        })
        .collect();
    registry.replace_snapshot(ToolKind::Codex, sessions);

    let items = registry
        .list(ToolSessionListQuery {
            tool: Some("all".to_string()),
            include_subagents: true,
            limit: Some(900),
            ..ToolSessionListQuery::default()
        })
        .unwrap();

    assert_eq!(items.len(), 500);
    assert_eq!(items[0].session_id, "codex-519");
}

#[test]
fn tool_session_list_limit_zero_returns_error() {
    let registry = ToolSessionRegistry::new();

    let error = registry
        .list(ToolSessionListQuery {
            limit: Some(0),
            ..ToolSessionListQuery::default()
        })
        .unwrap_err();

    assert!(error.contains("limit"));
}

#[test]
fn tool_session_find_session_matches_tool_and_session_id() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![tool_session_item(
            "shared-session",
            ToolKind::Codex,
            10,
            10,
            true,
            false,
        )],
    );
    registry.replace_snapshot(
        ToolKind::ClaudeCode,
        vec![tool_session_item(
            "shared-session",
            ToolKind::ClaudeCode,
            20,
            20,
            true,
            false,
        )],
    );

    let item = registry
        .find_session(&ToolKind::ClaudeCode, "shared-session")
        .unwrap();

    assert_eq!(item.tool, ToolKind::ClaudeCode);
    assert_eq!(item.id, "claude_code:shared-session");
}

#[test]
fn tool_session_project_groups_aggregates_subagents_under_normalized_session() {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![
            tool_session_item("parent-session", ToolKind::Codex, 30, 20, false, false),
            tool_session_item("child-session", ToolKind::Codex, 50, 50, true, true),
        ],
    );

    let page = registry
        .project_groups(crate::tool_sessions::ToolSessionProjectGroupsQuery {
            tool: Some("codex".to_string()),
            include_subagents: true,
            ..Default::default()
        })
        .unwrap();

    assert_eq!(page.total, 1);
    assert_eq!(page.list[0].normalized_session_count, 1);
    assert_eq!(page.list[0].raw_session_count, 2);
    assert_eq!(page.list[0].subagent_count, 1);
    assert_eq!(
        page.list[0].sessions[0].normalized_session_id,
        "parent-session"
    );
    assert_eq!(
        page.list[0].sessions[0].primary_session_id,
        "parent-session"
    );
    assert_eq!(page.list[0].sessions[0].status, ToolSessionStatus::Active);
    assert_eq!(
        page.list[0].sessions[0]
            .raw_sessions
            .as_ref()
            .unwrap()
            .len(),
        2
    );
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn post_json(router: &axum::Router, uri: &str, body: Value) -> Value {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    response_json(response).await
}

async fn get_json(router: &axum::Router, uri: &str) -> Value {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    response_json(response).await
}

async fn next_sse_chunk(body: &mut Body) -> String {
    let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .expect("SSE 应在超时时间内推送 state")
        .expect("SSE body 不应提前结束")
        .expect("SSE frame 读取成功");
    let bytes = frame.into_data().expect("SSE frame 应包含数据");
    String::from_utf8(bytes.to_vec()).expect("SSE frame 应是 UTF-8")
}

async fn no_sse_chunk_within(body: &mut Body, timeout: Duration) -> bool {
    tokio::time::timeout(timeout, body.frame()).await.is_err()
}

fn sample_event() -> NiumaEvent {
    sample_event_with_type("event-1", "dedupe-1", EventType::ApprovalRequested, 1_000)
}

fn sample_approval_request_body(request_id: &str) -> Value {
    serde_json::json!({
        "request_id": request_id,
        "tool": "codex",
        "session_id": "s1",
        "turn_id": "turn-1",
        "tool_name": "Bash",
        "command": "cargo test",
        "description": "运行测试",
        "project_path": "/tmp/demo",
        "project_name": "demo",
        "timeout_seconds": 600
    })
}

fn enable_codex_listener(store: &NiumaStore) {
    store
        .save_listener_config(&ListenerConfig {
            codex_listening_enabled: true,
            ..ListenerConfig::default()
        })
        .unwrap();
}

fn sample_event_with_type(
    id: &str,
    dedupe_key: &str,
    event_type: EventType,
    timestamp: i64,
) -> NiumaEvent {
    NiumaEvent {
        id: id.to_string(),
        dedupe_key: dedupe_key.to_string(),
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
        summary: "Bash: cargo test".to_string(),
        content: None,
        error_message: None,
        attention_resolve_key: None,
        completion_reason: None,
        failure_reason: None,
        payload_ref: None,
        created_at: Utc.timestamp_opt(timestamp, 0).single().unwrap(),
    }
}

fn tool_session_item(
    session_id: &str,
    tool: ToolKind,
    last_seen_at: i64,
    modified_at: i64,
    is_active: bool,
    is_subagent: bool,
) -> ToolSessionListItem {
    let tool_id = tool.as_str().to_string();
    ToolSessionListItem {
        id: format!("{tool_id}:{session_id}"),
        tool,
        session_id: session_id.to_string(),
        project_path: "/tmp/demo".to_string(),
        project_name: "demo".to_string(),
        file_path: format!("/tmp/demo/{session_id}.jsonl"),
        modified_at: Utc.timestamp_opt(modified_at, 0).single().unwrap(),
        discovered_at: Utc.timestamp_opt(1, 0).single().unwrap(),
        last_seen_at: Utc.timestamp_opt(last_seen_at, 0).single().unwrap(),
        is_active,
        is_subagent,
        parent_session_id: is_subagent.then(|| "parent-session".to_string()),
        normalized_session_id: Some(if is_subagent {
            "parent-session".to_string()
        } else {
            session_id.to_string()
        }),
        session_scope: Some(if is_subagent {
            niuma_core::tool_session::ToolSessionScope::Subagent
        } else {
            niuma_core::tool_session::ToolSessionScope::Main
        }),
        agent_nickname: None,
        agent_role: None,
        normalization_status: Some(
            niuma_core::tool_session::ToolSessionNormalizationStatus::Resolved,
        ),
        first_user_message_preview: None,
        first_user_message_at: None,
        control: None,
        status: if is_active {
            ToolSessionStatus::Active
        } else {
            ToolSessionStatus::Inactive
        },
    }
}

fn tool_session_item_with_project(
    session_id: &str,
    tool: ToolKind,
    last_seen_at: i64,
    modified_at: i64,
    is_active: bool,
    is_subagent: bool,
    project_path: &str,
    project_name: &str,
) -> ToolSessionListItem {
    let mut item = tool_session_item(
        session_id,
        tool,
        last_seen_at,
        modified_at,
        is_active,
        is_subagent,
    );
    item.project_path = project_path.to_string();
    item.project_name = project_name.to_string();
    item.file_path = format!("{project_path}/{session_id}.jsonl");
    item
}

struct FakeDetailProvider {
    detail: ToolSessionDetail,
    calls: Arc<StdMutex<Vec<usize>>>,
}

impl ToolSessionDetailProvider for FakeDetailProvider {
    fn detail(
        &self,
        _tool: &ToolKind,
        _session_id: &str,
        limit: usize,
        _cursor: Option<String>,
    ) -> Result<ToolSessionDetail, String> {
        self.calls.lock().unwrap().push(limit);
        Ok(self.detail.clone())
    }
}

struct MutableDetailProvider {
    detail: StdMutex<ToolSessionDetail>,
}

impl MutableDetailProvider {
    fn new(detail: ToolSessionDetail) -> Self {
        Self {
            detail: StdMutex::new(detail),
        }
    }

    fn replace(&self, detail: ToolSessionDetail) {
        *self.detail.lock().unwrap() = detail;
    }
}

impl ToolSessionDetailProvider for MutableDetailProvider {
    fn detail(
        &self,
        _tool: &ToolKind,
        _session_id: &str,
        _limit: usize,
        _cursor: Option<String>,
    ) -> Result<ToolSessionDetail, String> {
        Ok(self.detail.lock().unwrap().clone())
    }
}

fn session_detail_registry_with_provider(
    provider: Arc<dyn ToolSessionDetailProvider>,
) -> ToolSessionRegistry {
    let registry = ToolSessionRegistry::new();
    registry.replace_snapshot(
        ToolKind::Codex,
        vec![tool_session_item(
            "existing-session",
            ToolKind::Codex,
            30,
            20,
            true,
            false,
        )],
    );
    registry.register_detail_provider(ToolKind::Codex, provider);
    registry
}

fn sample_tool_session_detail(session_id: &str) -> ToolSessionDetail {
    ToolSessionDetail {
        tool: ToolKind::Codex,
        session_id: session_id.to_string(),
        project_path: "/tmp/demo".to_string(),
        project_name: "demo".to_string(),
        is_subagent: false,
        parent_session_id: None,
        normalized_session_id: Some(session_id.to_string()),
        session_scope: Some(niuma_core::tool_session::ToolSessionScope::Main),
        agent_nickname: None,
        agent_role: None,
        normalization_status: Some(
            niuma_core::tool_session::ToolSessionNormalizationStatus::Resolved,
        ),
        control: None,
        // provider 已经按倒序返回消息，API 不能再重排。
        messages: vec![
            ToolSessionMessage {
                id: "m2".to_string(),
                role: ToolSessionMessageRole::Assistant,
                content: "hello".to_string(),
                created_at: Utc.timestamp_opt(20, 0).single().unwrap(),
                metadata: Value::Null,
            },
            ToolSessionMessage {
                id: "m1".to_string(),
                role: ToolSessionMessageRole::User,
                content: "hi".to_string(),
                created_at: Utc.timestamp_opt(10, 0).single().unwrap(),
                metadata: Value::Null,
            },
        ],
        next_cursor: Some("next-1".to_string()),
    }
}

fn test_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "niuma-api-{name}-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("niuma.sqlite")
}

fn test_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("niuma-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn restore_codex_home(previous_codex_home: Option<String>) {
    if let Some(value) = previous_codex_home {
        std::env::set_var("CODEX_HOME", value);
    } else {
        std::env::remove_var("CODEX_HOME");
    }
}

fn write_demo_plugin(dir: &std::path::Path, id: &str) {
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{
                "id": "{id}",
                "tool_id": "demo_tool",
                "display_name": "Demo Tool",
                "version": "0.1.0",
                "command": "node",
                "args": ["./bin/demo.mjs"],
                "platforms": ["macos", "windows", "linux"],
                "capabilities": ["event_watcher"]
            }}"#
        ),
    )
    .unwrap();
}

fn write_session_provider_plugin(dir: &std::path::Path, id: &str, tool_id: &str) {
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{
                "id": "{id}",
                "kind": "tool",
                "tool_id": "{tool_id}",
                "display_name": "Session Provider",
                "version": "0.1.0",
                "command": "node",
                "args": ["./bin/session.mjs"],
                "platforms": ["macos", "windows", "linux"],
                "capabilities": ["tool_session_list_provider", "tool_session_detail_provider"]
            }}"#
        ),
    )
    .unwrap();
}
