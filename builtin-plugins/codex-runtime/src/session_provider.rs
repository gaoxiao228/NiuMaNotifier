use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use niuma_core::models::ToolKind;
use niuma_core::tool_session::{ToolSessionDetail, ToolSessionListItem, ToolSessionStatus};
use niuma_core::tool_session_rpc::{
    ProviderRpcNotification, ProviderRpcRequest, ProviderRpcResponse, SessionDetailParams,
    SessionDetailResult, SessionSnapshotParams, SessionSnapshotResult,
};
use serde::Deserialize;

use crate::session_messages::{is_detail_message_line, parse_codex_message_line};

const SNAPSHOT_FILE_LIMIT: usize = 128;
const SESSION_DAY_DIR_LIMIT: usize = 180;
const ACTIVE_MODIFIED_WINDOW: Duration = Duration::from_secs(60);
const SNAPSHOT_NOTIFY_INTERVAL: Duration = Duration::from_secs(2);

pub struct CodexSessionProvider {
    codex_home: PathBuf,
    index: HashMap<String, SessionIndex>,
    #[cfg(test)]
    scan_count: usize,
}

#[derive(Clone)]
struct SessionIndex {
    list_item: ToolSessionListItem,
    file_signature: SessionFileSignature,
    // 只保存可分页消息的原始 JSONL 行号，不在 provider 内存中长期持有完整对话正文。
    message_line_indexes: Vec<usize>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct SessionFileSignature {
    modified_system_time: SystemTime,
    size_bytes: u64,
    content_hash: u64,
}

#[derive(Deserialize)]
struct CodexRow {
    #[serde(rename = "type")]
    row_type: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Default)]
struct ParsedSessionFile {
    session_id: Option<String>,
    project_path: Option<String>,
    message_line_indexes: Vec<usize>,
}

impl CodexSessionProvider {
    pub fn from_config() -> Self {
        Self::with_codex_home(niuma_core::config::codex_home())
    }

