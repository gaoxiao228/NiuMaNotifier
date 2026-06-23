use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use niuma_core::models::ToolKind;
use niuma_core::tool_session::{ToolSessionDetail, ToolSessionListItem, ToolSessionStatus};
use niuma_core::tool_session_rpc::{
    ProviderRpcRequest, ProviderRpcResponse, SessionDetailParams, SessionDetailResult,
    SessionSnapshotParams, SessionSnapshotResult,
};
use serde::Deserialize;

use crate::session_messages::{is_detail_message_line, parse_codex_message_line};

const SNAPSHOT_FILE_LIMIT: usize = 128;
const SESSION_DAY_DIR_LIMIT: usize = 180;
const ACTIVE_MODIFIED_WINDOW: Duration = Duration::from_secs(60);

pub struct CodexSessionProvider {
    codex_home: PathBuf,
    index: HashMap<String, SessionIndex>,
}

#[derive(Clone)]
struct SessionIndex {
    list_item: ToolSessionListItem,
    modified_system_time: SystemTime,
    // 只保存可分页消息的原始 JSONL 行号，不在 provider 内存中长期持有完整对话正文。
    message_line_indexes: Vec<usize>,
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
        }
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
        for (path, modified_system_time) in recent_session_files(&self.codex_home) {
            match self.scan_session_file(&path, modified_system_time, now) {
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
        let index = self
            .index
            .get(&params.session_id)
            .cloned()
            .ok_or_else(|| ProviderError::not_found(&params.session_id))?;
        let total = index.message_line_indexes.len();
        let start = parse_cursor(params.cursor.as_deref(), total)?;
        let page_size = params.limit.max(1);

        // cursor 表示倒序消息列表中的起始偏移；line_indexes 本身按文件顺序保存，分页时再反向迭代。
        let page_line_indexes = index
            .message_line_indexes
            .iter()
            .rev()
            .skip(start)
            .take(page_size)
            .copied()
            .collect::<Vec<_>>();
        let messages = read_messages_by_line_index(
            &index.list_item.file_path,
            &index.list_item.session_id,
            &page_line_indexes,
        )
        .map_err(|error| ProviderError::internal(error))?;
        let next_offset = start + messages.len();
        let next_cursor = (next_offset < total).then(|| next_offset.to_string());

        Ok(ToolSessionDetail {
            tool: ToolKind::Codex,
            session_id: index.list_item.session_id,
            project_path: index.list_item.project_path,
            project_name: index.list_item.project_name,
            is_subagent: false,
            parent_session_id: None,
            messages,
            next_cursor,
        })
    }

    fn ensure_session_index(&mut self, session_id: &str) -> Result<(), ProviderError> {
        if !self.index.contains_key(session_id) {
            self.refresh_snapshot().map_err(ProviderError::internal)?;
        }
        let Some(index) = self.index.get(session_id).cloned() else {
            return Err(ProviderError::not_found(session_id));
        };
        let path = PathBuf::from(&index.list_item.file_path);
        let modified_system_time = file_modified_time(&path).map_err(|error| {
            ProviderError::internal(format!("读取 Codex session 文件失败：{error}"))
        })?;
        if modified_system_time != index.modified_system_time {
            let refreshed = self
                .scan_session_file(&path, modified_system_time, Utc::now())
                .map_err(ProviderError::internal)?;
            let refreshed_session_id = refreshed.list_item.session_id.clone();
            // 文件被截断或替换后可能属于另一个 session，旧 session_id 不能继续命中旧索引。
            self.index.remove(session_id);
            self.index.insert(refreshed_session_id.clone(), refreshed);
            if refreshed_session_id != session_id {
                return Err(ProviderError::not_found(session_id));
            }
        }
        Ok(())
    }

    fn scan_session_file(
        &self,
        path: &Path,
        modified_system_time: SystemTime,
        discovered_at: DateTime<Utc>,
    ) -> Result<SessionIndex, String> {
        let parsed = parse_session_file(path)?;
        let fallback_path = path.to_string_lossy();
        let session_id = parsed
            .session_id
            .or_else(|| filename_session_id(path))
            .unwrap_or_else(|| format!("fallback-{}", stable_hash(&fallback_path)));
        let project_path = parsed.project_path.unwrap_or_default();
        let project_name = project_name(&project_path);
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
            modified_system_time,
            message_line_indexes: parsed.message_line_indexes,
        })
    }
}

// 启动 stdio JSON Lines provider；同一进程复用 provider 实例，让 snapshot 建立的索引可服务后续 detail。
pub fn run_stdio_session_provider() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut provider = CodexSessionProvider::from_config();
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
        let response = provider.handle_request(request);
        let Ok(encoded) = serde_json::to_string(&response) else {
            eprintln!("NiumaNotifier Codex session provider response serialize failed");
            continue;
        };
        if writeln!(stdout, "{encoded}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            break;
        }
    }
}

pub fn handle_session_provider_request(request: ProviderRpcRequest) -> ProviderRpcResponse {
    CodexSessionProvider::from_config().handle_request(request)
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
        messages_by_index.insert(
            line_index,
            parse_codex_message_line(session_id, line_index, trimmed),
        );
        if messages_by_index.len() == wanted.len() {
            break;
        }
    }
    // line_indexes 已经是倒序分页顺序，按该顺序组装消息，避免 HashMap 破坏排序。
    Ok(line_indexes
        .iter()
        .filter_map(|line_index| messages_by_index.remove(line_index))
        .collect())
}

fn recent_session_files(codex_home: &Path) -> Vec<(PathBuf, SystemTime)> {
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
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((path, modified));
        }
    }
    files.sort_by(|left, right| right.1.cmp(&left.1));
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

fn parse_cursor(cursor: Option<&str>, total: usize) -> Result<usize, ProviderError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let value = cursor.trim().parse::<usize>().map_err(|_| {
        ProviderError::new(
            "invalid_cursor",
            format!("cursor 非法，必须是倒序消息偏移：{cursor}"),
        )
    })?;
    if value > total {
        return Err(ProviderError::new(
            "invalid_cursor",
            format!("cursor 超出消息范围：{cursor}"),
        ));
    }
    Ok(value)
}

fn file_modified_time(path: &Path) -> io::Result<SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified())
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
}
