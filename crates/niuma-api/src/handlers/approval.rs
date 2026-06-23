use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::response::Response;
use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::approval_arbitration::{
    ApprovalFingerprint, ExpiredWatcherApproval, HookApprovalDecision, WatcherApprovalDecision,
};
use niuma_core::models::{
    ApprovalDecisionKind, ApprovalProxyStatus, ApprovalRequest, ApprovalStatus, EventType,
    NiumaEvent, ToolId,
};
use serde::Deserialize;
use serde_json::json;

use super::shared;
use crate::response::json_response;
use crate::state::AppState;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ApprovalRequestBody {
    request_id: String,
    tool: String,
    session_id: String,
    turn_id: String,
    tool_name: String,
    command: Option<String>,
    description: Option<String>,
    project_path: String,
    project_name: String,
    timeout_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ApprovalDecisionBody {
    request_id: String,
    decision: String,
    decided_by: String,
    decided_source: String,
    reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ApprovalReturnBody {
    request_id: String,
    returned_by: String,
    reason: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ApprovalHeartbeatBody {
    request_id: String,
    #[allow(dead_code)]
    source: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ApprovalRequestsQuery {
    status: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ApprovalDecisionQuery {
    request_id: Option<String>,
}

pub(crate) async fn post_approval_request(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<ApprovalRequestBody>(&body) {
        Ok(request) => request,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterFormat,
                    format!("请求体无法解析：{error}"),
                ),
            );
        }
    };
    let request = match build_approval_request(request) {
        Ok(request) => request,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };
    let mut hook_decision = HookApprovalDecision::AcceptHook;
    for fingerprint in hook_approval_fingerprints(&request) {
        let decision = state
            .approval_arbiter
            .lock()
            .expect("approval arbiter mutex poisoned")
            .on_hook_approval(fingerprint.clone(), Utc::now());
        log_hook_approval_arbitration(&request, &fingerprint, &decision);
        if decision == HookApprovalDecision::ReturnToCodex {
            hook_decision = HookApprovalDecision::ReturnToCodex;
        }
    }
    if hook_decision == HookApprovalDecision::ReturnToCodex {
        return json_response(
            200,
            ApiResponse::ok(json!({
                "request_id": request.id,
                "accepted": false,
                "ownership": "watcher_fallback",
                "hook_action": "return_to_codex",
                "status": "already_fallback"
            })),
        );
    }
    if let Err(error) = state.store.upsert_approval_request(request.clone()) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }

    let event = approval_event(
        &request,
        EventType::ApprovalRequested,
        "urgent",
        "approval-api",
    );
    if let Err(error) = state.mutation_service.append_events(vec![event]) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }

    json_response(
        200,
        ApiResponse::ok(json!({
            "request_id": request.id,
            "accepted": true,
            "ownership": "hook",
            "hook_action": "wait_for_decision",
            "status": request.status
        })),
    )
}

pub(super) fn is_codex_watcher_approval(event: &NiumaEvent) -> bool {
    event.tool == niuma_core::models::ToolKind::Codex
        && event.source == "codex-session-file"
        && event.event_type == EventType::ApprovalRequested
}

fn watcher_approval_fingerprint(event: &NiumaEvent) -> Option<ApprovalFingerprint> {
    let content = event.content.as_deref().unwrap_or(event.summary.as_str());
    let command = content.strip_prefix("exec_command: ").unwrap_or(content);
    let fingerprint_session_id = event
        .normalized_session_id
        .as_deref()
        .or(event.parent_session_id.as_deref())
        .unwrap_or(event.session_id.as_str());
    ApprovalFingerprint::from_parts(
        &event.project_path,
        Some(fingerprint_session_id),
        Some(command),
        None,
    )
}

fn hook_approval_fingerprints(request: &ApprovalRequest) -> Vec<ApprovalFingerprint> {
    let mut fingerprints = Vec::new();
    // Codex session watcher 会把 justification/cmd 都压成一段 exec_command 文本。
    // hook 侧同时登记说明文案和真实命令，保证两种 watcher 文本都能命中同一授权。
    for text in [request.description.as_deref(), request.command.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Some(fingerprint) = ApprovalFingerprint::from_parts(
            &request.project_path,
            Some(&request.session_id),
            Some(text),
            None,
        ) {
            if !fingerprints
                .iter()
                .any(|existing: &ApprovalFingerprint| existing.key == fingerprint.key)
            {
                fingerprints.push(fingerprint);
            }
        }
    }
    fingerprints
}

