use super::*;
use chrono::TimeZone;
use niuma_core::listener_config::ListenerConfig;

#[test]
fn codex_session_runtime_accepts_only_jsonl_files() {
    assert!(is_codex_jsonl_path(std::path::Path::new("rollout.jsonl")));
    assert!(!is_codex_jsonl_path(std::path::Path::new("rollout.txt")));
    assert!(!is_codex_jsonl_path(std::path::Path::new("jsonl")));
}

#[test]
fn codex_session_runtime_listening_enabled_defaults_true_and_reads_saved_false() {
    let store = NiumaStore::new(test_sqlite_path("runtime_listener_config"));

    assert!(codex_listening_enabled(&store));
    store
        .save_listener_config(&ListenerConfig {
            codex_listening_enabled: false,
            ..ListenerConfig::default()
        })
        .unwrap();
    assert!(!codex_listening_enabled(&store));
}

#[test]
fn fallback_scan_interval_keeps_notify_path_observable() {
    assert_eq!(FALLBACK_SCAN_INTERVAL, Duration::from_secs(120));
}

#[test]
fn codex_session_provider_binary_target_is_declared() {
    let cargo_toml = include_str!("../Cargo.toml");

    // 内置 provider 由桌面运行时直接启动，workspace 必须声明可编译的 bin target。
    assert!(cargo_toml.contains("name = \"niuma-codex-session-provider\""));
}

#[test]
fn codex_session_provider_stub_returns_empty_snapshot() {
    let request = niuma_core::tool_session_rpc::ProviderRpcRequest::new(
        "req-1",
        "session_snapshot",
        niuma_core::tool_session_rpc::SessionSnapshotParams {
            tool: ToolKind::Codex,
        },
    )
    .unwrap();

    let response = session_provider::handle_session_provider_request(request);
    let snapshot = response
        .result_as::<niuma_core::tool_session_rpc::SessionSnapshotResult>()
        .unwrap();

    assert_eq!(response.id, "req-1");
    assert_eq!(snapshot.tool, ToolKind::Codex);
    assert!(snapshot.sessions.is_empty());
}

#[test]
fn codex_session_provider_stub_returns_not_found_for_detail() {
    let request = niuma_core::tool_session_rpc::ProviderRpcRequest::new(
        "req-2",
        "session_detail",
        niuma_core::tool_session_rpc::SessionDetailParams {
            tool: ToolKind::Codex,
            session_id: "missing-session".to_string(),
            limit: 20,
            cursor: None,
        },
    )
    .unwrap();

    let response = session_provider::handle_session_provider_request(request);
    let error = response.error.unwrap();

    assert_eq!(response.id, "req-2");
    assert_eq!(error.code, "session_not_found");
    assert_eq!(error.message, "session_id 不存在：missing-session");
}

fn test_sqlite_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "niuma-codex-runtime-{name}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("niuma.sqlite")
}

#[test]
fn active_scan_interval_keeps_session_updates_near_realtime() {
    assert_eq!(DISCOVERY_SCAN_INTERVAL, Duration::from_secs(1));
    assert_eq!(DISCOVERY_FILE_LIMIT, 32);
    assert_eq!(ACTIVE_SCAN_INTERVAL, Duration::from_millis(500));
    assert_eq!(ACTIVE_FILE_TTL, Duration::from_secs(60));
    assert_eq!(CODEX_LOG_SCAN_INTERVAL, Duration::from_secs(2));
}

#[test]
fn main_status_log_state_suppresses_repeated_status_keys() {
    let mut log_state = MainStatusLogState::default();

    let first_seen = Instant::now();
    let first_activity = chrono::Utc
        .timestamp_opt(1_000, 0)
        .single()
        .expect("valid timestamp");

    assert!(log_state.should_log(
        "Running|session-1|Running".to_string(),
        Some(first_activity),
        first_seen
    ));
    assert!(!log_state.should_log(
        "Running|session-1|Running".to_string(),
        Some(first_activity),
        first_seen + STATUS_LOG_REFRESH_INTERVAL
    ));
    assert!(log_state.should_log(
        "Idle|session-1|Completed".to_string(),
        Some(first_activity),
        first_seen + STATUS_LOG_REFRESH_INTERVAL
    ));
}

