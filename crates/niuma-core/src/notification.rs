use std::collections::BTreeMap;

use crate::event_display::detail_from_event;
use crate::models::{CompletionReason, EventType, FailureReason, NiumaEvent, ToolKind};
use crate::platform::locale::{active_language, SystemLanguage};
use serde_json::json;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationMessage {
    pub title: String,
    pub body: String,
    pub project_name: Option<String>,
    pub action_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BarkConfig {
    pub server: String,
    pub device_key: String,
    pub group: String,
    pub icon_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NtfyConfig {
    pub server: String,
    pub topic: String,
    pub token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub fn notification_decision(event: &NiumaEvent) -> Option<NotificationMessage> {
    notification_decision_for_language(event, active_language())
}

pub fn notification_decision_for_language(
    event: &NiumaEvent,
    language: SystemLanguage,
) -> Option<NotificationMessage> {
    let text = NotificationText::new(language);
    let detail = detail_from_event(event);
    match event.event_type {
        EventType::ApprovalRequested => Some(message_for_event(
            event,
            text.approval_required,
            notification_body(&detail),
            text,
        )),
        EventType::InputRequested => Some(message_for_event(
            event,
            text.input_required,
            notification_body(&detail),
            text,
        )),
        EventType::TaskFailed => Some(message_for_event(
            event,
            text.task_failed,
            &failure_notification_body(event.failure_reason.as_ref(), &detail, text),
            text,
        )),
        EventType::AssistantMessageCompleted => {
            match event.completion_reason.as_ref() {
                Some(CompletionReason::Interrupted) | Some(CompletionReason::RolledBack) => {
                    return None;
                }
                Some(CompletionReason::Normal) | Some(CompletionReason::AbortedUnknown) => {}
                None => return None,
            }
            let body = completion_body_for_notification(notification_body(&detail), text);
            Some(message_for_event(event, text.task_completed, body, text))
        }
        _ => None,
    }
}

pub fn test_notification_message(channel: &str) -> NotificationMessage {
    test_notification_message_for_language(channel, active_language())
}

pub fn test_notification_message_for_language(
    channel: &str,
    language: SystemLanguage,
) -> NotificationMessage {
    let text = NotificationText::new(language);
    NotificationMessage {
        title: text.test_title.to_string(),
        body: text.test_body(channel),
        project_name: Some("NiumaNotifier".to_string()),
        action_url: None,
    }
}

pub fn build_bark_request(
    config: &BarkConfig,
    message: &NotificationMessage,
) -> Result<OutboundRequest, String> {
    let server = trim_slash(&config.server);
    let device_key = config.device_key.trim();
    if server.is_empty() || device_key.is_empty() {
        return Err(bark_config_required_message(active_language()).to_string());
    }

    let group = message
        .project_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| config.group.trim());
    let url = format!("{server}/push");

    let mut payload = json!({
        "title": message.title,
        "body": message.body,
        "device_key": device_key,
    });
    if !group.is_empty() {
        payload["group"] = json!(group);
    }
    if !config.icon_url.trim().is_empty() {
        payload["icon"] = json!(config.icon_url.trim());
    }
    if let Some(action_url) = non_empty(message.action_url.as_deref()) {
        payload["url"] = json!(action_url);
    }

    let mut headers = BTreeMap::new();
    headers.insert(
        "Content-Type".to_string(),
        "application/json; charset=utf-8".to_string(),
    );
    Ok(OutboundRequest {
        method: "POST".to_string(),
        url,
        headers,
        body: serde_json::to_string(&payload).map_err(|error| error.to_string())?,
    })
}

pub fn build_ntfy_request(
    config: &NtfyConfig,
    message: &NotificationMessage,
) -> Result<OutboundRequest, String> {
    let server = trim_slash(&config.server);
    let topic = config.topic.trim();
    if server.is_empty() || topic.is_empty() || topic.contains('/') {
        return Err(ntfy_config_invalid_message(active_language()).to_string());
    }

    let mut headers = BTreeMap::new();
    headers.insert(
        "Title".to_string(),
        encode_ntfy_header_value(&message.title),
    );
    headers.insert("Tags".to_string(), "computer".to_string());
    headers.insert("X-Forwarded-By".to_string(), "NiumaNotifier".to_string());
    if let Some(token) = non_empty(config.token.as_deref()) {
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
    }
    if let Some(action_url) = non_empty(message.action_url.as_deref()) {
        // ntfy 支持 Click/Actions，MVP-0 先保留入口字段，远程审批后续再接。
        headers.insert("Click".to_string(), action_url.to_string());
        headers.insert(
            "Actions".to_string(),
            format!("view, Open, {action_url}, clear=true"),
        );
    }

    Ok(OutboundRequest {
        method: "POST".to_string(),
        url: format!("{server}/{topic}"),
        headers,
        body: message.body.clone(),
    })
}

fn trim_slash(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn encode_ntfy_header_value(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }
    format!("=?UTF-8?B?{}?=", base64_encode(value.as_bytes()))
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(third & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

fn message_for_event(
    event: &NiumaEvent,
    title: &str,
    body: &str,
    text: NotificationText,
) -> NotificationMessage {
    NotificationMessage {
        title: title.to_string(),
        body: detailed_notification_body(event, body, text),
        project_name: Some(event.project_name.clone()),
        action_url: None,
    }
}

fn notification_body(detail: &crate::event_display::EventDisplayDetail) -> &str {
    detail.content.as_deref().unwrap_or(&detail.summary)
}

fn failure_notification_body(
    reason: Option<&FailureReason>,
    detail: &crate::event_display::EventDisplayDetail,
    text: NotificationText,
) -> String {
    let error = detail.error_message.as_deref().unwrap_or(&detail.summary);
    if matches!(
        reason,
        Some(FailureReason::ServerOverloaded | FailureReason::Fatal)
    ) && !error.trim().is_empty()
    {
        return error.to_string();
    }
    format!("{}\n{}", failure_reason_label(reason, text.language), error)
}

fn detailed_notification_body(event: &NiumaEvent, body: &str, text: NotificationText) -> String {
    let separator = text.label_separator();
    // 手机推送正文只保留用户可读信息，避免暴露底层 session 标识。
    format!(
        "{}{}{}\n{}{}{}\n{}{}{}\n{}{}{}",
        text.project,
        separator,
        event.project_name,
        text.tool,
        separator,
        tool_label(&event.tool),
        text.event_type,
        separator,
        event_type_label(&event.event_type, text.language),
        text.content,
        separator,
        body.trim()
    )
}

fn tool_label(tool: &ToolKind) -> &'static str {
    match tool {
        ToolKind::Codex => "Codex",
        ToolKind::ClaudeCode => "Claude Code",
    }
}

fn event_type_label(event_type: &EventType, language: SystemLanguage) -> &'static str {
    let text = NotificationText::new(language);
    match event_type {
        EventType::ApprovalRequested => text.approval_required,
        EventType::InputRequested => text.input_required,
        EventType::TaskFailed => text.task_failed,
        EventType::AssistantMessageCompleted => text.task_completed,
        EventType::SessionStarted => text.session_started,
        EventType::SessionIdled => text.session_idled,
        EventType::ManualDismissed => text.manual_dismissed,
        EventType::SessionStaled => text.session_staled,
        EventType::SessionActivity => text.session_activity,
    }
}

fn completion_body_for_notification(body: &str, text: NotificationText) -> &str {
    if has_real_completion_body(body) {
        body
    } else {
        text.task_completed_body
    }
}

fn has_real_completion_body(body: &str) -> bool {
    let trimmed = body.trim();
    !trimmed.is_empty()
        && !matches!(
            trimmed,
            "Codex task completed" | "Codex 有新回复" | "Claude Code 有新回复"
        )
}

fn failure_reason_label(reason: Option<&FailureReason>, language: SystemLanguage) -> &'static str {
    match reason {
        Some(FailureReason::ContextWindowExceeded) => match language {
            SystemLanguage::ZhCn => "上下文超过限制",
            SystemLanguage::ZhTw => "上下文超過限制",
            SystemLanguage::En => "Context limit exceeded",
            SystemLanguage::Ja => "コンテキスト上限を超過",
            SystemLanguage::Ko => "컨텍스트 한도 초과",
            SystemLanguage::De => "Kontextlimit überschritten",
        },
        Some(FailureReason::QuotaExceeded) => match language {
            SystemLanguage::ZhCn => "额度不足或已耗尽",
            SystemLanguage::ZhTw => "額度不足或已用盡",
            SystemLanguage::En => "Quota is low or exhausted",
            SystemLanguage::Ja => "クォータ不足または使い切りました",
            SystemLanguage::Ko => "할당량 부족 또는 소진",
            SystemLanguage::De => "Kontingent niedrig oder aufgebraucht",
        },
        Some(FailureReason::UsageLimitReached) => match language {
            SystemLanguage::ZhCn => "达到使用限制",
            SystemLanguage::ZhTw => "已達使用限制",
            SystemLanguage::En => "Usage limit reached",
            SystemLanguage::Ja => "使用上限に達しました",
            SystemLanguage::Ko => "사용 한도 도달",
            SystemLanguage::De => "Nutzungslimit erreicht",
        },
        Some(FailureReason::ConnectionFailed) => match language {
            SystemLanguage::ZhCn => "网络连接失败",
            SystemLanguage::ZhTw => "網路連線失敗",
            SystemLanguage::En => "Network connection failed",
            SystemLanguage::Ja => "ネットワーク接続に失敗",
            SystemLanguage::Ko => "네트워크 연결 실패",
            SystemLanguage::De => "Netzwerkverbindung fehlgeschlagen",
        },
        Some(FailureReason::Timeout) => match language {
            SystemLanguage::ZhCn => "请求超时",
            SystemLanguage::ZhTw => "請求逾時",
            SystemLanguage::En => "Request timed out",
            SystemLanguage::Ja => "リクエストがタイムアウトしました",
            SystemLanguage::Ko => "요청 시간 초과",
            SystemLanguage::De => "Anfrage abgelaufen",
        },
        Some(FailureReason::SandboxError) => match language {
            SystemLanguage::ZhCn => "沙箱或权限执行失败",
            SystemLanguage::ZhTw => "沙箱或權限執行失敗",
            SystemLanguage::En => "Sandbox or permission execution failed",
            SystemLanguage::Ja => "サンドボックスまたは権限の実行に失敗",
            SystemLanguage::Ko => "샌드박스 또는 권한 실행 실패",
            SystemLanguage::De => "Sandbox- oder Berechtigungsausführung fehlgeschlagen",
        },
        Some(FailureReason::PolicyBlocked) => match language {
            SystemLanguage::ZhCn => "策略限制",
            SystemLanguage::ZhTw => "策略限制",
            SystemLanguage::En => "Blocked by policy",
            SystemLanguage::Ja => "ポリシーによりブロック",
            SystemLanguage::Ko => "정책에 의해 차단됨",
            SystemLanguage::De => "Durch Richtlinie blockiert",
        },
        Some(FailureReason::ServerOverloaded) => match language {
            SystemLanguage::ZhCn => "服务繁忙",
            SystemLanguage::ZhTw => "服務繁忙",
            SystemLanguage::En => "Service is busy",
            SystemLanguage::Ja => "サービスが混雑しています",
            SystemLanguage::Ko => "서비스가 혼잡함",
            SystemLanguage::De => "Dienst ist ausgelastet",
        },
        Some(FailureReason::ResponseStreamFailed) => match language {
            SystemLanguage::ZhCn => "响应流失败",
            SystemLanguage::ZhTw => "回應串流失敗",
            SystemLanguage::En => "Response stream failed",
            SystemLanguage::Ja => "レスポンスストリームに失敗",
            SystemLanguage::Ko => "응답 스트림 실패",
            SystemLanguage::De => "Antwortstream fehlgeschlagen",
        },
        Some(FailureReason::InternalServerError) => match language {
            SystemLanguage::ZhCn => "服务内部错误",
            SystemLanguage::ZhTw => "服務內部錯誤",
            SystemLanguage::En => "Internal server error",
            SystemLanguage::Ja => "サーバー内部エラー",
            SystemLanguage::Ko => "서버 내부 오류",
            SystemLanguage::De => "Interner Serverfehler",
        },
        Some(FailureReason::RetryLimit) => match language {
            SystemLanguage::ZhCn => "重试次数达到上限",
            SystemLanguage::ZhTw => "重試次數達到上限",
            SystemLanguage::En => "Retry limit reached",
            SystemLanguage::Ja => "再試行回数の上限に達しました",
            SystemLanguage::Ko => "재시도 한도 도달",
            SystemLanguage::De => "Wiederholungslimit erreicht",
        },
        Some(FailureReason::Fatal) => match language {
            SystemLanguage::ZhCn => "严重错误",
            SystemLanguage::ZhTw => "嚴重錯誤",
            SystemLanguage::En => "Fatal error",
            SystemLanguage::Ja => "重大なエラー",
            SystemLanguage::Ko => "치명적 오류",
            SystemLanguage::De => "Schwerer Fehler",
        },
        Some(FailureReason::Unknown) | None => match language {
            SystemLanguage::ZhCn => "未知失败",
            SystemLanguage::ZhTw => "未知失敗",
            SystemLanguage::En => "Unknown failure",
            SystemLanguage::Ja => "不明な失敗",
            SystemLanguage::Ko => "알 수 없는 실패",
            SystemLanguage::De => "Unbekannter Fehler",
        },
    }
}

fn bark_config_required_message(language: SystemLanguage) -> &'static str {
    match language {
        SystemLanguage::ZhCn => "Bark server 和 device key 不能为空",
        SystemLanguage::ZhTw => "Bark server 和 device key 不能為空",
        SystemLanguage::En => "Bark server and device key cannot be empty",
        SystemLanguage::Ja => "Bark server と device key は空にできません",
        SystemLanguage::Ko => "Bark server와 device key는 비워둘 수 없습니다",
        SystemLanguage::De => "Bark server und device key dürfen nicht leer sein",
    }
}