pub(super) async fn handle_watcher_approval_event(state: AppState, event: NiumaEvent) -> Response {
    match arbitrate_watcher_approval_event(&state, event) {
        WatcherApprovalApiOutcome::Apply(event) => append_events_response(&state, vec![event]),
        WatcherApprovalApiOutcome::Delayed { delay } => json_response(
            200,
            ApiResponse::ok(json!({
                "accepted": true,
                "delayed": true,
                "reason": "waiting_for_hook_approval",
                "delay_ms": delay.as_millis()
            })),
        ),
        WatcherApprovalApiOutcome::Suppressed { reason } => json_response(
            200,
            ApiResponse::ok(json!({
                "accepted": true,
                "delayed": false,
                "applied": false,
                "suppressed": true,
                "reason": reason
            })),
        ),
    }
}

pub(super) enum WatcherApprovalApiOutcome {
    Apply(NiumaEvent),
    Delayed { delay: std::time::Duration },
    Suppressed { reason: &'static str },
}

pub(super) fn arbitrate_watcher_approval_event(
    state: &AppState,
    event: NiumaEvent,
) -> WatcherApprovalApiOutcome {
    let Some(fingerprint) = watcher_approval_fingerprint(&event) else {
        log_watcher_approval_without_fingerprint(&event);
        return WatcherApprovalApiOutcome::Apply(event);
    };
    let decision = state
        .approval_arbiter
        .lock()
        .expect("approval arbiter mutex poisoned")
        .on_watcher_approval(fingerprint.clone(), event.clone(), Utc::now());
    log_watcher_approval_arbitration(&event, &fingerprint, &decision);
    match decision {
        WatcherApprovalDecision::EmitNow(event) => WatcherApprovalApiOutcome::Apply(event),
        WatcherApprovalDecision::Suppress => WatcherApprovalApiOutcome::Suppressed {
            reason: "hook_approval_already_emitted",
        },
        WatcherApprovalDecision::Delay(delay) => {
            spawn_watcher_candidate_expiry(state.clone(), fingerprint, delay);
            WatcherApprovalApiOutcome::Delayed { delay }
        }
    }
}

fn log_hook_approval_arbitration(
    request: &ApprovalRequest,
    fingerprint: &ApprovalFingerprint,
    decision: &HookApprovalDecision,
) {
    // 诊断 hook/watcher 双来源是否生成了同一个仲裁指纹；只在授权请求发生时输出。
    eprintln!(
        "NiumaNotifier approval arbiter hook request_id={} project_path={} session_id={} command={} description={} fingerprint_key={} fingerprint_basis={:?} decision={:?}",
        request.id,
        log_value(&request.project_path),
        log_value(&request.session_id),
        log_optional(request.command.as_deref()),
        log_optional(request.description.as_deref()),
        fingerprint.key,
        fingerprint.basis,
        decision
    );
}

fn log_watcher_approval_arbitration(
    event: &NiumaEvent,
    fingerprint: &ApprovalFingerprint,
    decision: &WatcherApprovalDecision,
) {
    // watcher event 已被 on_watcher_approval 消费；这里只记录指纹和决策，避免复制事件结构。
    eprintln!(
        "NiumaNotifier approval arbiter watcher project_path={} session_id={} parent_session_id={} normalized_session_id={} fingerprint_session_id={} fingerprint_key={} fingerprint_basis={:?} decision={}",
        log_value(&fingerprint.project_path),
        log_value(&event.session_id),
        log_optional(event.parent_session_id.as_deref()),
        log_optional(event.normalized_session_id.as_deref()),
        log_optional(fingerprint.session_id.as_deref()),
        fingerprint.key,
        fingerprint.basis,
        watcher_decision_label(decision)
    );
}

fn log_watcher_approval_without_fingerprint(event: &NiumaEvent) {
    let content = event.content.as_deref().unwrap_or(event.summary.as_str());
    eprintln!(
        "NiumaNotifier approval arbiter watcher no_fingerprint event_id={} project_path={} session_id={} source={} content={}",
        event.id,
        log_value(&event.project_path),
        log_value(&event.session_id),
        event.source,
        log_value(content)
    );
}

fn watcher_decision_label(decision: &WatcherApprovalDecision) -> &'static str {
    match decision {
        WatcherApprovalDecision::Delay(_) => "delay",
        WatcherApprovalDecision::EmitNow(_) => "emit_now",
        WatcherApprovalDecision::Suppress => "suppress",
    }
}

fn log_optional(value: Option<&str>) -> String {
    value.map(log_value).unwrap_or_else(|| "-".to_string())
}

fn log_value(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 180 {
        normalized
    } else {
        format!("{}...", normalized.chars().take(180).collect::<String>())
    }
}

