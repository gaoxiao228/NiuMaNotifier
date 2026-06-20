use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const PARENT_PID_ENV: &str = "NIUMA_PARENT_PID";
const DEFAULT_PLUGIN_ID: &str = "builtin-bark";
const DEFAULT_BARK_SERVER: &str = "https://api.day.app";
const DEFAULT_BARK_GROUP: &str = "NiumaNotifier";
const RECONNECT_INTERVAL: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const PARENT_WATCHDOG_INTERVAL: Duration = Duration::from_secs(2);
const SENT_EVENTS_FILE: &str = "sent-events.jsonl";
const ERROR_BODY_LIMIT_BYTES: u64 = 1024;
const CODEX_ICON_URL: &str =
    "https://cdn.jsdelivr.net/npm/@lobehub/icons-static-png@latest/light/codex-color.png";
const CLAUDE_CODE_ICON_URL: &str =
    "https://upload.wikimedia.org/wikipedia/commons/thumb/b/b0/Claude_AI_symbol.svg/1280px-Claude_AI_symbol.svg.png";

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
struct BarkConfig {
    server: String,
    device_key: String,
    group: String,
    icon_url: String,
}

impl BarkConfig {
    fn is_incomplete(&self) -> bool {
        self.server.trim().is_empty() || self.device_key.trim().is_empty()
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
struct BarkPluginConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    server: String,
    #[serde(default)]
    device_key: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    icon_url: String,
}

impl BarkPluginConfig {
    fn bark_config_for_event(&self, event: &NiumaEvent) -> BarkConfig {
        BarkConfig {
            server: non_empty_text(&self.server)
                .unwrap_or(DEFAULT_BARK_SERVER)
                .to_string(),
            device_key: self.device_key.trim().to_string(),
            group: non_empty_text(&self.group)
                .unwrap_or(DEFAULT_BARK_GROUP)
                .to_string(),
            icon_url: tool_notification_icon_url(&event.tool)
                .or_else(|| non_empty_text(&self.icon_url).map(ToString::to_string))
                .unwrap_or_default(),
        }
    }