    pub fn with_codex_home(codex_home: PathBuf) -> Self {
        Self {
            codex_home,
            index: HashMap::new(),
            #[cfg(test)]
            scan_count: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn scan_count(&self) -> usize {
        self.scan_count
    }

    pub fn handle_request(&mut self, request: ProviderRpcRequest) -> ProviderRpcResponse {
        match request.method.as_str() {
            "session_snapshot" => self.session_snapshot_response(request),
            "session_detail" => self.session_detail_response(request),
            method => ProviderRpcResponse::failure(
                request.id,
                "method_not_found",
                format!("provider method 不存在：{method}"),
            ),
        }
    }

    fn session_snapshot_response(&mut self, request: ProviderRpcRequest) -> ProviderRpcResponse {
        let params = match request.params_as::<SessionSnapshotParams>() {
            Ok(params) => params,
            Err(error) => {
                return ProviderRpcResponse::failure(request.id, "invalid_params", error);
            }
        };
        if params.tool != ToolKind::Codex {
            return ProviderRpcResponse::failure(
                request.id,
                "unsupported_tool",
                "Codex session provider 只支持 codex",
            );
        }
        match self.refresh_snapshot() {
            Ok(sessions) => ProviderRpcResponse::success(
                request.id,
                SessionSnapshotResult {
                    tool: ToolKind::Codex,
                    sessions,
                },
            )
            .expect("session snapshot response must serialize"),
            Err(error) => ProviderRpcResponse::failure(request.id, "snapshot_failed", error),
        }
    }

    fn session_detail_response(&mut self, request: ProviderRpcRequest) -> ProviderRpcResponse {
        let params = match request.params_as::<SessionDetailParams>() {
            Ok(params) => params,
            Err(error) => {
                return ProviderRpcResponse::failure(request.id, "invalid_params", error);
            }
        };
        if params.tool != ToolKind::Codex {
            return ProviderRpcResponse::failure(
                request.id,
                "unsupported_tool",
                "Codex session provider 只支持 codex",
            );
        }
        match self.session_detail(params) {
            Ok(detail) => ProviderRpcResponse::success(request.id, SessionDetailResult { detail })
                .expect("session detail response must serialize"),
            Err(ProviderError { code, message }) => {
                ProviderRpcResponse::failure(request.id, code, message)
            }
        }
    }

    fn refresh_snapshot(&mut self) -> Result<Vec<ToolSessionListItem>, String> {
        let now = Utc::now();
        let mut next_index = HashMap::new();
        let previous_by_path = self
            .index
            .values()
            .map(|index| (index.list_item.file_path.clone(), index.clone()))
            .collect::<HashMap<_, _>>();
        for (path, file_signature) in recent_session_files(&self.codex_home) {
            let file_path = path.to_string_lossy().to_string();
            let result = previous_by_path
                .get(&file_path)
                .filter(|index| index.file_signature == file_signature)
                .cloned()
                .map(|index| Ok(refresh_cached_index(index, now)))
                .unwrap_or_else(|| self.scan_session_file(&path, file_signature, now));
            match result {
                Ok(index) => {
                    next_index.insert(index.list_item.session_id.clone(), index);
                }
                Err(error) => {
                    eprintln!(
                        "NiumaNotifier Codex session provider skipped {}: {error}",
                        path.display()
                    );
                }
            }
        }

        let mut sessions = next_index
            .values()
            .map(|entry| entry.list_item.clone())
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| right.modified_at.cmp(&left.modified_at));
        self.index = next_index;
        Ok(sessions)
    }

    fn session_detail(
        &mut self,
        params: SessionDetailParams,
    ) -> Result<ToolSessionDetail, ProviderError> {
        self.ensure_session_index(&params.session_id)?;
        let mut retried_after_stale_index = false;
        loop {
            let index = self
                .index
                .get(&params.session_id)
                .cloned()
                .ok_or_else(|| ProviderError::not_found(&params.session_id))?;
            match detail_from_index(&index, &params) {
                Ok(detail) => return Ok(detail),
                Err(DetailFromIndexError::Provider(error)) => return Err(error),
                Err(DetailFromIndexError::Stale(_error)) if !retried_after_stale_index => {
                    retried_after_stale_index = true;
                    // 行号索引可能来自旧文件内容；强制重扫一次，避免用缺行数量推进 cursor。
                    self.refresh_session_index_from_file(&params.session_id, &index)?;
                    continue;
                }
                Err(DetailFromIndexError::Stale(error)) => {
                    return Err(ProviderError::stale_session_file(error));
                }
            }
        }
    }

    fn ensure_session_index(&mut self, session_id: &str) -> Result<(), ProviderError> {
        if !self.index.contains_key(session_id) {
            self.refresh_snapshot().map_err(ProviderError::internal)?;
        }
        let Some(index) = self.index.get(session_id).cloned() else {
            return Err(ProviderError::not_found(session_id));
        };
        let path = PathBuf::from(&index.list_item.file_path);
        let file_signature = session_file_signature(&path).map_err(|error| {
            ProviderError::internal(format!("读取 Codex session 文件失败：{error}"))
        })?;
        if file_signature != index.file_signature {
            self.refresh_session_index_from_file(session_id, &index)?;
        }
        Ok(())
    }

    fn refresh_session_index_from_file(
        &mut self,
        session_id: &str,
        index: &SessionIndex,
    ) -> Result<(), ProviderError> {
        let path = PathBuf::from(&index.list_item.file_path);
        let file_signature = session_file_signature(&path).map_err(|error| {
            ProviderError::internal(format!("读取 Codex session 文件失败：{error}"))
        })?;
        let refreshed = self
            .scan_session_file(&path, file_signature, Utc::now())
            .map_err(ProviderError::internal)?;
        let refreshed_session_id = refreshed.list_item.session_id.clone();
        // 文件被截断或替换后可能属于另一个 session，旧 session_id 不能继续命中旧索引。
        self.index.remove(session_id);
        self.index.insert(refreshed_session_id.clone(), refreshed);
        if refreshed_session_id != session_id {
            return Err(ProviderError::not_found(session_id));
        }
        Ok(())
    }

    fn scan_session_file(
        &mut self,
        path: &Path,
        file_signature: SessionFileSignature,
        discovered_at: DateTime<Utc>,
    ) -> Result<SessionIndex, String> {
        #[cfg(test)]
        {
            self.scan_count += 1;
        }
        let parsed = parse_session_file(path)?;
        let fallback_path = path.to_string_lossy();
        let session_id = parsed
            .session_id
            .or_else(|| filename_session_id(path))
            .unwrap_or_else(|| format!("fallback-{}", stable_hash(&fallback_path)));
        let project_path = parsed.project_path.unwrap_or_default();
        let project_name = project_name(&project_path);
        let modified_system_time = file_signature.modified_system_time;
        let modified_at = DateTime::<Utc>::from(modified_system_time);
        let is_active = recently_modified(modified_system_time, ACTIVE_MODIFIED_WINDOW);
        let status = if is_active {
            ToolSessionStatus::Active
        } else {
            ToolSessionStatus::Inactive
        };
        let list_item = ToolSessionListItem {
            id: format!("codex:{session_id}"),
            tool: ToolKind::Codex,
            session_id,
            project_path,
            project_name,
            file_path: path.to_string_lossy().to_string(),
            modified_at,
            discovered_at,
            last_seen_at: discovered_at,
            is_active,
            is_subagent: false,
            parent_session_id: None,
            status,
        };

        Ok(SessionIndex {
            list_item,
            file_signature,
            message_line_indexes: parsed.message_line_indexes,
        })
    }
}

fn refresh_cached_index(mut index: SessionIndex, discovered_at: DateTime<Utc>) -> SessionIndex {
    // 文件内容未变化时只刷新列表态字段，复用行号索引，避免后台 notifier 重复解析 JSONL。
    let modified_system_time = index.file_signature.modified_system_time;
    let is_active = recently_modified(modified_system_time, ACTIVE_MODIFIED_WINDOW);
    index.list_item.discovered_at = discovered_at;
    index.list_item.last_seen_at = discovered_at;
    index.list_item.is_active = is_active;
    index.list_item.status = if is_active {
        ToolSessionStatus::Active
    } else {
        ToolSessionStatus::Inactive
    };
    index
}

// 启动 stdio JSON Lines provider；同一进程复用 provider 实例，让 snapshot 建立的索引可服务后续 detail。
pub fn run_stdio_session_provider() {
    let stdin = io::stdin();
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let provider = Arc::new(Mutex::new(CodexSessionProvider::from_config()));
    let _snapshot_notifier =
        start_snapshot_notifier(provider.clone(), stdout.clone(), SNAPSHOT_NOTIFY_INTERVAL);
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            eprintln!("NiumaNotifier Codex session provider stdin read failed");
            continue;
        };
        let request = match serde_json::from_str::<ProviderRpcRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("NiumaNotifier Codex session provider ignored invalid JSON: {error}");
                continue;
            }
        };
        let response = match provider.lock() {
            Ok(mut provider) => provider.handle_request(request),
            Err(_) => {
                eprintln!("NiumaNotifier Codex session provider state lock poisoned");
                break;
            }
        };
        if write_provider_message(&stdout, &response).is_err() {
            break;
        }
    }
}