fn append_events_response(state: &AppState, events: Vec<NiumaEvent>) -> Response {
    match state.mutation_service.append_events(events) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "accepted": true,
                "delayed": false,
                "applied": true,
                "event_count": result.state.events.len(),
                "session_count": result.state.runtime_states.len()
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

fn spawn_watcher_candidate_expiry(
    state: AppState,
    fingerprint: ApprovalFingerprint,
    delay: std::time::Duration,
) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let expired = state
            .approval_arbiter
            .lock()
            .expect("approval arbiter mutex poisoned")
            .expire_candidate(&fingerprint, Utc::now());
        if let Some(ExpiredWatcherApproval::Emit(event)) = expired {
            let _ = state.mutation_service.append_events(vec![event]);
        }
    });
}

pub(crate) async fn get_approval_requests(
    State(state): State<AppState>,
    Query(query): Query<ApprovalRequestsQuery>,
) -> Response {
    let status = match query.status.as_deref() {
        Some(value) => match parse_approval_status(value) {
            Ok(status) => Some(status),
            Err(message) => {
                return json_response(
                    200,
                    ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
                );
            }
        },
        None => None,
    };
    match state.store.approval_requests() {
        Ok(mut requests) => {
            if let Some(status) = status {
                requests.retain(|request| request.status == status);
            }
            json_response(200, ApiResponse::ok(json!({ "list": requests })))
        }
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_approval_decision(
    State(state): State<AppState>,
    Query(query): Query<ApprovalDecisionQuery>,
) -> Response {
    let Some(request_id) = query
        .request_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, "request_id 不能为空"),
        );
    };
    match state.store.approval_request(request_id) {
        Ok(Some(request)) => json_response(200, ApiResponse::ok(approval_response(&request, None))),
        Ok(None) => json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("授权请求不存在：{request_id}"),
            ),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn post_approval_decision(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<ApprovalDecisionBody>(&body) {
        Ok(request) => request,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterFormat,
                    format!("请求体无法解析：{error}"),
                ),
            );
        }
    };
    let decision = match parse_approval_decision(&request.decision) {
        Ok(decision) => decision,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };
    let request_id = match required_trimmed(&request.request_id, "request_id") {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };
    let decided_by = match required_trimmed(&request.decided_by, "decided_by") {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };
    let decided_source = match required_trimmed(&request.decided_source, "decided_source") {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };

    let reason = shared::trim_optional_string(request.reason);
    match state.store.decide_approval(
        &request_id,
        decision,
        &decided_by,
        &decided_source,
        reason,
        Utc::now(),
    ) {
        Ok(result) => {
            if result.accepted {
                let event = approval_event(
                    &result.request,
                    EventType::ApprovalResolved,
                    "info",
                    "approval-api",
                );
                if let Err(error) = state.mutation_service.append_events(vec![event]) {
                    return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
                }
            }
            json_response(
                200,
                ApiResponse::ok(approval_response(&result.request, Some(result.accepted))),
            )
        }
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

pub(crate) async fn post_approval_return_to_codex(
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<ApprovalReturnBody>(&body) {
        Ok(request) => request,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterFormat,
                    format!("请求体无法解析：{error}"),
                ),
            );
        }
    };
    let request_id = match required_trimmed(&request.request_id, "request_id") {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };
    let returned_by = match required_trimmed(&request.returned_by, "returned_by") {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };
    let reason = match required_trimmed(&request.reason, "reason") {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };

    match state.store.return_approval_to_codex(
        &request_id,
        &returned_by,
        "timeout",
        &reason,
        Utc::now(),
    ) {
        Ok(result) => {
            if result.accepted {
                let event = approval_event(
                    &result.request,
                    EventType::ApprovalReturnedToCodex,
                    "info",
                    "approval-api",
                );
                if let Err(error) = state.mutation_service.append_events(vec![event]) {
                    return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
                }
            }
            json_response(
                200,
                ApiResponse::ok(approval_response(&result.request, Some(result.accepted))),
            )
        }
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

pub(crate) async fn post_approval_heartbeat(
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<ApprovalHeartbeatBody>(&body) {
        Ok(request) => request,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterFormat,
                    format!("请求体无法解析：{error}"),
                ),
            );
        }
    };
    let request_id = match required_trimmed(&request.request_id, "request_id") {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
            );
        }
    };

    match state
        .store
        .heartbeat_approval_proxy(&request_id, Utc::now())
    {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(approval_response(&result.request, Some(result.accepted))),
        ),
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

