use axum::body::{to_bytes, Body};
use axum::http::Request;
use chrono::{TimeZone, Utc};
use http_body_util::BodyExt;
use niuma_core::listener_config::ListenerConfig;
use niuma_core::models::{EventType, NiumaEvent, ToolKind};
use niuma_core::notification_store::{
    NotificationChannel, NotificationRecord, NotificationRecordStatus,
};
use niuma_core::runtime_event::{RuntimeEvent, RuntimeEventBus};
use niuma_core::store::SqliteStateStore;
use serde_json::Value;
use std::time::Duration;
use tower::ServiceExt;

use crate::{app, app_with_bus};

#[tokio::test]
async fn post_event_then_get_main_state_returns_waiting_approval() {
    let store = SqliteStateStore::new(test_path("post_event_then_get_main_state"));
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
async fn get_sessions_returns_standard_list_envelope() {
    let store = SqliteStateStore::new(test_path("get_sessions_list"));
    store.append_event(sample_event()).unwrap();
    let router = app(store);

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

    assert_eq!(status, 200);
    assert_eq!(value["code"], 0);
    assert_eq!(value["data"]["list"][0]["id"], "s1");
}

#[tokio::test]
async fn old_status_endpoint_is_removed() {
    let router = app(SqliteStateStore::new(test_path("old_status_removed")));
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
    let router = app(SqliteStateStore::new(test_path(
        "old_manual_test_reset_removed",
    )));
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
    let store = SqliteStateStore::new(test_path("post_event_runtime_event"));
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
    let router = app(SqliteStateStore::new(test_path(
        "state_reset_requires_confirm",
    )));

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
    let store = SqliteStateStore::new(test_path("state_reset_clears_state"));
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
    let router = app(SqliteStateStore::new(test_path("invalid_json")));
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
    let router = app(SqliteStateStore::new(test_path("not_found")));
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
    let router = app(SqliteStateStore::new(test_path("manual_empty_sessions")));
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
async fn notification_config_round_trip_uses_standard_envelope() {
    let router = app(SqliteStateStore::new(test_path(
        "notification_config_round_trip",
    )));
    let body = serde_json::json!({
        "channels": [{
            "channel": "bark",
            "enabled": true,
            "payload": {
                "server": "https://api.day.app",
                "device_key": "plain:abc",
                "group": "NiumaNotifier",
                "icon_url": "",
                "secret_ref": null
            }
        }]
    });

    let save = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notification-config/save")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(save.status(), 200);
    let save_value = response_json(save).await;
    assert_eq!(save_value["code"], 0);

    let get = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/notification-config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let get_value = response_json(get).await;

    assert_eq!(get_value["code"], 0);
    assert_eq!(get_value["data"]["channels"][0]["channel"], "bark");
}

#[tokio::test]
async fn listener_config_defaults_to_disabled_and_saves_enabled() {
    let router = app(SqliteStateStore::new(test_path(
        "listener_config_round_trip",
    )));

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
    assert_eq!(get_value["data"]["codex_listening_enabled"], false);

    let save = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/listener-config/save")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"codex_listening_enabled":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(save.status(), 200);
    let save_value = response_json(save).await;
    assert_eq!(save_value["code"], 0);
    assert_eq!(save_value["data"]["saved"], true);
    assert_eq!(save_value["data"]["codex_listening_enabled"], true);

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
    assert_eq!(get_value["data"]["codex_listening_enabled"], true);
}

#[tokio::test]
async fn listener_config_rejects_string_enabled_as_business_failure() {
    let router = app(SqliteStateStore::new(test_path(
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
async fn notification_config_rejects_unknown_channel_as_business_failure() {
    let router = app(SqliteStateStore::new(test_path(
        "notification_config_invalid_channel",
    )));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notification-config/save")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"channels":[{"channel":"sms","enabled":true,"payload":{}}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["data"].is_null());
}

#[tokio::test]
async fn notification_config_rejects_string_payload_as_business_failure() {
    let router = app(SqliteStateStore::new(test_path(
        "notification_config_string_payload",
    )));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notification-config/save")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"channels":[{"channel":"bark","enabled":true,"payload":"bad"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["data"].is_null());
}

#[tokio::test]
async fn notification_config_rejects_string_enabled_as_business_failure() {
    let router = app(SqliteStateStore::new(test_path(
        "notification_config_string_enabled",
    )));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notification-config/save")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"channels":[{"channel":"bark","enabled":"true","payload":{}}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value = response_json(response).await;

    assert_eq!(status, 200);
    assert_eq!(value["code"], 100101);
    assert!(value["data"].is_null());
}

#[tokio::test]
async fn notification_records_returns_standard_list_envelope() {
    let store = SqliteStateStore::new(test_path("notification_records_list"));
    store
        .insert_notification_record_if_absent(&NotificationRecord {
            id: "record-api-title-body".to_string(),
            event_id: "event-api-title-body".to_string(),
            event_type: EventType::InputRequested,
            channel: NotificationChannel::Ntfy,
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
    let router = app(SqliteStateStore::new(test_path("cors")));
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
    let router = app(SqliteStateStore::new(test_path("sse_cors")));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/stream")
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
    let store = SqliteStateStore::new(test_path("sse_runtime_event"));
    enable_codex_listener(&store);
    let bus = RuntimeEventBus::new();
    let router = app_with_bus(store, bus);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/stream")
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
    let store = SqliteStateStore::new(test_path("sse_multiple_clients"));
    enable_codex_listener(&store);
    let bus = RuntimeEventBus::new();
    let router = app_with_bus(store, bus);

    let first_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let second_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/stream")
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

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
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

fn sample_event() -> NiumaEvent {
    sample_event_with_type("event-1", "dedupe-1", EventType::ApprovalRequested, 1_000)
}

fn enable_codex_listener(store: &SqliteStateStore) {
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

fn test_path(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("niuma-api-{name}-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}
