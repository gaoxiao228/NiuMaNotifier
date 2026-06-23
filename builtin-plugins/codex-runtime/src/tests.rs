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
fn codex_session_provider_snapshot_discovers_fixture_and_detail_returns_newest_first() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_codex_session_fixture(temp.path());
    let mut provider = session_provider::CodexSessionProvider::with_codex_home(temp.path().into());

    let snapshot = provider_snapshot(&mut provider);
    let session = snapshot
        .sessions
        .iter()
        .find(|session| session.session_id == "session-fixture")
        .expect("fixture session should be discovered");
    assert_eq!(snapshot.tool, ToolKind::Codex);
    assert_eq!(session.project_path, "/tmp/fixture-project");
    assert_eq!(session.project_name, "fixture-project");
    assert_eq!(session.file_path, path.to_string_lossy());
    assert!(!session.is_subagent);
    assert_eq!(session.parent_session_id, None);

    let detail = provider_detail(&mut provider, "session-fixture", 20, None);

    assert_eq!(detail.session_id, "session-fixture");
    assert_eq!(detail.project_path, "/tmp/fixture-project");
    assert_eq!(detail.project_name, "fixture-project");
    assert_eq!(detail.messages[0].content, "助手回答");
    assert_eq!(detail.messages[1].content, "用户问题");
    assert_eq!(detail.next_cursor, None);
}

#[test]
fn codex_session_provider_detail_paginates_with_cursor() {
    let temp = tempfile::tempdir().unwrap();
    write_codex_session_fixture(temp.path());
    let mut provider = session_provider::CodexSessionProvider::with_codex_home(temp.path().into());
    let _ = provider_snapshot(&mut provider);

    let first = provider_detail(&mut provider, "session-fixture", 1, None);
    assert_eq!(first.messages.len(), 1);
    assert_eq!(first.messages[0].content, "助手回答");
    assert_eq!(first.next_cursor.as_deref(), Some("1"));

    let second = provider_detail(
        &mut provider,
        "session-fixture",
        1,
        first.next_cursor.as_deref(),
    );
    assert_eq!(second.messages.len(), 1);
    assert_eq!(second.messages[0].content, "用户问题");
    assert_eq!(second.next_cursor, None);
}

#[test]
fn codex_session_provider_detail_does_not_leak_raw_payload_or_raw_line() {
    let temp = tempfile::tempdir().unwrap();
    write_codex_session_fixture(temp.path());
    let mut provider = session_provider::CodexSessionProvider::with_codex_home(temp.path().into());
    let _ = provider_snapshot(&mut provider);

    let detail = provider_detail(&mut provider, "session-fixture", 20, None);
    let encoded = serde_json::to_string(&detail).unwrap();

    assert!(!encoded.contains("raw_line"));
    assert!(!encoded.contains("secret"));
    assert!(!encoded.contains("不能泄露"));
}

#[test]
fn codex_session_provider_detail_invalid_cursor_returns_provider_error() {
    let temp = tempfile::tempdir().unwrap();
    write_codex_session_fixture(temp.path());
    let mut provider = session_provider::CodexSessionProvider::with_codex_home(temp.path().into());
    let _ = provider_snapshot(&mut provider);

    let request = niuma_core::tool_session_rpc::ProviderRpcRequest::new(
        "req-invalid-cursor",
        "session_detail",
        niuma_core::tool_session_rpc::SessionDetailParams {
            tool: ToolKind::Codex,
            session_id: "session-fixture".to_string(),
            limit: 20,
            cursor: Some("not-a-number".to_string()),
        },
    )
    .unwrap();
    let response = provider.handle_request(request);

    assert_eq!(response.error.unwrap().code, "invalid_cursor");
}

#[test]
fn codex_session_provider_detail_missing_session_returns_provider_error() {
    let temp = tempfile::tempdir().unwrap();
    let mut provider = session_provider::CodexSessionProvider::with_codex_home(temp.path().into());

    let request = niuma_core::tool_session_rpc::ProviderRpcRequest::new(
        "req-missing",
        "session_detail",
        niuma_core::tool_session_rpc::SessionDetailParams {
            tool: ToolKind::Codex,
            session_id: "missing-session".to_string(),
            limit: 20,
            cursor: None,
        },
    )
    .unwrap();
    let response = provider.handle_request(request);
    let error = response.error.unwrap();

    assert_eq!(response.id, "req-missing");
    assert_eq!(error.code, "session_not_found");
    assert_eq!(error.message, "session_id 不存在：missing-session");
}

fn provider_snapshot(
    provider: &mut session_provider::CodexSessionProvider,
) -> niuma_core::tool_session_rpc::SessionSnapshotResult {
    let request = niuma_core::tool_session_rpc::ProviderRpcRequest::new(
        "req-snapshot",
        "session_snapshot",
        niuma_core::tool_session_rpc::SessionSnapshotParams {
            tool: ToolKind::Codex,
        },
    )
    .unwrap();

    provider
        .handle_request(request)
        .result_as::<niuma_core::tool_session_rpc::SessionSnapshotResult>()
        .unwrap()
}

fn provider_detail(
    provider: &mut session_provider::CodexSessionProvider,
    session_id: &str,
    limit: usize,
    cursor: Option<&str>,
) -> niuma_core::tool_session::ToolSessionDetail {
    let request = niuma_core::tool_session_rpc::ProviderRpcRequest::new(
        "req-detail",
        "session_detail",
        niuma_core::tool_session_rpc::SessionDetailParams {
            tool: ToolKind::Codex,
            session_id: session_id.to_string(),
            limit,
            cursor: cursor.map(ToString::to_string),
        },
    )
    .unwrap();

    provider
        .handle_request(request)
        .result_as::<niuma_core::tool_session_rpc::SessionDetailResult>()
        .unwrap()
        .detail
}

fn write_codex_session_fixture(codex_home: &std::path::Path) -> std::path::PathBuf {
    let day_dir = codex_home.join("sessions/2026/06/22");
    std::fs::create_dir_all(&day_dir).unwrap();
    let path = day_dir.join("rollout-2026-06-22-00000000-0000-0000-0000-000000000000.jsonl");
    // fixture 覆盖 session_meta、user、assistant，并带上不应出现在详情里的原始字段。
    std::fs::write(
        &path,
        concat!(
            "{\"timestamp\":\"2026-06-22T01:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-fixture\",\"cwd\":\"/tmp/fixture-project\"}}\n",
            "{\"timestamp\":\"2026-06-22T01:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"用户问题\"}],\"secret\":\"不能泄露\"}}\n",
            "{\"timestamp\":\"2026-06-22T01:00:02Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"助手回答\"}],\"raw_line\":\"不能泄露\"}}\n",
        ),
    )
    .unwrap();
    path
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
