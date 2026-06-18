use crate::models::{CompletionReason, EventType, FailureReason, NiumaEvent, ToolKind};
use crate::notification::{
    build_bark_request, build_ntfy_request, notification_decision_for_language, BarkConfig,
    NotificationMessage, NtfyConfig,
};
use crate::platform::locale::SystemLanguage;
use chrono::{TimeZone, Utc};

#[test]
fn notification_policy_skips_interrupted_completion() {
    let event = sample_event(
        EventType::AssistantMessageCompleted,
        "real answer",
        Some(CompletionReason::Interrupted),
        None,
    );

    assert_eq!(decide(&event), None);
}

#[test]
fn notification_policy_skips_rolled_back_completion() {
    let event = sample_event(
        EventType::AssistantMessageCompleted,
        "Codex task completed",
        Some(CompletionReason::RolledBack),
        None,
    );

    assert_eq!(decide(&event), None);
}

#[test]
fn notification_policy_sends_normal_completion_with_real_summary() {
    let event = sample_event(
        EventType::AssistantMessageCompleted,
        "实现完成，测试通过。",
        Some(CompletionReason::Normal),
        None,
    );

    let message = decide(&event).unwrap();

    assert_eq!(message.title, "任务完成");
    assert!(message.body.contains("实现完成，测试通过。"));
}

#[test]
fn notification_policy_sends_normal_completion_with_real_content() {
    let mut event = sample_event(
        EventType::AssistantMessageCompleted,
        "Codex task completed",
        Some(CompletionReason::Normal),
        None,
    );
    event.content = Some("已经完成通知历史刷新修复。".to_string());

    let message = decide(&event).unwrap();

    assert_eq!(message.title, "任务完成");
    assert!(message.body.contains("已经完成通知历史刷新修复。"));
    assert!(!message.body.contains("Codex task completed"));
}

#[test]
fn notification_policy_keeps_full_push_body() {
    let mut event = sample_event(
        EventType::AssistantMessageCompleted,
        "Codex task completed",
        Some(CompletionReason::Normal),
        None,
    );
    // 推送服务由下游客户端决定如何展示，NiumaNotifier 发送前不主动截断正文。
    event.content = Some(format!(
        "第一行\n第二行\n第三行\n第四行必须保留\n{}尾部标记必须保留",
        "长正文".repeat(180)
    ));

    let message = decide(&event).unwrap();

    assert!(message.body.contains("第四行必须保留"));
    assert!(message.body.contains("尾部标记必须保留"));
}

#[test]
fn notification_policy_sends_normal_completion_with_generic_summary() {
    let event = sample_event(
        EventType::AssistantMessageCompleted,
        "Codex task completed",
        Some(CompletionReason::Normal),
        None,
    );

    let message = decide(&event).unwrap();

    assert_eq!(message.title, "任务完成");
    assert!(message.body.contains("任务已完成"));
    assert!(message.body.contains("项目：demo"));
    assert!(message.body.contains("工具：Codex"));
    assert!(message.body.contains("类型：任务完成"));
    assert!(message.body.contains("内容：任务已完成"));
    assert!(!message.body.contains("Session"));
    assert!(!message.body.contains("session-notify"));
}

#[test]
fn notification_policy_sends_unknown_abort_completion() {
    let event = sample_event(
        EventType::AssistantMessageCompleted,
        "Codex task completed",
        Some(CompletionReason::AbortedUnknown),
        None,
    );

    let message = decide(&event).unwrap();

    assert_eq!(message.title, "任务完成");
    assert!(message.body.contains("任务已完成"));
}

#[test]
fn notification_policy_prefers_attention_content_over_summary() {
    let mut event = sample_event(EventType::InputRequested, "Codex 等待输入", None, None);
    event.content = Some("请输入部署环境名称".to_string());

    let message = decide(&event).unwrap();

    assert_eq!(message.title, "需要输入");
    assert!(message.body.contains("请输入部署环境名称"));
    assert!(!message.body.contains("Codex 等待输入"));
}

#[test]
fn notification_policy_renders_failure_reason() {
    let event = sample_event(
        EventType::TaskFailed,
        "Codex task failed",
        None,
        Some(FailureReason::ContextWindowExceeded),
    );

    let message = decide(&event).unwrap();

    assert_eq!(message.title, "任务失败");
    assert!(message.body.contains("上下文超过限制"));
    assert!(message.body.contains("类型：任务失败"));
    assert!(message.body.contains("内容：上下文超过限制"));
}

#[test]
fn notification_policy_localizes_push_message_labels() {
    let event = sample_event(
        EventType::TaskFailed,
        "Codex task failed",
        None,
        Some(FailureReason::ContextWindowExceeded),
    );
    let cases = [
        (
            SystemLanguage::ZhTw,
            "任務失敗",
            "專案：demo",
            "類型：任務失敗",
            "上下文超過限制",
        ),
        (
            SystemLanguage::En,
            "Task failed",
            "Project: demo",
            "Type: Task failed",
            "Context limit exceeded",
        ),
        (
            SystemLanguage::Ja,
            "タスク失敗",
            "プロジェクト：demo",
            "種類：タスク失敗",
            "コンテキスト上限を超過",
        ),
        (
            SystemLanguage::Ko,
            "작업 실패",
            "프로젝트：demo",
            "유형：작업 실패",
            "컨텍스트 한도 초과",
        ),
        (
            SystemLanguage::De,
            "Aufgabe fehlgeschlagen",
            "Projekt: demo",
            "Typ: Aufgabe fehlgeschlagen",
            "Kontextlimit überschritten",
        ),
    ];

    for (language, title, project_line, event_type_line, failure_label) in cases {
        let message = notification_decision_for_language(&event, language).unwrap();

        assert_eq!(message.title, title);
        assert!(message.body.contains(project_line));
        assert!(message.body.contains(event_type_line));
        assert!(message.body.contains(failure_label));
    }
}

