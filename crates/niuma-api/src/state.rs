use std::sync::{Arc, Mutex};

use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;

use crate::sse::MainStateBroadcaster;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: NiumaStore,
    pub(crate) runtime_events: RuntimeEventBus,
    pub(crate) mutation_service: StateMutationService,
    pub(crate) plugin_dir: std::path::PathBuf,
    pub(crate) main_state_broadcaster: Arc<Mutex<MainStateBroadcaster>>,
}
