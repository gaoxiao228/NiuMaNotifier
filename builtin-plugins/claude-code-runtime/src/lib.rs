use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use claude::discovery::{claude_projects_dir, recent_jsonl_files};
use claude::session_event_cursor::ClaudeSessionScanner;
use claude::session_repository::ClaudeSessionRepository;
use niuma_core::models::NiumaEvent;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

mod claude;
mod session_messages;
mod session_provider;

const WATCH_RECV_TIMEOUT: Duration = Duration::from_millis(250);
const DISCOVERY_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const ACTIVE_SCAN_INTERVAL: Duration = Duration::from_millis(500);
const FALLBACK_SCAN_INTERVAL: Duration = Duration::from_secs(120);
const ACTIVE_FILE_TTL: Duration = Duration::from_secs(60);
const DISCOVERY_FILE_LIMIT: usize = 32;
const PARENT_PID_ENV: &str = "NIUMA_PARENT_PID";
const PARENT_WATCHDOG_INTERVAL: Duration = Duration::from_secs(2);

pub fn run_combined_from_env() {
    start_parent_watchdog_from_env();
    let event_sink = match LocalApiClaudeEventSink::from_env() {
        Ok(event_sink) => event_sink,
        Err(error) => {
            eprintln!("NiumaNotifier Claude Code plugin process not started: {error}");
            std::process::exit(1);
        }
    };
    let claude_home = claude_home();
    let repository = Arc::new(Mutex::new(ClaudeSessionRepository::new(
        claude_home.clone(),
    )));
    let watcher_repository = repository.clone();
    if let Err(error) = thread::Builder::new()
        .name("claude-code-watcher-runtime".to_string())
        .spawn(move || run_runtime(Box::new(event_sink), watcher_repository, claude_home))
    {
        eprintln!("NiumaNotifier Claude Code watcher runtime not started: {error}");
        std::process::exit(1);
    }
    // 合并插件后 stdout 是 provider JSON Lines RPC 通道，watcher 只能写 stderr。
    session_provider::run_stdio_session_provider_with_repository(repository);
}

fn claude_home() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

fn start_parent_watchdog_from_env() {
    let Some(parent_pid) = parse_parent_pid(std::env::var(PARENT_PID_ENV).ok().as_deref()) else {
        return;
    };
    if let Err(error) = thread::Builder::new()
        .name("niuma-parent-watchdog".to_string())
        .spawn(move || run_parent_watchdog(parent_pid))
    {
        eprintln!("NiumaNotifier parent watchdog not started: {error}");
    }
}

fn run_parent_watchdog(parent_pid: u32) {
    loop {
        thread::sleep(PARENT_WATCHDOG_INTERVAL);
        if !parent_process_exists(parent_pid) {
            eprintln!("NiumaNotifier parent process {parent_pid} is gone; plugin exiting");
            std::process::exit(0);
        }
    }
}

fn parse_parent_pid(value: Option<&str>) -> Option<u32> {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0)
}

#[cfg(unix)]
fn parent_process_exists(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error()
        .raw_os_error()
        .is_some_and(|code| code != libc::ESRCH)
}

#[cfg(not(unix))]
fn parent_process_exists(_pid: u32) -> bool {
    true
}

trait ClaudeEventSink: Send + Sync {
    fn append_events(&self, events: Vec<NiumaEvent>) -> Result<(), String>;
}

struct LocalApiClaudeEventSink {
    api_url: String,
    plugin_id: String,
}

impl LocalApiClaudeEventSink {
    fn from_env() -> Result<Self, String> {
        let api_url = std::env::var("NIUMA_LOCAL_API_URL")
            .map_err(|_| "NIUMA_LOCAL_API_URL 未设置".to_string())?;
        let plugin_id =
            std::env::var("NIUMA_PLUGIN_ID").unwrap_or_else(|_| "builtin-claude-code".to_string());
        Ok(Self { api_url, plugin_id })
    }
}

impl ClaudeEventSink for LocalApiClaudeEventSink {
    fn append_events(&self, events: Vec<NiumaEvent>) -> Result<(), String> {
        let body = serde_json::json!({
            "plugin_id": self.plugin_id,
            "events": events
        });
        let response = ureq::post(&format!("{}/api/v1/plugin-events", self.api_url))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|error| format!("Claude Code 插件事件上报失败：{error}"))?;
        let text = response
            .into_string()
            .map_err(|error| format!("读取 Claude Code 插件上报响应失败：{error}"))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| format!("解析 Claude Code 插件上报响应失败：{error}"))?;
        if value.get("code").and_then(serde_json::Value::as_i64) == Some(0) {
            Ok(())
        } else {
            Err(value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Claude Code 插件上报返回业务失败")
                .to_string())
        }
    }
}

