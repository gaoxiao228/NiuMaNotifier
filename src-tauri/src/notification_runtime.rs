use chrono::Utc;
use niuma_core::models::{EventType, NiumaEvent};
use niuma_core::notification::{
    build_bark_request, build_ntfy_request, notification_decision_for_language,
    test_notification_message, BarkConfig, NotificationMessage, NtfyConfig, OutboundRequest,
};
use niuma_core::notification_store::{
    channel_id, NotificationChannel, NotificationChannelConfig, NotificationRecord,
    NotificationRecordStatus,
};
use niuma_core::platform::locale::{active_language, SystemLanguage};
use niuma_core::runtime_event::{RuntimeEvent, RuntimeEventBus};
use niuma_core::store::SqliteStateStore;
use niuma_core::tool_metadata::tool_notification_icon_url;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

#[path = "notification_runtime/http_sender.rs"]
mod http_sender;

#[cfg(test)]
use http_sender::{format_http_status_error, response_body_excerpt};
pub use http_sender::{NotificationSender, UreqNotificationSender};

const DEFAULT_BARK_SERVER: &str = "https://api.day.app";
const DEFAULT_BARK_GROUP: &str = "NiumaNotifier";
const DEFAULT_NTFY_SERVER: &str = "https://ntfy.sh";
const DEFAULT_NTFY_TOPIC_PREFIX: &str = "niuma-notifier";
static MANUAL_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static RUNTIME_RECORD_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationRuntimeErrorKind {
    BusinessValidation,
    ServiceUnavailable,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationRuntimeError {
    kind: NotificationRuntimeErrorKind,
    message: String,
}

impl NotificationRuntimeError {
    pub fn business_validation(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationRuntimeErrorKind::BusinessValidation,
            message: message.into(),
        }
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationRuntimeErrorKind::ServiceUnavailable,
            message: message.into(),
        }
    }

    pub fn system(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationRuntimeErrorKind::System,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> NotificationRuntimeErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn api_error_code(&self) -> niuma_core::api_response::ApiErrorCode {
        match self.kind() {
            NotificationRuntimeErrorKind::BusinessValidation => {
                niuma_core::api_response::ApiErrorCode::BusinessValidation
            }
            NotificationRuntimeErrorKind::ServiceUnavailable => {
                niuma_core::api_response::ApiErrorCode::ServiceUnavailable
            }
            NotificationRuntimeErrorKind::System => niuma_core::api_response::ApiErrorCode::System,
        }
    }
}

pub fn send_test_notification<S: NotificationSender>(
    store: SqliteStateStore,
    channel: String,
    sender: S,
) -> Result<serde_json::Value, NotificationRuntimeError> {
    let language = active_language();
    let channels = store
        .notification_channels()
        .map_err(NotificationRuntimeError::system)?;
    let target = channels
        .into_iter()
        .find(|config| channel_id(&config.channel) == channel)
        .ok_or_else(|| {
            NotificationRuntimeError::business_validation(notification_channel_not_configured(
                language,
            ))
        })?;
    if !target.enabled {
        return Err(NotificationRuntimeError::business_validation(
            notification_channel_disabled(language),
        ));
    }

    let message = test_notification_message(&channel);
    let request = match target.channel {
        NotificationChannel::Bark => {
            let config = parse_bark_config(&target.payload)?;
            build_bark_request(&config, &message)
                .map_err(NotificationRuntimeError::business_validation)?
        }
        NotificationChannel::Ntfy => {
            let config = parse_ntfy_config(&target.payload)?;
            build_ntfy_request(&config, &message)
                .map_err(NotificationRuntimeError::business_validation)?
        }
    };

    // 手动测试发送也写入通知记录，配置页可以复用历史列表展示成功/失败原因。
    let now = Utc::now();
    let send_result = sender.send(&request);
    let identity = manual_test_record_identity(&channel, now);
    let record = NotificationRecord {
        id: identity.record_id,
        event_id: identity.event_id,
        event_type: niuma_core::models::EventType::SessionActivity,
        channel: target.channel,
        status: if send_result.is_ok() {
            NotificationRecordStatus::Sent
        } else {
            NotificationRecordStatus::Failed
        },
        title: Some(message.title.clone()),
        body: Some(message.body.clone()),
        reason: Some("manual_test".to_string()),
        error_message: send_result.as_ref().err().cloned(),
        created_at: now,
        sent_at: send_result.is_ok().then_some(now),
    };
    let _ = store
        .insert_notification_record_if_absent(&record)
        .map_err(NotificationRuntimeError::system)?;
    send_result.map_err(NotificationRuntimeError::service_unavailable)?;
    Ok(json!({ "sent": true, "channel": channel }))
}

