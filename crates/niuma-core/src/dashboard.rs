use crate::models::{NiumaEvent, NiumaSession};
use crate::store::NiumaStore;

#[derive(Clone)]
pub struct DashboardService {
    store: NiumaStore,
}

impl DashboardService {
    pub fn new(store: NiumaStore) -> Self {
        Self { store }
    }

    pub fn sessions(&self) -> Result<Vec<NiumaSession>, String> {
        self.store.sessions()
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<NiumaEvent>, String> {
        self.store.public_recent_events(limit)
    }
}