fn build_approval_request(body: ApprovalRequestBody) -> Result<ApprovalRequest, String> {
    let now = Utc::now();
    Ok(ApprovalRequest {
        id: required_trimmed(&body.request_id, "request_id")?,
        tool: ToolId::from_id(required_trimmed(&body.tool, "tool")?),
        session_id: required_trimmed(&body.session_id, "session_id")?,
        turn_id: required_trimmed(&body.turn_id, "turn_id")?,
        tool_name: required_trimmed(&body.tool_name, "tool_name")?,
        command: shared::trim_optional_string(body.command),
        description: shared::trim_optional_string(body.description),
        project_path: required_trimmed(&body.project_path, "project_path")?,
        project_name: required_trimmed(&body.project_name, "project_name")?,
        status: ApprovalStatus::Pending,
        decided_by: None,
        decided_source: None,
        reason: None,
        created_at: now,
        updated_at: now,
        proxy_timeout_seconds: body.timeout_seconds.unwrap_or(600),
        proxy_status: ApprovalProxyStatus::Active,
        last_heartbeat_at: Some(now),
        proxy_lost_at: None,
    })
}

fn approval_event(
    request: &ApprovalRequest,
    event_type: EventType,
    severity: &str,
    source: &str,
) -> NiumaEvent {
    let now = Utc::now();
    let approval_ref = approval_ref(&request.id);
    NiumaEvent {
        id: format!(
            "event_{}_{}_{}",
            event_type_key(&event_type),
            sanitize_event_id_part(&request.id),
            now.timestamp_millis()
        ),
        dedupe_key: format!("{}:{}", event_type_key(&event_type), request.id),
        source: source.to_string(),
        tool: request.tool.clone(),
        session_id: request.session_id.clone(),
        parent_session_id: None,
        normalized_session_id: None,
        session_scope: None,
        agent_nickname: None,
        agent_role: None,
        project_path: request.project_path.clone(),
        project_name: request.project_name.clone(),
        event_type,
        severity: severity.to_string(),
        summary: approval_summary(request),
        content: request
            .description
            .clone()
            .or_else(|| request.command.clone())
            .or_else(|| Some(request.tool_name.clone())),
        error_message: None,
        attention_resolve_key: Some(approval_ref.clone()),
        completion_reason: None,
        failure_reason: None,
        payload_ref: Some(approval_ref),
        created_at: now,
    }
}

pub(crate) fn approval_event_for_internal(
    request: &ApprovalRequest,
    event_type: EventType,
    severity: &str,
    source: &str,
) -> NiumaEvent {
    approval_event(request, event_type, severity, source)
}

fn approval_response(request: &ApprovalRequest, accepted: Option<bool>) -> serde_json::Value {
    let mut value = json!({
        "request_id": request.id,
        "status": request.status,
        "decision": decision_value_for_status(&request.status),
        "decided_by": request.decided_by,
        "decided_source": request.decided_source,
        "reason": request.reason,
        "proxy_status": request.proxy_status
    });
    if let Some(accepted) = accepted {
        value["accepted"] = json!(accepted);
    }
    value
}

fn decision_value_for_status(status: &ApprovalStatus) -> serde_json::Value {
    match status {
        ApprovalStatus::Allowed => json!("allow"),
        ApprovalStatus::Denied => json!("deny"),
        ApprovalStatus::Pending | ApprovalStatus::ReturnedToCodex => serde_json::Value::Null,
    }
}

fn parse_approval_decision(value: &str) -> Result<ApprovalDecisionKind, String> {
    match value.trim() {
        "allow" => Ok(ApprovalDecisionKind::Allow),
        "deny" => Ok(ApprovalDecisionKind::Deny),
        _ => Err("decision 仅支持 allow 或 deny".to_string()),
    }
}

fn parse_approval_status(value: &str) -> Result<ApprovalStatus, String> {
    match value.trim() {
        "pending" => Ok(ApprovalStatus::Pending),
        "allowed" => Ok(ApprovalStatus::Allowed),
        "denied" => Ok(ApprovalStatus::Denied),
        "returned_to_codex" => Ok(ApprovalStatus::ReturnedToCodex),
        _ => Err("status 仅支持 pending、allowed、denied 或 returned_to_codex".to_string()),
    }
}

fn required_trimmed(value: &str, field: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{field} 不能为空"));
    }
    Ok(value.to_string())
}

fn approval_ref(request_id: &str) -> String {
    format!("approval:{request_id}")
}

fn approval_summary(request: &ApprovalRequest) -> String {
    match request.command.as_deref() {
        Some(command) if !command.trim().is_empty() => {
            format!("{}: {command}", request.tool_name)
        }
        _ => request
            .description
            .clone()
            .unwrap_or_else(|| request.tool_name.clone()),
    }
}

fn event_type_key(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::ApprovalRequested => "approval_requested",
        EventType::ApprovalResolved => "approval_resolved",
        EventType::ApprovalReturnedToCodex => "approval_returned_to_codex",
        _ => "approval_event",
    }
}

fn sanitize_event_id_part(value: &str) -> String {
    // 事件 id 只需要稳定可读；替换特殊字符，避免 SSE id 出现难处理的分隔符。
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}