pub fn process_event<S: NotificationSender>(
    store: &SqliteStateStore,
    sender: &S,
    event: &NiumaEvent,
) -> Result<(), String> {
    process_event_for_language(store, sender, event, active_language())
}

fn process_event_for_language<S: NotificationSender>(
    store: &SqliteStateStore,
    sender: &S,
    event: &NiumaEvent,
    language: SystemLanguage,
) -> Result<(), String> {
    let Some(message) = notification_decision_for_language(event, language) else {
        return Ok(());
    };

    for channel in store
        .notification_channels()?
        .into_iter()
        .filter(|channel| channel.enabled)
    {
        let now = Utc::now();
        let record = NotificationRecord {
            id: runtime_record_id(event, &channel.channel, now),
            event_id: event.id.clone(),
            event_type: event.event_type.clone(),
            channel: channel.channel.clone(),
            status: NotificationRecordStatus::Pending,
            title: Some(message.title.clone()),
            body: Some(message.body.clone()),
            reason: Some(notification_reason(event).to_string()),
            error_message: None,
            created_at: now,
            sent_at: None,
        };
        if !store.insert_notification_record_if_absent(&record)? {
            continue;
        }

        let (status, error_message, sent_at) =
            match request_for_channel(&channel, &message, Some(&event.tool)) {
                Ok(request) => match sender.send(&request) {
                    Ok(()) => (NotificationRecordStatus::Sent, None, Some(Utc::now())),
                    Err(error) => {
                        // 单个渠道失败不应阻断其他渠道和后续事件；失败原因已写入通知记录。
                        eprintln!("NiumaNotifier notification send failed: {error}");
                        (NotificationRecordStatus::Failed, Some(error), None)
                    }
                },
                Err(error) => {
                    eprintln!("NiumaNotifier notification request invalid: {error}");
                    (NotificationRecordStatus::Failed, Some(error), None)
                }
            };
        if let Err(error) =
            store.update_notification_record_result(&record.id, status, error_message, sent_at)
        {
            // 发送结果更新失败不能阻断后续渠道；占位记录已能避免重复推送。
            eprintln!("NiumaNotifier notification record update failed: {error}");
        }
    }
    Ok(())
}

pub fn spawn_notification_runtime(
    store: SqliteStateStore,
    runtime_events: RuntimeEventBus,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("notification-runtime".to_string())
        .spawn(move || {
            run_notification_runtime(store, runtime_events, UreqNotificationSender::default())
        })
}

