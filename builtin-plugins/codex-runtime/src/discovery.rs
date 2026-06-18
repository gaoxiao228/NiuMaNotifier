use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::codex::session_watcher::{codex_session_dirs, CodexSessionScanner};
use chrono::Utc;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::logging::{
    watcher_debug_enabled, watcher_debug_log, watcher_trace_enabled, watcher_trace_log,
};
use super::ACTIVE_FILE_TTL;

pub(super) struct SessionDayDirCache {
    refresh_interval: Duration,
    refreshed_at: Option<Instant>,
    dirs: Vec<PathBuf>,
}

impl SessionDayDirCache {
    pub(super) fn new(refresh_interval: Duration) -> Self {
        Self {
            refresh_interval,
            refreshed_at: None,
            dirs: Vec::new(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.refreshed_at = None;
        self.dirs.clear();
    }

    pub(super) fn dirs(&mut self, codex_home: &Path, now: Instant) -> Vec<PathBuf> {
        let should_refresh = self
            .refreshed_at
            .map(|refreshed_at| {
                now.checked_duration_since(refreshed_at)
                    .map(|elapsed| elapsed >= self.refresh_interval)
                    .unwrap_or(true)
            })
            .unwrap_or(true);
        if should_refresh {
            self.dirs = codex_session_day_dirs(codex_home);
            self.refreshed_at = Some(now);
        }
        self.dirs.clone()
    }
}

pub(super) fn is_codex_jsonl_path(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("jsonl")
}

pub(super) fn refresh_watched_dirs(
    watcher: &mut RecommendedWatcher,
    watched_dirs: &mut HashSet<PathBuf>,
    codex_home: &Path,
) {
    for dir in codex_session_dirs(codex_home, Utc::now()) {
        if let Err(error) = ensure_watched_dir(watcher, watched_dirs, &dir) {
            eprintln!(
                "NiumaNotifier cannot watch Codex session dir {}: {error}",
                dir.display()
            );
            continue;
        }
    }
}

fn ensure_watched_dir(
    watcher: &mut RecommendedWatcher,
    watched_dirs: &mut HashSet<PathBuf>,
    dir: &Path,
) -> Result<(), String> {
    if watched_dirs.contains(dir) {
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|error| format!("创建目录失败：{error}"))?;
    watcher
        .watch(dir, RecursiveMode::NonRecursive)
        .map_err(|error| format!("{error}"))?;
    if watcher_debug_enabled() {
        watcher_debug_log(format!(
            "NiumaNotifier Codex watcher watches dir {}",
            dir.display()
        ));
    }
    watched_dirs.insert(dir.to_path_buf());
    Ok(())
}

#[cfg(test)]
pub(super) fn recent_jsonl_files(codex_home: &Path, limit: usize) -> Vec<PathBuf> {
    recent_jsonl_files_in_dirs(codex_session_day_dirs(codex_home), limit)
}

fn recent_jsonl_files_in_dirs(dirs: Vec<PathBuf>, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::<(PathBuf, std::time::SystemTime)>::new();
    for dir in dirs {
        files.extend(recent_jsonl_file_entries_in_dir(&dir));
    }
    files.sort_by(|left, right| right.1.cmp(&left.1));
    files
        .into_iter()
        .take(limit)
        .map(|(path, _)| path)
        .collect()
}

fn codex_session_day_dirs(codex_home: &Path) -> Vec<PathBuf> {
    let sessions_dir = codex_home.join("sessions");
    let Ok(year_entries) = std::fs::read_dir(sessions_dir) else {
        return codex_session_dirs(codex_home, Utc::now());
    };
    let mut dirs = Vec::new();
    for year_entry in year_entries.flatten() {
        let year_path = year_entry.path();
        if !year_path.is_dir() {
            continue;
        }
        let Ok(month_entries) = std::fs::read_dir(year_path) else {
            continue;
        };
        for month_entry in month_entries.flatten() {
            let month_path = month_entry.path();
            if !month_path.is_dir() {
                continue;
            }
            let Ok(day_entries) = std::fs::read_dir(month_path) else {
                continue;
            };
            for day_entry in day_entries.flatten() {
                let day_path = day_entry.path();
                // Codex session 文件按 sessions/YYYY/MM/DD 归档；只扫日目录避免递归进无关层级。
                if day_path.is_dir() {
                    dirs.push(day_path);
                }
            }
        }
    }
    if dirs.is_empty() {
        codex_session_dirs(codex_home, Utc::now())
    } else {
        dirs
    }
}

pub(super) fn recent_jsonl_file_entries_in_dir(
    dir: &Path,
) -> Vec<(PathBuf, std::time::SystemTime)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !is_codex_jsonl_path(&path) {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            Some((path, modified))
        })
        .collect()
}

