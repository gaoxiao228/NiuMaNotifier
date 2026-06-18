use niuma_core::notification::OutboundRequest;
use niuma_core::platform::locale::{active_language, SystemLanguage};
use std::io::Read;
use std::time::Duration;

const NOTIFICATION_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const ERROR_BODY_LIMIT_BYTES: u64 = 1024;

/// 发送器抽象隔离真实 HTTP 调用，测试发送逻辑可用 mock 覆盖且不依赖外网。
pub trait NotificationSender: Send + Sync + 'static {
    fn send(&self, request: &OutboundRequest) -> Result<(), String>;
}

#[derive(Clone)]
pub struct UreqNotificationSender {
    agent: ureq::Agent,
}

impl Default for UreqNotificationSender {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(NOTIFICATION_HTTP_TIMEOUT)
                .build(),
        }
    }
}

impl NotificationSender for UreqNotificationSender {
    fn send(&self, request: &OutboundRequest) -> Result<(), String> {
        let method = request.method.to_uppercase();
        let mut call = match method.as_str() {
            "POST" => self.agent.post(&request.url),
            _ => return Err(unsupported_method_message(&method, active_language())),
        };
        for (key, value) in &request.headers {
            call = call.set(key, value);
        }

        let response = match call.send_string(&request.body) {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) => {
                return Err(format_http_status_error(status, &request.url, response));
            }
            Err(error) => return Err(send_failed_message(&error.to_string(), active_language())),
        };
        let status = response.status();
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(http_status_message(
                status,
                &request.url,
                None,
                active_language(),
            ))
        }
    }
}

pub(crate) fn format_http_status_error(status: u16, url: &str, response: ureq::Response) -> String {
    let body = response_body_excerpt(response);
    if body.is_empty() {
        http_status_message(status, url, None, active_language())
    } else {
        http_status_message(status, url, Some(&body), active_language())
    }
}

pub(crate) fn response_body_excerpt(response: ureq::Response) -> String {
    let mut bytes = Vec::new();
    let read_result = response
        .into_reader()
        .take(ERROR_BODY_LIMIT_BYTES)
        .read_to_end(&mut bytes);
    if read_result.is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes)
        .trim()
        .replace(['\r', '\n'], " ")
}

fn unsupported_method_message(method: &str, language: SystemLanguage) -> String {
    match language {
        SystemLanguage::ZhCn => format!("不支持的通知请求方法：{method}"),
        SystemLanguage::ZhTw => format!("不支援的通知請求方法：{method}"),
        SystemLanguage::En => format!("Unsupported notification request method: {method}"),
        SystemLanguage::Ja => format!("未対応の通知リクエストメソッドです: {method}"),
        SystemLanguage::Ko => format!("지원하지 않는 알림 요청 메서드: {method}"),
        SystemLanguage::De => {
            format!("Nicht unterstützte Benachrichtigungsanfragemethode: {method}")
        }
    }
}

fn send_failed_message(error: &str, language: SystemLanguage) -> String {
    match language {
        SystemLanguage::ZhCn => format!("发送通知失败：{error}"),
        SystemLanguage::ZhTw => format!("傳送通知失敗：{error}"),
        SystemLanguage::En => format!("Failed to send notification: {error}"),
        SystemLanguage::Ja => format!("通知の送信に失敗しました: {error}"),
        SystemLanguage::Ko => format!("알림 전송 실패: {error}"),
        SystemLanguage::De => format!("Benachrichtigung konnte nicht gesendet werden: {error}"),
    }
}

fn http_status_message(
    status: u16,
    url: &str,
    response_body: Option<&str>,
    language: SystemLanguage,
) -> String {
    match (language, response_body) {
        (SystemLanguage::ZhCn, Some(body)) => {
            format!("通知服务返回 HTTP {status}：{url}，响应：{body}")
        }
        (SystemLanguage::ZhCn, None) => format!("通知服务返回 HTTP {status}：{url}"),
        (SystemLanguage::ZhTw, Some(body)) => {
            format!("通知服務返回 HTTP {status}：{url}，回應：{body}")
        }
        (SystemLanguage::ZhTw, None) => format!("通知服務返回 HTTP {status}：{url}"),
        (SystemLanguage::En, Some(body)) => {
            format!("Notification service returned HTTP {status}: {url}; response: {body}")
        }
        (SystemLanguage::En, None) => {
            format!("Notification service returned HTTP {status}: {url}")
        }
        (SystemLanguage::Ja, Some(body)) => {
            format!("通知サービスが HTTP {status} を返しました: {url}、応答: {body}")
        }
        (SystemLanguage::Ja, None) => {
            format!("通知サービスが HTTP {status} を返しました: {url}")
        }
        (SystemLanguage::Ko, Some(body)) => {
            format!("알림 서비스가 HTTP {status}를 반환했습니다: {url}, 응답: {body}")
        }
        (SystemLanguage::Ko, None) => {
            format!("알림 서비스가 HTTP {status}를 반환했습니다: {url}")
        }
        (SystemLanguage::De, Some(body)) => {
            format!("Benachrichtigungsdienst gab HTTP {status} zurück: {url}; Antwort: {body}")
        }
        (SystemLanguage::De, None) => {
            format!("Benachrichtigungsdienst gab HTTP {status} zurück: {url}")
        }
    }
}
