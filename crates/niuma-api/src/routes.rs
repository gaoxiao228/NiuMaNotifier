use axum::routing::{get, post};
use axum::Router;
use niuma_core::plugin::default_user_plugin_dir;
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::SqliteStateStore;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::handlers::{
    dismiss_blocker, get_events, get_listener_config, get_main_state, get_notification_records,
    get_plugin_config, get_plugins, get_sessions, import_plugin, post_event, post_plugin_events,
    post_plugin_notification_result, post_plugin_notification_test_result, remove_plugin,
    reset_state, save_listener_config, save_plugin_config, set_plugin_enabled,
};
use crate::manual_test::manual_test_scenario;
use crate::response::{preflight, route_not_found};
use crate::sse::{events_stream, sse_stream, MainStateBroadcaster};
use crate::state::AppState;

pub fn app(store: SqliteStateStore) -> Router {
    app_with_bus(store, RuntimeEventBus::new())
}

pub fn app_with_bus(store: SqliteStateStore, runtime_events: RuntimeEventBus) -> Router {
    app_with_bus_and_plugin_dir(store, runtime_events, default_user_plugin_dir())
}

pub fn app_with_bus_and_plugin_dir(
    store: SqliteStateStore,
    runtime_events: RuntimeEventBus,
    plugin_dir: PathBuf,
) -> Router {
    let mutation_service = StateMutationService::new(store.clone(), runtime_events.clone());
    Router::new()
        .route("/api/v1/main-state", get(get_main_state).options(preflight))
        .route(
            "/api/v1/events",
            get(get_events).post(post_event).options(preflight),
        )
        .route(
            "/api/v1/events/stream",
            get(events_stream).options(preflight),
        )
        .route(
            "/api/v1/plugin-events",
            post(post_plugin_events).options(preflight),
        )
        .route(
            "/api/v1/plugins/notification-results",
            post(post_plugin_notification_result).options(preflight),
        )
        .route(
            "/api/v1/plugins/notification-test-results",
            post(post_plugin_notification_test_result).options(preflight),
        )
        .route("/api/v1/plugins", get(get_plugins).options(preflight))
        .route(
            "/api/v1/plugins/import",
            post(import_plugin).options(preflight),
        )
        .route(
            "/api/v1/plugins/remove",
            post(remove_plugin).options(preflight),
        )
        .route(
            "/api/v1/plugins/enabled",
            post(set_plugin_enabled).options(preflight),
        )
        .route(
            "/api/v1/plugins/config",
            get(get_plugin_config)
                .post(save_plugin_config)
                .options(preflight),
        )
        .route("/api/v1/sessions", get(get_sessions).options(preflight))
        .route("/api/v1/state/stream", get(sse_stream).options(preflight))
        .route(
            "/api/v1/blocker/dismiss",
            post(dismiss_blocker).options(preflight),
        )
        .route("/api/v1/state/reset", post(reset_state).options(preflight))
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
            plugin_dir,
            main_state_broadcaster: Arc::new(Mutex::new(MainStateBroadcaster::default())),
        })
}
