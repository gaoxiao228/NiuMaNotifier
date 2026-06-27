use axum::body::Bytes;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use chrono::Utc;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::codex_managed_control;
use niuma_core::models::{EventType, NiumaEvent, ToolKind};
use niuma_core::platform::paths::codex_managed_registry_path;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::response::json_response;
use crate::state::AppState;
use crate::tool_sessions::{capped_limit, ToolSessionListQuery, ToolSessionProjectGroupsQuery};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionListQuery {
    tool: Option<String>,
    include_subagents: Option<bool>,
    active_only: Option<bool>,
    limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionProjectGroupsQuery {
    tool: Option<String>,
    project_path: Option<String>,
    include_subagents: Option<bool>,
    page: Option<usize>,
    page_size: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionDetailQuery {
    tool: Option<String>,
    session_id: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionSendInstructionBody {
    tool: String,
    session_id: String,
    channel_id: String,
    content: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionInterruptBody {
    tool: String,
    session_id: String,
    channel_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionAnswerInputBody {
    tool: String,
    session_id: String,
    channel_id: String,
    request_id: String,
    answers: Value,
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

pub(crate) async fn get_session_project_groups(
    State(state): State<AppState>,
    query: Result<Query<SessionProjectGroupsQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(error) => {
            return json_response(
                400,
                ApiResponse::fail(
                    ApiErrorCode::ParameterType,
                    format!("查询参数类型错误（include_subagents/page/page_size）：{error}"),
                ),
            );
        }
    };

    let query = ToolSessionProjectGroupsQuery {
        tool: query
            .tool
            .map(|tool| tool.trim().to_string())
            .filter(|tool| !tool.is_empty()),
        project_path: query
            .project_path
            .map(|project_path| project_path.trim().to_string())
            .filter(|project_path| !project_path.is_empty()),
        include_subagents: query.include_subagents.unwrap_or(false),
        page: query.page,
        page_size: query.page_size,
    };

    // HTTP 列表和 SSE 列表都叠加同一份运行时状态，避免同一接口族返回不一致。
    let runtime_states = match state.store.runtime_state_list() {
        Ok(runtime_states) => runtime_states,
        Err(error) => return json_response(500, ApiResponse::fail(ApiErrorCode::System, error)),
    };
    match state
        .tool_sessions
        .project_groups_with_runtime(query, &runtime_states)
    {
        Ok(page) => json_response(200, ApiResponse::ok(page)),
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

pub(crate) async fn post_session_send_instruction(body: Bytes) -> Response {
    let request = match serde_json::from_slice::<SessionSendInstructionBody>(&body) {
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
    if request.tool.trim() != ToolKind::Codex.as_str() {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                "当前仅支持 codex 会话续写",
            ),
        );
    }
    let session_id = request.session_id.trim();
    let channel_id = request.channel_id.trim();
    match codex_managed_control::send_instruction(
        &codex_managed_registry_path(),
        session_id,
        channel_id,
        &request.content,
    ) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "sent": true,
                "channel_id": channel_id,
                "result": result.get("result").cloned().unwrap_or(result)
            })),
        ),
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

pub(crate) async fn post_session_interrupt(body: Bytes) -> Response {
    let request = match serde_json::from_slice::<SessionInterruptBody>(&body) {
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
    if request.tool.trim() != ToolKind::Codex.as_str() {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                "当前仅支持 codex 会话中断",
            ),
        );
    }
    let session_id = request.session_id.trim();
    let channel_id = request.channel_id.trim();
    match codex_managed_control::interrupt(&codex_managed_registry_path(), session_id, channel_id) {
        Ok(result) => json_response(
            200,
            ApiResponse::ok(json!({
                "interrupted": true,
                "channel_id": channel_id,
                "result": result.get("result").cloned().unwrap_or(result)
            })),
        ),
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

pub(crate) async fn post_session_answer_input(
    State(state): State<AppState>,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<SessionAnswerInputBody>(&body) {
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
    if request.tool.trim() != ToolKind::Codex.as_str() {
        return json_response(
            200,
            ApiResponse::fail(
                ApiErrorCode::BusinessValidation,
                "当前仅支持 codex 输入回答",
            ),
        );
    }

    let session_id = request.session_id.trim();
    let channel_id = request.channel_id.trim();
    let request_id = request.request_id.trim();
    match codex_managed_control::answer_input(
        &codex_managed_registry_path(),
        session_id,
        channel_id,
        request_id,
        &request.answers,
    ) {
        Ok(result) => {
            let state_cleared = append_answer_input_resolved_event(&state, session_id, request_id);
            json_response(
                200,
                ApiResponse::ok(json!({
                    "answered": true,
                    "channel_id": channel_id,
                    "request_id": request_id,
                    "state_cleared": state_cleared,
                    "result": result.get("result").cloned().unwrap_or(result)
                })),
            )
        }
        Err(error) => json_response(
            200,
            ApiResponse::fail(ApiErrorCode::BusinessValidation, error),
        ),
    }
}

fn append_answer_input_resolved_event(
    state: &AppState,
    session_id: &str,
    request_id: &str,
) -> bool {
    let now = Utc::now();
    let event_id = format!(
        "session-control-answer-input:{session_id}:{request_id}:{}",
        now.timestamp_nanos_opt().unwrap_or_default()
    );
    let dedupe_key = format!("session-control-answer-input:{session_id}:{request_id}");
    let event = NiumaEvent {
        id: event_id,
        dedupe_key,
        source: "session-control".to_string(),
        tool: ToolKind::Codex,
        session_id: session_id.to_string(),
        parent_session_id: None,
        normalized_session_id: None,
        session_scope: None,
        agent_nickname: None,
        agent_role: None,
        project_path: String::new(),
        project_name: String::new(),
        event_type: EventType::SessionActivity,
        severity: "info".to_string(),
        summary: "已提交等待输入".to_string(),
        content: None,
        error_message: None,
        // 让状态转移只清理本次已回答的等待输入。
        attention_resolve_key: Some(format!("input:{request_id}")),
        completion_reason: None,
        failure_reason: None,
        payload_ref: None,
        interaction: None,
        created_at: now,
    };
    match state.mutation_service.append_events(vec![event]) {
        Ok(result) => !result.applied_events.is_empty(),
        Err(error) => {
            eprintln!("追加 answer-input 清理事件失败：{error}");
            false
        }
    }
}

fn required_query_value(value: Option<String>) -> Option<String> {
    // GET 查询参数传空字符串时按业务参数缺失处理，保持 200 + 业务失败 envelope。
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
