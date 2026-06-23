pub mod plugin_runtime;

use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::store::NiumaStore;

pub fn spawn_tool_runtimes(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    tool_sessions: niuma_api::tool_sessions::ToolSessionRegistry,
) {
    plugin_runtime::spawn_plugin_runtimes(store, runtime_events, tool_sessions);
}
