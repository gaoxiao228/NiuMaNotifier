use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use niuma_core::models::ToolKind;
use niuma_core::tool_session::{
    ToolSessionListItem, ToolSessionNormalizationStatus, ToolSessionScope, ToolSessionStatus,
};
use niuma_core::tool_session_rpc::{
    ProviderRpcNotification, ProviderRpcRequest, ProviderRpcResponse, SessionDetailParams,
    SessionDetailResult, SessionSnapshotParams, SessionSnapshotResult,
};

#[cfg(test)]
use crate::codex::session_repository::SessionIndex;
use crate::codex::session_repository::{CodexSessionRepository, ProviderError};

const SNAPSHOT_NOTIFY_INTERVAL: Duration = Duration::from_secs(2);

pub struct CodexSessionProvider {
    repository: Arc<Mutex<CodexSessionRepository>>,
}

impl CodexSessionProvider {
    pub fn from_config() -> Self {
        Self::with_codex_home(niuma_core::config::codex_home())
    }

    pub fn with_codex_home(codex_home: PathBuf) -> Self {
        Self::with_repository(Arc::new(Mutex::new(CodexSessionRepository::new(
            codex_home,
        ))))
    }

    pub(crate) fn with_repository(repository: Arc<Mutex<CodexSessionRepository>>) -> Self {
        Self { repository }
    }

    #[cfg(test)]
    pub(crate) fn scan_count(&self) -> usize {
        self.repository.lock().unwrap().scan_count()
    }

    #[cfg(test)]
    fn mutate_index(&self, session_id: &str, mutate: impl FnOnce(&mut SessionIndex)) -> Option<()> {
        let mut repository = self.repository.lock().unwrap();
        mutate(repository.index_mut(session_id)?);
        Some(())
    }

    #[cfg(test)]
    fn contains_index(&self, session_id: &str) -> bool {
        self.repository.lock().unwrap().contains_index(session_id)
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
        let detail_result = match self.repository.lock() {
            Ok(mut repository) => repository.session_detail(params),
            Err(_) => Err(ProviderError::internal(
                "Codex session repository lock poisoned",
            )),
        };
        match detail_result {
            Ok(detail) => ProviderRpcResponse::success(request.id, SessionDetailResult { detail })
                .expect("session detail response must serialize"),
            Err(ProviderError { code, message }) => {
                ProviderRpcResponse::failure(request.id, code, message)
            }
        }
    }

    fn refresh_snapshot(&mut self) -> Result<Vec<ToolSessionListItem>, String> {
        self.repository
            .lock()
            .map_err(|_| "Codex session repository lock poisoned".to_string())?
            .refresh_snapshot()
    }
}

// 启动 stdio JSON Lines provider；同一进程复用 provider 实例，让 snapshot 建立的索引可服务后续 detail。
pub fn run_stdio_session_provider() {
    run_stdio_session_provider_with_repository(Arc::new(Mutex::new(CodexSessionRepository::new(
        niuma_core::config::codex_home(),
    ))));
}

