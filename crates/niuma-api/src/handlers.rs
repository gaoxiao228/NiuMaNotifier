use axum::body::Bytes;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::approval_arbitration::{
    ApprovalFingerprint, ExpiredWatcherApproval, HookApprovalDecision, WatcherApprovalDecision,
};
use niuma_core::codex_hook::{
    install_codex_hook, uninstall_codex_hook, CodexHookCommand, CodexHookStatus,
};
use niuma_core::config::codex_home;
use niuma_core::dashboard::DashboardService;
use niuma_core::main_state::MainStateService;
use niuma_core::models::{
    ApprovalDecisionKind, ApprovalProxyStatus, ApprovalRequest, ApprovalStatus, EventType,
    NiumaEvent, ToolId, ToolKind,
};
use niuma_core::notification_store::{NotificationRecordStatus, PluginNotificationResult};
use niuma_core::plugin::{
    import_external_plugin_dir, listener_config_after_plugin_removed, plugin_uses_listener_config,
    remove_external_plugin, resolve_plugin_config, save_plugin_enabled_state,
    validate_plugin_config, PluginKind, PluginManifest, PluginRegistry, PluginSource,
    ToolPluginInfo, BUILTIN_CODEX_PLUGIN_ID,
};
use niuma_core::runtime_event::StateChangeReason;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::response::json_response;
use crate::state::AppState;
use crate::tool_sessions::{capped_limit, ToolSessionListQuery};