#[test]
fn notification_policy_uses_raw_high_demand_error_without_reason_prefix() {
    let phrase = "We're currently experiencing high demand, which may cause temporary errors.";
    let event = sample_event(
        EventType::TaskFailed,
        phrase,
        None,
        Some(FailureReason::ServerOverloaded),
    );

    let message = decide(&event).unwrap();

    assert_eq!(message.title, "任务失败");
    assert!(message.body.contains(phrase));
    assert!(!message.body.contains("服务繁忙"));
}

#[test]
fn notification_policy_uses_raw_fatal_error_without_reason_prefix() {
    let error = "unexpected status 404 Not Found: 404 page not found";
    let event = sample_event(
        EventType::TaskFailed,
        error,
        None,
        Some(FailureReason::Fatal),
    );

    let message = decide(&event).unwrap();

    assert_eq!(message.title, "任务失败");
    assert!(message.body.contains(error));
    assert!(!message.body.contains("严重错误"));
}

#[test]
fn builds_bark_post_request() {
    let request = build_bark_request(
        &BarkConfig {
            server: "https://api.day.app".to_string(),
            device_key: "abc123".to_string(),
            group: "NiumaNotifier".to_string(),
            icon_url: "https://example.test/icon.png".to_string(),
        },
        &NotificationMessage {
            title: "Codex 需要批准".to_string(),
            body: "Bash: cargo test".to_string(),
            project_name: Some("demo".to_string()),
            action_url: Some("http://127.0.0.1:27873/approval".to_string()),
        },
    )
    .unwrap();

    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "https://api.day.app/push");
    assert_eq!(
        request.headers.get("Content-Type").unwrap(),
        "application/json; charset=utf-8"
    );
    assert!(request.body.contains(r#""title":"Codex 需要批准""#));
    assert!(request.body.contains(r#""device_key":"abc123""#));
    assert!(request.body.contains(r#""group":"demo""#));
    assert!(request
        .body
        .contains(r#""url":"http://127.0.0.1:27873/approval""#));
}

#[test]
fn builds_ntfy_post_request() {
    let request = build_ntfy_request(
        &NtfyConfig {
            server: "https://ntfy.sh".to_string(),
            topic: "niuma".to_string(),
            token: Some("secret".to_string()),
        },
        &NotificationMessage {
            title: "任务完成".to_string(),
            body: "Codex 有新回复".to_string(),
            project_name: None,
            action_url: Some("http://127.0.0.1:27873/status".to_string()),
        },
    )
    .unwrap();

    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "https://ntfy.sh/niuma");
    assert_eq!(request.body, "Codex 有新回复");
    assert_eq!(
        request.headers.get("Title").unwrap(),
        "=?UTF-8?B?5Lu75Yqh5a6M5oiQ?="
    );
    assert_eq!(request.headers.get("Tags").unwrap(), "computer");
    assert_eq!(
        request.headers.get("Authorization").unwrap(),
        "Bearer secret"
    );
    assert_eq!(
        request.headers.get("Click").unwrap(),
        "http://127.0.0.1:27873/status"
    );
}

#[test]
fn ntfy_title_header_encodes_non_ascii_for_http_clients() {
    let request = build_ntfy_request(
        &NtfyConfig {
            server: "https://ntfy.sh".to_string(),
            topic: "niuma".to_string(),
            token: None,
        },
        &NotificationMessage {
            title: "NiumaNotifier 测试通知".to_string(),
            body: "hello".to_string(),
            project_name: None,
            action_url: None,
        },
    )
    .unwrap();

    assert_eq!(
        request.headers.get("Title").unwrap(),
        "=?UTF-8?B?Tml1bWFOb3RpZmllciDmtYvor5XpgJrnn6U=?="
    );
    assert!(request.headers.get("Title").unwrap().is_ascii());
}

fn sample_event(
    event_type: EventType,
    summary: &str,
    completion_reason: Option<CompletionReason>,
    failure_reason: Option<FailureReason>,
) -> NiumaEvent {
    NiumaEvent {
        id: "event-notify".to_string(),
        dedupe_key: "dedupe-notify".to_string(),
        source: "test".to_string(),
        tool: ToolKind::Codex,
        session_id: "session-notify".to_string(),
        project_path: "/tmp/demo".to_string(),
        project_name: "demo".to_string(),
        event_type: event_type.clone(),
        severity: "info".to_string(),
        summary: summary.to_string(),
        content: Some(summary.to_string()),
        error_message: (event_type == EventType::TaskFailed).then(|| summary.to_string()),
        attention_resolve_key: None,
        payload_ref: None,
        completion_reason,
        failure_reason,
        created_at: Utc.timestamp_opt(1_000, 0).single().unwrap(),
    }
}

fn decide(event: &NiumaEvent) -> Option<NotificationMessage> {
    notification_decision_for_language(event, SystemLanguage::ZhCn)
}
