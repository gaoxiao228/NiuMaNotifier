use std::time::Instant;

use chrono::Utc;
use niuma_core::config;
use niuma_core::state::InternalStateEngine;
use niuma_core::store::StoredState;

pub(super) const STATUS_LOG_REFRESH_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(2);

pub(super) fn watcher_debug_enabled() -> bool {
    config::watcher_debug_enabled()
}

pub(super) fn watcher_debug_log(message: String) {
    println!("{message}");
}

pub(super) fn watcher_trace_enabled() -> bool {
    config::watcher_trace_enabled()
}

pub(super) fn watcher_trace_log(message: String) {
    println!("{message}");
}

#[derive(Default)]
pub(super) struct MainStatusLogState {
    // Debug 日志既要降噪，也要能看出 Running session 仍在持续活动。
    last_status_key: Option<String>,
    last_logged_updated_at: Option<chrono::DateTime<Utc>>,
    last_logged_at: Option<Instant>,
}

impl MainStatusLogState {
    pub(super) fn should_log(
        &mut self,
        status_key: String,
        updated_at: Option<chrono::DateTime<Utc>>,
        now: Instant,
    ) -> bool {
        if self.last_status_key.as_deref() != Some(status_key.as_str()) {
            self.last_status_key = Some(status_key);
            self.last_logged_updated_at = updated_at;
            self.last_logged_at = Some(now);
            return true;
        }

        let activity_moved = self.last_logged_updated_at != updated_at;
        let refresh_due = self
            .last_logged_at
            .map(|logged_at| now.duration_since(logged_at) >= STATUS_LOG_REFRESH_INTERVAL)
            .unwrap_or(true);
        if activity_moved && refresh_due {
            self.last_logged_updated_at = updated_at;
            self.last_logged_at = Some(now);
            return true;
        }
        false
    }
}

pub(super) fn log_main_status(
    reason: &str,
    state: &StoredState,
    log_state: &mut MainStatusLogState,
) {
    if !watcher_debug_enabled() {
        return;
    }
    let snapshot = InternalStateEngine::aggregate(
        &state.attention_items,
        state.latest_activity.as_ref(),
        &state.events,
    );
    let primary_session_id = snapshot.primary_session_id.as_deref().unwrap_or("-");
    let session_status = snapshot
        .primary_session_id
        .as_deref()
        .and_then(|id| state.sessions.iter().find(|session| session.id == id))
        .map(|session| format!("{:?}", session.status))
        .unwrap_or_else(|| "-".to_string());
    let status_key = format!(
        "{:?}|{}|{}",
        snapshot.status, primary_session_id, session_status
    );
    if !log_state.should_log(status_key, snapshot.updated_at, Instant::now()) {
        return;
    }
    watcher_debug_log(format!(
        "NiumaNotifier main status update reason={reason}, main_status={:?}, primary_session_id={}, session_status={}, updated_at={}",
        snapshot.status,
        primary_session_id,
        session_status,
        snapshot
            .updated_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "-".to_string())
    ));
}
