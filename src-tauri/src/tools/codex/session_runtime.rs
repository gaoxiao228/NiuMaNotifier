use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use niuma_core::config;
use niuma_core::models::ToolKind;
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::SqliteStateStore;
use niuma_core::tools::codex::log_watcher::{
    codex_internal_log_path, codex_log_schema_available, CodexLogScanner,
};
use niuma_core::tools::codex::session_watcher::CodexSessionScanner;
use notify::{Config, Event, RecommendedWatcher, Watcher};

#[path = "session_runtime/discovery.rs"]
mod discovery;
#[path = "session_runtime/logging.rs"]
mod logging;
#[path = "session_runtime/scanner.rs"]
mod scanner;

use self::discovery::{
    collect_event_paths, discover_recent_files, refresh_watched_dirs, SessionDayDirCache,
};
use self::logging::{watcher_debug_enabled, watcher_debug_log, MainStatusLogState};
use self::scanner::{flush_pending, scan_active_files, scan_codex_internal_log};

#[cfg(test)]
use self::discovery::{add_discovered_active_file, is_codex_jsonl_path, recent_jsonl_files};
#[cfg(test)]
use self::logging::{watcher_trace_enabled, STATUS_LOG_REFRESH_INTERVAL};
#[cfg(test)]
use notify::EventKind;

const FALLBACK_SCAN_INTERVAL: Duration = Duration::from_secs(120);
const DISCOVERY_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const SESSION_DIR_CACHE_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const DISCOVERY_FILE_LIMIT: usize = 32;
const ACTIVE_SCAN_INTERVAL: Duration = Duration::from_millis(500);
const ACTIVE_FILE_TTL: Duration = Duration::from_secs(60);
const STALE_SWEEP_INTERVAL: Duration = Duration::from_secs(30);
const WATCH_RECV_TIMEOUT: Duration = Duration::from_millis(250);
const CODEX_LOG_SCAN_INTERVAL: Duration = Duration::from_secs(2);
const CODEX_LOG_SCHEMA_RETRY_INTERVAL: Duration = Duration::from_secs(30);

pub fn spawn_codex_session_runtime(
    store: SqliteStateStore,
    runtime_events: RuntimeEventBus,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("codex-session-runtime".to_string())
        .spawn(move || run_runtime(store, runtime_events))
}