pub fn handle_session_provider_request(request: ProviderRpcRequest) -> ProviderRpcResponse {
    CodexSessionProvider::from_config().handle_request(request)
}

struct SnapshotNotifierHandle {
    stop_tx: Option<mpsc::Sender<()>>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl Drop for SnapshotNotifierHandle {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

#[derive(Default)]
pub(crate) struct SnapshotNotifierState {
    fingerprint: Option<SnapshotFingerprint>,
}

fn start_snapshot_notifier<W>(
    provider: Arc<Mutex<CodexSessionProvider>>,
    writer: Arc<Mutex<W>>,
    interval: Duration,
) -> SnapshotNotifierHandle
where
    W: Write + Send + 'static,
{
    let (stop_tx, stop_rx) = mpsc::channel();
    let join_handle = thread::Builder::new()
        .name("codex-session-snapshot-notifier".to_string())
        .spawn(move || {
            let mut state = SnapshotNotifierState::default();
            loop {
                if let Err(error) = notify_snapshot_update_once(&provider, &writer, &mut state) {
                    eprintln!(
                        "NiumaNotifier Codex session provider snapshot notify failed: {error}"
                    );
                }
                if stop_rx.recv_timeout(interval).is_ok() {
                    break;
                }
            }
        })
        .ok();

    SnapshotNotifierHandle {
        stop_tx: Some(stop_tx),
        join_handle,
    }
}

pub(crate) fn notify_snapshot_update_once<W>(
    provider: &Arc<Mutex<CodexSessionProvider>>,
    writer: &Arc<Mutex<W>>,
    state: &mut SnapshotNotifierState,
) -> Result<bool, String>
where
    W: Write,
{
    let sessions = provider
        .lock()
        .map_err(|_| "Codex session provider state lock poisoned".to_string())?
        .refresh_snapshot()?;
    let next_fingerprint = SnapshotFingerprint::from_sessions(&sessions);
    let changed = state
        .fingerprint
        .as_ref()
        .is_some_and(|fingerprint| fingerprint != &next_fingerprint);
    state.fingerprint = Some(next_fingerprint);
    if !changed {
        return Ok(false);
    }

    let notification = ProviderRpcNotification::new(
        "session_snapshot_updated",
        SessionSnapshotResult {
            tool: ToolKind::Codex,
            sessions,
        },
    )?;
    write_provider_message(writer, &notification)?;
    Ok(true)
}

pub(crate) fn write_provider_message<W, T>(
    writer: &Arc<Mutex<W>>,
    message: &T,
) -> Result<(), String>
where
    W: Write,
    T: serde::Serialize,
{
    let encoded = serde_json::to_string(message)
        .map_err(|error| format!("序列化 provider RPC 消息失败：{error}"))?;
    // notification 与 response 共用 stdout；单点加锁写入，避免两个线程交错输出 JSONL。
    let mut writer = writer
        .lock()
        .map_err(|_| "Codex session provider stdout lock poisoned".to_string())?;
    writeln!(writer, "{encoded}").map_err(|error| format!("写入 provider stdout 失败：{error}"))?;
    writer
        .flush()
        .map_err(|error| format!("刷新 provider stdout 失败：{error}"))
}

#[derive(Eq, PartialEq)]
struct SnapshotFingerprint(Vec<SnapshotSessionFingerprint>);

impl SnapshotFingerprint {
    fn from_sessions(sessions: &[ToolSessionListItem]) -> Self {
        let mut entries = sessions
            .iter()
            .map(SnapshotSessionFingerprint::from)
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        Self(entries)
    }
}

#[derive(Eq, PartialEq)]
struct SnapshotSessionFingerprint {
    session_id: String,
    project_path: String,
    project_name: String,
    file_path: String,
    modified_at: DateTime<Utc>,
    is_active: bool,
    is_subagent: bool,
    parent_session_id: Option<String>,
    status: ToolSessionStatus,
}

impl From<&ToolSessionListItem> for SnapshotSessionFingerprint {
    fn from(session: &ToolSessionListItem) -> Self {
        Self {
            session_id: session.session_id.clone(),
            project_path: session.project_path.clone(),
            project_name: session.project_name.clone(),
            file_path: session.file_path.clone(),
            modified_at: session.modified_at,
            is_active: session.is_active,
            is_subagent: session.is_subagent,
            parent_session_id: session.parent_session_id.clone(),
            status: session.status.clone(),
        }
    }
}

fn detail_from_index(
    index: &SessionIndex,
    params: &SessionDetailParams,
) -> Result<ToolSessionDetail, DetailFromIndexError> {
    let before_line_index =
        parse_cursor(params.cursor.as_deref()).map_err(DetailFromIndexError::Provider)?;
    let page_size = params.limit.max(1);

    // cursor 是稳定的行号边界；追加的新消息行号更大，不会影响旧 cursor 的下一页结果。
    let page_line_indexes = index
        .message_line_indexes
        .iter()
        .filter(|line_index| before_line_index.is_none_or(|before| **line_index < before))
        .rev()
        .take(page_size)
        .copied()
        .collect::<Vec<_>>();
    let messages = read_messages_by_line_index(
        &index.list_item.file_path,
        &index.list_item.session_id,
        &page_line_indexes,
    )
    .map_err(DetailFromIndexError::Stale)?;
    let next_cursor = page_line_indexes
        .last()
        .copied()
        .filter(|oldest_returned| {
            index
                .message_line_indexes
                .iter()
                .any(|line_index| line_index < oldest_returned)
        })
        .map(|oldest_returned| format!("before:{oldest_returned}"));

    Ok(ToolSessionDetail {
        tool: ToolKind::Codex,
        session_id: index.list_item.session_id.clone(),
        project_path: index.list_item.project_path.clone(),
        project_name: index.list_item.project_name.clone(),
        is_subagent: false,
        parent_session_id: None,
        messages,
        next_cursor,
    })
}

enum DetailFromIndexError {
    Provider(ProviderError),
    Stale(String),
}

fn parse_session_file(path: &Path) -> Result<ParsedSessionFile, String> {
    let file = File::open(path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
    let reader = BufReader::new(file);
    let mut parsed = ParsedSessionFile::default();
    for (line_index, line) in reader.lines().enumerate() {
        let line = line.map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(row) = serde_json::from_str::<CodexRow>(line) {
            if row.row_type == "session_meta" {
                if let Some(session_id) = row
                    .payload
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    parsed.session_id = Some(session_id.to_string());
                }
                if let Some(cwd) = row
                    .payload
                    .get("cwd")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    parsed.project_path = Some(cwd.to_string());
                }
            }
        }
        if is_detail_message_line(line) {
            parsed.message_line_indexes.push(line_index);
        }
    }
    Ok(parsed)
}

fn read_messages_by_line_index(
    file_path: &str,
    session_id: &str,
    line_indexes: &[usize],
) -> Result<Vec<niuma_core::tool_session::ToolSessionMessage>, String> {
    if line_indexes.is_empty() {
        return Ok(Vec::new());
    }
    let wanted = line_indexes
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let file =
        File::open(file_path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
    let reader = BufReader::new(file);
    let mut messages_by_index = HashMap::new();
    for (line_index, line) in reader.lines().enumerate() {
        if !wanted.contains(&line_index) {
            continue;
        }
        let line = line.map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;
        let trimmed = line.trim_end_matches('\r');
        if !is_detail_message_line(trimmed) {
            return Err(format!(
                "Codex session 索引已过期，第 {} 行不再是详情消息",
                line_index + 1
            ));
        }
        messages_by_index.insert(
            line_index,
            parse_codex_message_line(session_id, line_index, trimmed),
        );
        if messages_by_index.len() == wanted.len() {
            break;
        }
    }
    // line_indexes 已经是倒序分页顺序，按该顺序组装消息，避免 HashMap 破坏排序。
    let mut messages = Vec::with_capacity(line_indexes.len());
    for line_index in line_indexes {
        let Some(message) = messages_by_index.remove(line_index) else {
            return Err(format!(
                "Codex session 索引已过期，缺少第 {} 行",
                line_index + 1
            ));
        };
        messages.push(message);
    }
    Ok(messages)
}

fn recent_session_files(codex_home: &Path) -> Vec<(PathBuf, SessionFileSignature)> {
    let mut files = Vec::new();
    for dir in session_day_dirs(codex_home) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let file_signature = entry
                .metadata()
                .and_then(|metadata| session_file_signature_from_metadata(&path, &metadata))
                .unwrap_or(SessionFileSignature {
                    modified_system_time: SystemTime::UNIX_EPOCH,
                    size_bytes: 0,
                    content_hash: 0,
                });
            files.push((path, file_signature));
        }
    }
    files.sort_by(|left, right| {
        right
            .1
            .modified_system_time
            .cmp(&left.1.modified_system_time)
    });
    files.truncate(SNAPSHOT_FILE_LIMIT);
    files
}

