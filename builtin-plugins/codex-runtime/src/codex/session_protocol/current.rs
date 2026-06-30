use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::codex::session_identity::{
    codex_fallback_session_id, codex_project_name, CodexSessionMetadata,
};
use crate::codex::session_protocol::{
    detect_session_protocol_family, CodexProtocolFamily, CodexSessionProtocolParser,
};
use niuma_core::models::{
    CompletionReason, EventInteractionDetail, EventType, FailureReason, NiumaEvent, ToolKind,
};

// 只负责把单行 Codex JSONL 转换为核心事件，文件扫描和运行时监听由后续任务处理。
#[derive(Clone, Default)]
pub struct CodexJsonlParser {
    protocol_family: Option<CodexProtocolFamily>,
    session_id: Option<String>,
    session_metadata: CodexSessionMetadata,
    cwd: Option<String>,
    last_assistant_message: Option<String>,
    pending_plan_confirmation_turn_id: Option<String>,
    // Codex JSONL 没有单独的“授权已批准”事件，只能用 call_id 关联请求和输出。
    pending_approval_call_ids: HashSet<String>,
}

impl CodexSessionProtocolParser for CodexJsonlParser {
    fn parse_line(
        &mut self,
        line: &str,
        fallback_path: &str,
    ) -> Result<Option<NiumaEvent>, String> {
        CodexJsonlParser::parse_line(self, line, fallback_path)
    }
}

#[derive(Deserialize)]
struct CodexRow {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    row_type: String,
    payload: serde_json::Value,
}

