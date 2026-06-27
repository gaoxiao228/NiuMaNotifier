use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use niuma_core::codex_managed_session::{
    first_user_message_hash, match_managed_session, read_registry, update_registry, BindingMatch,
    CodexSessionBindingCandidate, ManagedCodexRegistry, ManagedCodexSessionState,
};
use niuma_core::models::ToolKind;
use niuma_core::tool_session::{
    ToolSessionControl, ToolSessionControlAction, ToolSessionControlChannel, ToolSessionDetail,
    ToolSessionListItem, ToolSessionMessageRole, ToolSessionStatus,
};
use niuma_core::tool_session_rpc::SessionDetailParams;
use serde::Deserialize;

use crate::codex::session_event_cursor::{file_identity, CodexEventCursor, CodexFileIdentity};
use crate::codex::session_file_index::{
    read_indexed_line, session_file_signature, trim_jsonl_line_bytes,
    CodexMessageLineIndex as MessageLineIndex, CodexSessionFileIndex,
    CodexSessionFileSignature as SessionFileSignature,
};
use crate::codex::session_identity::{
    codex_fallback_session_id, codex_project_name, CodexSessionIdentity, CodexSessionMetadata,
};
use crate::codex::session_protocol::current::CodexJsonlParser;
use crate::session_messages::{
    detail_message_signature, is_detail_message_line, parse_codex_message_line,
    DetailMessageSignature,
};

const SNAPSHOT_FILE_LIMIT: usize = 128;
const SESSION_DAY_DIR_LIMIT: usize = 180;
const ACTIVE_MODIFIED_WINDOW: Duration = Duration::from_secs(60);
const PRIME_METADATA_MAX_BYTES: u64 = 64 * 1024;
const FIRST_USER_MESSAGE_PREVIEW_CHARS: usize = 200;
const MANAGED_BINDING_WINDOW: chrono::Duration = chrono::Duration::seconds(10);
const BINDING_DIAGNOSTIC_LOG_PATH: &str = "/tmp/niuma-codex-binding-diagnostic.log";

pub(crate) struct CodexSessionRepository {
    codex_home: PathBuf,
    managed_registry_path: PathBuf,
    index: HashMap<String, SessionIndex>,
    event_cursors: HashMap<PathBuf, CodexEventCursor>,
    #[cfg(test)]
    scan_count: usize,
}

#[derive(Clone)]
pub(crate) struct SessionIndex {
    pub(crate) list_item: ToolSessionListItem,
    pub(crate) file_index: CodexSessionFileIndex,
    first_user_message_hash: Option<String>,
}

pub(crate) struct CodexSnapshotContext {
    codex_home: PathBuf,
    previous_by_path: HashMap<String, SessionIndex>,
    managed_registry_path: PathBuf,
}

pub(crate) struct CodexSnapshotRefresh {
    next_index: HashMap<String, SessionIndex>,
    sessions: Vec<ToolSessionListItem>,
    #[cfg(test)]
    scanned_count: usize,
}

pub(crate) struct CodexEventScanPlan {
    pub(crate) read_start: u64,
    pub(crate) file_len: u64,
    pub(crate) reset_parser: bool,
    pub(crate) file_replaced: bool,
    pub(crate) file_truncated: bool,
    pub(crate) file_identity: Option<CodexFileIdentity>,
}

pub(crate) struct CodexEventLine {
    pub(crate) line: String,
    pub(crate) byte_start: u64,
    pub(crate) byte_end: u64,
}

pub(crate) struct CodexEventReadResult {
    pub(crate) lines: Vec<CodexEventLine>,
    pub(crate) next_offset: u64,
    pub(crate) file_len: u64,
    pub(crate) file_identity: Option<CodexFileIdentity>,
    pub(crate) reset_parser: bool,
    pub(crate) file_replaced: bool,
    pub(crate) file_truncated: bool,
    pub(crate) ended_with_partial_line: bool,
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
    first_user_message: Option<FirstUserMessage>,
    message_lines: Vec<MessageLineIndex>,
}