fn session_day_dirs(codex_home: &Path) -> Vec<PathBuf> {
    let sessions_dir = codex_home.join("sessions");
    let Ok(year_entries) = std::fs::read_dir(sessions_dir) else {
        return fallback_session_day_dirs(codex_home);
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
                if day_path.is_dir() {
                    dirs.push(day_path);
                }
            }
        }
    }
    dirs.sort_by(|left, right| right.cmp(left));
    dirs.truncate(SESSION_DAY_DIR_LIMIT);
    if dirs.is_empty() {
        fallback_session_day_dirs(codex_home)
    } else {
        dirs
    }
}

fn fallback_session_day_dirs(codex_home: &Path) -> Vec<PathBuf> {
    let today = Utc::now().date_naive();
    [today, today - chrono::Duration::days(1)]
        .iter()
        .map(|day| {
            codex_home
                .join("sessions")
                .join(day.format("%Y").to_string())
                .join(day.format("%m").to_string())
                .join(day.format("%d").to_string())
        })
        .collect()
}

fn parse_cursor(cursor: Option<&str>) -> Result<Option<usize>, ProviderError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let cursor = cursor.trim();
    let value = cursor
        .strip_prefix("before:")
        .unwrap_or(cursor)
        .parse::<usize>()
        .map_err(|_| {
            ProviderError::new(
                "invalid_cursor",
                format!("cursor 非法，必须是行号边界，例如 before:42：{cursor}"),
            )
        })?;
    Ok(Some(value))
}

