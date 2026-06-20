use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const PARENT_PID_ENV: &str = "NIUMA_PARENT_PID";
const DEFAULT_PLUGIN_ID: &str = "builtin-ntfy";
const DEFAULT_NTFY_SERVER: &str = "https://ntfy.sh";
const DEFAULT_NTFY_TOPIC_PREFIX: &str = "niuma-notifier";
const RECONNECT_INTERVAL: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const PARENT_WATCHDOG_INTERVAL: Duration = Duration::from_secs(2);
const SENT_EVENTS_FILE: &str = "sent-events.jsonl";
const ERROR_BODY_LIMIT_BYTES: u64 = 1024;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
struct NiumaEvent {
    id: String,
    tool: String,
    project_name: String,
    event_type: EventType,
    summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completion_reason: Option<CompletionReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure_reason: Option<FailureReason>,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum EventType {
    SessionStarted,
    SessionIdled,
    ApprovalRequested,
    InputRequested,
    TaskFailed,
    AssistantMessageCompleted,
    ManualDismissed,
    SessionStaled,
    SessionActivity,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CompletionReason {
    Normal,
    Interrupted,
    RolledBack,
    AbortedUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum FailureReason {
    Timeout,
    ContextWindowExceeded,
    UsageLimitReached,
    ServerOverloaded,
    PolicyBlocked,
    ResponseStreamFailed,
    ConnectionFailed,
    QuotaExceeded,
    InternalServerError,
    RetryLimit,
    SandboxError,
    Fatal,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
struct PluginNotificationTestRequest {
    test_id: String,
    plugin_id: String,
    title: String,
    body: String,
    created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NotificationMessage {
    title: String,
    body: String,
    project_name: Option<String>,
    action_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NtfyConfig {
    server: String,
    topic: String,
    token: Option<String>,
}

impl NtfyConfig {
    fn is_incomplete(&self) -> bool {
        let topic = self.topic.trim();
        self.server.trim().is_empty() || topic.is_empty() || topic.contains('/')
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OutboundRequest {
    method: String,
    url: String,
    headers: BTreeMap<String, String>,
    body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeEnv {
    api_url: String,
    plugin_id: String,
    config_path: Option<PathBuf>,
    data_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SseEventFrame {
    event: Option<String>,
    id: Option<String>,
    data_lines: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
struct NtfyPluginConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    server: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    token: String,
}

impl NtfyPluginConfig {
    fn ntfy_config(&self) -> NtfyConfig {
        NtfyConfig {
            server: non_empty_text(&self.server)
                .unwrap_or(DEFAULT_NTFY_SERVER)
                .to_string(),
            topic: non_empty_text(&self.topic)
                .map(ToString::to_string)
                .unwrap_or_else(default_ntfy_topic),
            token: non_empty_text(&self.token).map(ToString::to_string),
        }
    }
}

fn notification_decision(event: &NiumaEvent) -> Option<NotificationMessage> {
    match event.event_type {
        EventType::ApprovalRequested => Some(message_for_event(
            event,
            "需要授权",
            notification_body(event),
        )),
        EventType::InputRequested => Some(message_for_event(
            event,
            "等待输入",
            notification_body(event),
        )),
        EventType::TaskFailed => Some(message_for_event(
            event,
            "任务失败",
            &failure_notification_body(event),
        )),
        EventType::AssistantMessageCompleted => {
            match event.completion_reason.as_ref() {
                Some(CompletionReason::Interrupted) | Some(CompletionReason::RolledBack) => {
                    return None;
                }
                Some(CompletionReason::Normal) | Some(CompletionReason::AbortedUnknown) => {}
                None => return None,
            }
            Some(message_for_event(
                event,
                "任务完成",
                completion_body_for_notification(notification_body(event)),
            ))
        }
        _ => None,
    }
}

fn message_for_event(event: &NiumaEvent, title: &str, body: &str) -> NotificationMessage {
    NotificationMessage {
        title: title.to_string(),
        body: detailed_notification_body(event, body),
        project_name: Some(event.project_name.clone()),
        action_url: None,
    }
}

fn notification_body(event: &NiumaEvent) -> &str {
    event
        .content
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(event.summary.trim())
}

fn failure_notification_body(event: &NiumaEvent) -> String {
    let error = event
        .error_message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(event.summary.trim());
    if matches!(
        event.failure_reason,
        Some(FailureReason::ServerOverloaded | FailureReason::Fatal)
    ) && !error.is_empty()
    {
        return error.to_string();
    }
    format!(
        "{}\n{}",
        failure_reason_label(event.failure_reason.as_ref()),
        error
    )
}

fn detailed_notification_body(event: &NiumaEvent, body: &str) -> String {
    // 手机推送正文只保留用户可读信息，避免暴露底层 session 标识。
    format!(
        "项目：{}\n工具：{}\n事件：{}\n内容：{}",
        event.project_name,
        tool_label(&event.tool),
        event_type_label(&event.event_type),
        body.trim()
    )
}

fn completion_body_for_notification(body: &str) -> &str {
    if has_real_completion_body(body) {
        body
    } else {
        "任务已完成"
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

fn tool_label(tool: &str) -> &str {
    match tool {
        "codex" => "Codex",
        "claude_code" => "Claude Code",
        value => value,
    }
}

fn event_type_label(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::ApprovalRequested => "需要授权",
        EventType::InputRequested => "等待输入",
        EventType::TaskFailed => "任务失败",
        EventType::AssistantMessageCompleted => "任务完成",
        EventType::SessionStarted => "会话开始",
        EventType::SessionIdled => "会话空闲",
        EventType::ManualDismissed => "手动忽略",
        EventType::SessionStaled => "会话过期",
        EventType::SessionActivity => "会话活动",
    }
}

fn failure_reason_label(reason: Option<&FailureReason>) -> &'static str {
    match reason {
        Some(FailureReason::ContextWindowExceeded) => "上下文超过限制",
        Some(FailureReason::QuotaExceeded) => "额度不足或已耗尽",
        Some(FailureReason::UsageLimitReached) => "达到使用限制",
        Some(FailureReason::ConnectionFailed) => "网络连接失败",
        Some(FailureReason::Timeout) => "请求超时",
        Some(FailureReason::SandboxError) => "沙箱或权限执行失败",
        Some(FailureReason::PolicyBlocked) => "策略限制",
        Some(FailureReason::ServerOverloaded) => "服务繁忙",
        Some(FailureReason::ResponseStreamFailed) => "响应流失败",
        Some(FailureReason::InternalServerError) => "服务内部错误",
        Some(FailureReason::RetryLimit) => "重试次数达到上限",
        Some(FailureReason::Fatal) => "严重错误",
        Some(FailureReason::Unknown) | None => "未知失败",
    }
}

fn build_ntfy_request(
    config: &NtfyConfig,
    message: &NotificationMessage,
) -> Result<OutboundRequest, String> {
    let server = config.server.trim().trim_end_matches('/');
    let topic = config.topic.trim();
    if server.is_empty() || topic.is_empty() || topic.contains('/') {
        return Err("ntfy server 和 topic 配置无效".to_string());
    }

    let mut headers = BTreeMap::new();
    headers.insert(
        "Title".to_string(),
        encode_ntfy_header_value(&message.title),
    );
    headers.insert("Tags".to_string(), "computer".to_string());
    headers.insert("X-Forwarded-By".to_string(), "NiumaNotifier".to_string());
    if let Some(token) = config
        .token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
    }
    if let Some(action_url) = message
        .action_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum StreamItem {
    Event(NiumaEvent),
    NotificationTest(PluginNotificationTestRequest),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SentEventRecord {
    key: String,
    sent_at: String,
}

pub fn run_from_env() {
    start_parent_watchdog_from_env();
    let env = match RuntimeEnv::from_env() {
        Ok(env) => env,
        Err(error) => {
            eprintln!("NiumaNotifier ntfy plugin process not started: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = prepare_runtime_files(&env) {
        eprintln!("NiumaNotifier ntfy plugin runtime file setup failed: {error}");
    }
    run_event_stream_loop(env);
}

impl RuntimeEnv {
    fn from_env() -> Result<Self, String> {
        let api_url = std::env::var("NIUMA_LOCAL_API_URL")
            .map_err(|_| "NIUMA_LOCAL_API_URL 未设置".to_string())?;
        let plugin_id =
            std::env::var("NIUMA_PLUGIN_ID").unwrap_or_else(|_| DEFAULT_PLUGIN_ID.to_string());
        let config_path = non_empty_env_path("NIUMA_PLUGIN_CONFIG_PATH");
        let data_dir = non_empty_env_path("NIUMA_PLUGIN_DATA_DIR");
        Ok(Self {
            api_url,
            plugin_id,
            config_path,
            data_dir,
        })
    }
}

fn non_empty_env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn prepare_runtime_files(env: &RuntimeEnv) -> Result<(), String> {
    if let Some(data_dir) = &env.data_dir {
        // 插件数据目录只保存 ntfy 自己的去重记录，不写主程序持久化文件。
        std::fs::create_dir_all(data_dir)
            .map_err(|error| format!("创建 ntfy 插件数据目录失败：{error}"))?;
    }
    if let Some(config_path) = &env.config_path {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建 ntfy 插件配置目录失败：{error}"))?;
        }
        if config_path.exists() {
            let _ = load_config(config_path)?;
        }
    }
    Ok(())
}

fn load_config(path: &std::path::Path) -> Result<NtfyPluginConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 ntfy 插件配置失败：{error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析 ntfy 插件配置失败：{error}"))
}

fn run_event_stream_loop(env: RuntimeEnv) {
    let sender = UreqNtfySender::default();
    let reporter = UreqNotificationResultReporter::default();
    loop {
        match consume_events_once(&env, &sender, &reporter) {
            Ok(()) => {}
            Err(error) => {
                eprintln!("NiumaNotifier ntfy plugin event stream disconnected: {error}");
            }
        }
        thread::sleep(RECONNECT_INTERVAL);
    }
}

fn consume_events_once<S: NtfySender, R: NotificationResultReporter>(
    env: &RuntimeEnv,
    sender: &S,
    reporter: &R,
) -> Result<(), String> {
    let url = format!("{}/api/v1/events/stream", env.api_url.trim_end_matches('/'));
    let response = ureq::get(&url)
        .set("Accept", "text/event-stream")
        .timeout(REQUEST_TIMEOUT)
        .call()
        .map_err(|error| format!("连接事件流失败：{error}"))?;
    consume_event_lines(response.into_reader(), |item| match item {
        StreamItem::Event(event) => handle_event(env, sender, reporter, event),
        StreamItem::NotificationTest(request) => handle_test_event(env, sender, reporter, request),
    })
}

fn consume_event_lines<R, F>(reader: R, mut on_event: F) -> Result<(), String>
where
    R: std::io::Read,
    F: FnMut(StreamItem) -> Result<(), String>,
{
    let mut frame = SseEventFrame::default();
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|error| format!("读取事件流失败：{error}"))?;
        if line.is_empty() {
            dispatch_sse_frame(&mut frame, &mut on_event)?;
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line.as_str(), ""),
        };
        match field {
            "event" => frame.event = Some(value.to_string()),
            "id" => frame.id = Some(value.to_string()),
            "data" => frame.data_lines.push(value.to_string()),
            _ => {}
        }
    }
    dispatch_sse_frame(&mut frame, &mut on_event)
}

fn dispatch_sse_frame<F>(frame: &mut SseEventFrame, on_event: &mut F) -> Result<(), String>
where
    F: FnMut(StreamItem) -> Result<(), String>,
{
    if !frame.data_lines.is_empty() {
        let event_name = frame.event.clone();
        let data = frame.data_lines.join("\n");
        match event_name.as_deref() {
            Some("event") => {
                let event = serde_json::from_str::<NiumaEvent>(&data)
                    .map_err(|error| format!("解析事件流数据失败：{error}"))?;
                on_event(StreamItem::Event(event))?;
            }
            Some("notification_test") => {
                let request = serde_json::from_str::<PluginNotificationTestRequest>(&data)
                    .map_err(|error| format!("解析测试通知数据失败：{error}"))?;
                on_event(StreamItem::NotificationTest(request))?;
            }
            _ => {}
        }
    }
    *frame = SseEventFrame::default();
    Ok(())
}

fn handle_event<S: NtfySender, R: NotificationResultReporter>(
    env: &RuntimeEnv,
    sender: &S,
    reporter: &R,
    event: NiumaEvent,
) -> Result<(), String> {
    let Some(message) = notification_decision(&event) else {
        return Ok(());
    };
    let Some(config_path) = &env.config_path else {
        return Ok(());
    };
    let config = load_config(config_path)?;
    if !config.enabled {
        return Ok(());
    }
    if config.ntfy_config().is_incomplete() {
        return Ok(());
    }

    let dedupe_key = plugin_event_dedupe_key(&env.plugin_id, &event.id);
    let mut sent_events = SentEventStore::from_env(env)?;
    if sent_events.contains(&dedupe_key) {
        return Ok(());
    }

    let request = build_ntfy_request(&config.ntfy_config(), &message)?;
    match sender.send(&request) {
        Ok(()) => {
            let sent_at = Utc::now();
            if let Err(error) = reporter.report(env, &event, "sent", &message, None, Some(sent_at))
            {
                eprintln!("NiumaNotifier ntfy notification result report failed: {error}");
            }
        }
        Err(error) => {
            if let Err(report_error) =
                reporter.report(env, &event, "failed", &message, Some(&error), None)
            {
                eprintln!(
                    "NiumaNotifier ntfy notification failed result report failed: {report_error}"
                );
            }
            return Err(error);
        }
    }
    sent_events.record_sent(&dedupe_key)?;
    Ok(())
}

fn handle_test_event<S: NtfySender, R: NotificationResultReporter>(
    env: &RuntimeEnv,
    sender: &S,
    reporter: &R,
    test: PluginNotificationTestRequest,
) -> Result<(), String> {
    if test.plugin_id != env.plugin_id {
        return Ok(());
    }
    let message = NotificationMessage {
        title: test.title.clone(),
        body: test.body.clone(),
        project_name: None,
        action_url: None,
    };
    let Some(config_path) = &env.config_path else {
        reporter.report_test(
            env,
            &test,
            "failed",
            &message,
            Some("插件配置文件未设置"),
            None,
        )?;
        return Ok(());
    };
    let config = load_config(config_path)?;
    if !config.enabled {
        reporter.report_test(env, &test, "failed", &message, Some("通知插件未启用"), None)?;
        return Ok(());
    }
    if config.ntfy_config().is_incomplete() {
        return Ok(());
    }

    let request = build_ntfy_request(&config.ntfy_config(), &message)?;
    match sender.send(&request) {
        Ok(()) => reporter.report_test(env, &test, "sent", &message, None, Some(Utc::now())),
        Err(error) => {
            reporter.report_test(env, &test, "failed", &message, Some(&error), None)?;
            Err(error)
        }
    }
}

trait NtfySender {
    fn send(&self, request: &OutboundRequest) -> Result<(), String>;
}

trait NotificationResultReporter {
    fn report(
        &self,
        env: &RuntimeEnv,
        event: &NiumaEvent,
        status: &str,
        message: &NotificationMessage,
        error_message: Option<&str>,
        sent_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), String>;

    fn report_test(
        &self,
        env: &RuntimeEnv,
        test: &PluginNotificationTestRequest,
        status: &str,
        message: &NotificationMessage,
        error_message: Option<&str>,
        sent_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), String>;
}

#[derive(Clone)]
struct UreqNotificationResultReporter {
    agent: ureq::Agent,
}

impl Default for UreqNotificationResultReporter {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build(),
        }
    }
}

impl NotificationResultReporter for UreqNotificationResultReporter {
    fn report(
        &self,
        env: &RuntimeEnv,
        event: &NiumaEvent,
        status: &str,
        message: &NotificationMessage,
        error_message: Option<&str>,
        sent_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), String> {
        let url = format!(
            "{}/api/v1/plugins/notification-results",
            env.api_url.trim_end_matches('/')
        );
        let mut payload = serde_json::json!({
            "plugin_id": env.plugin_id,
            "event_id": event.id,
            "status": status,
            "title": message.title,
            "body": message.body,
            "reason": notification_reason(event),
            "error_message": error_message,
        });
        if let Some(sent_at) = sent_at {
            payload["sent_at"] = serde_json::json!(sent_at.to_rfc3339());
        }
        let response = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json; charset=utf-8")
            .send_string(&payload.to_string())
            .map_err(|error| format!("回写通知结果失败：{error}"))?;
        ensure_api_success(response)
    }

    fn report_test(
        &self,
        env: &RuntimeEnv,
        test: &PluginNotificationTestRequest,
        status: &str,
        message: &NotificationMessage,
        error_message: Option<&str>,
        sent_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), String> {
        let url = format!(
            "{}/api/v1/plugins/notification-test-results",
            env.api_url.trim_end_matches('/')
        );
        let mut payload = serde_json::json!({
            "plugin_id": env.plugin_id,
            "test_id": test.test_id,
            "status": status,
            "title": message.title,
            "body": message.body,
            "error_message": error_message,
        });
        if let Some(sent_at) = sent_at {
            payload["sent_at"] = serde_json::json!(sent_at.to_rfc3339());
        }
        let response = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json; charset=utf-8")
            .send_string(&payload.to_string())
            .map_err(|error| format!("回写测试通知结果失败：{error}"))?;
        ensure_api_success(response)
    }
}

fn ensure_api_success(response: ureq::Response) -> Result<(), String> {
    let body = response
        .into_string()
        .map_err(|error| format!("读取通知结果回写响应失败：{error}"))?;
    let value = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|error| format!("解析通知结果回写响应失败：{error}"))?;
    if value.get("code").and_then(serde_json::Value::as_i64) == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "通知结果回写失败：{}",
            value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("未知错误")
        ))
    }
}

#[derive(Clone)]
struct UreqNtfySender {
    agent: ureq::Agent,
}

impl Default for UreqNtfySender {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build(),
        }
    }
}

impl NtfySender for UreqNtfySender {
    fn send(&self, request: &OutboundRequest) -> Result<(), String> {
        send_outbound_request(&self.agent, request)
    }
}

fn send_outbound_request(agent: &ureq::Agent, request: &OutboundRequest) -> Result<(), String> {
    let method = request.method.to_uppercase();
    let mut call = match method.as_str() {
        "POST" => agent.post(&request.url),
        _ => return Err(format!("不支持的 ntfy 请求方法：{method}")),
    };
    for (key, value) in &request.headers {
        call = call.set(key, value);
    }
    match call.send_string(&request.body) {
        Ok(response) if (200..300).contains(&response.status()) => Ok(()),
        Ok(response) => Err(format_ntfy_status_error(
            response.status(),
            &request.url,
            response,
        )),
        Err(ureq::Error::Status(status, response)) => {
            Err(format_ntfy_status_error(status, &request.url, response))
        }
        Err(error) => Err(format!("发送 ntfy 推送失败：{error}")),
    }
}

fn format_ntfy_status_error(status: u16, url: &str, response: ureq::Response) -> String {
    let body = response_body_excerpt(response);
    if body.is_empty() {
        format!("ntfy 服务返回 HTTP {status}：{url}")
    } else {
        format!("ntfy 服务返回 HTTP {status}：{url}，响应：{body}")
    }
}

fn response_body_excerpt(response: ureq::Response) -> String {
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

struct SentEventStore {
    path: Option<PathBuf>,
    keys: HashSet<String>,
}

impl SentEventStore {
    fn from_env(env: &RuntimeEnv) -> Result<Self, String> {
        let path = env
            .data_dir
            .as_ref()
            .map(|data_dir| data_dir.join(SENT_EVENTS_FILE));
        let keys = match &path {
            Some(path) => load_sent_event_keys(path)?,
            None => HashSet::new(),
        };
        Ok(Self { path, keys })
    }

    fn contains(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    fn record_sent(&mut self, key: &str) -> Result<(), String> {
        if self.keys.contains(key) {
            return Ok(());
        }
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("创建 ntfy 去重目录失败：{error}"))?;
            }
            let record = SentEventRecord {
                key: key.to_string(),
                sent_at: Utc::now().to_rfc3339(),
            };
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|error| format!("打开 ntfy 去重文件失败：{error}"))?;
            writeln!(
                file,
                "{}",
                serde_json::to_string(&record)
                    .map_err(|error| format!("序列化 ntfy 去重记录失败：{error}"))?
            )
            .map_err(|error| format!("写入 ntfy 去重记录失败：{error}"))?;
        }
        self.keys.insert(key.to_string());
        Ok(())
    }
}

