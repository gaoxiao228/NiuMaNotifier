use niuma_api::{local_api_addr, spawn_local_api_with_bus_and_tool_sessions};
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;
use std::thread;
use std::time::Duration;

use crate::tools;
use crate::{remote, remote::status::RemoteAgentStatusHandle};

const LOCAL_API_START_DELAY: Duration = Duration::ZERO;
const WATCHER_START_DELAY: Duration = Duration::from_secs(1);
const STALE_SWEEP_INTERVAL: Duration = Duration::from_secs(30);

pub fn spawn_background_services(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    tool_sessions: niuma_api::tool_sessions::ToolSessionRegistry,
    remote_agent_status: RemoteAgentStatusHandle,
    remote_agent_wake: remote::agent::RemoteAgentWake,
) {
    let spawn_result = thread::Builder::new()
        .name("niuma-background-services-startup".to_string())
        .spawn(move || {
            if LOCAL_API_START_DELAY > Duration::ZERO {
                thread::sleep(LOCAL_API_START_DELAY);
            }
            match spawn_local_api_with_bus_and_tool_sessions(
                store.clone(),
                runtime_events.clone(),
                tool_sessions.clone(),
            ) {
                Ok(_) => {
                    eprintln!("NiumaNotifier Local API started at {}", local_api_addr());
                }
                Err(error) => {
                    // 端口可能已被另一个开发实例占用；UI 仍可读取同一份状态文件。
                    eprintln!("NiumaNotifier Local API not started: {error}");
                }
            }
            spawn_stale_sweep_runtime(store.clone(), runtime_events.clone());
            remote::agent::spawn_remote_agent_runtime(
                store.clone(),
                tool_sessions.clone(),
                remote_agent_status.clone(),
                remote_agent_wake.clone(),
            );

            // Codex session 扫描放到首屏之后，避免文件系统监听和活跃文件轮询抢首屏资源。
            thread::sleep(WATCHER_START_DELAY);
            tools::spawn_tool_runtimes(
                store.clone(),
                runtime_events.clone(),
                tool_sessions.clone(),
            );
        });

    if let Err(error) = spawn_result {
        eprintln!("NiumaNotifier background services startup thread not started: {error}");
    }
}

fn spawn_stale_sweep_runtime(store: NiumaStore, runtime_events: RuntimeEventBus) {
    if let Err(error) = thread::Builder::new()
        .name("stale-sweep-runtime".to_string())
        .spawn(move || {
            let service = StateMutationService::new(store, runtime_events);
            loop {
                thread::sleep(STALE_SWEEP_INTERVAL);
                if let Err(error) = run_stale_sweep_once(&service, chrono::Utc::now()) {
                    eprintln!("NiumaNotifier stale sweep failed: {error}");
                }
            }
        })
    {
        eprintln!("NiumaNotifier stale sweep runtime not started: {error}");
    }
}

pub(crate) fn run_stale_sweep_once(
    service: &StateMutationService,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    service
        .mark_stale_running_sessions(now, chrono::Duration::minutes(10))
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use niuma_core::models::{EventType, NiumaEvent, RuntimeStateStatus, ToolKind};

    #[test]
    fn stale_sweep_once_marks_old_running_sessions() {
        let path = std::env::temp_dir().join(format!(
            "niuma-desktop-stale-sweep-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = NiumaStore::new(path.clone());
        store
            .append_event(sample_event("event-running", 1_000))
            .unwrap();
        let service = StateMutationService::new(store.clone(), RuntimeEventBus::new());

        run_stale_sweep_once(
            &service,
            chrono::Utc.timestamp_opt(1_700, 0).single().unwrap(),
        )
        .unwrap();

        assert_eq!(
            store.load().unwrap().runtime_states[0].status,
            RuntimeStateStatus::Stale
        );
        let _ = std::fs::remove_file(path);
    }

    fn sample_event(id: &str, timestamp: i64) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: id.to_string(),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: "session-1".to_string(),
            parent_session_id: None,
            normalized_session_id: None,
            session_scope: None,
            agent_nickname: None,
            agent_role: None,
            tool_call_id: None,
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            event_type: EventType::SessionStarted,
            severity: "info".to_string(),
            summary: "started".to_string(),
            content: None,
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            interaction: None,
            created_at: chrono::Utc.timestamp_opt(timestamp, 0).single().unwrap(),
        }
    }

    #[test]
    fn startup_keeps_local_api_immediate_and_delays_watcher_only() {
        assert_eq!(LOCAL_API_START_DELAY, Duration::ZERO);
        assert_eq!(WATCHER_START_DELAY, Duration::from_secs(1));
    }
}
