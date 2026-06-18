pub mod plugin_runtime;

use niuma_core::runtime_event::RuntimeEventBus;

pub fn spawn_tool_runtimes(runtime_events: RuntimeEventBus) {
    plugin_runtime::spawn_plugin_runtimes(runtime_events);
}