    fn bark_config_for_test(&self) -> BarkConfig {
        BarkConfig {
            server: non_empty_text(&self.server)
                .unwrap_or(DEFAULT_BARK_SERVER)
                .to_string(),
            device_key: self.device_key.trim().to_string(),
            group: non_empty_text(&self.group)
                .unwrap_or(DEFAULT_BARK_GROUP)
                .to_string(),
            icon_url: non_empty_text(&self.icon_url)
                .map(ToString::to_string)
                .unwrap_or_default(),
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

fn build_bark_request(
    config: &BarkConfig,
    message: &NotificationMessage,
) -> Result<OutboundRequest, String> {
    let server = config.server.trim().trim_end_matches('/');
    let device_key = config.device_key.trim();
    if server.is_empty() || device_key.is_empty() {
        return Err("Bark server 和 device key 不能为空".to_string());
    }

    let group = message
        .project_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| config.group.trim());
    let mut payload = serde_json::json!({
        "title": message.title,
        "body": message.body,
        "device_key": device_key,
    });
    if !group.is_empty() {
        payload["group"] = serde_json::json!(group);
    }
    if !config.icon_url.trim().is_empty() {
        payload["icon"] = serde_json::json!(config.icon_url.trim());
    }
    if let Some(action_url) = message
        .action_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        payload["url"] = serde_json::json!(action_url);
    }

    let mut headers = BTreeMap::new();
    headers.insert(
        "Content-Type".to_string(),
        "application/json; charset=utf-8".to_string(),
    );
    Ok(OutboundRequest {
        method: "POST".to_string(),
        url: format!("{server}/push"),
        headers,
        body: serde_json::to_string(&payload).map_err(|error| error.to_string())?,
    })
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
            eprintln!("NiumaNotifier Bark plugin process not started: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = prepare_runtime_files(&env) {
        eprintln!("NiumaNotifier Bark plugin runtime file setup failed: {error}");
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
        // 数据目录只归插件自己使用，当前用于 sent-events.jsonl 本地去重。
        std::fs::create_dir_all(data_dir)
            .map_err(|error| format!("创建 Bark 插件数据目录失败：{error}"))?;
    }
    if let Some(config_path) = &env.config_path {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建 Bark 插件配置目录失败：{error}"))?;
        }
        if config_path.exists() {
            let _ = load_config(config_path)?;
        }
    }
    Ok(())
}

fn load_config(path: &std::path::Path) -> Result<BarkPluginConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 Bark 插件配置失败：{error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析 Bark 插件配置失败：{error}"))
}

fn run_event_stream_loop(env: RuntimeEnv) {
    let sender = UreqBarkSender::default();
    let reporter = UreqNotificationResultReporter::default();
    let agent = ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build();
    loop {
        if let Err(error) = consume_events_once(&agent, &sender, &reporter, &env) {
            eprintln!("NiumaNotifier Bark plugin event stream disconnected: {error}");
        }
        thread::sleep(RECONNECT_INTERVAL);
    }
}

fn consume_events_once<S: BarkSender, R: NotificationResultReporter>(
    agent: &ureq::Agent,
    sender: &S,
    reporter: &R,
    env: &RuntimeEnv,
) -> Result<(), String> {
    let stream_url = format!("{}/api/v1/events/stream", env.api_url.trim_end_matches('/'));
    let response = agent
        .get(&stream_url)
        .set("Accept", "text/event-stream")
        .call()
        .map_err(|error| format!("连接事件流失败：{error}"))?;
    let reader = BufReader::new(response.into_reader());
    consume_event_lines(reader, |item| match item {
        StreamItem::Event(event) => handle_event(env, sender, reporter, event),
        StreamItem::NotificationTest(request) => handle_test_event(env, sender, reporter, request),
    })
}

fn consume_event_lines<R, F>(reader: R, mut on_event: F) -> Result<(), String>
where
    R: BufRead,
    F: FnMut(StreamItem) -> Result<(), String>,
{
    let mut frame = SseEventFrame::default();
    for line in reader.lines() {
        let line = line.map_err(|error| format!("读取事件流失败：{error}"))?;
        if line.is_empty() {
            flush_frame(&mut frame, &mut on_event)?;
            continue;
        }
        apply_sse_line(&mut frame, &line);
    }
    flush_frame(&mut frame, &mut on_event)
}

fn apply_sse_line(frame: &mut SseEventFrame, line: &str) {
    if line.starts_with(':') {
        return;
    }
    let (field, value) = line
        .split_once(':')
        .map(|(field, value)| (field, value.strip_prefix(' ').unwrap_or(value)))
        .unwrap_or((line, ""));
    match field {
        "event" => frame.event = Some(value.to_string()),
        "id" => frame.id = Some(value.to_string()),
        "data" => frame.data_lines.push(value.to_string()),
        _ => {}
    }
}

fn flush_frame<F>(frame: &mut SseEventFrame, on_event: &mut F) -> Result<(), String>
where
    F: FnMut(StreamItem) -> Result<(), String>,
{
    if frame.data_lines.is_empty() {
        *frame = SseEventFrame::default();
        return Ok(());
    }
    let event_name = frame.event.clone();
    let data = frame.data_lines.join("\n");
    *frame = SseEventFrame::default();
    match event_name.as_deref().unwrap_or("event") {
        "event" => {
            let event = serde_json::from_str::<NiumaEvent>(&data)
                .map_err(|error| format!("解析事件流数据失败：{error}"))?;
            on_event(StreamItem::Event(event))
        }
        "notification_test" => {
            let request = serde_json::from_str::<PluginNotificationTestRequest>(&data)
                .map_err(|error| format!("解析测试通知数据失败：{error}"))?;
            on_event(StreamItem::NotificationTest(request))
        }
        _ => Ok(()),
    }
}

fn handle_event<S: BarkSender, R: NotificationResultReporter>(
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
    if config.bark_config_for_event(&event).is_incomplete() {
        return Ok(());
    }

    let dedupe_key = plugin_event_dedupe_key(&env.plugin_id, &event.id);
    let mut sent_events = SentEventStore::from_env(env)?;
    if sent_events.contains(&dedupe_key) {
        return Ok(());
    }

    let request = build_bark_request(&config.bark_config_for_event(&event), &message)?;
    match sender.send(&request) {
        Ok(()) => {
            let sent_at = Utc::now();
            if let Err(error) = reporter.report(env, &event, "sent", &message, None, Some(sent_at))
            {
                eprintln!("NiumaNotifier Bark notification result report failed: {error}");
            }
        }
        Err(error) => {
            if let Err(report_error) =
                reporter.report(env, &event, "failed", &message, Some(&error), None)
            {
                eprintln!(
                    "NiumaNotifier Bark notification failed result report failed: {report_error}"
                );
            }
            return Err(error);
        }
    }
    sent_events.record_sent(&dedupe_key)?;
    Ok(())
}

fn handle_test_event<S: BarkSender, R: NotificationResultReporter>(
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
    if config.bark_config_for_test().is_incomplete() {
        return Ok(());
    }

    let request = build_bark_request(&config.bark_config_for_test(), &message)?;
    match sender.send(&request) {
        Ok(()) => reporter.report_test(env, &test, "sent", &message, None, Some(Utc::now())),
        Err(error) => {
            reporter.report_test(env, &test, "failed", &message, Some(&error), None)?;
            Err(error)
        }
    }
}

trait BarkSender {
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
struct UreqBarkSender {
    agent: ureq::Agent,
}

impl Default for UreqBarkSender {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build(),
        }
    }
}

