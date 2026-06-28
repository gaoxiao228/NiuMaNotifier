use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use niuma_core::claude_code_managed_session::{
    managed_claude_code_channel_id, read_registry, ManagedClaudeCodeRegistry,
    ManagedClaudeCodeSession,
};
use niuma_core::models::ToolKind;
use niuma_core::tool_session::{
    ToolSessionControl, ToolSessionControlAction, ToolSessionControlChannel, ToolSessionDetail,
    ToolSessionListItem, ToolSessionScope, ToolSessionStatus,
};
use niuma_core::tool_session_rpc::SessionDetailParams;
use serde::Deserialize;
use serde_json::Value;

use crate::claude::discovery::recent_jsonl_files;
use crate::claude::session_event_cursor::ClaudeEventCursor;
use crate::claude::session_file_index::{
    read_indexed_line, session_file_signature, trim_jsonl_line_bytes, ClaudeMessageLineIndex,
    ClaudeSessionFileIndex,
};
use crate::session_messages::{is_detail_message_line, parse_claude_message_line};

const SNAPSHOT_FILE_LIMIT: usize = 128;
const ACTIVE_MODIFIED_WINDOW: Duration = Duration::from_secs(60);
const FIRST_USER_MESSAGE_PREVIEW_CHARS: usize = 200;

pub(crate) struct ClaudeSessionRepository {
    claude_home: PathBuf,
    managed_registry_path: PathBuf,
    index: HashMap<String, SessionIndex>,
    event_cursors: HashMap<PathBuf, ClaudeEventCursor>,
}

#[derive(Clone)]
pub(crate) struct SessionIndex {
    pub(crate) list_item: ToolSessionListItem,
    pub(crate) file_index: ClaudeSessionFileIndex,
}

#[derive(Default)]
struct ParsedSessionFile {
    session_id: Option<String>,
    project_path: Option<String>,
    first_user_message: Option<FirstUserMessage>,
    message_lines: Vec<ClaudeMessageLineIndex>,
}

struct FirstUserMessage {
    preview: String,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct ClaudeRow {
    #[serde(rename = "type")]
    row_type: String,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    #[serde(default)]
    message: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderError {
    pub(crate) code: String,
    pub(crate) message: String,
}

impl ProviderError {
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "internal".to_string(),
            message: message.into(),
        }
    }

    pub(crate) fn not_found(session_id: &str) -> Self {
        Self {
            code: "not_found".to_string(),
            message: format!("Claude Code session 不存在：{session_id}"),
        }
    }

    fn invalid_cursor(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_cursor".to_string(),
            message: message.into(),
        }
    }
}

impl ClaudeSessionRepository {
    pub(crate) fn new(claude_home: PathBuf) -> Self {
        Self::with_managed_registry_path(
            claude_home,
            niuma_core::platform::paths::claude_code_managed_registry_path(),
        )
    }

    pub(crate) fn with_managed_registry_path(
        claude_home: PathBuf,
        managed_registry_path: PathBuf,
    ) -> Self {
        Self {
            claude_home,
            managed_registry_path,
            index: HashMap::new(),
            event_cursors: HashMap::new(),
        }
    }

