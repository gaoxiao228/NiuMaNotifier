use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use niuma_core::models::ToolKind;
use niuma_core::tool_session::{ToolSessionDetail, ToolSessionListItem, ToolSessionStatus};
use niuma_core::tool_session_rpc::SessionDetailParams;
use serde::Deserialize;

use crate::codex::session_file_index::{
    read_indexed_line, session_file_signature, trim_jsonl_line_bytes,
    CodexMessageLineIndex as MessageLineIndex, CodexSessionFileIndex,
    CodexSessionFileSignature as SessionFileSignature,
};
use crate::codex::session_identity::{
    codex_fallback_session_id, codex_project_name, CodexSessionIdentity, CodexSessionMetadata,
};
use crate::session_messages::{is_detail_message_line, parse_codex_message_line};

const SNAPSHOT_FILE_LIMIT: usize = 128;
const SESSION_DAY_DIR_LIMIT: usize = 180;
const ACTIVE_MODIFIED_WINDOW: Duration = Duration::from_secs(60);

pub(crate) struct CodexSessionRepository {
    codex_home: PathBuf,
    index: HashMap<String, SessionIndex>,
    #[cfg(test)]
    scan_count: usize,
}

#[derive(Clone)]
pub(crate) struct SessionIndex {
    pub(crate) list_item: ToolSessionListItem,
    pub(crate) file_index: CodexSessionFileIndex,
}

// 候选阶段只保留轻量 metadata；排序截断后才读取文件内容计算 hash。
struct SessionFileCandidate {
    path: PathBuf,
    modified_system_time: SystemTime,
    size_bytes: u64,
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
    session_metadata: CodexSessionMetadata,
    session_meta_line: Option<MessageLineIndex>,
    message_lines: Vec<MessageLineIndex>,
}

