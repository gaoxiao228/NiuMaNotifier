use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::codex::log_watcher::CodexLogScanner;
use crate::codex::session_watcher::CodexSessionScanner;

use super::discovery::{add_active_file, discover_recent_dir_files, path_recently_modified};
use super::logging::{watcher_trace_enabled, watcher_trace_log, MainStatusLogState};
use super::{CodexEventSink, ACTIVE_FILE_TTL, DISCOVERY_FILE_LIMIT};

pub(super) fn flush_pending(
    event_sink: &dyn CodexEventSink,
    scanner: &mut CodexSessionScanner,
    pending_files: &mut Vec<PathBuf>,
    pending_dirs: &mut Vec<PathBuf>,
    active_files: &mut HashMap<PathBuf, Instant>,
    status_log_state: &mut MainStatusLogState,
    now: Instant,
) {
    let paths = std::mem::take(pending_files);
    for path in paths {
        add_active_file(active_files, path.clone(), now);
        scan_jsonl_file(event_sink, scanner, status_log_state, &path, true);
    }
    let dirs = std::mem::take(pending_dirs);
    for dir in dirs {
        discover_recent_dir_files(scanner, active_files, &dir, DISCOVERY_FILE_LIMIT, now);
    }
}

pub(super) fn scan_active_files(
    event_sink: &dyn CodexEventSink,
    scanner: &mut CodexSessionScanner,
    active_files: &mut HashMap<PathBuf, Instant>,
    status_log_state: &mut MainStatusLogState,
    now: Instant,
) {
    active_files.retain(|path, last_seen| {
        let recently_seen = now.duration_since(*last_seen) <= ACTIVE_FILE_TTL;
        let recently_modified = path_recently_modified(path, ACTIVE_FILE_TTL);
        recently_seen || recently_modified
    });

    for path in active_files.keys().cloned().collect::<Vec<_>>() {
        scan_jsonl_file(event_sink, scanner, status_log_state, &path, false);
    }
}

fn scan_jsonl_file(
    event_sink: &dyn CodexEventSink,
    scanner: &mut CodexSessionScanner,
    status_log_state: &mut MainStatusLogState,
    path: &Path,
    log_empty: bool,
) {
    match scanner.scan_file(path) {
        Ok(events) if !events.is_empty() => {
            if watcher_trace_enabled() {
                watcher_trace_log(format!(
                    "NiumaNotifier Codex watcher parsed {} events from {}",
                    events.len(),
                    path.display()
                ));
            }
            if let Err(error) = event_sink.append_events(events, "scan", status_log_state) {
                eprintln!("NiumaNotifier append Codex session events failed: {error}");
            }
        }
        Ok(_) if log_empty => {
            if watcher_trace_enabled() {
                watcher_trace_log(format!(
                    "NiumaNotifier Codex watcher scanned 0 events from {}",
                    path.display()
                ));
            }
        }
        Ok(_) => {}
        Err(error) => eprintln!("NiumaNotifier scan Codex session file failed: {error}"),
    }
}

pub(super) fn scan_codex_internal_log(
    event_sink: &dyn CodexEventSink,
    scanner: &mut CodexLogScanner,
    path: &Path,
    status_log_state: &mut MainStatusLogState,
) {
    match scanner.scan_file(path) {
        Ok(events) if !events.is_empty() => {
            if let Err(error) = event_sink.append_events(events, "codex-log", status_log_state) {
                eprintln!("NiumaNotifier append Codex log events failed: {error}");
            }
        }
        Ok(_) => {}
        Err(error) => eprintln!("NiumaNotifier scan Codex internal log failed: {error}"),
    }
}