#[test]
fn main_status_log_state_refreshes_when_activity_keeps_moving() {
    let mut log_state = MainStatusLogState::default();
    let first_seen = Instant::now();
    let first_activity = chrono::Utc
        .timestamp_opt(1_000, 0)
        .single()
        .expect("valid timestamp");
    let second_activity = chrono::Utc
        .timestamp_opt(1_001, 0)
        .single()
        .expect("valid timestamp");

    assert!(log_state.should_log(
        "Running|session-1|Running".to_string(),
        Some(first_activity),
        first_seen
    ));
    assert!(!log_state.should_log(
        "Running|session-1|Running".to_string(),
        Some(second_activity),
        first_seen + Duration::from_millis(500)
    ));
    assert!(log_state.should_log(
        "Running|session-1|Running".to_string(),
        Some(second_activity),
        first_seen + STATUS_LOG_REFRESH_INTERVAL
    ));
}

#[test]
fn watcher_debug_and_trace_flags_are_independent() {
    std::env::set_var("NIUMA_CODEX_WATCHER_DEBUG", "1");
    std::env::remove_var("NIUMA_CODEX_WATCHER_TRACE");

    assert!(watcher_debug_enabled());
    assert!(!watcher_trace_enabled());

    std::env::set_var("NIUMA_CODEX_WATCHER_TRACE", "1");
    assert!(watcher_trace_enabled());

    std::env::remove_var("NIUMA_CODEX_WATCHER_DEBUG");
    std::env::remove_var("NIUMA_CODEX_WATCHER_TRACE");
}

#[test]
fn codex_session_runtime_collects_directory_events_for_immediate_scan() {
    let temp = std::env::temp_dir().join(format!("niuma-watch-dir-event-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();
    let event = Event {
        kind: EventKind::Any,
        paths: vec![temp.clone()],
        attrs: Default::default(),
    };
    let mut pending_files = Vec::<PathBuf>::new();
    let mut pending_dirs = Vec::<PathBuf>::new();

    collect_event_paths(Ok(event), &mut pending_files, &mut pending_dirs);

    assert!(pending_files.is_empty());
    assert_eq!(pending_dirs, vec![temp]);
}

#[test]
fn discovery_includes_recently_modified_file_from_older_session_day() {
    let codex_home = std::env::temp_dir().join(format!(
        "niuma-codex-old-day-discovery-{}",
        std::process::id()
    ));
    let old_day = codex_home.join("sessions/2026/06/10");
    let today = chrono::Utc::now();
    let today_dir = codex_home
        .join("sessions")
        .join(today.format("%Y").to_string())
        .join(today.format("%m").to_string())
        .join(today.format("%d").to_string());
    let _ = std::fs::remove_dir_all(&codex_home);
    std::fs::create_dir_all(&old_day).unwrap();
    std::fs::create_dir_all(&today_dir).unwrap();
    let old_active = old_day.join("rollout-old-active.jsonl");
    let today_file = today_dir.join("rollout-today.jsonl");
    std::fs::write(&today_file, "{}\n").unwrap();
    std::fs::write(&old_active, "{}\n").unwrap();

    let files = recent_jsonl_files(&codex_home, 8);

    assert!(files.contains(&old_active));
    let _ = std::fs::remove_dir_all(codex_home);
}

#[test]
fn active_file_stays_active_when_recently_modified() {
    let path =
        std::env::temp_dir().join(format!("niuma-active-expire-{}.jsonl", std::process::id()));
    std::fs::write(&path, "").unwrap();
    let now = Instant::now();
    let mut active_files = HashMap::from([(path.clone(), now - ACTIVE_FILE_TTL * 2)]);
    let mut status_log_state = MainStatusLogState::default();
    let mutation_service = StateMutationService::new(
        NiumaStore::new(path.with_extension("sqlite")),
        RuntimeEventBus::new(),
    );
    let event_sink = StoreCodexEventSink::new(mutation_service);

    scan_active_files(
        &event_sink,
        &mut CodexSessionScanner::default(),
        &mut active_files,
        &mut status_log_state,
        now,
    );

    assert!(active_files.contains_key(&path));
    let _ = std::fs::remove_file(path);
}

#[test]
fn active_file_expires_when_not_recently_seen_or_modified() {
    let path =
        std::env::temp_dir().join(format!("niuma-active-missing-{}.jsonl", std::process::id()));
    let now = Instant::now();
    let mut active_files = HashMap::from([(path, now - ACTIVE_FILE_TTL * 2)]);
    let mut status_log_state = MainStatusLogState::default();
    let mutation_service = StateMutationService::new(
        NiumaStore::new(test_sqlite_path("active_missing")),
        RuntimeEventBus::new(),
    );
    let event_sink = StoreCodexEventSink::new(mutation_service);

    scan_active_files(
        &event_sink,
        &mut CodexSessionScanner::default(),
        &mut active_files,
        &mut status_log_state,
        now,
    );

    assert!(active_files.is_empty());
}

#[test]
fn discovered_active_file_primes_to_end_without_replaying_old_tail() {
    use std::io::Write;

    let path = std::env::temp_dir().join(format!(
        "niuma-discovered-prime-{}.jsonl",
        std::process::id()
    ));
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(
        file,
        r#"{{"type":"session_meta","payload":{{"id":"session-123","cwd":"/tmp/demo"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"old"}}}}"#
    )
    .unwrap();

    let mut scanner = CodexSessionScanner::default();
    let mut active_files = HashMap::<PathBuf, Instant>::new();
    let store = NiumaStore::new(path.with_extension("sqlite"));
    add_discovered_active_file(
        &mut scanner,
        &mut active_files,
        path.clone(),
        Instant::now(),
    );
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"new"}}}}"#
    )
    .unwrap();
    drop(file);

    let events = scanner.scan_file(&path).unwrap();
    let snapshot = store.internal_status_snapshot().unwrap();

    assert_eq!(active_files.len(), 1);
    assert_eq!(snapshot.primary_session_id, None);
    assert_eq!(events.len(), 1);
    assert!(events[0].dedupe_key.ends_with(":new:task_started"));
    assert_eq!(events[0].project_path, "/tmp/demo");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite"));
}