impl BarkSender for UreqBarkSender {
    fn send(&self, request: &OutboundRequest) -> Result<(), String> {
        send_outbound_request(&self.agent, request)
    }
}

fn send_outbound_request(agent: &ureq::Agent, request: &OutboundRequest) -> Result<(), String> {
    let method = request.method.to_uppercase();
    let mut call = match method.as_str() {
        "POST" => agent.post(&request.url),
        _ => return Err(format!("不支持的 Bark 请求方法：{method}")),
    };
    for (key, value) in &request.headers {
        call = call.set(key, value);
    }
    match call.send_string(&request.body) {
        Ok(response) if (200..300).contains(&response.status()) => Ok(()),
        Ok(response) => Err(format_bark_status_error(
            response.status(),
            &request.url,
            response,
        )),
        Err(ureq::Error::Status(status, response)) => {
            Err(format_bark_status_error(status, &request.url, response))
        }
        Err(error) => Err(format!("发送 Bark 推送失败：{error}")),
    }
}

fn format_bark_status_error(status: u16, url: &str, response: ureq::Response) -> String {
    let body = response_body_excerpt(response);
    if body.is_empty() {
        format!("Bark 服务返回 HTTP {status}：{url}")
    } else {
        format!("Bark 服务返回 HTTP {status}：{url}，响应：{body}")
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
                    .map_err(|error| format!("创建 Bark 去重目录失败：{error}"))?;
            }
            let record = SentEventRecord {
                key: key.to_string(),
                sent_at: Utc::now().to_rfc3339(),
            };
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|error| format!("打开 Bark 去重文件失败：{error}"))?;
            writeln!(
                file,
                "{}",
                serde_json::to_string(&record)
                    .map_err(|error| format!("序列化 Bark 去重记录失败：{error}"))?
            )
            .map_err(|error| format!("写入 Bark 去重记录失败：{error}"))?;
        }
        self.keys.insert(key.to_string());
        Ok(())
    }
}