fn ntfy_config_invalid_message(language: SystemLanguage) -> &'static str {
    match language {
        SystemLanguage::ZhCn => "ntfy server 和 topic 配置无效",
        SystemLanguage::ZhTw => "ntfy server 和 topic 設定無效",
        SystemLanguage::En => "ntfy server and topic configuration is invalid",
        SystemLanguage::Ja => "ntfy server と topic の設定が無効です",
        SystemLanguage::Ko => "ntfy server와 topic 설정이 올바르지 않습니다",
        SystemLanguage::De => "ntfy server und topic sind ungültig konfiguriert",
    }
}

#[derive(Clone, Copy)]
struct NotificationText {
    language: SystemLanguage,
    approval_required: &'static str,
    input_required: &'static str,
    task_failed: &'static str,
    task_completed: &'static str,
    task_completed_body: &'static str,
    session_started: &'static str,
    session_idled: &'static str,
    manual_dismissed: &'static str,
    session_staled: &'static str,
    session_activity: &'static str,
    project: &'static str,
    tool: &'static str,
    event_type: &'static str,
    content: &'static str,
    test_title: &'static str,
}

impl NotificationText {
    fn new(language: SystemLanguage) -> Self {
        // 推送通知在 Rust 运行时生成，需独立于前端 localStorage 的 i18n 表。
        match language {
            SystemLanguage::ZhCn => Self {
                language,
                approval_required: "需要授权",
                input_required: "需要输入",
                task_failed: "任务失败",
                task_completed: "任务完成",
                task_completed_body: "任务已完成",
                session_started: "任务开始",
                session_idled: "任务空闲",
                manual_dismissed: "已标记处理",
                session_staled: "会话过期",
                session_activity: "会话活动",
                project: "项目",
                tool: "工具",
                event_type: "类型",
                content: "内容",
                test_title: "NiumaNotifier 测试通知",
            },
            SystemLanguage::ZhTw => Self {
                language,
                approval_required: "需要授權",
                input_required: "需要輸入",
                task_failed: "任務失敗",
                task_completed: "任務完成",
                task_completed_body: "任務已完成",
                session_started: "任務開始",
                session_idled: "任務閒置",
                manual_dismissed: "已標記處理",
                session_staled: "會話過期",
                session_activity: "會話活動",
                project: "專案",
                tool: "工具",
                event_type: "類型",
                content: "內容",
                test_title: "NiumaNotifier 測試通知",
            },
            SystemLanguage::En => Self {
                language,
                approval_required: "Approval required",
                input_required: "Input required",
                task_failed: "Task failed",
                task_completed: "Task completed",
                task_completed_body: "Task completed",
                session_started: "Task started",
                session_idled: "Task idle",
                manual_dismissed: "Marked handled",
                session_staled: "Session stale",
                session_activity: "Session activity",
                project: "Project",
                tool: "Tool",
                event_type: "Type",
                content: "Content",
                test_title: "NiumaNotifier test notification",
            },
            SystemLanguage::Ja => Self {
                language,
                approval_required: "承認が必要",
                input_required: "入力が必要",
                task_failed: "タスク失敗",
                task_completed: "タスク完了",
                task_completed_body: "タスクが完了しました",
                session_started: "タスク開始",
                session_idled: "タスク待機中",
                manual_dismissed: "対応済みにしました",
                session_staled: "セッション期限切れ",
                session_activity: "セッション活動",
                project: "プロジェクト",
                tool: "ツール",
                event_type: "種類",
                content: "内容",
                test_title: "NiumaNotifier テスト通知",
            },
            SystemLanguage::Ko => Self {
                language,
                approval_required: "승인 필요",
                input_required: "입력 필요",
                task_failed: "작업 실패",
                task_completed: "작업 완료",
                task_completed_body: "작업이 완료되었습니다",
                session_started: "작업 시작",
                session_idled: "작업 유휴",
                manual_dismissed: "처리됨으로 표시",
                session_staled: "세션 만료",
                session_activity: "세션 활동",
                project: "프로젝트",
                tool: "도구",
                event_type: "유형",
                content: "내용",
                test_title: "NiumaNotifier 테스트 알림",
            },
            SystemLanguage::De => Self {
                language,
                approval_required: "Genehmigung erforderlich",
                input_required: "Eingabe erforderlich",
                task_failed: "Aufgabe fehlgeschlagen",
                task_completed: "Aufgabe abgeschlossen",
                task_completed_body: "Aufgabe abgeschlossen",
                session_started: "Aufgabe gestartet",
                session_idled: "Aufgabe inaktiv",
                manual_dismissed: "Als erledigt markiert",
                session_staled: "Sitzung abgelaufen",
                session_activity: "Sitzungsaktivität",
                project: "Projekt",
                tool: "Werkzeug",
                event_type: "Typ",
                content: "Inhalt",
                test_title: "NiumaNotifier-Testbenachrichtigung",
            },
        }
    }

    fn label_separator(self) -> &'static str {
        match self.language {
            SystemLanguage::ZhCn
            | SystemLanguage::ZhTw
            | SystemLanguage::Ja
            | SystemLanguage::Ko => "：",
            SystemLanguage::En | SystemLanguage::De => ": ",
        }
    }

    fn test_body(self, channel: &str) -> String {
        match self.language {
            SystemLanguage::ZhCn => format!("测试通知已发送到 {channel}"),
            SystemLanguage::ZhTw => format!("測試通知已傳送到 {channel}"),
            SystemLanguage::En => format!("Test notification sent to {channel}"),
            SystemLanguage::Ja => format!("テスト通知を {channel} に送信しました"),
            SystemLanguage::Ko => format!("테스트 알림을 {channel}에 보냈습니다"),
            SystemLanguage::De => format!("Testbenachrichtigung an {channel} gesendet"),
        }
    }
}

#[cfg(test)]
mod tests;
