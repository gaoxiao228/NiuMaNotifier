use async_stream::stream;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use niuma_core::main_state::{MainStatePayload, MainStateService, MainStateWatcher};
use niuma_core::runtime_event::RuntimeEvent;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use crate::response::apply_cors_headers;
use crate::response::json_response;
use crate::state::AppState;
use crate::tool_sessions::ToolSessionProjectGroupsQuery;

#[derive(Default)]
pub(crate) struct MainStateBroadcaster {
    version: u64,
    last_content: Option<String>,
}

#[derive(Default)]
struct MainStateClient {
    last_content: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SessionProjectGroupsStreamQuery {
    tool: Option<String>,
    project_path: Option<String>,
    include_subagents: Option<bool>,
    page: Option<usize>,
    page_size: Option<usize>,
}

impl MainStateBroadcaster {
    fn version_for_content(&mut self, content: &str) -> u64 {
        if self.last_content.as_deref() != Some(content) {
            self.version += 1;
            self.last_content = Some(content.to_string());
        }
        self.version
    }
}

impl MainStateClient {
    fn should_send(&mut self, content: &str, force: bool) -> bool {
        if !force && self.last_content.as_deref() == Some(content) {
            return false;
        }
        // 每个 SSE 连接独立记录已发送内容，避免多客户端互相吞掉同一次状态变化。
        self.last_content = Some(content.to_string());
        true
    }
}

pub(crate) async fn sse_stream(State(state): State<AppState>) -> Response {
    let event_stream = stream! {
        let mut watcher = MainStateWatcher::new(&state.runtime_events);
        let mut client = MainStateClient::default();
        if let Some(event) = next_state_event(&state, &mut client, true) {
            yield Ok::<Event, std::convert::Infallible>(event);
        }
        while watcher.wait_for_refresh().await {
            if let Some(event) = next_state_event(&state, &mut client, false) {
                yield Ok::<Event, std::convert::Infallible>(event);
            }
        }
    };
    let mut response = Sse::new(event_stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    apply_cors_headers(response.headers_mut());
    response
}

pub(crate) async fn events_stream(State(state): State<AppState>) -> Response {
    let mut receiver = state.runtime_events.subscribe();
    let event_stream = stream! {
        loop {
            match receiver.recv().await {
                Ok(RuntimeEvent::NiumaEventsAppended { events, .. }) => {
                    for niuma_event in events {
                        // 事件流只广播实际应用的新事件，推送插件自行判断是否消费。
                        if let Ok(data) = serde_json::to_string(&niuma_event) {
                            yield Ok::<Event, std::convert::Infallible>(
                                Event::default()
                                    .event("event")
                                    .id(niuma_event.id)
                                    .data(data)
                            );
                        }
                    }
                }
                Ok(RuntimeEvent::PluginNotificationTestRequested { request, .. }) => {
                    // 测试通知是控制事件，不写入公开事件缓存，避免污染主事件历史。
                    if let Ok(data) = serde_json::to_string(&request) {
                        yield Ok::<Event, std::convert::Infallible>(
                            Event::default()
                                .event("notification_test")
                                .id(request.test_id)
                                .data(data)
                        );
                    }
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    let mut response = Sse::new(event_stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    apply_cors_headers(response.headers_mut());
    response
}

pub(crate) async fn session_project_groups_stream(
    State(state): State<AppState>,
    query: Result<Query<SessionProjectGroupsStreamQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(error) => {
            return json_response(
                400,
                niuma_core::api_response::ApiResponse::fail(
                    niuma_core::api_response::ApiErrorCode::ParameterType,
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
    if let Err(error) = session_project_groups_payload(&state, &query) {
        return json_response(
            200,
            niuma_core::api_response::ApiResponse::fail(
                niuma_core::api_response::ApiErrorCode::BusinessValidation,
                error,
            ),
        );
    }

    let event_stream = stream! {
        let mut receiver = state.runtime_events.subscribe();
        let mut client = MainStateClient::default();
        if let Some(event) = next_session_project_groups_event(&state, &query, &mut client, true) {
            yield Ok::<Event, std::convert::Infallible>(event);
        }
        loop {
            match receiver.recv().await {
                Ok(RuntimeEvent::NiumaEventsAppended { .. })
                | Ok(RuntimeEvent::AttentionDismissed { .. })
                | Ok(RuntimeEvent::StateReset { .. })
                | Ok(RuntimeEvent::StateChanged { .. }) => {
                    if let Some(event) =
                        next_session_project_groups_event(&state, &query, &mut client, false)
                    {
                        yield Ok::<Event, std::convert::Infallible>(event);
                    }
                }
                Ok(RuntimeEvent::PluginNotificationTestRequested { .. }) => {}
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    let mut response = Sse::new(event_stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    apply_cors_headers(response.headers_mut());
    response
}

fn next_state_event(state: &AppState, client: &mut MainStateClient, force: bool) -> Option<Event> {
    let mut payload = MainStateService::new(state.store.clone())
        .current_state(Utc::now())
        .ok()?;
    let content = main_state_content_key(&payload);
    if !client.should_send(&content, force) {
        return None;
    }
    let version = state
        .main_state_broadcaster
        .lock()
        .ok()?
        .version_for_content(&content);
    payload.version = version;
    let version = payload.version.to_string();
    let data = serde_json::to_string(&payload).ok()?;
    Some(Event::default().event("state").id(version).data(data))
}

fn next_session_project_groups_event(
    state: &AppState,
    query: &ToolSessionProjectGroupsQuery,
    client: &mut MainStateClient,
    force: bool,
) -> Option<Event> {
    let content = session_project_groups_payload(state, query).ok()?;
    if !client.should_send(&content, force) {
        return None;
    }
    let data: serde_json::Value = serde_json::from_str(&content).ok()?;
    let id = state
        .runtime_events
        .current_version()
        .saturating_add(1)
        .to_string();
    Some(
        Event::default()
            .event("session_project_groups")
            .id(id)
            .data(serde_json::to_string(&data).ok()?),
    )
}

fn session_project_groups_payload(
    state: &AppState,
    query: &ToolSessionProjectGroupsQuery,
) -> Result<String, String> {
    let runtime_states = state.store.runtime_state_list()?;
    let page = state
        .tool_sessions
        .project_groups_with_runtime(query.clone(), &runtime_states)?;
    serde_json::to_string(&page)
        .map_err(|error| format!("session project groups 序列化失败：{error}"))
}

fn main_state_content_key(payload: &MainStatePayload) -> String {
    let mut payload = payload.clone();
    payload.version = 0;
    serde_json::to_string(&payload).expect("主状态 payload 必须可序列化")
}
