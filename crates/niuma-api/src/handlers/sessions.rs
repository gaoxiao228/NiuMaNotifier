use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::models::ToolKind;
use serde::Deserialize;
use serde_json::json;

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

    match state.tool_sessions.project_groups(query) {
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

fn required_query_value(value: Option<String>) -> Option<String> {
    // GET 查询参数传空字符串时按业务参数缺失处理，保持 200 + 业务失败 envelope。
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
