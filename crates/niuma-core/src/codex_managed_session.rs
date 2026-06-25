use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};

const REGISTRY_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedCodexRegistry {
    #[serde(default = "registry_version")]
    pub version: u32,
    #[serde(default)]
    pub sessions: Vec<ManagedCodexSession>,
}

impl Default for ManagedCodexRegistry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            sessions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedCodexSessionState {
    Created,
    WaitingFirstUserMessage,
    BindingPending,
    Bound,
    Ambiguous,
    Exited,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedCodexSession {
    pub wrapper_session_id: String,
    pub state: ManagedCodexSessionState,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub real_socket: String,
    pub relay_socket: String,
    pub control_socket: String,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message_submitted_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_failure_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexSessionBindingCandidate {
    pub session_id: String,
    pub session_file_path: String,
    pub project_path: String,
    pub first_user_message_hash: Option<String>,
    pub first_user_message_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindingMatch {
    None,
    Unique {
        session_id: String,
        session_file_path: String,
    },
    Ambiguous,
}

impl ManagedCodexRegistry {
    pub fn upsert(&mut self, session: ManagedCodexSession) {
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|item| item.wrapper_session_id == session.wrapper_session_id)
        {
            *existing = session;
            return;
        }

        self.sessions.push(session);
    }
}

pub fn first_user_message_hash(value: &str) -> String {
    let normalized = normalize_first_user_message(value);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn normalize_first_user_message(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn read_registry(path: &Path) -> Result<ManagedCodexRegistry, String> {
    if !path.exists() {
        return Ok(ManagedCodexRegistry::default());
    }

    let body = fs::read_to_string(path)
        .map_err(|error| format!("读取 Codex managed registry 失败：{error}"))?;
    serde_json::from_str(&body)
        .map_err(|error| format!("解析 Codex managed registry 失败：{error}"))
}

pub fn write_registry_atomic(path: &Path, registry: &ManagedCodexRegistry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建 Codex managed registry 目录失败：{error}"))?;
    }

    let body = serde_json::to_string_pretty(registry)
        .map_err(|error| format!("序列化 Codex managed registry 失败：{error}"))?;
    let tmp = path.with_extension("json.tmp");
    // 先写临时文件再 rename，避免进程中断时留下半截 JSON。
    fs::write(&tmp, format!("{body}\n"))
        .map_err(|error| format!("写入 Codex managed registry 临时文件失败：{error}"))?;
    fs::rename(&tmp, path).map_err(|error| format!("替换 Codex managed registry 失败：{error}"))
}

pub fn match_managed_session(
    managed: &ManagedCodexSession,
    candidates: &[CodexSessionBindingCandidate],
    window: chrono::Duration,
) -> BindingMatch {
    let Some(hash) = managed.first_user_message_hash.as_deref() else {
        return BindingMatch::None;
    };
    let Some(submitted_at) = managed.first_user_message_submitted_at else {
        return BindingMatch::None;
    };

    let normalized_cwd = normalize_path_for_match(&managed.cwd);
    let window_millis = window.num_milliseconds().abs();
    let matches = candidates
        .iter()
        .filter(|candidate| normalize_path_for_match(&candidate.project_path) == normalized_cwd)
        .filter(|candidate| candidate.first_user_message_hash.as_deref() == Some(hash))
        .filter(|candidate| {
            candidate
                .first_user_message_at
                .map(|first_at| (first_at - submitted_at).num_milliseconds().abs() <= window_millis)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => BindingMatch::None,
        [candidate] => BindingMatch::Unique {
            session_id: candidate.session_id.clone(),
            session_file_path: candidate.session_file_path.clone(),
        },
        _ => BindingMatch::Ambiguous,
    }
}

fn registry_version() -> u32 {
    REGISTRY_VERSION
}

fn normalize_path_for_match(path: &str) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| normalize_lexical_path(Path::new(path)))
        .to_string_lossy()
        .trim_end_matches(std::path::MAIN_SEPARATOR)
        .to_string()
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    fn managed_session(message: &str, submitted_at: i64) -> ManagedCodexSession {
        ManagedCodexSession {
            wrapper_session_id: "niuma_codex_1".to_string(),
            state: ManagedCodexSessionState::BindingPending,
            cwd: "/repo".into(),
            pid: Some(42),
            real_socket: "/tmp/real.sock".into(),
            relay_socket: "/tmp/relay.sock".into(),
            control_socket: "/tmp/control.sock".into(),
            started_at: Utc.timestamp_opt(1_000, 0).unwrap(),
            first_user_message_hash: Some(first_user_message_hash(message)),
            first_user_message_preview: Some(message.into()),
            first_user_message_submitted_at: Some(Utc.timestamp_opt(submitted_at, 0).unwrap()),
            codex_session_id: None,
            codex_session_file_path: None,
            bound_at: None,
            binding_failure_reason: None,
        }
    }

    fn binding_candidate(
        session_id: &str,
        message: &str,
        first_user_message_at: i64,
    ) -> CodexSessionBindingCandidate {
        CodexSessionBindingCandidate {
            session_id: session_id.into(),
            session_file_path: format!("/codex/{session_id}.jsonl"),
            project_path: "/repo".into(),
            first_user_message_hash: Some(first_user_message_hash(message)),
            first_user_message_at: Some(Utc.timestamp_opt(first_user_message_at, 0).unwrap()),
        }
    }

    #[test]
    fn message_hash_normalizes_whitespace_and_line_endings() {
        let left = first_user_message_hash("  hello\r\n  world  ");
        let right = first_user_message_hash("hello world");

        assert_eq!(
            normalize_first_user_message("  hello\r\n  world  "),
            "hello world"
        );
        assert_eq!(left, right);
    }

    #[test]
    fn registry_round_trips_session_and_updates_atomically() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("codex.json");
        let mut registry = ManagedCodexRegistry::default();
        registry.upsert(managed_session("hello", 1_005));
        write_registry_atomic(&path, &registry).unwrap();

        let loaded = read_registry(&path).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].wrapper_session_id, "niuma_codex_1");

        let missing = read_registry(&dir.path().join("missing.json")).unwrap();
        assert_eq!(missing.version, 1);
        assert!(missing.sessions.is_empty());
    }

    #[test]
    fn binding_matches_only_unique_candidate_inside_window() {
        let managed = managed_session("继续", 1_010);
        let candidate = binding_candidate("codex-session-1", "继续", 1_012);

        let result = match_managed_session(&managed, &[candidate], chrono::Duration::seconds(10));

        assert_eq!(
            result,
            BindingMatch::Unique {
                session_id: "codex-session-1".into(),
                session_file_path: "/codex/codex-session-1.jsonl".into(),
            }
        );
    }

    #[test]
    fn binding_reports_ambiguous_when_multiple_candidates_match() {
        let managed = managed_session("继续", 1_010);
        let first = binding_candidate("codex-session-1", "继续", 1_012);
        let second = binding_candidate("codex-session-2", "继续", 1_013);

        let result =
            match_managed_session(&managed, &[first, second], chrono::Duration::seconds(10));

        assert_eq!(result, BindingMatch::Ambiguous);
    }

    #[test]
    fn binding_ignores_candidates_outside_window() {
        let managed = managed_session("继续", 1_010);
        let candidate = binding_candidate("codex-session-1", "继续", 1_030);

        let result = match_managed_session(&managed, &[candidate], chrono::Duration::seconds(10));

        assert_eq!(result, BindingMatch::None);
    }
}
