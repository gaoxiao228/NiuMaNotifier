use crate::models::{NiumaEvent, NiumaSession};
use crate::store::SqliteStateStore;

#[derive(Clone)]
pub struct DashboardService {
    store: SqliteStateStore,
}

impl DashboardService {
    pub fn new(store: SqliteStateStore) -> Self {
        Self { store }
    }

    pub fn sessions(&self) -> Result<Vec<NiumaSession>, String> {
        self.store.sessions()
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<NiumaEvent>, String> {
        self.store.public_recent_events(limit)
    }
}