fn run_runtime(store: SqliteStateStore, runtime_events: RuntimeEventBus) {
    let listener_store = store.clone();
    let mutation_service = StateMutationService::new(store, runtime_events);
    let codex_home = config::codex_home();
    if watcher_debug_enabled() {
        watcher_debug_log(format!(
            "NiumaNotifier Codex watcher runtime started: codex_home={}, fallback_scan_interval={}s",
            codex_home.display(),
            FALLBACK_SCAN_INTERVAL.as_secs()
        ));
    }
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("NiumaNotifier Codex session watcher not started: {error}");
            return;
        }
    };

    let mut watched_dirs = HashSet::<PathBuf>::new();

    let mut scanner = CodexSessionScanner::default();
    let codex_log_path = codex_internal_log_path(&codex_home);
    let mut codex_log_scanner = CodexLogScanner::default();
    let mut pending_files = Vec::<PathBuf>::new();
    let mut pending_dirs = Vec::<PathBuf>::new();
    let mut status_log_state = MainStatusLogState::default();
    let mut active_files = HashMap::<PathBuf, Instant>::new();
    let mut dir_cache = SessionDayDirCache::new(SESSION_DIR_CACHE_REFRESH_INTERVAL);
    let mut runtime_initialized = false;
    let mut last_scan = Instant::now();
    let mut last_discovery_scan = Instant::now();
    let mut last_active_scan = Instant::now();
    let mut last_stale_sweep = Instant::now() - STALE_SWEEP_INTERVAL;
    let mut last_codex_log_scan = Instant::now();
    let mut next_codex_log_probe = Instant::now();

    loop {
        if !codex_listening_enabled(&listener_store) {
            if runtime_initialized {
                clear_runtime_buffers(&mut pending_files, &mut pending_dirs, &mut active_files);
                clear_watched_dirs(&mut watcher, &mut watched_dirs);
                dir_cache.clear();
                scanner = CodexSessionScanner::default();
                codex_log_scanner = CodexLogScanner::default();
                runtime_initialized = false;
            }
            while rx.try_recv().is_ok() {}
            thread::sleep(WATCH_RECV_TIMEOUT);
            continue;
        }

        if !runtime_initialized {
            refresh_watched_dirs(&mut watcher, &mut watched_dirs, &codex_home);
            last_scan = Instant::now();
            last_discovery_scan = Instant::now() - DISCOVERY_SCAN_INTERVAL;
            last_active_scan = Instant::now();
            last_codex_log_scan = Instant::now();
            next_codex_log_probe = Instant::now();
            prime_codex_log_scanner(
                &mut codex_log_scanner,
                &codex_log_path,
                &mut next_codex_log_probe,
                Instant::now(),
            );
            runtime_initialized = true;
        }

        if let Ok(event) = rx.recv_timeout(WATCH_RECV_TIMEOUT) {
            collect_event_paths(event, &mut pending_files, &mut pending_dirs);
        }
        flush_pending(
            &mutation_service,
            &mut scanner,
            &mut pending_files,
            &mut pending_dirs,
            &mut active_files,
            &mut status_log_state,
            Instant::now(),
        );

        // 轻量发现最近写入的 session 文件，避免完全依赖 notify 的 Create/Modify 事件。
        if last_discovery_scan.elapsed() >= DISCOVERY_SCAN_INTERVAL {
            discover_recent_files(
                &mut scanner,
                &mut active_files,
                &codex_home,
                &mut dir_cache,
                DISCOVERY_FILE_LIMIT,
                Instant::now(),
            );
            last_discovery_scan = Instant::now();
        }

        // notify 在 macOS 上对追加写入不总是稳定触发；活跃文件轮询承担实时主路径。
        if last_active_scan.elapsed() >= ACTIVE_SCAN_INTERVAL {
            scan_active_files(
                &mutation_service,
                &mut scanner,
                &mut active_files,
                &mut status_log_state,
                Instant::now(),
            );
            last_active_scan = Instant::now();
        }

        // session JSONL 不一定记录模型请求层错误；内部日志里有结构化 API/SSE 错误。
        let now = Instant::now();
        if last_codex_log_scan.elapsed() >= CODEX_LOG_SCAN_INTERVAL && now >= next_codex_log_probe {
            match codex_log_schema_available(&codex_log_path) {
                Ok(true) => {
                    scan_codex_internal_log(
                        &mutation_service,
                        &mut codex_log_scanner,
                        &codex_log_path,
                        &mut status_log_state,
                    );
                }
                Ok(false) => {
                    next_codex_log_probe = now + CODEX_LOG_SCHEMA_RETRY_INTERVAL;
                }
                Err(error) => {
                    eprintln!("NiumaNotifier Codex internal log schema probe failed: {error}");
                    next_codex_log_probe = now + CODEX_LOG_SCHEMA_RETRY_INTERVAL;
                }
            }
            last_codex_log_scan = Instant::now();
        }

        // 文件监听偶发丢事件时，低频目录扫描提供兜底同步。
        if last_scan.elapsed() >= FALLBACK_SCAN_INTERVAL {
            refresh_watched_dirs(&mut watcher, &mut watched_dirs, &codex_home);
            discover_recent_files(
                &mut scanner,
                &mut active_files,
                &codex_home,
                &mut dir_cache,
                DISCOVERY_FILE_LIMIT,
                Instant::now(),
            );
            last_scan = Instant::now();
        }

        // 长时间未更新的 running session 需要定期标记为 stale。
        if last_stale_sweep.elapsed() >= STALE_SWEEP_INTERVAL {
            if let Err(error) = mutation_service
                .mark_stale_running_sessions(Utc::now(), chrono::Duration::minutes(10))
            {
                eprintln!("NiumaNotifier stale sweep failed: {error}");
            }
            last_stale_sweep = Instant::now();
        }
    }
}

fn codex_listening_enabled(store: &SqliteStateStore) -> bool {
    store
        .listener_config()
        .map(|config| config.is_tool_enabled(&ToolKind::Codex))
        .unwrap_or(false)
}

fn clear_runtime_buffers(
    pending_files: &mut Vec<PathBuf>,
    pending_dirs: &mut Vec<PathBuf>,
    active_files: &mut HashMap<PathBuf, Instant>,
) {
    pending_files.clear();
    pending_dirs.clear();
    active_files.clear();
}

fn clear_watched_dirs(watcher: &mut RecommendedWatcher, watched_dirs: &mut HashSet<PathBuf>) {
    for dir in watched_dirs.drain().collect::<Vec<_>>() {
        if let Err(error) = watcher.unwatch(&dir) {
            eprintln!(
                "NiumaNotifier cannot unwatch Codex session dir {}: {error}",
                dir.display()
            );
        }
    }
}

fn prime_codex_log_scanner(
    scanner: &mut CodexLogScanner,
    path: &std::path::Path,
    next_probe: &mut Instant,
    now: Instant,
) {
    match codex_log_schema_available(path) {
        Ok(true) => {
            if let Err(error) = scanner.prime_to_end(path) {
                eprintln!("NiumaNotifier prime Codex internal log failed: {error}");
                *next_probe = now + CODEX_LOG_SCHEMA_RETRY_INTERVAL;
            }
        }
        Ok(false) => {
            *next_probe = now + CODEX_LOG_SCHEMA_RETRY_INTERVAL;
        }
        Err(error) => {
            eprintln!("NiumaNotifier Codex internal log schema probe failed: {error}");
            *next_probe = now + CODEX_LOG_SCHEMA_RETRY_INTERVAL;
        }
    }
}

#[cfg(test)]
#[path = "session_runtime/tests.rs"]
mod tests;