pub(crate) fn run_stdio_session_provider_with_repository(
    repository: Arc<Mutex<CodexSessionRepository>>,
) {
    let stdin = io::stdin();
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let provider = Arc::new(Mutex::new(CodexSessionProvider::with_repository(
        repository,
    )));
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
    normalized_session_id: Option<String>,
    session_scope: Option<ToolSessionScope>,
    agent_nickname: Option<String>,
    agent_role: Option<String>,
    normalization_status: Option<ToolSessionNormalizationStatus>,
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
            normalized_session_id: session.normalized_session_id.clone(),
            session_scope: session.session_scope.clone(),
            agent_nickname: session.agent_nickname.clone(),
            agent_role: session.agent_role.clone(),
            normalization_status: session.normalization_status.clone(),
            status: session.status.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use crate::codex::session_event_cursor::CodexSessionScanner;
    use crate::codex::session_file_index::session_file_signature;
    use crate::codex::session_identity::codex_filename_session_id;

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
        let mut cached_content_hash = 0;
        provider
            .mutate_index("session-alpha", |cached_index| {
                // 模拟文件系统 mtime 精度不足或 mtime 被恢复：旧缓存只剩 content_hash 与新文件不同。
                cached_index.file_index.signature.modified_system_time =
                    replaced_signature.modified_system_time;
                cached_index.file_index.signature.size_bytes = replaced_signature.size_bytes;
                cached_content_hash = cached_index.file_index.signature.content_hash;
            })
            .unwrap();
        assert_ne!(cached_content_hash, replaced_signature.content_hash);

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
    fn codex_session_provider_and_watcher_share_repository_instance() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_test_session(
            temp.path(),
            &test_session_content(
                "session-shared",
                "/tmp/shared-project",
                "shared question",
                "shared answer",
            ),
        );
        let repository = Arc::new(Mutex::new(CodexSessionRepository::new(temp.path().into())));
        let mut provider = CodexSessionProvider::with_repository(repository.clone());
        let mut scanner = CodexSessionScanner::with_repository(repository.clone());

        let snapshot = provider
            .handle_request(snapshot_request("req-shared-snapshot"))
            .result_as::<SessionSnapshotResult>()
            .unwrap();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].session_id, "session-shared");

        let _ = scanner.scan_file(&path).unwrap();
        assert!(repository.lock().unwrap().event_cursor(&path).is_some());

        let detail = provider
            .handle_request(detail_request_for_session(
                "req-shared-detail",
                "session-shared",
                20,
                None,
            ))
            .result_as::<SessionDetailResult>()
            .unwrap()
            .detail;
        assert_eq!(detail.messages[0].content, "shared answer");
        assert_eq!(detail.messages[1].content, "shared question");
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
        provider
            .mutate_index("session-fixture", |index| {
                // 保留旧行号但同步文件签名，强制走“读取发现缺行后重建索引”的防护分支。
                index.file_index.signature = truncated_signature;
            })
            .unwrap();

        let response = provider.handle_request(detail_request("req-detail", 2, None));
        assert!(response.error.is_none());
        let detail = response.result_as::<SessionDetailResult>().unwrap().detail;

        assert_eq!(detail.messages.len(), 1);
        assert_eq!(detail.messages[0].content, "用户问题");
        assert_eq!(detail.next_cursor, None);
    }

    #[test]
    fn codex_session_provider_detail_refreshes_stale_index_after_line_becomes_non_detail() {
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
                "{\"timestamp\":\"2026-06-22T01:00:02Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-fixture\",\"cwd\":\"/tmp/fixture-project\"}}\n",
            ),
        )
        .unwrap();
        let updated_signature = session_file_signature(&path).unwrap();
        provider
            .mutate_index("session-fixture", |index| {
                // 保留旧消息行号但同步文件签名，强制覆盖“行号仍存在但已不是详情消息”的防护分支。
                index.file_index.signature = updated_signature;
            })
            .unwrap();

        let response = provider.handle_request(detail_request("req-detail", 2, None));
        assert!(response.error.is_none());
        let detail = response.result_as::<SessionDetailResult>().unwrap().detail;

        assert_eq!(detail.messages.len(), 1);
        assert_eq!(detail.messages[0].content, "用户问题");
        assert_eq!(detail.next_cursor, None);
    }

    #[test]
    fn codex_session_provider_detail_rejects_same_page_content_from_replaced_session() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_test_session(
            temp.path(),
            &test_session_content(
                "session-alpha",
                "/tmp/project-alpha",
                "same question",
                "same answer",
            ),
        );
        let mut provider = CodexSessionProvider::with_codex_home(temp.path().into());
        let _ = provider.handle_request(snapshot_request("req-snapshot"));

        std::fs::write(
            &path,
            test_session_content(
                "session-bravo",
                "/tmp/project-bravo",
                "same question",
                "same answer",
            ),
        )
        .unwrap();

        let response = provider.handle_request(detail_request_for_session(
            "req-detail",
            "session-alpha",
            1,
            None,
        ));
        let error = response.error.unwrap();

        assert_eq!(error.code, "session_not_found");
        assert!(!provider.contains_index("session-alpha"));
        assert!(provider.contains_index("session-bravo"));
    }

    #[test]
    fn codex_session_provider_detail_rejects_fallback_session_without_session_meta() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_test_session(
            temp.path(),
            concat!(
                "{\"timestamp\":\"2026-06-22T01:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"用户问题\"}]}}\n",
                "{\"timestamp\":\"2026-06-22T01:00:02Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"助手回答\"}]}}\n",
            ),
        );
        let session_id = codex_filename_session_id(&path).unwrap();
        let mut provider = CodexSessionProvider::with_codex_home(temp.path().into());
        let snapshot = provider
            .handle_request(snapshot_request("req-snapshot"))
            .result_as::<SessionSnapshotResult>()
            .unwrap();
        assert!(snapshot
            .sessions
            .iter()
            .any(|session| session.session_id == session_id));

        let response = provider.handle_request(detail_request_for_session(
            "req-detail",
            &session_id,
            1,
            None,
        ));
        let error = response.error.unwrap();

        assert_eq!(error.code, "stale_session_file");
        assert!(error.message.contains("缺少 session_meta"));
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