impl CodexSessionRepository {
    pub(crate) fn new(codex_home: PathBuf) -> Self {
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

    #[cfg(test)]
    pub(crate) fn index_mut(&mut self, session_id: &str) -> Option<&mut SessionIndex> {
        self.index.get_mut(session_id)
    }

    #[cfg(test)]
    pub(crate) fn contains_index(&self, session_id: &str) -> bool {
        self.index.contains_key(session_id)
    }

    pub(crate) fn refresh_snapshot(&mut self) -> Result<Vec<ToolSessionListItem>, String> {
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
                .filter(|index| index.file_index.signature == file_signature)
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

    pub(crate) fn session_detail(
        &mut self,
        params: SessionDetailParams,
    ) -> Result<ToolSessionDetail, ProviderError> {
        self.ensure_session_index(&params.session_id)?;
        if params.cursor.is_none() {
            self.refresh_latest_detail_index_if_file_metadata_changed(&params.session_id)?;
        }
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
                    // range 索引可能来自旧文件内容；强制重扫一次，避免用旧 session 或缺行数量推进 cursor。
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
        if !self.index.contains_key(session_id) {
            return Err(ProviderError::not_found(session_id));
        }
        Ok(())
    }

    fn refresh_latest_detail_index_if_file_metadata_changed(
        &mut self,
        session_id: &str,
    ) -> Result<(), ProviderError> {
        let index = self
            .index
            .get(session_id)
            .cloned()
            .ok_or_else(|| ProviderError::not_found(session_id))?;
        if session_file_metadata_changed(&index)? {
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
            .clone()
            .unwrap_or_else(|| codex_fallback_session_id(&fallback_path));
        let project_path = parsed.project_path.clone().unwrap_or_default();
        let project_name = codex_project_name(&project_path);
        let identity = session_identity(&session_id, &parsed);
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
            is_subagent: identity.session_scope.is_subagent(),
            parent_session_id: identity.parent_session_id,
            normalized_session_id: Some(identity.normalized_session_id),
            session_scope: Some(identity.session_scope.as_tool_scope()),
            agent_nickname: identity.agent_nickname,
            agent_role: identity.agent_role,
            normalization_status: Some(identity.normalization_status),
            status,
        };

        Ok(SessionIndex {
            list_item,
            file_index: CodexSessionFileIndex {
                signature: file_signature,
                session_meta_line: parsed.session_meta_line,
                message_lines: parsed.message_lines,
            },
        })
    }
}

fn refresh_cached_index(mut index: SessionIndex, discovered_at: DateTime<Utc>) -> SessionIndex {
    // 文件内容未变化时只刷新列表态字段，复用行号索引，避免后台 notifier 重复解析 JSONL。
    let modified_system_time = index.file_index.signature.modified_system_time;
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

fn detail_from_index(
    index: &SessionIndex,
    params: &SessionDetailParams,
) -> Result<ToolSessionDetail, DetailFromIndexError> {
    verify_session_identity(index).map_err(DetailFromIndexError::Stale)?;
    let before_line_index =
        parse_cursor(params.cursor.as_deref()).map_err(DetailFromIndexError::Provider)?;
    let page_size = params.limit.max(1);

    // cursor 是稳定的行号边界；追加的新消息行号更大，不会影响旧 cursor 的下一页结果。
    let page_lines = index
        .file_index
        .message_lines
        .iter()
        .filter(|line| before_line_index.is_none_or(|before| line.line_index < before))
        .rev()
        .take(page_size)
        .copied()
        .collect::<Vec<_>>();
    let messages = read_messages_by_range(
        &index.list_item.file_path,
        &index.list_item.session_id,
        &page_lines,
    )
    .map_err(DetailFromIndexError::Stale)?;
    let next_cursor = page_lines
        .last()
        .map(|line| line.line_index)
        .filter(|oldest_returned| {
            index
                .file_index
                .message_lines
                .iter()
                .any(|line| line.line_index < *oldest_returned)
        })
        .map(|oldest_returned| format!("before:{oldest_returned}"));

    Ok(ToolSessionDetail {
        tool: ToolKind::Codex,
        session_id: index.list_item.session_id.clone(),
        project_path: index.list_item.project_path.clone(),
        project_name: index.list_item.project_name.clone(),
        is_subagent: index.list_item.is_subagent,
        parent_session_id: index.list_item.parent_session_id.clone(),
        normalized_session_id: index.list_item.normalized_session_id.clone(),
        session_scope: index.list_item.session_scope.clone(),
        agent_nickname: index.list_item.agent_nickname.clone(),
        agent_role: index.list_item.agent_role.clone(),
        normalization_status: index.list_item.normalization_status.clone(),
        messages,
        next_cursor,
    })
}

fn verify_session_identity(index: &SessionIndex) -> Result<(), String> {
    let Some(session_meta_line) = index.file_index.session_meta_line else {
        return Err("Codex session 缺少 session_meta，无法校验会话身份".to_string());
    };
    let line = read_indexed_line(&index.list_item.file_path, &session_meta_line)?;
    let row: CodexRow = serde_json::from_str(line.trim_end_matches('\r'))
        .map_err(|error| format!("Codex session_meta 已失效：{error}"))?;
    let current_session_id = row
        .payload
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if row.row_type != "session_meta" || current_session_id != index.list_item.session_id {
        return Err("Codex session_meta 已变更".to_string());
    }
    Ok(())
}

enum DetailFromIndexError {
    Provider(ProviderError),
    Stale(String),
}

fn parse_session_file(path: &Path) -> Result<ParsedSessionFile, String> {
    let file = File::open(path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
    let mut reader = BufReader::new(file);
    let mut parsed = ParsedSessionFile::default();
    let mut line_index = 0usize;
    let mut byte_start = 0u64;
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let bytes_read = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;
        if bytes_read == 0 {
            break;
        }
        let next_byte_start = byte_start + bytes_read as u64;
        let line_bytes = trim_jsonl_line_bytes(&buffer);
        let line = String::from_utf8_lossy(line_bytes);
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            line_index += 1;
            byte_start = next_byte_start;
            continue;
        }
        if let Ok(row) = serde_json::from_str::<CodexRow>(line) {
            if row.row_type == "session_meta" {
                let current_line_index = MessageLineIndex::new(line_index, byte_start, line_bytes);
                let row_session_id = row
                    .payload
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty());
                if parsed.session_meta_line.is_none() {
                    parsed.session_meta_line = Some(current_line_index);
                }
                if let Some(session_id) = row_session_id.filter(|_| parsed.session_id.is_none()) {
                    // raw session_id 只由首个有效 session_meta 决定，避免后续 parent meta 覆盖文件身份。
                    parsed.session_id = Some(session_id.to_string());
                    parsed.session_meta_line = Some(current_line_index);
                }
                if let Some(cwd) = row
                    .payload
                    .get("cwd")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .filter(|_| parsed.project_path.is_none())
                {
                    parsed.project_path = Some(cwd.to_string());
                }
                parsed.session_metadata.merge_session_meta(&row.payload);
            }
        }
        if is_detail_message_line(line) {
            parsed
                .message_lines
                .push(MessageLineIndex::new(line_index, byte_start, line_bytes));
        }
        line_index += 1;
        byte_start = next_byte_start;
    }
    Ok(parsed)
}

fn session_identity(session_id: &str, parsed: &ParsedSessionFile) -> CodexSessionIdentity {
    parsed.session_metadata.identity_for_session(session_id)
}

fn read_messages_by_range(
    file_path: &str,
    session_id: &str,
    message_lines: &[MessageLineIndex],
) -> Result<Vec<niuma_core::tool_session::ToolSessionMessage>, String> {
    if message_lines.is_empty() {
        return Ok(Vec::new());
    }
    // message_lines 已经是倒序分页顺序；按 range 读取本页需要的行，避免从文件头扫描。
    let mut messages = Vec::with_capacity(message_lines.len());
    for line_index in message_lines {
        let line = read_indexed_line(file_path, line_index)?;
        let trimmed = line.trim_end_matches('\r');
        if !is_detail_message_line(trimmed) {
            return Err(format!(
                "Codex session 索引已过期，第 {} 行不再是详情消息",
                line_index.line_index + 1
            ));
        }
        messages.push(parse_codex_message_line(
            session_id,
            line_index.line_index,
            trimmed,
        ));
    }
    Ok(messages)
}

fn recent_session_files(codex_home: &Path) -> Vec<(PathBuf, SessionFileSignature)> {
    let mut files = Vec::new();
    for dir in codex_session_day_dirs(codex_home) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            files.push(SessionFileCandidate {
                path,
                modified_system_time: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                size_bytes: metadata.len(),
            });
        }
    }
    files.sort_by(|left, right| right.modified_system_time.cmp(&left.modified_system_time));
    files.truncate(SNAPSHOT_FILE_LIMIT);
    files
        .into_iter()
        .map(|candidate| {
            let signature = session_file_signature(&candidate.path).unwrap_or_else(|_| {
                SessionFileSignature::fallback(candidate.modified_system_time, candidate.size_bytes)
            });
            (candidate.path, signature)
        })
        .collect()
}

