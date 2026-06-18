use axum::routing::{get, post};
use axum::Router;
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::SqliteStateStore;
use std::sync::{Arc, Mutex};

use crate::handlers::{
    dismiss_blocker, get_events, get_listener_config, get_main_state, get_notification_config,
    get_notification_records, get_sessions, post_event, reset_state, save_listener_config,
    save_notification_config,
};
use crate::manual_test::manual_test_scenario;
use crate::response::{preflight, route_not_found};
use crate::sse::{sse_stream, MainStateBroadcaster};
use crate::state::AppState;

pub fn app(store: SqliteStateStore) -> Router {
    app_with_bus(store, RuntimeEventBus::new())
}

pub fn app_with_bus(store: SqliteStateStore, runtime_events: RuntimeEventBus) -> Router {
    let mutation_service = StateMutationService::new(store.clone(), runtime_events.clone());
    Router::new()
        .route("/api/v1/main-state", get(get_main_state).options(preflight))
        .route(
            "/api/v1/events",
            get(get_events).post(post_event).options(preflight),
        )
        .route("/api/v1/sessions", get(get_sessions).options(preflight))
        .route("/api/v1/stream", get(sse_stream).options(preflight))
        .route(
            "/api/v1/blocker/dismiss",
            post(dismiss_blocker).options(preflight),
        )
        .route("/api/v1/state/reset", post(reset_state).options(preflight))
        .route(
            "/api/v1/notification-config",
            get(get_notification_config).options(preflight),
        )
        .route(
            "/api/v1/notification-config/save",
            post(save_notification_config).options(preflight),
        )
        .route(
            "/api/v1/notification-records",
            get(get_notification_records).options(preflight),
        )
        .route(
            "/api/v1/listener-config",
            get(get_listener_config).options(preflight),
        )
        .route(
            "/api/v1/listener-config/save",
            post(save_listener_config).options(preflight),
        )
        .route(
            "/api/v1/manual-test/scenario",
            post(manual_test_scenario).options(preflight),
        )
        .fallback(route_not_found)
        .with_state(AppState {
            store,
            runtime_events,
            mutation_service,
            main_state_broadcaster: Arc::new(Mutex::new(MainStateBroadcaster::default())),
        })
}
