use crate::models::{NiumaEvent, RuntimeStateItem};
use crate::store::NiumaStore;

#[derive(Clone)]
pub struct DashboardService {
    store: NiumaStore,
}

impl DashboardService {
    pub fn new(store: NiumaStore) -> Self {
        Self { store }
    }

    pub fn runtime_state_list(&self) -> Result<Vec<RuntimeStateItem>, String> {
        self.store.runtime_state_list()
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<NiumaEvent>, String> {
        self.store.public_recent_events(limit)
    }
}
