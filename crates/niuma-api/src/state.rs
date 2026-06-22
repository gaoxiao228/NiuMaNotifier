use std::sync::{Arc, Mutex};

use niuma_core::approval_arbitration::ApprovalArbiter;
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;

use crate::sse::MainStateBroadcaster;
use crate::tool_sessions::ToolSessionRegistry;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: NiumaStore,
    pub(crate) runtime_events: RuntimeEventBus,
    pub(crate) mutation_service: StateMutationService,
    pub(crate) approval_arbiter: Arc<Mutex<ApprovalArbiter>>,
    pub(crate) plugin_dir: std::path::PathBuf,
    pub(crate) main_state_broadcaster: Arc<Mutex<MainStateBroadcaster>>,
    // Task 5 的 session_list/session_detail 路由会读取该 registry；Task 4 只负责先接入状态。
    #[allow(dead_code)]
    pub(crate) tool_sessions: ToolSessionRegistry,
}