#[derive(Clone)]
struct FirstUserMessage {
    preview: String,
    hash: String,
    created_at: DateTime<Utc>,
}

struct DetailMessageCandidate {
    line_index: MessageLineIndex,
    signature: Option<DetailMessageSignature>,
}

impl CodexSessionRepository {
    pub(crate) fn new(codex_home: PathBuf) -> Self {
        Self::with_managed_registry_path(
            codex_home,
            niuma_core::platform::paths::codex_managed_registry_path(),
        )
    }

    pub(crate) fn with_managed_registry_path(
        codex_home: PathBuf,
        managed_registry_path: PathBuf,
    ) -> Self {
        Self {
            codex_home,
            managed_registry_path,
            index: HashMap::new(),
            event_cursors: HashMap::new(),
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

    pub(crate) fn snapshot_context(&self) -> CodexSnapshotContext {
        let previous_by_path = self
            .index
            .values()
            .map(|index| (index.list_item.file_path.clone(), index.clone()))
            .collect::<HashMap<_, _>>();
        CodexSnapshotContext {
            codex_home: self.codex_home.clone(),
            previous_by_path,
            managed_registry_path: self.managed_registry_path.clone(),
        }
    }

    pub(crate) fn build_snapshot_refresh(
        context: CodexSnapshotContext,
    ) -> Result<CodexSnapshotRefresh, String> {
        let now = Utc::now();
        let mut managed_registry =
            read_registry(&context.managed_registry_path).unwrap_or_else(|error| {
                if context.managed_registry_path.exists() {
                    eprintln!("NiumaNotifier Codex managed registry 读取失败：{error}");
                }
                ManagedCodexRegistry::default()
            });
        let mut next_index = HashMap::new();
        #[cfg(test)]
        let mut scanned_count = 0;
        for (path, file_signature) in recent_session_files(&context.codex_home) {
            let file_path = path.to_string_lossy().to_string();
            let result = context
                .previous_by_path
                .get(&file_path)
                .filter(|index| index.file_index.signature == file_signature)
                .cloned()
                .map(|index| Ok(refresh_cached_index(index, now)))
                .unwrap_or_else(|| {
                    #[cfg(test)]
                    {
                        scanned_count += 1;
                    }
                    Self::scan_session_file_index(&path, file_signature, now)
                });
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
        let candidates = managed_binding_candidates(&next_index);
        if !candidates.is_empty() {
            match bind_managed_registry_sessions(&context.managed_registry_path, &candidates) {
                Ok(registry) => managed_registry = registry,
                Err(error) => eprintln!("NiumaNotifier Codex managed registry 绑定失败：{error}"),
            }
        }
        for index in next_index.values_mut() {
            index.list_item.control =
                control_for_session(&index.list_item.session_id, &managed_registry);
        }

        let mut sessions = next_index
            .values()
            .map(|entry| entry.list_item.clone())
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| right.modified_at.cmp(&left.modified_at));
        Ok(CodexSnapshotRefresh {
            next_index,
            sessions,
            #[cfg(test)]
            scanned_count,
        })
    }

    pub(crate) fn apply_snapshot_refresh(
        &mut self,
        refresh: CodexSnapshotRefresh,
    ) -> Vec<ToolSessionListItem> {
        #[cfg(test)]
        {
            self.scan_count += refresh.scanned_count;
        }
        self.index = refresh.next_index;
        refresh.sessions
    }

    pub(crate) fn session_index(&self, session_id: &str) -> Option<SessionIndex> {
        self.index.get(session_id).cloned()
    }

    pub(crate) fn session_file_metadata_changed(
        index: &SessionIndex,
    ) -> Result<bool, ProviderError> {
        session_file_metadata_changed(index)
    }

    pub(crate) fn detail_from_session_index(
        index: &SessionIndex,
        params: &SessionDetailParams,
    ) -> Result<ToolSessionDetail, DetailFromIndexError> {
        detail_from_index(index, params)
    }

    pub(crate) fn rebuild_session_index_from_file(
        index: &SessionIndex,
    ) -> Result<SessionIndex, ProviderError> {
        let path = PathBuf::from(&index.list_item.file_path);
        let file_signature = session_file_signature(&path).map_err(|error| {
            ProviderError::internal(format!("读取 Codex session 文件失败：{error}"))
        })?;
        Self::scan_session_file_index(&path, file_signature, Utc::now())
            .map_err(ProviderError::internal)
    }

    pub(crate) fn replace_session_index(
        &mut self,
        session_id: &str,
        refreshed: SessionIndex,
    ) -> Result<(), ProviderError> {
        let refreshed_session_id = refreshed.list_item.session_id.clone();
        // 文件被截断或替换后可能属于另一个 session，旧 session_id 不能继续命中旧索引。
        self.index.remove(session_id);
        self.index.insert(refreshed_session_id.clone(), refreshed);
        if refreshed_session_id != session_id {
            return Err(ProviderError::not_found(session_id));
        }
        Ok(())
    }

    pub(crate) fn clear_runtime_indexes(&mut self) {
        self.index.clear();
        self.event_cursors.clear();
    }

    pub(crate) fn event_scan_plan(
        path: &Path,
        cursor: Option<&CodexEventCursor>,
    ) -> Result<CodexEventScanPlan, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("读取 Codex session 文件信息失败：{error}"))?;
        let file_len = metadata.len();
        let current_identity = file_identity(&metadata);
        let (previous_offset, previous_len, previous_identity) = cursor
            .map(|cursor| (cursor.offset, cursor.last_len, cursor.file_identity))
            .unwrap_or((0, 0, None));
        let file_replaced = previous_identity.is_some()
            && current_identity.is_some()
            && previous_identity != current_identity;
        let file_truncated = file_len < previous_len || previous_offset > file_len;
        let reset_parser = cursor.is_none() || file_replaced || file_truncated;
        let read_start = if reset_parser { 0 } else { previous_offset };

        Ok(CodexEventScanPlan {
            read_start,
            file_len,
            reset_parser,
            file_replaced,
            file_truncated,
            file_identity: current_identity,
        })
    }