pub(crate) fn codex_session_day_dirs(codex_home: &Path) -> Vec<PathBuf> {
    let sessions_dir = codex_home.join("sessions");
    let Ok(year_entries) = std::fs::read_dir(sessions_dir) else {
        return codex_fallback_session_day_dirs(codex_home, Utc::now());
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
                // Codex session 文件按 sessions/YYYY/MM/DD 归档；只返回日目录避免递归进无关层级。
                if day_path.is_dir() {
                    dirs.push(day_path);
                }
            }
        }
    }
    dirs.sort_by(|left, right| right.cmp(left));
    dirs.truncate(SESSION_DAY_DIR_LIMIT);
    if dirs.is_empty() {
        codex_fallback_session_day_dirs(codex_home, Utc::now())
    } else {
        dirs
    }
}

pub(crate) fn codex_fallback_session_day_dirs(
    codex_home: &Path,
    now: DateTime<Utc>,
) -> Vec<PathBuf> {
    let today = now.date_naive();
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

fn session_file_metadata_changed(index: &SessionIndex) -> Result<bool, ProviderError> {
    index
        .file_index
        .signature
        .metadata_changed(&index.list_item.file_path)
        .map_err(|error| ProviderError::internal(format!("读取 Codex session 文件失败：{error}")))
}

fn recently_modified(modified: SystemTime, max_age: Duration) -> bool {
    modified
        .elapsed()
        .map(|elapsed| elapsed <= max_age)
        .unwrap_or(true)
}

pub(crate) struct ProviderError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
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