impl CodexJsonlParser {
    pub fn parse_line(
        &mut self,
        line: &str,
        fallback_path: &str,
    ) -> Result<Option<NiumaEvent>, String> {
        match detect_session_protocol_family(line)? {
            CodexProtocolFamily::Current => {
                self.protocol_family
                    .get_or_insert(CodexProtocolFamily::Current);
            }
            CodexProtocolFamily::Unsupported => return Ok(None),
        }
        let row: CodexRow = serde_json::from_str(line)
            .map_err(|error| format!("解析 Codex JSONL 失败：{error}"))?;
        if row.row_type == "session_meta" {
            if self.session_id.is_none() {
                if let Some(session_id) = row
                    .payload
                    .get("id")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                {
                    self.session_id = Some(session_id.to_string());
                }
            }
            self.session_metadata.merge_session_meta(&row.payload);
            if self.cwd.is_none() {
                if let Some(cwd) = row
                    .payload
                    .get("cwd")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                {
                    self.cwd = Some(cwd.to_string());
                }
            }
            return Ok(None);
        }
        if !matches!(row.row_type.as_str(), "event_msg" | "response_item") {
            return Ok(None);
        }

        let Some(kind) = row.payload.get("type").and_then(|value| value.as_str()) else {
            return Ok(None);
        };
        if let Some(message) = assistant_message_from_payload(&row.row_type, kind, &row.payload) {
            self.last_assistant_message = Some(message);
        }
        let session_id = self
            .session_id
            .clone()
            .unwrap_or_else(|| codex_fallback_session_id(fallback_path));
        let project_path = self.cwd.clone().unwrap_or_default();
        let identity = self.session_metadata.identity_for_session(&session_id);
        let project_name = codex_project_name(&project_path);

        let mut attention_resolve_key = None;
        let mut summary = None;
        let mut content_override = None;
        let mut completion_reason = None;
        let mut failure_reason = None;
        let mut watcher_approval_fallback = false;
        let event_type = match row.row_type.as_str() {
            "event_msg" => match kind {
                "task_started" => {
                    self.pending_plan_confirmation_turn_id = None;
                    self.last_assistant_message = None;
                    EventType::SessionStarted
                }
                "task_complete" => {
                    if self.is_pending_plan_confirmation_complete(&row.payload) {
                        return Ok(None);
                    }
                    let Some(message) = task_complete_message(&row.payload)
                        .or_else(|| self.last_assistant_message.clone())
                    else {
                        return Ok(None);
                    };
                    summary = Some(message.clone());
                    content_override = Some(message);
                    completion_reason = Some(CompletionReason::Normal);
                    EventType::AssistantMessageCompleted
                }
                "thread_rolled_back" => {
                    completion_reason = Some(CompletionReason::RolledBack);
                    EventType::AssistantMessageCompleted
                }
                "turn_aborted" => {
                    let classification = classification_for_abort_reason(
                        row.payload.get("reason").and_then(|value| value.as_str()),
                    );
                    completion_reason = classification.1;
                    failure_reason = classification.2;
                    classification.0
                }
                "item_completed" => {
                    let Some(input_request) = plan_item_completed(&row.payload) else {
                        return Ok(None);
                    };
                    self.pending_plan_confirmation_turn_id = row
                        .payload
                        .get("turn_id")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string);
                    summary = Some(input_request.summary);
                    content_override = Some(input_request.content);
                    EventType::InputRequested
                }
                "token_count" if self.pending_plan_confirmation_turn_id.is_some() => {
                    // Plan Mode 输出计划后会写 token_count，它只是遥测，不能清掉等待确认状态。
                    return Ok(None);
                }
                _ if is_event_message_activity(kind) => EventType::SessionActivity,
                _ => return Ok(None),
            },
            "response_item" => {
                if self.is_pending_plan_confirmation_message(kind, &row.payload) {
                    return Ok(None);
                }
                if kind == "function_call" {
                    if let Some(input_request) = request_user_input_function_call(&row.payload) {
                        summary = Some(input_request.summary);
                        content_override = Some(input_request.content);
                        EventType::InputRequested
                    } else if let Some(approval) =
                        escalated_function_call(&row.payload, &session_id)
                    {
                        let EscalatedFunctionCall {
                            call_id,
                            resolve_key,
                            summary: approval_summary,
                        } = approval;
                        if let Some(call_id) = call_id.as_deref() {
                            self.pending_approval_call_ids.insert(call_id.to_string());
                        }
                        watcher_approval_fallback = true;
                        attention_resolve_key = resolve_key;
                        summary = Some(approval_summary.clone());
                        content_override = Some(approval_summary);
                        EventType::ApprovalRequested
                    } else if is_response_item_activity(kind) {
                        EventType::SessionActivity
                    } else {
                        return Ok(None);
                    }
                } else if kind == "function_call_output" {
                    if let Some(call_id) =
                        row.payload.get("call_id").and_then(|value| value.as_str())
                    {
                        // watcher 重启或从文件尾部接管时可能只看到 output，看不到前置审批请求。
                        // resolve key 是精确匹配的，额外带上不存在的 key 不会清掉其他阻塞项。
                        self.pending_approval_call_ids.remove(call_id);
                        attention_resolve_key =
                            Some(codex_permission_resolve_key(&session_id, call_id));
                    }
                    EventType::SessionActivity
                } else if is_response_item_activity(kind) {
                    EventType::SessionActivity
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };

        let timestamp = parse_timestamp(row.timestamp.as_deref()).unwrap_or_else(Utc::now);
        let activity_key = event_key_fragment(&row, kind);
        let summary_text =
            summary.unwrap_or_else(|| summary_for_event_type(&event_type).to_string());
        let dedupe_key = if event_type == EventType::AssistantMessageCompleted {
            // Codex hook 与文件监听会同时看到同一轮完成事件；完成事件用同一去重键避免双路上报。
            format!(
                "codex:{session_id}:{activity_key}:assistant_message_completed:{}",
                stable_hash(&summary_text)
            )
        } else {
            format!("codex_file:{session_id}:{activity_key}:{kind}")
        };
        // 事件 ID 从去重键派生，避免同一毫秒内多行事件生成相同 ID。
        let event_id = format!("event_codex_file_{}", stable_hash(&dedupe_key));
        let content = match &event_type {
            EventType::AssistantMessageCompleted => Some(
                content_override
                    .clone()
                    .or_else(|| self.last_assistant_message.clone())
                    .unwrap_or_else(|| summary_text.clone()),
            ),
            EventType::ApprovalRequested | EventType::InputRequested => {
                Some(content_override.unwrap_or_else(|| summary_text.clone()))
            }
            _ => None,
        };
        let error_message = (event_type == EventType::TaskFailed).then(|| summary_text.clone());
        let interaction = match event_type {
            EventType::ApprovalRequested => Some(EventInteractionDetail::tool_approval(
                "请回到 Codex 中同意或拒绝",
            )),
            EventType::InputRequested => Some(EventInteractionDetail::tool_input(
                "请回到 Codex 中继续输入",
            )),
            _ => None,
        };

        Ok(Some(NiumaEvent {
            id: event_id,
            dedupe_key,
            source: "codex-session-file".to_string(),
            tool: ToolKind::Codex,
            session_id,
            parent_session_id: identity.parent_session_id,
            normalized_session_id: Some(identity.normalized_session_id),
            session_scope: Some(identity.session_scope.as_event_scope()),
            agent_nickname: identity.agent_nickname,
            agent_role: identity.agent_role,
            tool_call_id: None,
            project_path,
            project_name,
            event_type: event_type.clone(),
            severity: severity_for_event_type(&event_type).to_string(),
            summary: summary_text.clone(),
            content,
            error_message,
            attention_resolve_key,
            completion_reason,
            failure_reason,
            payload_ref: if watcher_approval_fallback {
                Some(format!(
                    "codex_watcher_approval:{}",
                    stable_hash(&summary_text)
                ))
            } else {
                Some(fallback_path.to_string())
            },
            interaction,
            created_at: timestamp,
        }))
    }

    pub(crate) fn has_session_metadata(&self) -> bool {
        self.session_id.is_some() && self.cwd.is_some()
    }

    fn is_pending_plan_confirmation_complete(&mut self, payload: &serde_json::Value) -> bool {
        let Some(pending_turn_id) = self.pending_plan_confirmation_turn_id.as_deref() else {
            return false;
        };
        let is_same_turn =
            payload.get("turn_id").and_then(|value| value.as_str()) == Some(pending_turn_id);
        if is_same_turn {
            // Plan Mode 的 task_complete 只表示计划输出结束；确认菜单仍在等待用户选择。
            self.pending_plan_confirmation_turn_id = None;
        }
        is_same_turn
    }

    fn is_pending_plan_confirmation_message(
        &self,
        kind: &str,
        payload: &serde_json::Value,
    ) -> bool {
        if self.pending_plan_confirmation_turn_id.is_none() || kind != "message" {
            return false;
        }
        if payload.get("role").and_then(|value| value.as_str()) != Some("assistant") {
            return false;
        }
        // Plan Mode 会先写 Plan item_completed，再写计划正文 message；这不是任务继续运行。
        true
    }
}

fn assistant_message_from_payload(
    row_type: &str,
    kind: &str,
    payload: &serde_json::Value,
) -> Option<String> {
    if !matches!(
        (row_type, kind),
        ("event_msg", "agent_message") | ("response_item", "message")
    ) {
        return None;
    }
    if row_type == "response_item"
        && payload.get("role").and_then(|value| value.as_str()) != Some("assistant")
    {
        return None;
    }
    payload
        .get("message")
        .and_then(|value| value.as_str())
        .or_else(|| payload.get("text").and_then(|value| value.as_str()))
        .or_else(|| message_content_text(payload.get("content")?))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn task_complete_message(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("last_agent_message")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn message_content_text(content: &serde_json::Value) -> Option<&str> {
    match content {
        serde_json::Value::String(value) => Some(value.as_str()),
        serde_json::Value::Array(items) => items.iter().find_map(|item| {
            item.get("text")
                .and_then(|value| value.as_str())
                .or_else(|| item.get("content").and_then(|value| value.as_str()))
        }),
        _ => None,
    }
}

struct UserInputFunctionCall {
    summary: String,
    content: String,
}

struct EscalatedFunctionCall {
    call_id: Option<String>,
    resolve_key: Option<String>,
    summary: String,
}

fn plan_item_completed(payload: &serde_json::Value) -> Option<UserInputFunctionCall> {
    let item = payload.get("item")?;
    if item.get("type").and_then(|value| value.as_str()) != Some("Plan") {
        return None;
    }
    let plan_text = item
        .get("text")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    // Codex 客户端会在 Plan item 完成后显示本地确认框，但不会写 request_user_input。
    // 这里把该稳定 JSONL 形态归一为等待输入，交给现有主状态链路展示。
    Some(UserInputFunctionCall {
        summary: "Codex 等待确认：Implement this plan?".to_string(),
        content: format!(
            "Implement this plan?\n\n{plan_text}\n\n1. Yes, implement this plan\n2. Yes, clear context and implement\n3. No, stay in Plan mode"
        ),
    })
}

fn request_user_input_function_call(payload: &serde_json::Value) -> Option<UserInputFunctionCall> {
    if payload.get("name").and_then(|value| value.as_str()) != Some("request_user_input") {
        return None;
    }

    // Codex Plan Mode 用 request_user_input 渲染选择题或文本输入，等价于等待用户输入。
    let question = payload
        .get("arguments")
        .and_then(|value| value.as_str())
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|arguments| request_user_input_detail(&arguments));
    let summary = question
        .as_ref()
        .map(|detail| detail.summary_question.as_str())
        .map(|question| format!("Codex 等待输入：{question}"))
        .unwrap_or_else(|| "Codex 正在等待用户输入".to_string());
    let content = question
        .map(|detail| detail.content)
        .unwrap_or_else(|| summary.clone());

    Some(UserInputFunctionCall { summary, content })
}

struct UserInputDetail {
    summary_question: String,
    content: String,
}

fn request_user_input_detail(arguments: &serde_json::Value) -> Option<UserInputDetail> {
    let questions = arguments.get("questions")?.as_array()?;
    let rendered_questions = questions
        .iter()
        .filter_map(render_request_user_input_question)
        .collect::<Vec<_>>();
    let summary_question = rendered_questions
        .iter()
        .find_map(|detail| (!detail.question.is_empty()).then(|| detail.question.clone()))?;
    let content = rendered_questions
        .into_iter()
        .map(|detail| detail.content)
        .collect::<Vec<_>>()
        .join("\n\n");

    Some(UserInputDetail {
        summary_question,
        content,
    })
}

struct RenderedUserInputQuestion {
    question: String,
    content: String,
}

fn render_request_user_input_question(
    question: &serde_json::Value,
) -> Option<RenderedUserInputQuestion> {
    let question_text = question
        .get("question")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let options = question
        .get("options")
        .and_then(|value| value.as_array())
        .map(|options| render_request_user_input_options(options))
        .filter(|value| !value.is_empty());
    let content = options
        .map(|options| format!("{question_text}\n\n{options}"))
        .unwrap_or_else(|| question_text.clone());

    Some(RenderedUserInputQuestion {
        question: question_text,
        content,
    })
}

fn render_request_user_input_options(options: &[serde_json::Value]) -> String {
    options
        .iter()
        .enumerate()
        .filter_map(|(index, option)| render_request_user_input_option(index + 1, option))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_request_user_input_option(index: usize, option: &serde_json::Value) -> Option<String> {
    let label = option
        .get("label")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let description = option
        .get("description")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Some(match description {
        Some(description) => format!("{index}. {label}\n{description}"),
        None => format!("{index}. {label}"),
    })
}

fn escalated_function_call(
    payload: &serde_json::Value,
    session_id: &str,
) -> Option<EscalatedFunctionCall> {
    let arguments = payload
        .get("arguments")
        .and_then(|value| value.as_str())
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())?;
    if arguments
        .get("sandbox_permissions")
        .and_then(|value| value.as_str())
        != Some("require_escalated")
    {
        return None;
    }

    let call_id = payload
        .get("call_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let resolve_key = call_id
        .as_deref()
        .map(|call_id| codex_permission_resolve_key(session_id, call_id));
    let tool_name = payload
        .get("name")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Tool");
    let command = arguments
        .get("justification")
        .and_then(|value| value.as_str())
        .or_else(|| arguments.get("cmd").and_then(|value| value.as_str()))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Codex 正在等待批准")
        .trim();

    Some(EscalatedFunctionCall {
        call_id,
        resolve_key,
        summary: format!("{tool_name}: {command}"),
    })
}

fn codex_permission_resolve_key(session_id: &str, call_id: &str) -> String {
    format!("codex_permission:{session_id}:{call_id}")
}

fn event_key_fragment(row: &CodexRow, kind: &str) -> String {
    row.payload
        .get("turn_id")
        .and_then(|value| value.as_str())
        .or_else(|| row.payload.get("call_id").and_then(|value| value.as_str()))
        .or(row.timestamp.as_deref())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!(
                "payload-{}",
                stable_hash(&format!("{kind}:{}", row.payload))
            )
        })
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn is_event_message_activity(kind: &str) -> bool {
    matches!(
        kind,
        "agent_message"
            | "token_count"
            | "patch_apply_end"
            | "web_search_end"
            | "image_generation_end"
    )
}

fn is_response_item_activity(kind: &str) -> bool {
    matches!(
        kind,
        "reasoning"
            | "message"
            | "function_call"
            | "function_call_output"
            | "custom_tool_call"
            | "custom_tool_call_output"
            | "web_search_call"
            | "tool_search_call"
            | "tool_search_output"
            | "image_generation_call"
    )
}

fn classification_for_abort_reason(
    reason: Option<&str>,
) -> (EventType, Option<CompletionReason>, Option<FailureReason>) {
    match reason {
        Some("timeout") | Some("request_timeout") => {
            (EventType::TaskFailed, None, Some(FailureReason::Timeout))
        }
        Some("context_window_exceeded") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::ContextWindowExceeded),
        ),
        Some("usage_limit_reached") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::UsageLimitReached),
        ),
        Some("server_overloaded") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::ServerOverloaded),
        ),
        Some("cyber_policy") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::PolicyBlocked),
        ),
        Some("response_stream_failed") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::ResponseStreamFailed),
        ),
        Some("connection_failed") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::ConnectionFailed),
        ),
        Some("quota_exceeded") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::QuotaExceeded),
        ),
        Some("internal_server_error") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::InternalServerError),
        ),
        Some("retry_limit") => (EventType::TaskFailed, None, Some(FailureReason::RetryLimit)),
        Some("sandbox_error") => (
            EventType::TaskFailed,
            None,
            Some(FailureReason::SandboxError),
        ),
        Some("fatal") => (EventType::TaskFailed, None, Some(FailureReason::Fatal)),
        Some("interrupted") => (
            EventType::AssistantMessageCompleted,
            Some(CompletionReason::Interrupted),
            None,
        ),
        None | Some(_) => (
            EventType::AssistantMessageCompleted,
            Some(CompletionReason::AbortedUnknown),
            None,
        ),
    }
}

fn severity_for_event_type(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::ApprovalRequested | EventType::InputRequested | EventType::TaskFailed => {
            "urgent"
        }
        _ => "info",
    }
}

fn summary_for_event_type(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::SessionStarted => "Codex task started",
        EventType::AssistantMessageCompleted => "Codex task completed",
        EventType::TaskFailed => "Codex task failed",
        EventType::SessionActivity => "Codex session activity",
        _ => "Codex session updated",
    }
}

fn stable_hash(text: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:x}")
}