pub(super) fn add_active_file(
    active_files: &mut HashMap<PathBuf, Instant>,
    path: PathBuf,
    now: Instant,
) {
    if is_codex_jsonl_path(&path) {
        active_files.insert(path, now);
    }
}

pub(super) fn add_discovered_active_file(
    scanner: &mut CodexSessionScanner,
    active_files: &mut HashMap<PathBuf, Instant>,
    path: PathBuf,
    now: Instant,
) {
    if !is_codex_jsonl_path(&path) || active_files.contains_key(&path) {
        return;
    }
    match scanner.prime_file_to_end(&path) {
        Ok(()) => {}
        Err(error) => {
            eprintln!(
                "NiumaNotifier prime Codex session file failed {}: {error}",
                path.display()
            );
            return;
        }
    }
    if watcher_trace_enabled() {
        watcher_trace_log(format!(
            "NiumaNotifier Codex watcher discovers active file {}",
            path.display()
        ));
    }
    active_files.insert(path, now);
}

pub(super) fn discover_recent_files(
    scanner: &mut CodexSessionScanner,
    active_files: &mut HashMap<PathBuf, Instant>,
    codex_home: &Path,
    dir_cache: &mut SessionDayDirCache,
    limit: usize,
    now: Instant,
) {
    for path in recent_jsonl_files_in_dirs(dir_cache.dirs(codex_home, now), limit) {
        add_discovered_active_file(scanner, active_files, path, now);
    }
}

pub(super) fn collect_event_paths(
    event: notify::Result<Event>,
    pending_files: &mut Vec<PathBuf>,
    pending_dirs: &mut Vec<PathBuf>,
) {
    let event = match event {
        Ok(event) => event,
        Err(error) => {
            eprintln!("NiumaNotifier Codex watcher event failed: {error}");
            return;
        }
    };
    if watcher_debug_enabled() {
        watcher_debug_log(format!(
            "NiumaNotifier Codex watcher raw event: kind={:?}, paths={:?}",
            event.kind, event.paths
        ));
    }
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    ) {
        if watcher_debug_enabled() {
            watcher_debug_log(format!(
                "NiumaNotifier Codex watcher ignored event kind={:?}",
                event.kind
            ));
        }
        return;
    }
    for path in event.paths {
        if is_codex_jsonl_path(&path) {
            push_unique(pending_files, path);
        } else if path.is_dir() {
            push_unique(pending_dirs, path);
        } else if let Some(parent) = path.parent() {
            push_unique(pending_dirs, parent.to_path_buf());
        }
    }
}

pub(super) fn path_recently_modified(path: &Path, max_age: std::time::Duration) -> bool {
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

pub(super) fn discover_recent_dir_files(
    scanner: &mut CodexSessionScanner,
    active_files: &mut HashMap<PathBuf, Instant>,
    dir: &Path,
    limit: usize,
    now: Instant,
) {
    if watcher_trace_enabled() {
        watcher_trace_log(format!(
            "NiumaNotifier Codex watcher discovers dir {}",
            dir.display()
        ));
    }
    let mut files = recent_jsonl_file_entries_in_dir(dir);
    files.sort_by(|left, right| right.1.cmp(&left.1));
    for (path, _) in files.into_iter().take(limit) {
        if is_codex_jsonl_path(&path) && path_recently_modified(&path, ACTIVE_FILE_TTL) {
            add_discovered_active_file(scanner, active_files, path, now);
        }
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}