const RESET_CONFIRMATION: &str = "RESET_NIUMA_STATE";

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct EventsQuery {
    limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ResetStateRequest {
    confirm: String,
    #[allow(dead_code)]
    reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginEventsRequest {
    plugin_id: String,
    events: Vec<NiumaEvent>,
}

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

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionListQuery {
    tool: Option<String>,
    include_subagents: Option<bool>,
    active_only: Option<bool>,
    limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionDetailQuery {
    tool: Option<String>,
    session_id: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginImportRequest {
    source_dir: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginRemoveRequest {
    plugin_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginEnabledRequest {
    plugin_id: String,
    enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginConfigQuery {
    plugin_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginConfigSaveRequest {
    plugin_id: String,
    config: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginActionRequest {
    plugin_id: String,
    action_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginNotificationResultRequest {
    plugin_id: String,
    event_id: String,
    status: String,
    title: Option<String>,
    body: Option<String>,
    reason: Option<String>,
    error_message: Option<String>,
    sent_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginNotificationTestResultRequest {
    plugin_id: String,
    test_id: String,
    status: String,
    title: Option<String>,
    body: Option<String>,
    error_message: Option<String>,
    sent_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ListenerToolView {
    id: String,
    plugin_id: String,
    display_name: String,
    enabled: bool,
    source: String,
    icon_url: Option<String>,
}

pub(crate) async fn get_main_state(State(state): State<AppState>) -> Response {
    match MainStateService::new(state.store).current_state(Utc::now()) {
        Ok(main_state) => json_response(200, ApiResponse::ok(json!({ "state": main_state }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_events(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> Response {
    match DashboardService::new(state.store).recent_events(query.limit.unwrap_or(50)) {
        Ok(events) => json_response(200, ApiResponse::ok(json!({ "list": events }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_runtime_state_list(State(state): State<AppState>) -> Response {
    match DashboardService::new(state.store).runtime_state_list() {
        Ok(items) => json_response(200, ApiResponse::ok(json!({ "list": items }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_session_list(
    State(state): State<AppState>,
    query: Result<Query<SessionListQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterType,
                    format!("查询参数类型错误（limit/include_subagents/active_only）：{error}"),
                ),
            );
        }
    };

    let query = ToolSessionListQuery {
        // 空 tool 等价于未传，避免生成不可见的空自定义工具过滤条件。
        tool: query
            .tool
            .map(|tool| tool.trim().to_string())
            .filter(|tool| !tool.is_empty()),
        include_subagents: query.include_subagents.unwrap_or(false),
        active_only: query.active_only.unwrap_or(false),
        limit: query.limit,
    };

    match state.tool_sessions.list(query) {
        Ok(items) => json_response(200, ApiResponse::ok(json!({ "list": items }))),
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

pub(crate) async fn get_session_detail(
    State(state): State<AppState>,
    query: Result<Query<SessionDetailQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterType,
                    format!("查询参数类型错误：{error}"),
                ),
            );
        }
    };

    let Some(tool) = required_query_value(query.tool) else {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, "tool 不能为空"),
        );
    };
    let Some(session_id) = required_query_value(query.session_id) else {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, "session_id 不能为空"),
        );
    };
    let limit = match capped_limit(query.limit) {
        Ok(limit) => limit,
        Err(error) => {
            // limit 属于业务参数范围校验，按统一 envelope 返回业务失败。
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
            );
        }
    };

    let tool = ToolKind::from_id(tool);
    if state
        .tool_sessions
        .find_session(&tool, &session_id)
        .is_none()
    {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, "session_id 不存在"),
        );
    }

    match state
        .tool_sessions
        .detail(&tool, &session_id, limit, query.cursor)
    {
        Ok(detail) => json_response(200, ApiResponse::ok(json!(detail))),
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

fn required_query_value(value: Option<String>) -> Option<String> {
    // GET 查询参数传空字符串时按业务参数缺失处理，保持 200 + 业务失败 envelope。
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) async fn post_event(State(state): State<AppState>, body: Bytes) -> Response {
    match serde_json::from_slice::<NiumaEvent>(&body) {
        Ok(event) => {
            if is_codex_watcher_approval(&event) {
                return handle_watcher_approval_event(state, event).await;
            }
            append_events_response(&state, vec![event])
        }
        Err(error) => json_response(
            400,
            ApiResponse::fail(
                ApiErrorCode::ParameterFormat,
                format!("请求体无法解析：{error}"),
            ),
        ),
    }
}

pub(crate) async fn post_plugin_events(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginEventsRequest>(&body) {
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
    let registry = plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id) else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };
    let Some(plugin_tool) = plugin.tool_id.as_ref() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("插件 {} 不能上报工具事件", plugin.id),
            ),
        );
    };
    if let Some(event) = request
        .events
        .iter()
        .find(|event| &event.tool != plugin_tool)
    {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!(
                    "插件 {} 只能上报 {} 事件，收到 {}",
                    plugin.id,
                    plugin_tool.as_str(),
                    event.tool.as_str()
                ),
            ),
        );
    }

    let mut immediate_events = Vec::new();
    let mut delayed_count = 0usize;
    let mut suppressed_count = 0usize;
    for event in request.events {
        if is_codex_watcher_approval(&event) {
            match arbitrate_watcher_approval_event(&state, event) {
                WatcherApprovalApiOutcome::Apply(event) => immediate_events.push(event),
                WatcherApprovalApiOutcome::Delayed { .. } => delayed_count += 1,
                WatcherApprovalApiOutcome::Suppressed { .. } => suppressed_count += 1,
            }
        } else {
            immediate_events.push(event);
        }
    }

    if immediate_events.is_empty() {
        return json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": plugin.id,
                "event_count": 0,
                "applied_count": 0,
                "session_count": 0,
                "delayed_count": delayed_count,
                "suppressed_count": suppressed_count
            })),
        );
    }

    match state.mutation_service.append_events(immediate_events) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": plugin.id,
                "event_count": result.state.events.len(),
                "applied_count": result.applied_events.len(),
                "session_count": result.state.runtime_states.len(),
                "delayed_count": delayed_count,
                "suppressed_count": suppressed_count
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
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

fn is_codex_watcher_approval(event: &NiumaEvent) -> bool {
    event.tool == niuma_core::models::ToolKind::Codex
        && event.source == "codex-session-file"
        && event.event_type == EventType::ApprovalRequested
}

fn watcher_approval_fingerprint(event: &NiumaEvent) -> Option<ApprovalFingerprint> {
    let content = event.content.as_deref().unwrap_or(event.summary.as_str());
    let command = content.strip_prefix("exec_command: ").unwrap_or(content);
    ApprovalFingerprint::from_parts(
        &event.project_path,
        Some(&event.session_id),
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

async fn handle_watcher_approval_event(state: AppState, event: NiumaEvent) -> Response {
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

enum WatcherApprovalApiOutcome {
    Apply(NiumaEvent),
    Delayed { delay: std::time::Duration },
    Suppressed { reason: &'static str },
}

fn arbitrate_watcher_approval_event(
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
        .on_watcher_approval(fingerprint.clone(), event, Utc::now());
    log_watcher_approval_arbitration(&fingerprint, &decision);
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
    fingerprint: &ApprovalFingerprint,
    decision: &WatcherApprovalDecision,
) {
    // watcher event 已被 on_watcher_approval 消费；这里只记录指纹和决策，避免复制事件结构。
    eprintln!(
        "NiumaNotifier approval arbiter watcher project_path={} session_id={} fingerprint_key={} fingerprint_basis={:?} decision={}",
        log_value(&fingerprint.project_path),
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

    let reason = trim_optional_string(request.reason);
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
        command: trim_optional_string(body.command),
        description: trim_optional_string(body.description),
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

pub(crate) async fn post_plugin_notification_result(
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<PluginNotificationResultRequest>(&body) {
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
    match save_plugin_notification_result(&state, request) {
        Ok(record_id) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "record_id": record_id
            })),
        ),
        Err(PluginNotificationResultError::Business(message)) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
        ),
        Err(PluginNotificationResultError::System(message)) => {
            json_response(500, ApiResponse::fail(ApiErrorCode::System, message))
        }
    }
}

pub(crate) async fn post_plugin_notification_test_result(
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<PluginNotificationTestResultRequest>(&body) {
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
    match save_plugin_notification_test_result(&state, request) {
        Ok(record_id) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "record_id": record_id
            })),
        ),
        Err(PluginNotificationResultError::Business(message)) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, message),
        ),
        Err(PluginNotificationResultError::System(message)) => {
            json_response(500, ApiResponse::fail(ApiErrorCode::System, message))
        }
    }
}

pub(crate) async fn get_plugins(State(state): State<AppState>) -> Response {
    match plugin_management_items(&state) {
        Ok(plugins) => json_response(200, ApiResponse::ok(json!({ "list": plugins }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

enum PluginNotificationResultError {
    Business(String),
    System(String),
}

fn save_plugin_notification_result(
    state: &AppState,
    request: PluginNotificationResultRequest,
) -> Result<String, PluginNotificationResultError> {
    let plugin_id = request.plugin_id.trim();
    if plugin_id.is_empty() {
        return Err(PluginNotificationResultError::Business(
            "plugin_id 不能为空".to_string(),
        ));
    }
    let event_id = request.event_id.trim();
    if event_id.is_empty() {
        return Err(PluginNotificationResultError::Business(
            "event_id 不能为空".to_string(),
        ));
    }

    let registry = plugin_registry(state);
    let Some(plugin) = registry.plugin_by_id(plugin_id) else {
        return Err(PluginNotificationResultError::Business(format!(
            "未知插件：{plugin_id}"
        )));
    };
    if plugin.kind != PluginKind::Notification {
        return Err(PluginNotificationResultError::Business(format!(
            "插件 {plugin_id} 不是通知插件"
        )));
    }

    let event = state
        .store
        .public_event_by_id(event_id)
        .map_err(PluginNotificationResultError::System)?
        .ok_or_else(|| {
            PluginNotificationResultError::Business(format!("事件不存在：{event_id}"))
        })?;
    let status = parse_plugin_notification_status(&request.status)?;
    let sent_at = match (status.clone(), request.sent_at.as_deref()) {
        (NotificationRecordStatus::Sent, Some(value)) => {
            Some(parse_rfc3339_time(value, "sent_at")?)
        }
        (NotificationRecordStatus::Sent, None) => Some(Utc::now()),
        (NotificationRecordStatus::Failed, _) => None,
        _ => {
            return Err(PluginNotificationResultError::Business(
                "status 仅支持 sent 或 failed".to_string(),
            ));
        }
    };
    let record_id = plugin_notification_record_id(plugin_id, event_id);
    let result = PluginNotificationResult {
        id: record_id.clone(),
        plugin_id: plugin_id.to_string(),
        event_id: event_id.to_string(),
        event_type: event.event_type,
        status,
        title: trim_optional_string(request.title),
        body: trim_optional_string(request.body),
        reason: trim_optional_string(request.reason),
        error_message: trim_optional_string(request.error_message),
        created_at: Utc::now(),
        sent_at,
    };
    state
        .store
        .save_plugin_notification_result(&result)
        .map_err(PluginNotificationResultError::System)?;
    Ok(record_id)
}

fn save_plugin_notification_test_result(
    state: &AppState,
    request: PluginNotificationTestResultRequest,
) -> Result<String, PluginNotificationResultError> {
    let plugin_id = request.plugin_id.trim();
    if plugin_id.is_empty() {
        return Err(PluginNotificationResultError::Business(
            "plugin_id 不能为空".to_string(),
        ));
    }
    let test_id = request.test_id.trim();
    if test_id.is_empty() {
        return Err(PluginNotificationResultError::Business(
            "test_id 不能为空".to_string(),
        ));
    }

    let registry = plugin_registry(state);
    let Some(plugin) = registry.plugin_by_id(plugin_id) else {
        return Err(PluginNotificationResultError::Business(format!(
            "未知插件：{plugin_id}"
        )));
    };
    if plugin.kind != PluginKind::Notification {
        return Err(PluginNotificationResultError::Business(format!(
            "插件 {plugin_id} 不是通知插件"
        )));
    }

    let status = parse_plugin_notification_status(&request.status)?;
    let sent_at = match (status.clone(), request.sent_at.as_deref()) {
        (NotificationRecordStatus::Sent, Some(value)) => {
            Some(parse_rfc3339_time(value, "sent_at")?)
        }
        (NotificationRecordStatus::Sent, None) => Some(Utc::now()),
        (NotificationRecordStatus::Failed, _) => None,
        _ => {
            return Err(PluginNotificationResultError::Business(
                "status 仅支持 sent 或 failed".to_string(),
            ));
        }
    };
    let record_id = plugin_notification_test_record_id(plugin_id, test_id);
    let result = PluginNotificationResult {
        id: record_id.clone(),
        plugin_id: plugin_id.to_string(),
        event_id: test_id.to_string(),
        event_type: EventType::SessionActivity,
        status,
        title: trim_optional_string(request.title),
        body: trim_optional_string(request.body),
        reason: Some("manual_test".to_string()),
        error_message: trim_optional_string(request.error_message),
        created_at: Utc::now(),
        sent_at,
    };
    state
        .store
        .save_plugin_notification_result(&result)
        .map_err(PluginNotificationResultError::System)?;
    Ok(record_id)
}

fn parse_plugin_notification_status(
    value: &str,
) -> Result<NotificationRecordStatus, PluginNotificationResultError> {
    match value.trim() {
        "sent" => Ok(NotificationRecordStatus::Sent),
        "failed" => Ok(NotificationRecordStatus::Failed),
        _ => Err(PluginNotificationResultError::Business(
            "status 仅支持 sent 或 failed".to_string(),
        )),
    }
}

fn parse_rfc3339_time(
    value: &str,
    field: &str,
) -> Result<chrono::DateTime<Utc>, PluginNotificationResultError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| PluginNotificationResultError::Business(format!("{field} 格式无效")))
}

fn trim_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn plugin_notification_record_id(plugin_id: &str, event_id: &str) -> String {
    format!("plugin_notification:{plugin_id}:{event_id}")
}

fn plugin_notification_test_record_id(plugin_id: &str, test_id: &str) -> String {
    format!("plugin_notification_test:{plugin_id}:{test_id}")
}

pub(crate) async fn import_plugin(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginImportRequest>(&body) {
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
    let config = match state.store.listener_config() {
        Ok(config) => config,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    let runtime_states = match state.store.plugin_runtime_states() {
        Ok(states) => states,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    let plugin_enabled_map = match state.store.plugin_enabled_map() {
        Ok(map) => map,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };

    match import_external_plugin_dir(
        std::path::Path::new(&request.source_dir),
        &state.plugin_dir,
        &config,
        &plugin_enabled_map,
        &runtime_states,
    ) {
        Ok(mut result) => {
            let registry = plugin_registry(&state);
            let Some(manifest) = registry.plugin_by_id(&result.plugin.id).cloned() else {
                return json_response(
                    500,
                    ApiResponse::fail(
                        ApiErrorCode::System,
                        format!("插件导入后未被发现：{}", result.plugin.id),
                    ),
                );
            };
            if let Err(error) =
                save_plugin_enabled_state(&state.store, &state.mutation_service, &manifest, true)
            {
                return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
            }
            let plugins = match plugin_management_items(&state) {
                Ok(plugins) => plugins,
                Err(error) => {
                    return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
                }
            };
            if let Some(plugin) = plugins.iter().find(|item| item.id == result.plugin.id) {
                result.plugin = plugin.clone();
            }
            result.plugins = plugins;
            state
                .runtime_events
                .publish_state_changed(StateChangeReason::ListenerConfigChanged);
            json_response(200, ApiResponse::ok(json!(result)))
        }
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

pub(crate) async fn remove_plugin(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginRemoveRequest>(&body) {
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
    let registry = plugin_registry(&state);
    if PluginRegistry::with_builtin_plugins()
        .plugin_by_id(&request.plugin_id)
        .is_some()
    {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("不能移除内置插件：{}", request.plugin_id),
            ),
        );
    }
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };
    if plugin.source == PluginSource::Builtin {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("不能移除内置插件：{}", request.plugin_id),
            ),
        );
    }

    let config = match listener_config_after_plugin_removed(
        &state.store,
        &state.mutation_service,
        &plugin,
    ) {
        Ok(config) => config,
        Err(error) => {
            return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
        }
    };
    if let Err(error) = state.store.remove_plugin_runtime_state(&plugin.id) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }
    let runtime_states = match state.store.plugin_runtime_states() {
        Ok(states) => states,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    let plugin_enabled_map = match state.store.plugin_enabled_map() {
        Ok(map) => map,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };

    match remove_external_plugin(
        &request.plugin_id,
        &state.plugin_dir,
        &config,
        &plugin_enabled_map,
        &runtime_states,
    ) {
        Ok(result) => {
            state
                .runtime_events
                .publish_state_changed(StateChangeReason::ListenerConfigChanged);
            json_response(200, ApiResponse::ok(json!(result)))
        }
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

pub(crate) async fn set_plugin_enabled(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginEnabledRequest>(&body) {
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
    let registry = plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };

    let uses_listener_config = plugin_uses_listener_config(&plugin);
    if let Err(error) = save_plugin_enabled_state(
        &state.store,
        &state.mutation_service,
        &plugin,
        request.enabled,
    ) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }
    if !uses_listener_config {
        // 非 event_watcher 插件的启用状态不会触发 listener 变更，需要显式刷新运行管理。
        state
            .runtime_events
            .publish_state_changed(StateChangeReason::PluginConfigChanged);
    }

    match plugin_management_items(&state) {
        Ok(plugins) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "plugin_id": request.plugin_id,
                "enabled": request.enabled,
                "plugins": plugins
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn run_plugin_action(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginActionRequest>(&body) {
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
    let registry = plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id) else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };
    if plugin.id != BUILTIN_CODEX_PLUGIN_ID {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("插件 {} 不支持管理动作", request.plugin_id),
            ),
        );
    }

    // 插件管理动作走后端 allowlist，避免外部插件通过 manifest 注入任意本机命令。
    let action_result = match request.action_id.as_str() {
        "codex_hook_install" => {
            install_codex_hook(&codex_home(), codex_hook_command_mode()).map(|status| {
                (
                    "Hook 已安装，请在 Codex 中执行 /hooks 信任 Niuma Hook",
                    status,
                )
            })
        }
        "codex_hook_uninstall" => {
            uninstall_codex_hook(&codex_home()).map(|status| ("Hook 已移除", status))
        }
        _ => Err(format!(
            "未知插件动作：{} / {}",
            request.plugin_id, request.action_id
        )),
    };
    let (message, status) = match action_result {
        Ok(result) => result,
        Err(error)
            if request.action_id != "codex_hook_install"
                && request.action_id != "codex_hook_uninstall" =>
        {
            return json_response(
                200,
                ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
            );
        }
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    match plugin_management_items(&state) {
        Ok(plugins) => json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": request.plugin_id,
                "action_id": request.action_id,
                "message": message,
                "status": codex_hook_status_json(status),
                "plugins": plugins
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_plugin_config(
    State(state): State<AppState>,
    Query(query): Query<PluginConfigQuery>,
) -> Response {
    let registry = plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&query.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", query.plugin_id),
            ),
        );
    };
    match resolved_plugin_config(&state, &plugin) {
        Ok(config) => json_response(
            200,
            ApiResponse::ok(json!({
                "plugin_id": query.plugin_id,
                "config": config,
                "config_schema": plugin.config_schema
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

fn codex_hook_status_json(status: CodexHookStatus) -> serde_json::Value {
    json!(status)
}

fn codex_hook_command_mode() -> CodexHookCommand {
    if niuma_core::platform::executable::command_on_path("niuma") {
        CodexHookCommand::Installed
    } else {
        CodexHookCommand::Dev {
            manifest_path: repo_manifest_path(),
        }
    }
}

fn repo_manifest_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("Cargo.toml")
}

pub(crate) async fn save_plugin_config(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<PluginConfigSaveRequest>(&body) {
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
    let registry = plugin_registry(&state);
    let Some(plugin) = registry.plugin_by_id(&request.plugin_id).cloned() else {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                format!("未知插件：{}", request.plugin_id),
            ),
        );
    };
    let Some(config) = request.config.as_object().cloned() else {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, "config 必须是对象"),
        );
    };
    if let Err(error) = validate_plugin_config(&plugin, &config) {
        return json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        );
    }
    if let Err(error) = state.store.save_plugin_config(&plugin.id, &config) {
        return json_response(500, ApiResponse::fail(ApiErrorCode::System, error));
    }
    state
        .runtime_events
        .publish_state_changed(StateChangeReason::PluginConfigChanged);
    match resolved_plugin_config(&state, &plugin) {
        Ok(saved_config) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "plugin_id": request.plugin_id,
                "config": saved_config,
                "config_schema": plugin.config_schema
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn dismiss_blocker(State(state): State<AppState>) -> Response {
    match state.mutation_service.dismiss_active_blocker() {
        Ok(Some(result)) => json_response(
            200,
            ApiResponse::ok(json!({
                "dismissed": true,
                "dismissed_count": result.dismissed_count,
                "event": result.event
            })),
        ),
        Ok(None) => json_response(
            200,
            ApiResponse::ok(json!({
                "dismissed": false,
                "dismissed_count": 0
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn reset_state(State(state): State<AppState>, body: Bytes) -> Response {
    let request = match serde_json::from_slice::<ResetStateRequest>(&body) {
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
    if request.confirm != RESET_CONFIRMATION {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                "confirm 必须为 RESET_NIUMA_STATE",
            ),
        );
    }
    let reset_at = Utc::now();
    match state.mutation_service.reset() {
        Ok(stored) => match MainStateService::new(state.store).current_state(reset_at) {
            Ok(main_state) => json_response(
                200,
                ApiResponse::ok(json!({
                    "reset": true,
                    "reset_at": reset_at,
                    "event_count": stored.events.len(),
                    "session_count": stored.runtime_states.len(),
                    "state": main_state
                })),
            ),
            Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
        },
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_listener_config(State(state): State<AppState>) -> Response {
    match state.store.listener_config() {
        Ok(config) => json_response(
            200,
            ApiResponse::ok(json!({
                "codex_listening_enabled": config.is_tool_enabled(&ToolId::Codex),
                "tool_listening_enabled": config.tool_enabled_map(),
                "tools": listener_tools(&plugin_registry(&state).tools(), &config)
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn save_listener_config(State(state): State<AppState>, body: Bytes) -> Response {
    let value = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(value) => value,
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
    let mut config = match state.store.listener_config() {
        Ok(config) => config,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    if let Some(map) = value
        .get("tool_listening_enabled")
        .and_then(serde_json::Value::as_object)
    {
        for (tool_id, enabled_value) in map {
            let Some(enabled) = enabled_value.as_bool() else {
                return json_response(
                    200,
                    ApiResponse::fail(
                        ApiErrorCode::BusinessValidation,
                        "tool_listening_enabled 的值必须是布尔值",
                    ),
                );
            };
            config = config.with_tool_enabled(&ToolId::from_id(tool_id.clone()), enabled);
        }
    } else {
        let Some(enabled_value) = value.get("codex_listening_enabled") else {
            return json_response(
                200,
                ApiResponse::fail(
                    ApiErrorCode::BusinessValidation,
                    "codex_listening_enabled 不能为空",
                ),
            );
        };
        let Some(enabled) = enabled_value.as_bool() else {
            return json_response(
                200,
                ApiResponse::fail(
                    ApiErrorCode::BusinessValidation,
                    "codex_listening_enabled 必须是布尔值",
                ),
            );
        };
        config = config.with_tool_enabled(&ToolId::Codex, enabled);
    }
    let enabled = config.is_tool_enabled(&ToolId::Codex);
    match state.mutation_service.set_listener_config(config.clone()) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "saved": true,
                "codex_listening_enabled": enabled,
                "tool_listening_enabled": result.config.tool_enabled_map(),
                "tools": listener_tools(&plugin_registry(&state).tools(), &result.config)
            })),
        ),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

pub(crate) async fn get_notification_records(State(state): State<AppState>) -> Response {
    match state.store.notification_history_records(20) {
        Ok(records) => json_response(200, ApiResponse::ok(json!({ "list": records }))),
        Err(error) => json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    }
}

fn listener_tools(
    plugins: &[ToolPluginInfo],
    config: &niuma_core::listener_config::ListenerConfig,
) -> Vec<ListenerToolView> {
    plugins
        .iter()
        .map(|plugin| ListenerToolView {
            id: plugin.tool_id.as_str().to_string(),
            plugin_id: plugin.id.clone(),
            display_name: plugin.display_name.clone(),
            enabled: config.is_tool_enabled(&plugin.tool_id),
            source: format!("{:?}", plugin.source).to_lowercase(),
            icon_url: plugin.icon_url.clone(),
        })
        .collect()
}

fn plugin_management_items(
    state: &AppState,
) -> Result<Vec<niuma_core::plugin::PluginManagementItem>, String> {
    let config = state.store.listener_config()?;
    let runtime_states = state.store.plugin_runtime_states()?;
    let plugin_enabled_map = state.store.plugin_enabled_map()?;
    Ok(plugin_registry(state).management_items(&config, &plugin_enabled_map, &runtime_states))
}

fn plugin_registry(state: &AppState) -> PluginRegistry {
    PluginRegistry::with_builtin_plugins().discover_external_plugins(&state.plugin_dir)
}

fn resolved_plugin_config(
    state: &AppState,
    plugin: &PluginManifest,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let stored_config = state.store.plugin_config(&plugin.id)?;
    Ok(resolve_plugin_config(plugin, stored_config))
}
