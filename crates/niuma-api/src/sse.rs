use async_stream::stream;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use niuma_core::main_state::{MainStatePayload, MainStateService, MainStateWatcher};

use crate::response::apply_cors_headers;
use crate::state::AppState;

#[derive(Default)]
pub(crate) struct MainStateBroadcaster {
    version: u64,
    last_content: Option<String>,
}

#[derive(Default)]
struct MainStateClient {
    last_content: Option<String>,
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

fn main_state_content_key(payload: &MainStatePayload) -> String {
    let mut payload = payload.clone();
    payload.version = 0;
    serde_json::to_string(&payload).expect("主状态 payload 必须可序列化")
}