fn run_runtime(
    event_sink: Box<dyn ClaudeEventSink>,
    repository: Arc<Mutex<ClaudeSessionRepository>>,
    claude_home: PathBuf,
) {
    let projects_dir = claude_projects_dir(&claude_home);
    if let Err(error) = std::fs::create_dir_all(&projects_dir) {
        eprintln!(
            "NiumaNotifier cannot create Claude Code projects dir {}: {error}",
            projects_dir.display()
        );
    }
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("NiumaNotifier Claude Code session watcher not started: {error}");
            return;
        }
    };
    if let Err(error) = watcher.watch(&projects_dir, RecursiveMode::Recursive) {
        eprintln!(
            "NiumaNotifier cannot watch Claude Code projects dir {}: {error}",
            projects_dir.display()
        );
    }

    let mut scanner = ClaudeSessionScanner::with_shared_repository(repository);
    let mut pending_files = Vec::<PathBuf>::new();
    let mut active_files = HashMap::<PathBuf, Instant>::new();
    let mut last_discovery_scan = Instant::now() - DISCOVERY_SCAN_INTERVAL;
    let mut last_active_scan = Instant::now();
    let mut last_fallback_scan = Instant::now();

    loop {
        if let Ok(event) = rx.recv_timeout(WATCH_RECV_TIMEOUT) {
            collect_event_paths(event, &mut pending_files);
        }
        flush_pending(
            event_sink.as_ref(),
            &mut scanner,
            &mut pending_files,
            &mut active_files,
            Instant::now(),
        );

        if last_discovery_scan.elapsed() >= DISCOVERY_SCAN_INTERVAL {
            discover_recent_files(
                event_sink.as_ref(),
                &mut scanner,
                &mut active_files,
                &claude_home,
                Instant::now(),
            );
            last_discovery_scan = Instant::now();
        }

        // macOS 追加写入事件可能被合并或遗漏，活跃文件轮询承担实时兜底。
        if last_active_scan.elapsed() >= ACTIVE_SCAN_INTERVAL {
            scan_active_files(
                event_sink.as_ref(),
                &mut scanner,
                &mut active_files,
                Instant::now(),
            );
            last_active_scan = Instant::now();
        }

        if last_fallback_scan.elapsed() >= FALLBACK_SCAN_INTERVAL {
            discover_recent_files(
                event_sink.as_ref(),
                &mut scanner,
                &mut active_files,
                &claude_home,
                Instant::now(),
            );
            last_fallback_scan = Instant::now();
        }
    }
}

fn collect_event_paths(event: notify::Result<Event>, pending_files: &mut Vec<PathBuf>) {
    let event = match event {
        Ok(event) => event,
        Err(error) => {
            eprintln!("NiumaNotifier Claude Code watcher event failed: {error}");
            return;
        }
    };
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any
    ) {
        return;
    }
    for path in event.paths {
        if is_claude_jsonl_path(&path) {
            push_unique(pending_files, path);
        }
    }
}

fn flush_pending(
    event_sink: &dyn ClaudeEventSink,
    scanner: &mut ClaudeSessionScanner,
    pending_files: &mut Vec<PathBuf>,
    active_files: &mut HashMap<PathBuf, Instant>,
    now: Instant,
) {
    let files = std::mem::take(pending_files);
    for path in files {
        active_files.insert(path.clone(), now);
        scan_and_emit(event_sink, scanner, &path);
    }
}

fn discover_recent_files(
    event_sink: &dyn ClaudeEventSink,
    scanner: &mut ClaudeSessionScanner,
    active_files: &mut HashMap<PathBuf, Instant>,
    claude_home: &Path,
    now: Instant,
) {
    for path in recent_jsonl_files(claude_home, DISCOVERY_FILE_LIMIT) {
        if !path_recently_modified(&path, ACTIVE_FILE_TTL) {
            continue;
        }
        active_files.insert(path.clone(), now);
        scan_and_emit(event_sink, scanner, &path);
    }
}

fn scan_active_files(
    event_sink: &dyn ClaudeEventSink,
    scanner: &mut ClaudeSessionScanner,
    active_files: &mut HashMap<PathBuf, Instant>,
    now: Instant,
) {
    active_files.retain(|path, seen_at| {
        if now.duration_since(*seen_at) > ACTIVE_FILE_TTL {
            return false;
        }
        scan_and_emit(event_sink, scanner, path);
        true
    });
}

fn scan_and_emit(
    event_sink: &dyn ClaudeEventSink,
    scanner: &mut ClaudeSessionScanner,
    path: &Path,
) {
    match scanner.scan_file(path) {
        Ok(events) if events.is_empty() => {}
        Ok(events) => {
            if let Err(error) = event_sink.append_events(events) {
                eprintln!("NiumaNotifier Claude Code watcher append failed: {error}");
            }
        }
        Err(error) => {
            eprintln!(
                "NiumaNotifier Claude Code watcher scan failed {}: {error}",
                path.display()
            );
        }
    }
}

fn is_claude_jsonl_path(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("jsonl")
}

fn path_recently_modified(path: &Path, max_age: Duration) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|elapsed| elapsed <= max_age)
        .unwrap_or(true)
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

#[cfg(test)]
mod tests;
