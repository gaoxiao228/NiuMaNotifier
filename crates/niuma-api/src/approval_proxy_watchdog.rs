use std::time::Duration;

use chrono::{DateTime, Utc};
use niuma_core::models::{ApprovalStatus, EventType};
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;

// hook helper 每 2 秒心跳；默认 watchdog 留足 UI/进程调度抖动窗口，避免按钮过早消失。
const APPROVAL_PROXY_STALE_AFTER: chrono::Duration = chrono::Duration::seconds(30);
const APPROVAL_PROXY_WATCHDOG_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) fn spawn_approval_proxy_watchdog(
    store: NiumaStore,
    mutation_service: StateMutationService,
) {
    // watchdog 只检测 hook 代理是否还活着，不参与 allow/deny 决策。
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(APPROVAL_PROXY_WATCHDOG_INTERVAL);
        loop {
            interval.tick().await;
            if let Err(error) = sweep_approval_proxy_watchdog(&store, &mutation_service) {
                eprintln!("NiumaNotifier approval proxy watchdog error: {error}");
            }
        }
    });
}

pub(crate) fn sweep_approval_proxy_watchdog(
    store: &NiumaStore,
    mutation_service: &StateMutationService,
) -> Result<usize, String> {
    sweep_approval_proxy_watchdog_at(
        store,
        mutation_service,
        Utc::now(),
        APPROVAL_PROXY_STALE_AFTER,
    )
}

pub(crate) fn sweep_approval_proxy_watchdog_at(
    store: &NiumaStore,
    mutation_service: &StateMutationService,
    now: DateTime<Utc>,
    stale_after: chrono::Duration,
) -> Result<usize, String> {
    let results = store.return_stale_approval_proxies_to_codex(now, stale_after)?;
    let events = results
        .iter()
        .map(|result| {
            crate::handlers::approval_event_for_internal(
                &result.request,
                returned_event_type_for_status(&result.request.status),
                "info",
                "approval-watchdog",
            )
        })
        .collect::<Vec<_>>();
    if !events.is_empty() {
        mutation_service.append_events(events)?;
    }
    Ok(results.len())
}

fn returned_event_type_for_status(status: &ApprovalStatus) -> EventType {
    match status {
        ApprovalStatus::ReturnedToTool => EventType::ApprovalReturnedToTool,
        _ => EventType::ApprovalReturnedToCodex,
    }
}