#[test]
fn runtime_buffers_are_cleared_when_listener_is_disabled() {
    let mut pending_files = vec![PathBuf::from("pending.jsonl")];
    let mut pending_dirs = vec![PathBuf::from("pending-dir")];
    let mut active_files = HashMap::from([(PathBuf::from("active.jsonl"), Instant::now())]);

    clear_runtime_buffers(&mut pending_files, &mut pending_dirs, &mut active_files);

    assert!(pending_files.is_empty());
    assert!(pending_dirs.is_empty());
    assert!(active_files.is_empty());
}

#[test]
fn discovery_dir_cache_reuses_recent_snapshot_until_refresh_interval() {
    let codex_home =
        std::env::temp_dir().join(format!("niuma-codex-dir-cache-{}", std::process::id()));
    let first_day = codex_home.join("sessions/2026/06/11");
    let second_day = codex_home.join("sessions/2026/06/12");
    let _ = std::fs::remove_dir_all(&codex_home);
    std::fs::create_dir_all(&first_day).unwrap();
    let mut cache = SessionDayDirCache::new(Duration::from_secs(30));
    let now = Instant::now();

    let first = cache.dirs(&codex_home, now);
    std::fs::create_dir_all(&second_day).unwrap();
    let cached = cache.dirs(&codex_home, now + Duration::from_secs(1));
    let refreshed = cache.dirs(&codex_home, now + Duration::from_secs(31));

    assert!(first.contains(&first_day));
    assert_eq!(cached, first);
    assert!(refreshed.contains(&second_day));
    let _ = std::fs::remove_dir_all(codex_home);
}

#[test]
fn prime_codex_log_scanner_defers_when_log_file_is_missing() {
    let path = std::env::temp_dir().join(format!(
        "niuma-missing-codex-log-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let now = Instant::now();
    let mut next_probe = now;
    let mut scanner = CodexLogScanner::default();

    prime_codex_log_scanner(&mut scanner, &path, &mut next_probe, now);

    assert!(next_probe >= now + CODEX_LOG_SCHEMA_RETRY_INTERVAL);
}
