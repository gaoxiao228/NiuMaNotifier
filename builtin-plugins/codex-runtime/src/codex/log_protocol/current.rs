use chrono::{DateTime, TimeZone, Utc};

use crate::codex::log_protocol::CodexLogProtocolParser;
use crate::codex::log_watcher::CodexLogRow;
use niuma_core::models::{EventType, FailureReason, NiumaEvent, ToolKind};

const HIGH_DEMAND_ERROR: &str =
    "We're currently experiencing high demand, which may cause temporary errors.";
const ERROR_DEDUPE_BUCKET_SECONDS: i64 = 10;

#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
pub struct CurrentLogProtocol;

impl CodexLogProtocolParser for CurrentLogProtocol {
    fn parse_row(&self, row: &CodexLogRow, source_path: &str) -> Option<NiumaEvent> {
        parse_codex_log_row(row, source_path)
    }
}

pub fn parse_codex_log_row(row: &CodexLogRow, source_path: &str) -> Option<NiumaEvent> {
    let body = row.feedback_log_body.as_deref()?;
    if !is_codex_runtime_error_shape(&row.target, body) {
        return None;
    }
    let failure = classify_codex_log_failure(body)?;
    let summary = failure.summary;
    let error_message = failure.display_message.unwrap_or_else(|| body.to_string());
    let session_id = extract_field(body, "conversation.id")
        .or_else(|| row.thread_id.clone())
        .filter(|value| !value.is_empty())?;
    let created_at = timestamp_from_row(row);
    // Codex 对同一个模型/API 错误会写多条内部日志，例如 otel safe/log_only 和
    // session::turn。去重键按短时间桶归并，避免同一真实失败触发多次主状态通知。
    let dedupe_key = format!(
        "codex_log:{session_id}:{}:{}",
        error_dedupe_bucket(&created_at),
        failure.dedupe_suffix
    );

    Some(NiumaEvent {
        id: format!("event_codex_log_{}", stable_hash(&dedupe_key)),
        dedupe_key,
        source: "codex-internal-log".to_string(),
        tool: ToolKind::Codex,
        session_id,
        project_path: String::new(),
        project_name: "Codex".to_string(),
        event_type: EventType::TaskFailed,
        severity: "urgent".to_string(),
        summary,
        content: None,
        error_message: Some(error_message),
        attention_resolve_key: None,
        completion_reason: None,
        failure_reason: Some(failure.reason),
        payload_ref: Some(source_path.to_string()),
        created_at,
    })
}

fn is_codex_runtime_error_shape(target: &str, body: &str) -> bool {
    // 只信任 Codex 运行时错误形态，避免把助手输出、工具输出或查询语句里的错误样例当成真实失败。
    if target.starts_with("codex_otel.") {
        // codex.tool_result 的工具输出可能嵌入 SSE 错误样例；只接受日志顶层事件名为 codex.sse_event。
        let top_level_event = extract_field(body, "event.name");
        return top_level_event.as_deref() == Some("codex.sse_event")
            && body.contains("error.message=");
    }
    if target == "codex_core::session::turn" {
        return body.contains("Turn error:");
    }
    // Codex 有些终端错误只写入通用 log target；必须同时满足 websocket 错误事件形态。
    target == "log" && is_received_message_error(body)
}

struct LogFailure {
    reason: FailureReason,
    dedupe_suffix: String,
    summary: String,
    display_message: Option<String>,
}

fn classify_codex_log_failure(body: &str) -> Option<LogFailure> {
    let has_error_shape = body.contains("error.message=")
        || body.contains("Turn error:")
        || is_received_message_error(body);
    if !has_error_shape {
        return None;
    }

    if body.contains("\"type\":\"invalid_request_error\"")
        && body.contains("\"code\":\"context_too_large\"")
    {
        return Some(LogFailure {
            reason: FailureReason::ContextWindowExceeded,
            dedupe_suffix: "context_too_large".to_string(),
            summary: "Codex context window exceeded".to_string(),
            display_message: None,
        });
    }

    if body.contains(HIGH_DEMAND_ERROR) {
        // Codex 可能在内部重试后继续完成当前 turn；这个提示不是稳定的终止信号。
        return None;
    }

    if let Some(message) = turn_error_message(body) {
        let turn_id = extract_field(body, "turn.id").unwrap_or_else(|| "unknown_turn".to_string());
        return Some(LogFailure {
            reason: FailureReason::Fatal,
            dedupe_suffix: format!("turn_error:{turn_id}"),
            summary: "Codex turn failed".to_string(),
            display_message: Some(message),
        });
    }

    None
}

fn turn_error_message(body: &str) -> Option<String> {
    body.split_once("Turn error:")
        .map(|(_, message)| message.trim())
        .filter(|message| !message.is_empty())
        .map(ToString::to_string)
}

fn is_received_message_error(body: &str) -> bool {
    body.contains("Received message {\"type\":\"error\"") && body.contains("\"status\":400")
}

fn extract_field(body: &str, key: &str) -> Option<String> {
    let start = body.find(key)?;
    let value_start = start + key.len() + usize::from(body[start + key.len()..].starts_with('='));
    body.get(value_start..)?
        .split_whitespace()
        .next()
        .map(|value| value.trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
}

fn timestamp_from_row(row: &CodexLogRow) -> DateTime<Utc> {
    Utc.timestamp_opt(row.ts, row.ts_nanos.max(0) as u32)
        .single()
        .unwrap_or_else(Utc::now)
}

fn error_dedupe_bucket(timestamp: &DateTime<Utc>) -> i64 {
    timestamp.timestamp() / ERROR_DEDUPE_BUCKET_SECONDS
}

fn stable_hash(text: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:x}")
}