fn load_sent_event_keys(path: &std::path::Path) -> Result<HashSet<String>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(format!("读取 ntfy 去重文件失败：{error}")),
    };
    let mut keys = HashSet::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| format!("读取 ntfy 去重记录失败：{error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<SentEventRecord>(&line) {
            Ok(record) => {
                keys.insert(record.key);
            }
            Err(error) => {
                eprintln!("NiumaNotifier ntfy dedupe record ignored: {error}");
            }
        }
    }
    Ok(keys)
}

fn plugin_event_dedupe_key(plugin_id: &str, event_id: &str) -> String {
    format!("{plugin_id}:{event_id}")
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

fn default_ntfy_topic() -> String {
    let seed = std::env::var("NIUMA_DB_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("NIUMA_PLUGIN_DATA_DIR").ok())
        .unwrap_or_else(|| DEFAULT_PLUGIN_ID.to_string());
    format!("{}-{}", DEFAULT_NTFY_TOPIC_PREFIX, stable_hash(&seed))
}

fn stable_hash(value: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:016x}")
}

fn non_empty_text(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn start_parent_watchdog_from_env() {
    let Some(parent_pid) = parse_parent_pid(std::env::var(PARENT_PID_ENV).ok().as_deref()) else {
        return;
    };
    if let Err(error) = thread::Builder::new()
        .name("niuma-parent-watchdog".to_string())
        .spawn(move || run_parent_watchdog(parent_pid))
    {
        eprintln!("NiumaNotifier parent watchdog not started: {error}");
    }
}

fn run_parent_watchdog(parent_pid: u32) {
    loop {
        thread::sleep(PARENT_WATCHDOG_INTERVAL);
        if !parent_process_exists(parent_pid) {
            eprintln!("NiumaNotifier parent process {parent_pid} is gone; plugin exiting");
            std::process::exit(0);
        }
    }
}

fn parse_parent_pid(value: Option<&str>) -> Option<u32> {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0)
}

#[cfg(unix)]
fn parent_process_exists(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error()
        .raw_os_error()
        .is_some_and(|code| code != libc::ESRCH)
}

#[cfg(not(unix))]
fn parent_process_exists(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parses_event_stream_frames() {
        let event = event_with_type("event-1", EventType::ApprovalRequested);
        let payload = serde_json::to_string(&event).unwrap();
        let body = format!("event: event\nid: event-1\ndata: {payload}\n\n");
        let mut received = Vec::new();

        consume_event_lines(Cursor::new(body), |item| {
            received.push(item);
            Ok(())
        })
        .unwrap();

        assert_eq!(received, vec![StreamItem::Event(event)]);
    }

    #[test]
    fn parses_notification_test_stream_frames() {
        let request = PluginNotificationTestRequest {
            test_id: "manual-test:builtin-ntfy:1".to_string(),
            plugin_id: "builtin-ntfy".to_string(),
            title: "NiuMa 测试通知".to_string(),
            body: "测试正文".to_string(),
            created_at: Utc::now(),
        };
        let payload = serde_json::to_string(&request).unwrap();
        let body = format!(
            "event: notification_test\nid: {}\ndata: {payload}\n\n",
            request.test_id
        );
        let mut received = Vec::new();

        consume_event_lines(Cursor::new(body), |item| {
            received.push(item);
            Ok(())
        })
        .unwrap();

        assert_eq!(received, vec![StreamItem::NotificationTest(request)]);
    }

    #[test]
    fn ignores_non_event_frames() {
        let body = "event: state\ndata: {\"version\":1}\n\n";
        let mut received = Vec::new();

        consume_event_lines(Cursor::new(body), |item| {
            received.push(item);
            Ok(())
        })
        .unwrap();

        assert!(received.is_empty());
    }

    #[test]
    fn notification_rule_accepts_completion_normal_event() {
        let mut event = event_with_type("event-completed", EventType::AssistantMessageCompleted);
        event.completion_reason = Some(CompletionReason::Normal);

        assert!(notification_decision(&event).is_some());
    }

    #[test]
    fn notification_rule_skips_running_activity_event() {
        let event = event_with_type("event-running", EventType::SessionActivity);

        assert!(notification_decision(&event).is_none());
    }

    #[test]
    fn handle_event_sends_ntfy_and_records_dedupe_key() {
        let temp = TempRuntimeDir::new("send_ntfy");
        temp.write_config(json!({
            "enabled": true,
            "topic": "niuma-test"
        }));
        let env = temp.env();
        let sender = RecordingNtfySender::default();
        let reporter = RecordingNotificationResultReporter::default();

        handle_event(
            &env,
            &sender,
            &reporter,
            event_with_type("event-send-1", EventType::ApprovalRequested),
        )
        .unwrap();

        let requests = sender.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "https://ntfy.sh/niuma-test");
        assert!(requests[0].body.contains("需要处理"));
        assert!(load_sent_event_keys(&temp.data_dir.join(SENT_EVENTS_FILE))
            .unwrap()
            .contains("builtin-ntfy:event-send-1"));
        let reports = reporter.reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, "sent");
    }

    #[test]
    fn handle_event_does_not_record_dedupe_key_when_send_fails() {
        let temp = TempRuntimeDir::new("send_failed");
        temp.write_config(json!({
            "enabled": true,
            "topic": "niuma-test"
        }));
        let env = temp.env();
        let sender = RecordingNtfySender {
            error: Some("network failed".to_string()),
            ..RecordingNtfySender::default()
        };
        let reporter = RecordingNotificationResultReporter::default();

        let error = handle_event(
            &env,
            &sender,
            &reporter,
            event_with_type("event-send-failed", EventType::ApprovalRequested),
        )
        .unwrap_err();

        assert_eq!(error, "network failed");
        assert!(!load_sent_event_keys(&temp.data_dir.join(SENT_EVENTS_FILE))
            .unwrap()
            .contains("builtin-ntfy:event-send-failed"));
        let reports = reporter.reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, "failed");
        assert_eq!(reports[0].error_message.as_deref(), Some("network failed"));
    }

    #[test]
    fn handle_event_skips_when_ntfy_config_is_disabled() {
        let temp = TempRuntimeDir::new("disabled");
        temp.write_config(json!({
            "enabled": false,
            "topic": "niuma-test"
        }));
        let env = temp.env();
        let sender = RecordingNtfySender::default();
        let reporter = RecordingNotificationResultReporter::default();

        handle_event(
            &env,
            &sender,
            &reporter,
            event_with_type("event-disabled", EventType::ApprovalRequested),
        )
        .unwrap();

        assert!(sender.requests.lock().unwrap().is_empty());
        assert!(reporter.reports.lock().unwrap().is_empty());
    }

    #[test]
    fn handle_event_skips_when_ntfy_topic_is_invalid() {
        let temp = TempRuntimeDir::new("invalid_topic");
        temp.write_config(json!({
            "enabled": true,
            "topic": "bad/topic"
        }));
        let env = temp.env();
        let sender = RecordingNtfySender::default();
        let reporter = RecordingNotificationResultReporter::default();

        handle_event(
            &env,
            &sender,
            &reporter,
            event_with_type("event-invalid-topic", EventType::ApprovalRequested),
        )
        .unwrap();

        assert!(sender.requests.lock().unwrap().is_empty());
        assert!(reporter.reports.lock().unwrap().is_empty());
    }

    #[test]
    fn handle_test_event_sends_ntfy_without_dedupe() {
        let temp = TempRuntimeDir::new("test_send");
        temp.write_config(json!({
            "enabled": true,
            "topic": "niuma-test"
        }));
        let env = temp.env();
        let sender = RecordingNtfySender::default();
        let reporter = RecordingNotificationResultReporter::default();
        let request = PluginNotificationTestRequest {
            test_id: "manual-test:builtin-ntfy:2".to_string(),
            plugin_id: "builtin-ntfy".to_string(),
            title: "测试标题".to_string(),
            body: "测试正文".to_string(),
            created_at: Utc::now(),
        };

        handle_test_event(&env, &sender, &reporter, request).unwrap();

        let requests = sender.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "https://ntfy.sh/niuma-test");
        assert!(requests[0].body.contains("测试正文"));
        assert!(!temp.data_dir.join(SENT_EVENTS_FILE).exists());
        let reports = reporter.reports.lock().unwrap();
        assert_eq!(reports[0].status, "sent");
    }

    #[test]
    fn handle_test_event_skips_when_ntfy_topic_is_invalid() {
        let temp = TempRuntimeDir::new("test_invalid_topic");
        temp.write_config(json!({
            "enabled": true,
            "topic": "bad/topic"
        }));
        let env = temp.env();
        let sender = RecordingNtfySender::default();
        let reporter = RecordingNotificationResultReporter::default();
        let request = PluginNotificationTestRequest {
            test_id: "manual-test:builtin-ntfy:invalid".to_string(),
            plugin_id: "builtin-ntfy".to_string(),
            title: "测试标题".to_string(),
            body: "测试正文".to_string(),
            created_at: Utc::now(),
        };

        handle_test_event(&env, &sender, &reporter, request).unwrap();

        assert!(sender.requests.lock().unwrap().is_empty());
        assert!(reporter.reports.lock().unwrap().is_empty());
    }

    #[test]
    fn parent_pid_parser_ignores_invalid_values() {
        assert_eq!(parse_parent_pid(None), None);
        assert_eq!(parse_parent_pid(Some("not-a-pid")), None);
        assert_eq!(parse_parent_pid(Some("0")), None);
        assert_eq!(parse_parent_pid(Some("123")), Some(123));
    }

    #[derive(Default)]
    struct RecordingNtfySender {
        requests: Arc<Mutex<Vec<OutboundRequest>>>,
        error: Option<String>,
    }

    impl NtfySender for RecordingNtfySender {
        fn send(&self, request: &OutboundRequest) -> Result<(), String> {
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            self.requests.lock().unwrap().push(request.clone());
            Ok(())
        }
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct RecordedNotificationResult {
        status: String,
        error_message: Option<String>,
    }

    #[derive(Clone, Default)]
    struct RecordingNotificationResultReporter {
        reports: Arc<Mutex<Vec<RecordedNotificationResult>>>,
    }

    impl NotificationResultReporter for RecordingNotificationResultReporter {
        fn report(
            &self,
            _env: &RuntimeEnv,
            _event: &NiumaEvent,
            status: &str,
            _message: &NotificationMessage,
            error_message: Option<&str>,
            _sent_at: Option<chrono::DateTime<Utc>>,
        ) -> Result<(), String> {
            self.reports
                .lock()
                .unwrap()
                .push(RecordedNotificationResult {
                    status: status.to_string(),
                    error_message: error_message.map(ToString::to_string),
                });
            Ok(())
        }

        fn report_test(
            &self,
            _env: &RuntimeEnv,
            _test: &PluginNotificationTestRequest,
            status: &str,
            _message: &NotificationMessage,
            error_message: Option<&str>,
            _sent_at: Option<chrono::DateTime<Utc>>,
        ) -> Result<(), String> {
            self.reports
                .lock()
                .unwrap()
                .push(RecordedNotificationResult {
                    status: status.to_string(),
                    error_message: error_message.map(ToString::to_string),
                });
            Ok(())
        }
    }

    struct TempRuntimeDir {
        root: PathBuf,
        config_path: PathBuf,
        data_dir: PathBuf,
    }

    impl TempRuntimeDir {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("niuma-ntfy-runtime-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let config_path = root.join("config.json");
            let data_dir = root.join("data");
            Self {
                root,
                config_path,
                data_dir,
            }
        }

        fn write_config(&self, value: serde_json::Value) {
            std::fs::write(&self.config_path, serde_json::to_string(&value).unwrap()).unwrap();
        }

        fn env(&self) -> RuntimeEnv {
            RuntimeEnv {
                api_url: "http://127.0.0.1:1".to_string(),
                plugin_id: "builtin-ntfy".to_string(),
                config_path: Some(self.config_path.clone()),
                data_dir: Some(self.data_dir.clone()),
            }
        }
    }

    impl Drop for TempRuntimeDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn event_with_type(id: &str, event_type: EventType) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            tool: "codex".to_string(),
            project_name: "project".to_string(),
            event_type,
            summary: "需要处理".to_string(),
            content: Some("需要处理这个事件".to_string()),
            error_message: None,
            completion_reason: None,
            failure_reason: None,
            created_at: Some(Utc::now()),
        }
    }
}