    pub(crate) fn read_new_event_lines(
        path: &Path,
        cursor: Option<&CodexEventCursor>,
    ) -> Result<CodexEventReadResult, String> {
        let plan = Self::event_scan_plan(path, cursor)?;
        let mut file =
            File::open(path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
        file.seek(SeekFrom::Start(plan.read_start))
            .map_err(|error| format!("定位 Codex session 文件失败：{error}"))?;

        let mut buffer = String::new();
        file.read_to_string(&mut buffer)
            .map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;

        let mut lines = Vec::new();
        let mut next_offset = plan.read_start;
        let mut ended_with_partial_line = false;
        for segment in buffer.split_inclusive('\n') {
            // 最后一段未落盘换行时不推进 offset，等待下次补齐后再解析。
            if !segment.ends_with('\n') {
                ended_with_partial_line = !segment.is_empty();
                break;
            }
            let byte_start = next_offset;
            next_offset += segment.as_bytes().len() as u64;
            lines.push(CodexEventLine {
                line: segment.trim_end_matches('\n').to_string(),
                byte_start,
                byte_end: next_offset,
            });
        }

        Ok(CodexEventReadResult {
            lines,
            next_offset,
            file_len: plan.file_len,
            file_identity: plan.file_identity,
            reset_parser: plan.reset_parser,
            file_replaced: plan.file_replaced,
            file_truncated: plan.file_truncated,
            ended_with_partial_line,
        })
    }

    #[cfg(test)]
    pub(crate) fn event_cursor(&self, path: &Path) -> Option<&CodexEventCursor> {
        self.event_cursors.get(path)
    }

    pub(crate) fn event_cursor_cloned(&self, path: &Path) -> Option<CodexEventCursor> {
        self.event_cursors.get(path).cloned()
    }

    pub(crate) fn store_event_cursor(&mut self, path: &Path, cursor: CodexEventCursor) {
        self.event_cursors.insert(path.to_path_buf(), cursor);
    }

    pub(crate) fn prime_event_cursor_to_end(path: &Path) -> Result<CodexEventCursor, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("读取 Codex session 文件信息失败：{error}"))?;
        let mut parser = CodexJsonlParser::default();
        prime_event_parser_metadata(path, &mut parser, PRIME_METADATA_MAX_BYTES)?;
        let file_len = metadata.len();

