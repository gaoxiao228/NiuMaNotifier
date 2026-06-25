use axum::routing::{get, post};
use axum::Router;
use niuma_core::approval_arbitration::ApprovalArbiter;
use niuma_core::plugin::default_user_plugin_dir;
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::handlers::{
    dismiss_blocker, get_approval_decision, get_approval_requests, get_events, get_listener_config,
    get_main_state, get_notification_records, get_plugin_config, get_plugins,
    get_runtime_state_list, get_session_detail, get_session_list, get_session_project_groups,
    import_plugin, post_approval_decision, post_approval_heartbeat, post_approval_request,
    post_approval_return_to_codex, post_event, post_plugin_events, post_plugin_notification_result,
    post_plugin_notification_test_result, remove_plugin, reset_state, run_plugin_action,
    save_listener_config, save_plugin_config, set_plugin_enabled,
};
use crate::manual_test::manual_test_scenario;
use crate::response::{preflight, route_not_found};
use crate::sse::{
    events_stream, session_detail_stream, session_project_groups_stream, sse_stream,
    MainStateBroadcaster,
};
use crate::state::AppState;
use crate::tool_sessions::ToolSessionRegistry;

pub fn app(store: NiumaStore) -> Router {
    app_with_bus(store, RuntimeEventBus::new())
}

pub fn app_with_tool_sessions(store: NiumaStore, tool_sessions: ToolSessionRegistry) -> Router {
    app_with_bus_and_plugin_dir_and_tool_sessions(
        store,
        RuntimeEventBus::new(),
        default_user_plugin_dir(),
        tool_sessions,
    )
}

pub fn app_with_bus_and_tool_sessions(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    tool_sessions: ToolSessionRegistry,
) -> Router {
    app_with_bus_and_plugin_dir_and_tool_sessions(
        store,
        runtime_events,
        default_user_plugin_dir(),
        tool_sessions,
    )
}

pub fn app_with_bus(store: NiumaStore, runtime_events: RuntimeEventBus) -> Router {
    app_with_bus_and_plugin_dir_and_tool_sessions(
        store,
        runtime_events,
        default_user_plugin_dir(),
        ToolSessionRegistry::new(),
    )
}

pub fn app_with_bus_and_plugin_dir(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    plugin_dir: PathBuf,
) -> Router {
    app_with_bus_and_plugin_dir_and_tool_sessions(
        store,
        runtime_events,
        plugin_dir,
        ToolSessionRegistry::new(),
    )
}

fn app_with_bus_and_plugin_dir_and_tool_sessions(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    plugin_dir: PathBuf,
    tool_sessions: ToolSessionRegistry,
) -> Router {
    let mutation_service = StateMutationService::new(store.clone(), runtime_events.clone());
    let approval_arbiter = Arc::new(Mutex::new(ApprovalArbiter::default()));
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
            "/api/v1/approval-requests",
            get(get_approval_requests)
                .post(post_approval_request)
                .options(preflight),
        )
        .route(
            "/api/v1/approval-decisions",
            get(get_approval_decision)
                .post(post_approval_decision)
                .options(preflight),
        )
        .route(
            "/api/v1/approval-requests/return",
            post(post_approval_return_to_codex).options(preflight),
        )
        .route(
            "/api/v1/approval-requests/heartbeat",
            post(post_approval_heartbeat).options(preflight),
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
            "/api/v1/plugins/actions",
            post(run_plugin_action).options(preflight),
        )
        .route(
            "/api/v1/plugins/config",
            get(get_plugin_config)
                .post(save_plugin_config)
                .options(preflight),
        )
        .route(
            "/api/v1/runtime_state_list",
            get(get_runtime_state_list).options(preflight),
        )
        .route(
            "/api/v1/session_list",
            get(get_session_list).options(preflight),
        )
        .route(
            "/api/v1/session_detail",
            get(get_session_detail).options(preflight),
        )
        .route(
            "/api/v1/session_detail/stream",
            get(session_detail_stream).options(preflight),
        )
        .route(
            "/api/v1/session_project_groups",
            get(get_session_project_groups).options(preflight),
        )
        .route(
            "/api/v1/session_project_groups/stream",
            get(session_project_groups_stream).options(preflight),
        )
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
            approval_arbiter,
            plugin_dir,
            main_state_broadcaster: Arc::new(Mutex::new(MainStateBroadcaster::default())),
            tool_sessions,
        })
}
