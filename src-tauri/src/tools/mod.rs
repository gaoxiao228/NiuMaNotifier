pub mod plugin_runtime;

use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::store::NiumaStore;

pub fn spawn_tool_runtimes(store: NiumaStore, runtime_events: RuntimeEventBus) {
    plugin_runtime::spawn_plugin_runtimes(store, runtime_events);
}