        // 旧 session 文件首次纳入监听时跳到尾部，但保留 session_meta 中的项目上下文。
        Ok(CodexEventCursor {
            offset: file_len,
            last_len: file_len,
            file_identity: file_identity(&metadata),
            parser,
        })
    }

    fn scan_session_file_index(
        path: &Path,
        file_signature: SessionFileSignature,
        discovered_at: DateTime<Utc>,
    ) -> Result<SessionIndex, String> {
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
            first_user_message_preview: parsed
                .first_user_message
                .as_ref()
                .map(|message| message.preview.clone()),
            first_user_message_at: parsed
                .first_user_message
                .as_ref()
                .map(|message| message.created_at),
            control: None,
            status,
        };

        Ok(SessionIndex {
            first_user_message_hash: parsed
                .first_user_message
                .as_ref()
                .map(|message| message.hash.clone()),
            list_item,
            file_index: CodexSessionFileIndex {
                signature: file_signature,
                session_meta_line: parsed.session_meta_line,
                message_lines: parsed.message_lines,
            },
        })
    }
}

fn managed_binding_candidates(
    next_index: &HashMap<String, SessionIndex>,
) -> Vec<CodexSessionBindingCandidate> {
    next_index
        .values()
        .map(|index| CodexSessionBindingCandidate {
            session_id: index.list_item.session_id.clone(),
            session_file_path: index.list_item.file_path.clone(),
            project_path: index.list_item.project_path.clone(),
            first_user_message_hash: index.first_user_message_hash.clone(),
            first_user_message_at: index.list_item.first_user_message_at,
        })
        .collect()
}

fn bind_managed_registry_sessions(
    registry_path: &Path,
    candidates: &[CodexSessionBindingCandidate],
) -> Result<ManagedCodexRegistry, String> {
    update_registry(registry_path, |registry| {
        for session in registry.sessions.iter_mut() {
            if session.state != ManagedCodexSessionState::BindingPending {
                continue;
            }
            match match_managed_session(session, candidates, MANAGED_BINDING_WINDOW) {
                BindingMatch::Unique {
                    session_id,
                    session_file_path,
                } => {
                    if let Err(error) = append_binding_diagnostic_log(
                        Path::new(BINDING_DIAGNOSTIC_LOG_PATH),
                        &session.wrapper_session_id,
                        session.codex_session_id.as_deref(),
                        &session_id,
                        &session_file_path,
                    ) {
                        eprintln!("NiumaNotifier Codex binding diagnostic 写入失败：{error}");
                    }
                    session.state = ManagedCodexSessionState::Bound;
                    session.codex_session_id = Some(session_id);
                    session.codex_session_file_path = Some(session_file_path);
                    session.bound_at = Some(Utc::now());
                    session.binding_failure_reason = None;
                }
                BindingMatch::Ambiguous => {
                    session.state = ManagedCodexSessionState::Ambiguous;
                    session.binding_failure_reason =
                        Some("第一条用户消息和时间窗口匹配到多个 Codex session".to_string());
                }
                BindingMatch::None => {}
            }
        }
    })
}