fn load_sent_event_keys(path: &std::path::Path) -> Result<HashSet<String>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(format!("读取 Bark 去重文件失败：{error}")),
    };
    let mut keys = HashSet::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| format!("读取 Bark 去重记录失败：{error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<SentEventRecord>(&line) {
            Ok(record) => {
                keys.insert(record.key);
            }
            Err(error) => {
                eprintln!("NiumaNotifier Bark dedupe record ignored: {error}");
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

fn tool_notification_icon_url(tool: &str) -> Option<String> {
    match tool {
        "codex" => Some(CODEX_ICON_URL.to_string()),
        "claude_code" => Some(CLAUDE_CODE_ICON_URL.to_string()),
        _ => None,
    }
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
    use chrono::Utc;
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
            test_id: "manual-test:builtin-bark:1".to_string(),
            plugin_id: "builtin-bark".to_string(),
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
    fn parent_pid_parser_ignores_invalid_values() {
        assert_eq!(parse_parent_pid(None), None);
        assert_eq!(parse_parent_pid(Some("not-a-pid")), None);
        assert_eq!(parse_parent_pid(Some("0")), None);
        assert_eq!(parse_parent_pid(Some("123")), Some(123));
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
    fn handle_event_sends_bark_and_records_dedupe_key() {
        let temp = TestRuntimeFiles::new("send_and_dedupe");
        temp.write_config(json!({
            "enabled": true,
            "device_key": "abc123"
        }));
        let env = temp.env();
        let sender = RecordingBarkSender::default();
        let reporter = RecordingNotificationResultReporter::default();

        handle_event(&env, &sender, &reporter, completed_event("event-send-1")).unwrap();
        handle_event(&env, &sender, &reporter, completed_event("event-send-1")).unwrap();

        let requests = sender.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "https://api.day.app/push");
        assert!(requests[0].body.contains(r#""device_key":"abc123""#));
        assert!(load_sent_event_keys(&temp.data_dir.join(SENT_EVENTS_FILE))
            .unwrap()
            .contains("builtin-bark:event-send-1"));
        let reports = reporter.reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, "sent");
    }

    #[test]
    fn handle_event_does_not_record_dedupe_key_when_send_fails() {
        let temp = TestRuntimeFiles::new("send_failed");
        temp.write_config(json!({
            "enabled": true,
            "device_key": "abc123"
        }));
        let env = temp.env();
        let sender = RecordingBarkSender {
            requests: Arc::new(Mutex::new(Vec::new())),
            error: Some("network unavailable".to_string()),
        };
        let reporter = RecordingNotificationResultReporter::default();

        let error = handle_event(
            &env,
            &sender,
            &reporter,
            completed_event("event-send-failed"),
        )
        .unwrap_err();

        assert!(error.contains("network unavailable"));
        assert!(!load_sent_event_keys(&temp.data_dir.join(SENT_EVENTS_FILE))
            .unwrap()
            .contains("builtin-bark:event-send-failed"));
        let reports = reporter.reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, "failed");
        assert_eq!(
            reports[0].error_message.as_deref(),
            Some("network unavailable")
        );
    }

    #[test]
    fn handle_event_skips_when_bark_config_is_disabled() {
        let temp = TestRuntimeFiles::new("disabled");
        temp.write_config(json!({
            "enabled": false,
            "device_key": "abc123"
        }));
        let env = temp.env();
        let sender = RecordingBarkSender::default();
        let reporter = RecordingNotificationResultReporter::default();

        handle_event(&env, &sender, &reporter, completed_event("event-disabled")).unwrap();

        assert!(sender.requests.lock().unwrap().is_empty());
        assert!(reporter.reports.lock().unwrap().is_empty());
    }

    #[test]
    fn handle_event_skips_when_bark_device_key_is_missing() {
        let temp = TestRuntimeFiles::new("missing_device_key");
        temp.write_config(json!({
            "enabled": true
        }));
        let env = temp.env();
        let sender = RecordingBarkSender::default();
        let reporter = RecordingNotificationResultReporter::default();

        handle_event(
            &env,
            &sender,
            &reporter,
            completed_event("event-missing-device-key"),
        )
        .unwrap();

        assert!(sender.requests.lock().unwrap().is_empty());
        assert!(reporter.reports.lock().unwrap().is_empty());
    }

    #[test]
    fn handle_test_event_sends_bark_without_dedupe() {
        let temp = TestRuntimeFiles::new("test_send");
        temp.write_config(json!({
            "enabled": true,
            "device_key": "abc123"
        }));
        let env = temp.env();
        let sender = RecordingBarkSender::default();
        let reporter = RecordingNotificationResultReporter::default();
        let request = PluginNotificationTestRequest {
            test_id: "manual-test:builtin-bark:2".to_string(),
            plugin_id: "builtin-bark".to_string(),
            title: "测试标题".to_string(),
            body: "测试正文".to_string(),
            created_at: Utc::now(),
        };

        handle_test_event(&env, &sender, &reporter, request).unwrap();

        let requests = sender.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].body.contains("测试标题"));
        assert!(!temp.data_dir.join(SENT_EVENTS_FILE).exists());
        let reports = reporter.reports.lock().unwrap();
        assert_eq!(reports[0].status, "sent");
    }

    #[test]
    fn handle_test_event_skips_when_bark_device_key_is_missing() {
        let temp = TestRuntimeFiles::new("test_missing_device_key");
        temp.write_config(json!({
            "enabled": true
        }));
        let env = temp.env();
        let sender = RecordingBarkSender::default();
        let reporter = RecordingNotificationResultReporter::default();
        let request = PluginNotificationTestRequest {
            test_id: "manual-test:builtin-bark:missing".to_string(),
            plugin_id: "builtin-bark".to_string(),
            title: "测试标题".to_string(),
            body: "测试正文".to_string(),
            created_at: Utc::now(),
        };

        handle_test_event(&env, &sender, &reporter, request).unwrap();

        assert!(sender.requests.lock().unwrap().is_empty());
        assert!(reporter.reports.lock().unwrap().is_empty());
    }

    #[test]
    fn bark_config_for_event_uses_codex_tool_icon() {
        let config = BarkPluginConfig {
            enabled: true,
            device_key: "abc123".to_string(),
            ..BarkPluginConfig::default()
        };

        let bark_config = config.bark_config_for_event(&completed_event("event-codex-icon"));

        assert_eq!(
            bark_config.icon_url,
            "https://cdn.jsdelivr.net/npm/@lobehub/icons-static-png@latest/light/codex-color.png"
        );
    }

    #[test]
    fn bark_config_for_event_uses_claude_code_tool_icon() {
        let config = BarkPluginConfig {
            enabled: true,
            device_key: "abc123".to_string(),
            ..BarkPluginConfig::default()
        };
        let mut event = completed_event("event-claude-icon");
        event.tool = "claude_code".to_string();

        let bark_config = config.bark_config_for_event(&event);

        assert_eq!(
            bark_config.icon_url,
            "https://upload.wikimedia.org/wikipedia/commons/thumb/b/b0/Claude_AI_symbol.svg/1280px-Claude_AI_symbol.svg.png"
        );
    }

    #[test]
    fn bark_config_for_event_prefers_tool_icon_over_payload_icon() {
        let config = BarkPluginConfig {
            enabled: true,
            device_key: "abc123".to_string(),
            icon_url: "https://example.test/custom.png".to_string(),
            ..BarkPluginConfig::default()
        };

        let bark_config = config.bark_config_for_event(&completed_event("event-tool-icon-wins"));

        assert_eq!(
            bark_config.icon_url,
            "https://cdn.jsdelivr.net/npm/@lobehub/icons-static-png@latest/light/codex-color.png"
        );
    }

    #[derive(Clone, Default)]
    struct RecordingBarkSender {
        requests: Arc<Mutex<Vec<OutboundRequest>>>,
        error: Option<String>,
    }

    impl BarkSender for RecordingBarkSender {
        fn send(&self, request: &OutboundRequest) -> Result<(), String> {
            self.requests.lock().unwrap().push(request.clone());
            match &self.error {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
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

    struct TestRuntimeFiles {
        root: PathBuf,
        config_path: PathBuf,
        data_dir: PathBuf,
    }

    impl TestRuntimeFiles {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("niuma-bark-runtime-{name}-{}", std::process::id()));
            if root.exists() {
                let _ = std::fs::remove_dir_all(&root);
            }
            let config_path = root.join("config.json");
            let data_dir = root.join("data");
            std::fs::create_dir_all(&data_dir).unwrap();
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
                api_url: "http://127.0.0.1:27874".to_string(),
                plugin_id: DEFAULT_PLUGIN_ID.to_string(),
                config_path: Some(self.config_path.clone()),
                data_dir: Some(self.data_dir.clone()),
            }
        }
    }

    impl Drop for TestRuntimeFiles {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn completed_event(id: &str) -> NiumaEvent {
        let mut event = event_with_type(id, EventType::AssistantMessageCompleted);
        event.completion_reason = Some(CompletionReason::Normal);
        event
    }

    fn event_with_type(id: &str, event_type: EventType) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            tool: "codex".to_string(),
            project_name: "project".to_string(),
            event_type,
            summary: "测试事件".to_string(),
            content: Some("测试内容".to_string()),
            error_message: None,
            completion_reason: None,
            failure_reason: None,
            created_at: Some(Utc::now()),
        }
    }
}
