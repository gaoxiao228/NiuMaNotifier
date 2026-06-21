use std::time::Duration;

use chrono::{DateTime, Utc};
use niuma_core::models::EventType;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;

const APPROVAL_PROXY_STALE_AFTER: chrono::Duration = chrono::Duration::seconds(8);
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
    // 失联请求统一走 returned_to_codex 事件，保持 UI 和外部消费者行为一致。
    let results = store.return_stale_approval_proxies_to_codex(now, stale_after)?;
    let events = results
        .iter()
        .map(|result| {
            crate::handlers::approval_event_for_internal(
                &result.request,
                EventType::ApprovalReturnedToCodex,
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