fn session_file_signature(path: &Path) -> io::Result<SessionFileSignature> {
    let metadata = std::fs::metadata(path)?;
    session_file_signature_from_metadata(path, &metadata)
}

fn session_file_signature_from_metadata(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> io::Result<SessionFileSignature> {
    // 签名只读取原始字节，不解析 JSONL，避免把缓存校验退化成重复逐行索引。
    let content = std::fs::read(path)?;
    Ok(SessionFileSignature {
        modified_system_time: metadata.modified()?,
        size_bytes: metadata.len(),
        content_hash: stable_bytes_hash(&content),
    })
}

fn recently_modified(modified: SystemTime, max_age: Duration) -> bool {
    modified
        .elapsed()
        .map(|elapsed| elapsed <= max_age)
        .unwrap_or(true)
}

fn project_name(project_path: &str) -> String {
    project_path
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("Codex")
        .to_string()
}

fn filename_session_id(path: &Path) -> Option<String> {
    let basename = path.file_stem()?.to_str()?;
    let parts = basename.rsplit('-').take(5).collect::<Vec<_>>();
    if parts.len() != 5 {
        return None;
    }
    let candidate = parts.into_iter().rev().collect::<Vec<_>>().join("-");
    is_uuid_like(&candidate).then_some(candidate)
}

fn is_uuid_like(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.iter().map(|part| part.len()).collect::<Vec<_>>() != [8, 4, 4, 4, 12] {
        return false;
    }
    value
        .chars()
        .all(|char| char == '-' || char.is_ascii_hexdigit())
}

fn stable_hash(text: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:x}")
}