fn run_notification_runtime<S: NotificationSender>(
    store: SqliteStateStore,
    runtime_events: RuntimeEventBus,
    sender: S,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("NiumaNotifier notification runtime failed: {error}");
            return;
        }
    };

    runtime.block_on(async move {
        let mut events = runtime_events.subscribe();
        loop {
            tokio::select! {
                received = events.recv() => {
                    match received {
                        Ok(RuntimeEvent::NiumaEventsAppended { events, .. }) => {
                            for event in events {
                                if let Err(error) = process_event(&store, &sender, &event) {
                                    eprintln!("NiumaNotifier notification event failed: {error}");
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            eprintln!("NiumaNotifier notification runtime lagged {skipped} events");
                            // 通知只处理实时收到的事件，避免渠道启用后自动回放历史事件。
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });
}

fn parse_bark_config(payload: &Value) -> Result<BarkConfig, NotificationRuntimeError> {
    Ok(BarkConfig {
        server: optional_non_empty_string(payload, "server")
            .unwrap_or_else(|| DEFAULT_BARK_SERVER.to_string()),
        device_key: required_string(payload, "device_key", "Bark")?,
        group: optional_non_empty_string(payload, "group")
            .unwrap_or_else(|| DEFAULT_BARK_GROUP.to_string()),
        icon_url: String::new(),
    })
}

fn parse_ntfy_config(payload: &Value) -> Result<NtfyConfig, NotificationRuntimeError> {
    Ok(NtfyConfig {
        server: optional_non_empty_string(payload, "server")
            .unwrap_or_else(|| DEFAULT_NTFY_SERVER.to_string()),
        topic: optional_non_empty_string(payload, "topic").unwrap_or_else(default_ntfy_topic),
        token: optional_non_empty_string(payload, "token"),
    })
}

fn request_for_channel(
    channel: &NotificationChannelConfig,
    message: &NotificationMessage,
    tool: Option<&niuma_core::models::ToolKind>,
) -> Result<OutboundRequest, String> {
    match channel.channel {
        NotificationChannel::Bark => {
            let mut config = parse_bark_config(&channel.payload).map_err(|error| error.message)?;
            if let Some(tool) = tool {
                if let Some(icon_url) = tool_notification_icon_url(tool) {
                    config.icon_url = icon_url;
                }
            }
            build_bark_request(&config, message)
        }
        NotificationChannel::Ntfy => {
            let config = parse_ntfy_config(&channel.payload).map_err(|error| error.message)?;
            build_ntfy_request(&config, message)
        }
    }
}

fn required_string(
    payload: &Value,
    key: &str,
    channel: &str,
) -> Result<String, NotificationRuntimeError> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            NotificationRuntimeError::business_validation(invalid_channel_field_message(
                channel,
                key,
                active_language(),
            ))
        })
}

fn optional_non_empty_string(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn default_ntfy_topic() -> String {
    let seed = niuma_core::config::state_path()
        .to_string_lossy()
        .to_string();
    // topic 不暴露给用户填写时仍需保持本机稳定且不与其他安装冲突。
    format!("{}-{}", DEFAULT_NTFY_TOPIC_PREFIX, stable_hash(&seed))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ManualTestRecordIdentity {
    record_id: String,
    event_id: String,
}

fn manual_test_record_identity(
    channel: &str,
    now: chrono::DateTime<Utc>,
) -> ManualTestRecordIdentity {
    let nanos = now
        .timestamp_nanos_opt()
        .unwrap_or_else(|| now.timestamp_millis() * 1_000_000);
    let sequence = MANUAL_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    ManualTestRecordIdentity {
        record_id: format!("record_test_{channel}_{nanos}_{sequence}"),
        event_id: format!("manual-test:{channel}:{nanos}:{sequence}"),
    }
}

fn notification_reason(event: &NiumaEvent) -> &'static str {
    match event.event_type {
        EventType::ApprovalRequested => "approval_requested",
        EventType::InputRequested => "input_requested",
        EventType::TaskFailed => "task_failed",
        EventType::AssistantMessageCompleted => "completed",
        _ => "unknown",
    }
}

fn runtime_record_id(
    event: &NiumaEvent,
    channel: &NotificationChannel,
    now: chrono::DateTime<Utc>,
) -> String {
    let nanos = now
        .timestamp_nanos_opt()
        .unwrap_or_else(|| now.timestamp_millis() * 1_000_000);
    let sequence = RUNTIME_RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "record_{}_{}_{}_{}",
        event.id,
        channel_id(channel),
        nanos,
        sequence
    )
}

fn stable_hash(text: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:x}")
}

fn notification_channel_not_configured(language: SystemLanguage) -> &'static str {
    match language {
        SystemLanguage::ZhCn => "通知渠道未配置",
        SystemLanguage::ZhTw => "通知渠道未設定",
        SystemLanguage::En => "Notification channel is not configured",
        SystemLanguage::Ja => "通知チャンネルが設定されていません",
        SystemLanguage::Ko => "알림 채널이 설정되지 않았습니다",
        SystemLanguage::De => "Benachrichtigungskanal ist nicht konfiguriert",
    }
}

fn notification_channel_disabled(language: SystemLanguage) -> &'static str {
    match language {
        SystemLanguage::ZhCn => "通知渠道未启用",
        SystemLanguage::ZhTw => "通知渠道未啟用",
        SystemLanguage::En => "Notification channel is disabled",
        SystemLanguage::Ja => "通知チャンネルが無効です",
        SystemLanguage::Ko => "알림 채널이 비활성화되어 있습니다",
        SystemLanguage::De => "Benachrichtigungskanal ist deaktiviert",
    }
}