fn binding_diagnostic_log_line(
    wrapper_session_id: &str,
    relay_thread_id: Option<&str>,
    session_meta_id: &str,
    session_file_path: &str,
) -> String {
    let relay_thread_id = relay_thread_id.unwrap_or("<none>");
    format!(
        "NiumaNotifier niuma-codex binding diagnostic: wrapper_session_id={wrapper_session_id} relay_thread_id={relay_thread_id} session_meta_id={session_meta_id} session_file_path={session_file_path} thread_id_matches_session_id={}",
        relay_thread_id == session_meta_id
    )
}

fn append_binding_diagnostic_log(
    log_path: &Path,
    wrapper_session_id: &str,
    relay_thread_id: Option<&str>,
    session_meta_id: &str,
    session_file_path: &str,
) -> Result<(), String> {
    let line = binding_diagnostic_log_line(
        wrapper_session_id,
        relay_thread_id,
        session_meta_id,
        session_file_path,
    );
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|error| format!("打开诊断日志失败：{error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("写入诊断日志失败：{error}"))
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

fn prime_event_parser_metadata(
    path: &Path,
    parser: &mut CodexJsonlParser,
    max_metadata_bytes: u64,
) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut read_bytes = 0_u64;
    let fallback_path = path.to_string_lossy();

    while read_bytes < max_metadata_bytes {
        line.clear();
        let count = reader
            .read_line(&mut line)
            .map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;
        if count == 0 {
            break;
        }
        read_bytes += count as u64;
        if !line.ends_with('\n') {
            break;
        }
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.trim().is_empty() {
            continue;
        }
        let _ = parser.parse_line(trimmed, &fallback_path);
        if parser.has_session_metadata() {
            break;
        }
    }

    Ok(())
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
        control: index.list_item.control.clone(),
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

pub(crate) enum DetailFromIndexError {
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
    let mut message_candidates = Vec::new();
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
        if let Some(signature) = detail_message_signature(line) {
            remember_first_user_message(&mut parsed, &signature);
            // 详情索引只保留有可展示正文的行，避免 reasoning/token_count 等空事件污染分页。
            message_candidates.push(DetailMessageCandidate {
                line_index: MessageLineIndex::new(line_index, byte_start, line_bytes),
                signature: Some(signature),
            });
        }
        line_index += 1;
        byte_start = next_byte_start;
    }
    parsed.message_lines = deduplicate_mirrored_detail_messages(message_candidates);
    Ok(parsed)
}

fn remember_first_user_message(parsed: &mut ParsedSessionFile, signature: &DetailMessageSignature) {
    if signature.role != ToolSessionMessageRole::User {
        return;
    }
    if parsed
        .first_user_message
        .as_ref()
        .is_some_and(|message| message.created_at <= signature.created_at)
    {
        return;
    }
    parsed.first_user_message = Some(FirstUserMessage {
        preview: message_preview(&signature.content),
        hash: first_user_message_hash(&signature.content),
        created_at: signature.created_at,
    });
}

fn control_for_session(
    session_id: &str,
    registry: &ManagedCodexRegistry,
) -> Option<ToolSessionControl> {
    registry
        .sessions
        .iter()
        .find(|session| {
            session.codex_session_id.as_deref() == Some(session_id)
                && session.state == ManagedCodexSessionState::Bound
        })
        .map(|session| ToolSessionControl {
            resumable: true,
            preferred_channel_id: Some(format!(
                "niuma_codex_managed:{}",
                session.wrapper_session_id
            )),
            channels: vec![ToolSessionControlChannel {
                id: format!("niuma_codex_managed:{}", session.wrapper_session_id),
                provider: "niuma_codex".to_string(),
                kind: "managed_relay".to_string(),
                available: true,
                capabilities: vec![
                    "answer_input".to_string(),
                    "approve".to_string(),
                    "reject".to_string(),
                    "send_instruction".to_string(),
                    "interrupt".to_string(),
                ],
                actions: vec![
                    ToolSessionControlAction {
                        action_type: "send_instruction".to_string(),
                        transport: "local_api".to_string(),
                        endpoint: Some("/api/v1/session-control/send-instruction".to_string()),
                        debug_command: Some(format!(
                            "niuma codex-send {} \"继续\"",
                            session.wrapper_session_id
                        )),
                    },
                    ToolSessionControlAction {
                        action_type: "interrupt".to_string(),
                        transport: "local_api".to_string(),
                        endpoint: Some("/api/v1/session-control/interrupt".to_string()),
                        debug_command: Some(format!(
                            "niuma codex-interrupt {}",
                            session.wrapper_session_id
                        )),
                    },
                ],
                unavailable_reason: None,
                updated_at: session.bound_at.unwrap_or(session.started_at),
            }],
        })
}

fn message_preview(content: &str) -> String {
    content
        .chars()
        .take(FIRST_USER_MESSAGE_PREVIEW_CHARS)
        .collect()
}

fn deduplicate_mirrored_detail_messages(
    candidates: Vec<DetailMessageCandidate>,
) -> Vec<MessageLineIndex> {
    // Codex 会把同一条对话同时写成 event_msg 和 response_item；只过滤相邻镜像，避免误删用户真实重复输入。
    candidates
        .iter()
        .enumerate()
        .filter(|(index, candidate)| !is_mirrored_event_message(&candidates, *index, candidate))
        .map(|(_, candidate)| candidate.line_index)
        .collect()
}

fn is_mirrored_event_message(
    candidates: &[DetailMessageCandidate],
    index: usize,
    candidate: &DetailMessageCandidate,
) -> bool {
    let Some(signature) = candidate.signature.as_ref() else {
        return false;
    };
    if signature.is_structured_message {
        return false;
    }
    let previous = index
        .checked_sub(1)
        .and_then(|index| candidates.get(index))
        .and_then(|candidate| candidate.signature.as_ref());
    let next = candidates
        .get(index + 1)
        .and_then(|candidate| candidate.signature.as_ref());
    [previous, next]
        .into_iter()
        .flatten()
        .any(|neighbor| is_same_structured_message(signature, neighbor))
}

fn is_same_structured_message(
    event_signature: &DetailMessageSignature,
    neighbor: &DetailMessageSignature,
) -> bool {
    neighbor.is_structured_message
        && neighbor.role == event_signature.role
        && neighbor.content == event_signature.content
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

    pub(crate) fn not_found(session_id: &str) -> Self {
        Self::new(
            "session_not_found",
            format!("session_id 不存在：{session_id}"),
        )
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::new("provider_internal_error", message)
    }

    pub(crate) fn stale_session_file(message: impl Into<String>) -> Self {
        Self::new("stale_session_file", message)
    }

    pub(crate) fn provider_disabled() -> Self {
        Self::new("session_provider_disabled", "Codex 监听已关闭")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn binding_diagnostic_log_line_reports_thread_id_match() {
        let line = super::binding_diagnostic_log_line(
            "niuma_codex_1",
            Some("session-1"),
            "session-1",
            "/tmp/session.jsonl",
        );

        assert!(line.contains("wrapper_session_id=niuma_codex_1"));
        assert!(line.contains("relay_thread_id=session-1"));
        assert!(line.contains("session_meta_id=session-1"));
        assert!(line.contains("session_file_path=/tmp/session.jsonl"));
        assert!(line.contains("thread_id_matches_session_id=true"));
    }

    #[test]
    fn append_binding_diagnostic_log_writes_line_to_file() {
        let temp = tempfile::tempdir().unwrap();
        let log_path = temp.path().join("binding.log");

        super::append_binding_diagnostic_log(
            &log_path,
            "niuma_codex_1",
            Some("session-1"),
            "session-1",
            "/tmp/session.jsonl",
        )
        .unwrap();

        let body = fs::read_to_string(log_path).unwrap();
        assert!(body.contains("wrapper_session_id=niuma_codex_1"));
        assert!(body.ends_with('\n'));
    }
}