    pub(crate) fn refresh_snapshot(&mut self) -> Result<Vec<ToolSessionListItem>, String> {
        let mut next_index = HashMap::new();
        let discovered_at = Utc::now();
        let managed_registry = read_registry(&self.managed_registry_path).unwrap_or_else(|error| {
            if self.managed_registry_path.exists() {
                eprintln!("NiumaNotifier Claude Code managed registry 读取失败：{error}");
            }
            ManagedClaudeCodeRegistry::default()
        });
        for path in recent_jsonl_files(&self.claude_home, SNAPSHOT_FILE_LIMIT) {
            match self.scan_session_file_index(&path, discovered_at) {
                Ok(mut index) => {
                    index.list_item.control =
                        control_for_session(&index.list_item.session_id, &managed_registry);
                    next_index.insert(index.list_item.session_id.clone(), index);
                }
                Err(error) => {
                    eprintln!(
                        "NiumaNotifier Claude Code session provider skipped {}: {error}",
                        path.display()
                    );
                }
            }
        }
        let mut sessions = next_index
            .values()
            .map(|index| index.list_item.clone())
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| right.modified_at.cmp(&left.modified_at));
        self.index = next_index;
        Ok(sessions)
    }

    pub(crate) fn session_index(&self, session_id: &str) -> Option<SessionIndex> {
        self.index.get(session_id).cloned()
    }

    pub(crate) fn session_detail(
        &mut self,
        params: &SessionDetailParams,
    ) -> Result<ToolSessionDetail, ProviderError> {
        if self.session_index(&params.session_id).is_none() {
            self.refresh_snapshot().map_err(ProviderError::internal)?;
        }
        let index = self
            .session_index(&params.session_id)
            .ok_or_else(|| ProviderError::not_found(&params.session_id))?;
        detail_from_index(&index, params)
    }

    #[allow(dead_code)]
    pub(crate) fn clear_runtime_indexes(&mut self) {
        self.index.clear();
        self.event_cursors.clear();
    }

    pub(crate) fn event_cursor_cloned(&self, path: &Path) -> Option<ClaudeEventCursor> {
        self.event_cursors.get(path).cloned()
    }

    pub(crate) fn store_event_cursor(&mut self, path: &Path, cursor: ClaudeEventCursor) {
        self.event_cursors.insert(path.to_path_buf(), cursor);
    }

    fn scan_session_file_index(
        &self,
        path: &Path,
        discovered_at: DateTime<Utc>,
    ) -> Result<SessionIndex, String> {
        let file_signature = session_file_signature(path)
            .map_err(|error| format!("读取 Claude Code session 文件信息失败：{error}"))?;
        let parsed = parse_session_file(path)?;
        let session_id = parsed
            .session_id
            .clone()
            .or_else(|| {
                path.file_stem()
                    .and_then(|value| value.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let project_path = parsed.project_path.clone().unwrap_or_default();
        let project_name = project_name_for_path(&project_path);
        let modified_at = DateTime::<Utc>::from(file_signature.modified_system_time);
        let is_active =
            recently_modified(file_signature.modified_system_time, ACTIVE_MODIFIED_WINDOW);
        let status = if is_active {
            ToolSessionStatus::Active
        } else {
            ToolSessionStatus::Inactive
        };
        let is_subagent = path
            .components()
            .any(|component| component.as_os_str() == "subagents");
        let list_item = ToolSessionListItem {
            id: format!("claude_code:{session_id}"),
            tool: ToolKind::ClaudeCode,
            session_id,
            project_path,
            project_name,
            file_path: path.to_string_lossy().to_string(),
            modified_at,
            discovered_at,
            last_seen_at: discovered_at,
            is_active,
            is_subagent,
            parent_session_id: None,
            normalized_session_id: None,
            session_scope: Some(if is_subagent {
                ToolSessionScope::Subagent
            } else {
                ToolSessionScope::Main
            }),
            agent_nickname: None,
            agent_role: None,
            normalization_status: None,
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
            list_item,
            file_index: ClaudeSessionFileIndex {
                signature: file_signature,
                message_lines: parsed.message_lines,
            },
        })
    }
}

fn control_for_session(
    session_id: &str,
    registry: &ManagedClaudeCodeRegistry,
) -> Option<ToolSessionControl> {
    let mut channels = registry
        .sessions
        .iter()
        .filter(|session| session.claude_session_id.as_deref() == Some(session_id))
        .map(managed_session_control_channel)
        .collect::<Vec<_>>();
    if channels.is_empty() {
        return None;
    }
    channels.sort_by(|left, right| {
        right
            .available
            .cmp(&left.available)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    let preferred_channel_id = channels
        .iter()
        .find(|channel| {
            channel.available
                && channel
                    .capabilities
                    .iter()
                    .any(|capability| capability == "send_instruction")
        })
        .map(|channel| channel.id.clone());
    Some(ToolSessionControl {
        resumable: preferred_channel_id.is_some(),
        preferred_channel_id,
        channels,
    })
}

fn managed_session_control_channel(
    session: &ManagedClaudeCodeSession,
) -> ToolSessionControlChannel {
    // Claude Code 当前没有 Codex 等价的 live control channel，不能把 resume 伪装成实时控制。
    let available = false;
    ToolSessionControlChannel {
        id: managed_claude_code_channel_id(&session.wrapper_session_id),
        provider: "niuma_claude".to_string(),
        kind: "managed_process".to_string(),
        available,
        capabilities: if available {
            vec!["send_instruction".to_string(), "interrupt".to_string()]
        } else {
            Vec::new()
        },
        actions: if available {
            managed_channel_actions()
        } else {
            Vec::new()
        },
        unavailable_reason: (!available).then(|| unavailable_reason_for_managed_session(session)),
        updated_at: session.bound_at.unwrap_or(session.started_at),
    }
}

fn managed_channel_actions() -> Vec<ToolSessionControlAction> {
    vec![
        ToolSessionControlAction {
            action_type: "send_instruction".to_string(),
            transport: "local_api".to_string(),
            endpoint: Some("/api/v1/session-control/send-instruction".to_string()),
            debug_command: None,
        },
        ToolSessionControlAction {
            action_type: "interrupt".to_string(),
            transport: "local_api".to_string(),
            endpoint: Some("/api/v1/session-control/interrupt".to_string()),
            debug_command: None,
        },
    ]
}

fn unavailable_reason_for_managed_session(session: &ManagedClaudeCodeSession) -> String {
    session
        .binding_failure_reason
        .clone()
        .unwrap_or_else(|| "Claude Code 实时控制尚未实现".to_string())
}

fn parse_session_file(path: &Path) -> Result<ParsedSessionFile, String> {
    let file =
        File::open(path).map_err(|error| format!("打开 Claude Code session 文件失败：{error}"))?;
    let mut reader = BufReader::new(file);
    let mut parsed = ParsedSessionFile::default();
    let mut line_index = 0usize;
    let mut byte_start = 0u64;
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let bytes_read = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| format!("读取 Claude Code session 文件失败：{error}"))?;
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
        if let Ok(row) = serde_json::from_str::<ClaudeRow>(line) {
            remember_session_metadata(&mut parsed, &row);
            remember_first_user_message(&mut parsed, &row);
        }
        if is_detail_message_line(line) {
            parsed.message_lines.push(ClaudeMessageLineIndex::new(
                line_index, byte_start, line_bytes,
            ));
        }
        line_index += 1;
        byte_start = next_byte_start;
    }
    Ok(parsed)
}

fn remember_session_metadata(parsed: &mut ParsedSessionFile, row: &ClaudeRow) {
    if parsed.session_id.is_none() {
        parsed.session_id = row
            .session_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned();
    }
    if parsed.project_path.is_none() {
        parsed.project_path = row
            .cwd
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned();
    }
}

fn remember_first_user_message(parsed: &mut ParsedSessionFile, row: &ClaudeRow) {
    if row.row_type != "user" || parsed.first_user_message.is_some() {
        return;
    }
    let Some(text) = row
        .message
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    parsed.first_user_message = Some(FirstUserMessage {
        preview: message_preview(text),
        created_at: parse_timestamp(row.timestamp.as_deref()).unwrap_or_else(Utc::now),
    });
}

fn detail_from_index(
    index: &SessionIndex,
    params: &SessionDetailParams,
) -> Result<ToolSessionDetail, ProviderError> {
    let before_line_index = parse_cursor(params.cursor.as_deref())?;
    let page_size = params.limit.max(1);
    let page_lines = index
        .file_index
        .message_lines
        .iter()
        .filter(|line| before_line_index.is_none_or(|before| line.line_index < before))
        .rev()
        .take(page_size)
        .copied()
        .collect::<Vec<_>>();
    let messages = page_lines
        .iter()
        .map(|line| {
            read_indexed_line(&index.list_item.file_path, line).map(|text| {
                parse_claude_message_line(&index.list_item.session_id, line.line_index, &text)
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(ProviderError::internal)?;
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
        tool: ToolKind::ClaudeCode,
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
        pending_action: None,
        messages,
        next_cursor,
    })
}

fn parse_cursor(cursor: Option<&str>) -> Result<Option<usize>, ProviderError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let Some(value) = cursor.strip_prefix("before:") else {
        return Err(ProviderError::invalid_cursor(
            "Claude Code detail cursor 格式无效",
        ));
    };
    value
        .parse::<usize>()
        .map(Some)
        .map_err(|_| ProviderError::invalid_cursor("Claude Code detail cursor 行号无效"))
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn project_name_for_path(path: &str) -> String {
    path.rsplit('/')
        .find(|value| !value.is_empty())
        .unwrap_or("Claude Code")
        .to_string()
}

fn message_preview(text: &str) -> String {
    text.chars()
        .take(FIRST_USER_MESSAGE_PREVIEW_CHARS)
        .collect()
}

fn recently_modified(modified_system_time: SystemTime, max_age: Duration) -> bool {
    modified_system_time
        .elapsed()
        .map(|elapsed| elapsed <= max_age)
        .unwrap_or(true)
}