fn invalid_channel_field_message(channel: &str, key: &str, language: SystemLanguage) -> String {
    match language {
        SystemLanguage::ZhCn => format!("{channel} 配置无效：{key} 必须是字符串"),
        SystemLanguage::ZhTw => format!("{channel} 設定無效：{key} 必須是字串"),
        SystemLanguage::En => {
            format!("{channel} configuration is invalid: {key} must be a string")
        }
        SystemLanguage::Ja => {
            format!("{channel} の設定が無効です: {key} は文字列である必要があります")
        }
        SystemLanguage::Ko => {
            format!("{channel} 설정이 올바르지 않습니다: {key}는 문자열이어야 합니다")
        }
        SystemLanguage::De => {
            format!("{channel}-Konfiguration ist ungültig: {key} muss eine Zeichenkette sein")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use niuma_core::models::{CompletionReason, EventType, NiumaEvent, ToolKind};
    use niuma_core::notification::OutboundRequest;
    use niuma_core::notification_store::{
        NotificationChannel, NotificationChannelConfig, NotificationRecordStatus,
    };
    use niuma_core::store::SqliteStateStore;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn runtime_skips_duplicate_event_channel_record() {
        let store = test_store("runtime_duplicate");
        save_enabled_ntfy(&store);
        let sender = CountingSender::default();
        let event = completed_event("event-duplicate");

        process_event(&store, &sender, &event).unwrap();
        process_event(&store, &sender, &event).unwrap();

        assert_eq!(sender.count(), 1);
        assert_eq!(store.notification_records(20).unwrap().len(), 1);
    }

    #[test]
    fn runtime_skips_concurrent_duplicate_event_channel_record() {
        let store = test_store("runtime_concurrent_duplicate");
        save_enabled_ntfy(&store);
        let sender = CountingSender::with_delay(Duration::from_millis(100));
        let event = completed_event("event-concurrent-duplicate");

        let first_store = store.clone();
        let first_sender = sender.clone();
        let first_event = event.clone();
        let first = thread::spawn(move || process_event(&first_store, &first_sender, &first_event));

        thread::sleep(Duration::from_millis(20));
        let second_store = store.clone();
        let second_sender = sender.clone();
        let second_event = event.clone();
        let second =
            thread::spawn(move || process_event(&second_store, &second_sender, &second_event));

        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();

        assert_eq!(sender.count(), 1);
        assert_eq!(store.notification_records(20).unwrap().len(), 1);
    }

    #[test]
    fn runtime_does_not_send_when_channel_disabled() {
        let store = test_store("runtime_disabled");
        save_disabled_ntfy(&store);
        let sender = CountingSender::default();
        let event = completed_event("event-disabled");

        process_event(&store, &sender, &event).unwrap();

        assert_eq!(sender.count(), 0);
        assert!(store.notification_records(20).unwrap().is_empty());
    }

    #[test]
    fn runtime_records_failed_send_without_returning_error() {
        let store = test_store("runtime_failed_send");
        save_enabled_ntfy(&store);
        let sender = FailingSender("network unavailable".to_string());
        let event = completed_event("event-failed-send");

        process_event_for_language(&store, &sender, &event, SystemLanguage::ZhCn).unwrap();

        let records = store.notification_records(20).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, NotificationRecordStatus::Failed);
        assert_eq!(records[0].title.as_deref(), Some("任务完成"));
        assert!(records[0]
            .body
            .as_deref()
            .unwrap_or_default()
            .contains("项目：runtime"));
        assert!(records[0]
            .body
            .as_deref()
            .unwrap_or_default()
            .contains("完成了运行时通知测试"));
        assert_eq!(
            records[0].error_message.as_deref(),
            Some("network unavailable")
        );
        assert!(records[0].sent_at.is_none());
    }

    #[test]
    fn runtime_does_not_scan_public_events_from_store() {
        let store = test_store("runtime_no_backlog_scan");
        save_enabled_ntfy(&store);
        let sender = CountingSender::default();
        store
            .append_event(completed_event("event-public-only"))
            .expect("测试事件应写入公开事件表");

        // 通知发送只由实时事件入口触发，公开事件表不再作为历史补扫来源。
        assert_eq!(sender.count(), 0);
        assert!(store.notification_records(20).unwrap().is_empty());
    }

    #[test]
    fn runtime_bad_channel_config_does_not_block_other_channels() {
        let store = test_store("runtime_bad_channel_continues");
        save_invalid_bark_and_enabled_ntfy(&store);
        let sender = CountingSender::default();
        let event = completed_event("event-bad-channel");

        process_event(&store, &sender, &event).unwrap();

        let records = store.notification_records(20).unwrap();
        assert_eq!(sender.count(), 1);
        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|record| {
            record.channel == NotificationChannel::Bark
                && record.status == NotificationRecordStatus::Failed
                && record
                    .error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("device_key")
        }));
        assert!(records.iter().any(|record| {
            record.channel == NotificationChannel::Ntfy
                && record.status == NotificationRecordStatus::Sent
                && record.error_message.is_none()
        }));
    }

    #[test]
    fn runtime_adds_codex_tool_icon_to_bark_request() {
        let store = test_store("runtime_bark_codex_icon");
        save_enabled_bark(&store, json!({ "device_key": "abc123" }));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sender = RecordingSender {
            response_status: 200,
            requests: Arc::clone(&requests),
            error_message: None,
        };

        process_event(&store, &sender, &completed_event("event-bark-codex-icon")).unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].body.contains(
            "https://cdn.jsdelivr.net/npm/@lobehub/icons-static-png@latest/light/codex-color.png"
        ));
    }

    #[test]
    fn runtime_adds_claude_code_tool_icon_to_bark_request() {
        let store = test_store("runtime_bark_claude_icon");
        save_enabled_bark(&store, json!({ "device_key": "abc123" }));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sender = RecordingSender {
            response_status: 200,
            requests: Arc::clone(&requests),
            error_message: None,
        };
        let mut event = completed_event("event-bark-claude-icon");
        event.tool = ToolKind::ClaudeCode;

        process_event(&store, &sender, &event).unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].body.contains(
            "https://upload.wikimedia.org/wikipedia/commons/thumb/b/b0/Claude_AI_symbol.svg/1280px-Claude_AI_symbol.svg.png"
        ));
    }

    #[test]
    fn runtime_ignores_bark_payload_icon_url_for_tool_events() {
        let store = test_store("runtime_bark_ignores_payload_icon_url");
        save_enabled_bark(
            &store,
            json!({
                "device_key": "abc123",
                "icon_url": "https://example.test/custom.png"
            }),
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sender = RecordingSender {
            response_status: 200,
            requests: Arc::clone(&requests),
            error_message: None,
        };

        process_event(&store, &sender, &completed_event("event-bark-ignore-icon")).unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].body.contains(
            "https://cdn.jsdelivr.net/npm/@lobehub/icons-static-png@latest/light/codex-color.png"
        ));
        assert!(!requests[0].body.contains("https://example.test/custom.png"));
    }

    #[test]
    fn http_sender_treats_non_2xx_as_error() {
        let sender = RecordingSender {
            response_status: 500,
            requests: Arc::new(Mutex::new(Vec::new())),
            error_message: None,
        };
        let request = OutboundRequest {
            method: "POST".to_string(),
            url: "https://example.test".to_string(),
            headers: BTreeMap::new(),
            body: "body".to_string(),
        };

        let result = sender.send(&request);

        assert!(result.unwrap_err().contains("HTTP 500"));
    }

    #[test]
    fn send_test_notification_records_sent_ntfy_result() {
        let store = test_store("send_test_notification_sent");
        save_enabled_ntfy(&store);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sender = RecordingSender {
            response_status: 204,
            requests: Arc::clone(&requests),
            error_message: None,
        };

        let result = send_test_notification(store.clone(), "ntfy".to_string(), sender).unwrap();

        assert_eq!(result["sent"], json!(true));
        assert_eq!(result["channel"], json!("ntfy"));
        assert_eq!(requests.lock().unwrap().len(), 1);
        let records = store.notification_records(20).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].channel, NotificationChannel::Ntfy);
        assert_eq!(records[0].status, NotificationRecordStatus::Sent);
        assert_eq!(records[0].reason.as_deref(), Some("manual_test"));
        assert!(records[0].error_message.is_none());
        assert!(records[0].sent_at.is_some());
    }

    #[test]
    fn send_test_notification_accepts_bark_device_key_only() {
        let store = test_store("send_test_notification_bark_key_only");
        store
            .save_notification_channels(vec![NotificationChannelConfig {
                channel: NotificationChannel::Bark,
                enabled: true,
                payload: json!({
                    "device_key": "abc123"
                }),
                updated_at: Utc::now(),
            }])
            .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sender = RecordingSender {
            response_status: 200,
            requests: Arc::clone(&requests),
            error_message: None,
        };

        send_test_notification(store, "bark".to_string(), sender).unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "https://api.day.app/push");
        assert!(requests[0].body.contains(r#""device_key":"abc123""#));
        assert!(!requests[0].body.contains(r#""icon""#));
    }

    #[test]
    fn send_test_notification_accepts_ntfy_token_only() {
        let store = test_store("send_test_notification_ntfy_token_only");
        store
            .save_notification_channels(vec![NotificationChannelConfig {
                channel: NotificationChannel::Ntfy,
                enabled: true,
                payload: json!({
                    "token": "secret"
                }),
                updated_at: Utc::now(),
            }])
            .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sender = RecordingSender {
            response_status: 204,
            requests: Arc::clone(&requests),
            error_message: None,
        };

        send_test_notification(store, "ntfy".to_string(), sender).unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .url
            .starts_with("https://ntfy.sh/niuma-notifier-"));
        assert_eq!(
            requests[0].headers.get("Authorization").map(String::as_str),
            Some("Bearer secret")
        );
    }

    #[test]
    fn send_test_notification_records_failed_ntfy_result() {
        let store = test_store("send_test_notification_failed");
        save_enabled_ntfy(&store);
        let sender = RecordingSender {
            response_status: 500,
            requests: Arc::new(Mutex::new(Vec::new())),
            error_message: Some("HTTP 500".to_string()),
        };

        let error = send_test_notification(store.clone(), "ntfy".to_string(), sender)
            .expect_err("mock sender 失败时应返回错误");

        assert_eq!(
            error.kind(),
            NotificationRuntimeErrorKind::ServiceUnavailable
        );
        assert!(error.message().contains("HTTP 500"));
        let records = store.notification_records(20).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].channel, NotificationChannel::Ntfy);
        assert_eq!(records[0].status, NotificationRecordStatus::Failed);
        assert_eq!(records[0].reason.as_deref(), Some("manual_test"));
        assert_eq!(records[0].error_message.as_deref(), Some("HTTP 500"));
        assert!(records[0].sent_at.is_none());
    }

    #[test]
    fn send_test_notification_returns_business_error_for_missing_channel() {
        let store = test_store("send_test_notification_missing_channel");
        let sender = RecordingSender {
            response_status: 204,
            requests: Arc::new(Mutex::new(Vec::new())),
            error_message: None,
        };

        let error = send_test_notification(store, "ntfy".to_string(), sender)
            .expect_err("未配置渠道应返回业务校验错误");

        assert_eq!(
            error.kind(),
            NotificationRuntimeErrorKind::BusinessValidation
        );
        assert!(!error.message().is_empty());
    }

    #[test]
    fn send_test_notification_returns_business_error_for_disabled_channel() {
        let store = test_store("send_test_notification_disabled_channel");
        store
            .save_notification_channels(vec![NotificationChannelConfig {
                channel: NotificationChannel::Ntfy,
                enabled: false,
                payload: json!({
                    "server": "https://ntfy.example.test",
                    "topic": "niuma-test"
                }),
                updated_at: Utc::now(),
            }])
            .unwrap();
        let sender = RecordingSender {
            response_status: 204,
            requests: Arc::new(Mutex::new(Vec::new())),
            error_message: None,
        };

        let error = send_test_notification(store, "ntfy".to_string(), sender)
            .expect_err("未启用渠道应返回业务校验错误");

        assert_eq!(
            error.kind(),
            NotificationRuntimeErrorKind::BusinessValidation
        );
        assert!(!error.message().is_empty());
    }

    #[test]
    fn manual_test_record_ids_are_unique_for_fast_repeated_sends() {
        let first = manual_test_record_identity("ntfy", Utc::now());
        let second = manual_test_record_identity("ntfy", Utc::now());

        assert_ne!(first.record_id, second.record_id);
        assert_ne!(first.event_id, second.event_id);
    }

    #[test]
    fn ureq_sender_includes_status_url_and_body_excerpt_for_non_2xx() {
        let url = "https://ntfy.example.test/niuma-test";
        let response = ureq::Response::new(503, "Service Unavailable", "upstream is offline")
            .expect("测试响应应可构造");

        let error = format_http_status_error(503, url, response);

        assert!(error.contains("HTTP 503"));
        assert!(error.contains(url));
        assert!(error.contains("upstream is offline"));
    }

    #[test]
    fn response_body_excerpt_keeps_lossy_text_when_truncated_mid_utf8_char() {
        let body = format!("{}测 trailing text", "a".repeat(1023));
        let response =
            ureq::Response::new(500, "Internal Server Error", &body).expect("测试响应应可构造");

        let excerpt = response_body_excerpt(response);

        assert!(!excerpt.is_empty());
        assert!(excerpt.starts_with("aaa"));
    }

    #[derive(Clone)]
    struct RecordingSender {
        response_status: u16,
        requests: Arc<Mutex<Vec<OutboundRequest>>>,
        error_message: Option<String>,
    }

    impl NotificationSender for RecordingSender {
        fn send(&self, request: &OutboundRequest) -> Result<(), String> {
            self.requests.lock().unwrap().push(request.clone());
            if (200..300).contains(&self.response_status) {
                Ok(())
            } else {
                Err(self
                    .error_message
                    .clone()
                    .unwrap_or_else(|| format!("HTTP {}", self.response_status)))
            }
        }
    }

    #[derive(Clone, Default)]
    struct CountingSender {
        count: Arc<Mutex<usize>>,
        delay: Duration,
    }

    impl CountingSender {
        fn with_delay(delay: Duration) -> Self {
            Self {
                count: Arc::new(Mutex::new(0)),
                delay,
            }
        }

        fn count(&self) -> usize {
            *self.count.lock().unwrap()
        }
    }

    impl NotificationSender for CountingSender {
        fn send(&self, _request: &OutboundRequest) -> Result<(), String> {
            *self.count.lock().unwrap() += 1;
            if self.delay > Duration::ZERO {
                thread::sleep(self.delay);
            }
            Ok(())
        }
    }

    struct FailingSender(String);

    impl NotificationSender for FailingSender {
        fn send(&self, _request: &OutboundRequest) -> Result<(), String> {
            Err(self.0.clone())
        }
    }

    fn save_enabled_ntfy(store: &SqliteStateStore) {
        store
            .save_notification_channels(vec![NotificationChannelConfig {
                channel: NotificationChannel::Ntfy,
                enabled: true,
                payload: json!({
                    "server": "https://ntfy.example.test",
                    "topic": "niuma-test",
                    "token": "secret"
                }),
                updated_at: Utc::now(),
            }])
            .unwrap();
    }

    fn save_disabled_ntfy(store: &SqliteStateStore) {
        store
            .save_notification_channels(vec![NotificationChannelConfig {
                channel: NotificationChannel::Ntfy,
                enabled: false,
                payload: json!({
                    "server": "https://ntfy.example.test",
                    "topic": "niuma-test"
                }),
                updated_at: Utc::now(),
            }])
            .unwrap();
    }

    fn save_enabled_bark(store: &SqliteStateStore, payload: serde_json::Value) {
        store
            .save_notification_channels(vec![NotificationChannelConfig {
                channel: NotificationChannel::Bark,
                enabled: true,
                payload,
                updated_at: Utc::now(),
            }])
            .unwrap();
    }

    fn save_invalid_bark_and_enabled_ntfy(store: &SqliteStateStore) {
        store
            .save_notification_channels(vec![
                NotificationChannelConfig {
                    channel: NotificationChannel::Bark,
                    enabled: true,
                    payload: json!({
                        "server": "https://api.day.app"
                    }),
                    updated_at: Utc::now(),
                },
                NotificationChannelConfig {
                    channel: NotificationChannel::Ntfy,
                    enabled: true,
                    payload: json!({
                        "server": "https://ntfy.example.test",
                        "topic": "niuma-test"
                    }),
                    updated_at: Utc::now(),
                },
            ])
            .unwrap();
    }

    fn completed_event(id: &str) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: format!("dedupe-{id}"),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: "session-runtime".to_string(),
            project_path: "/tmp/runtime".to_string(),
            project_name: "runtime".to_string(),
            event_type: EventType::AssistantMessageCompleted,
            severity: "info".to_string(),
            summary: "完成了运行时通知测试".to_string(),
            content: Some("完成了运行时通知测试".to_string()),
            error_message: None,
            attention_resolve_key: None,
            completion_reason: Some(CompletionReason::Normal),
            failure_reason: None,
            payload_ref: None,
            created_at: Utc::now(),
        }
    }

    fn test_store(name: &str) -> SqliteStateStore {
        let path = std::env::temp_dir().join(format!(
            "niuma-notification-runtime-{name}-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        SqliteStateStore::new(path)
    }
}