fn stable_bytes_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

struct ProviderError {
    code: &'static str,
    message: String,
}

impl ProviderError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn not_found(session_id: &str) -> Self {
        Self::new(
            "session_not_found",
            format!("session_id 不存在：{session_id}"),
        )
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new("provider_internal_error", message)
    }

    fn stale_session_file(message: impl Into<String>) -> Self {
        Self::new("stale_session_file", message)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_session_provider_snapshot_refreshes_same_size_replaced_file_by_content_hash() {
        let temp = tempfile::tempdir().unwrap();
        let session_a =
            test_session_content("session-alpha", "/tmp/project-a", "question A", "answer A");
        let session_b =
            test_session_content("session-bravo", "/tmp/project-b", "question B", "answer B");
        assert_eq!(session_a.len(), session_b.len());
        let path = write_test_session(temp.path(), &session_a);
        let mut provider = CodexSessionProvider::with_codex_home(temp.path().into());
        let first_snapshot = provider
            .handle_request(snapshot_request("req-first-snapshot"))
            .result_as::<SessionSnapshotResult>()
            .unwrap();
        assert!(first_snapshot
            .sessions
            .iter()
            .any(|session| session.session_id == "session-alpha"));
        let first_scan_count = provider.scan_count();

        std::fs::write(&path, &session_b).unwrap();
        let replaced_signature = session_file_signature(&path).unwrap();
        let cached_index = provider.index.get_mut("session-alpha").unwrap();
        // 模拟文件系统 mtime 精度不足或 mtime 被恢复：旧缓存只剩 content_hash 与新文件不同。
        cached_index.file_signature.modified_system_time = replaced_signature.modified_system_time;
        cached_index.file_signature.size_bytes = replaced_signature.size_bytes;
        assert_ne!(
            cached_index.file_signature.content_hash,
            replaced_signature.content_hash
        );

        let second_snapshot = provider
            .handle_request(snapshot_request("req-second-snapshot"))
            .result_as::<SessionSnapshotResult>()
            .unwrap();
        assert!(second_snapshot
            .sessions
            .iter()
            .any(|session| session.session_id == "session-bravo"));
        assert!(!second_snapshot
            .sessions
            .iter()
            .any(|session| session.session_id == "session-alpha"));
        assert_eq!(provider.scan_count(), first_scan_count + 1);

        let detail = provider
            .handle_request(detail_request_for_session(
                "req-detail",
                "session-bravo",
                20,
                None,
            ))
            .result_as::<SessionDetailResult>()
            .unwrap()
            .detail;
        assert_eq!(detail.session_id, "session-bravo");
        assert_eq!(detail.messages[0].content, "answer B");
        assert_eq!(detail.messages[1].content, "question B");
    }

    #[test]
    fn codex_session_provider_detail_refreshes_stale_index_after_file_truncate() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_test_session(
            temp.path(),
            concat!(
                "{\"timestamp\":\"2026-06-22T01:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-fixture\",\"cwd\":\"/tmp/fixture-project\"}}\n",
                "{\"timestamp\":\"2026-06-22T01:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"用户问题\"}]}}\n",
                "{\"timestamp\":\"2026-06-22T01:00:02Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"助手回答\"}]}}\n",
            ),
        );
        let mut provider = CodexSessionProvider::with_codex_home(temp.path().into());
        let _ = provider.handle_request(snapshot_request("req-snapshot"));

        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-06-22T01:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-fixture\",\"cwd\":\"/tmp/fixture-project\"}}\n",
                "{\"timestamp\":\"2026-06-22T01:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"用户问题\"}]}}\n",
            ),
        )
        .unwrap();
        let truncated_signature = session_file_signature(&path).unwrap();
        let index = provider.index.get_mut("session-fixture").unwrap();
        // 保留旧行号但同步文件签名，强制走“读取发现缺行后重建索引”的防护分支。
        index.file_signature = truncated_signature;

        let response = provider.handle_request(detail_request("req-detail", 2, None));
        assert!(response.error.is_none());
        let detail = response.result_as::<SessionDetailResult>().unwrap().detail;

        assert_eq!(detail.messages.len(), 1);
        assert_eq!(detail.messages[0].content, "用户问题");
        assert_eq!(detail.next_cursor, None);
    }

    fn snapshot_request(id: &str) -> ProviderRpcRequest {
        ProviderRpcRequest::new(
            id,
            "session_snapshot",
            SessionSnapshotParams {
                tool: ToolKind::Codex,
            },
        )
        .unwrap()
    }

    fn detail_request(id: &str, limit: usize, cursor: Option<&str>) -> ProviderRpcRequest {
        detail_request_for_session(id, "session-fixture", limit, cursor)
    }

    fn detail_request_for_session(
        id: &str,
        session_id: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> ProviderRpcRequest {
        ProviderRpcRequest::new(
            id,
            "session_detail",
            SessionDetailParams {
                tool: ToolKind::Codex,
                session_id: session_id.to_string(),
                limit,
                cursor: cursor.map(ToString::to_string),
            },
        )
        .unwrap()
    }

    fn test_session_content(
        session_id: &str,
        project_path: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> String {
        format!(
            "{{\"timestamp\":\"2026-06-22T01:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\",\"cwd\":\"{project_path}\"}}}}\n\
             {{\"timestamp\":\"2026-06-22T01:00:01Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{user_message}\"}}]}}}}\n\
             {{\"timestamp\":\"2026-06-22T01:00:02Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{assistant_message}\"}}]}}}}\n",
        )
    }

    fn write_test_session(codex_home: &Path, content: &str) -> PathBuf {
        let day_dir = codex_home.join("sessions/2026/06/22");
        std::fs::create_dir_all(&day_dir).unwrap();
        let path = day_dir.join("rollout-2026-06-22-00000000-0000-0000-0000-000000000000.jsonl");
        std::fs::write(&path, content).unwrap();
        path
    }
}
