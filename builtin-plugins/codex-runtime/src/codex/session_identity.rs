#[cfg(test)]
use std::path::Path;

use niuma_core::models::EventSessionScope;
use niuma_core::tool_session::{ToolSessionNormalizationStatus, ToolSessionScope};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexSessionScope {
    Main,
    Subagent,
}

impl CodexSessionScope {
    pub fn is_subagent(self) -> bool {
        self == Self::Subagent
    }

    pub fn as_event_scope(self) -> EventSessionScope {
        match self {
            Self::Main => EventSessionScope::Main,
            Self::Subagent => EventSessionScope::Subagent,
        }
    }

    pub fn as_tool_scope(self) -> ToolSessionScope {
        match self {
            Self::Main => ToolSessionScope::Main,
            Self::Subagent => ToolSessionScope::Subagent,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodexSessionMetadata {
    thread_source: Option<String>,
    parent_session_id: Option<String>,
    agent_nickname: Option<String>,
    agent_role: Option<String>,
}

impl CodexSessionMetadata {
    pub fn merge_session_meta(&mut self, payload: &serde_json::Value) {
        if self.thread_source.is_none() {
            self.thread_source = string_payload_field(payload, "thread_source");
        }
        if self.parent_session_id.is_none() {
            self.parent_session_id = parent_thread_id_from_session_meta(payload);
        }
        if self.agent_nickname.is_none() {
            self.agent_nickname = string_payload_field(payload, "agent_nickname");
        }
        if self.agent_role.is_none() {
            self.agent_role = string_payload_field(payload, "agent_role");
        }
    }

    pub fn identity_for_session(&self, session_id: &str) -> CodexSessionIdentity {
        let session_scope = if self.thread_source.as_deref() == Some("subagent") {
            CodexSessionScope::Subagent
        } else {
            CodexSessionScope::Main
        };
        let parent_session_id = self.parent_session_id.clone();
        let (normalized_session_id, normalization_status) = if session_scope.is_subagent() {
            match parent_session_id.as_ref() {
                Some(parent_session_id) => (
                    parent_session_id.clone(),
                    ToolSessionNormalizationStatus::Resolved,
                ),
                None => (
                    session_id.to_string(),
                    ToolSessionNormalizationStatus::ParentMissing,
                ),
            }
        } else {
            (
                session_id.to_string(),
                ToolSessionNormalizationStatus::Resolved,
            )
        };

        CodexSessionIdentity {
            parent_session_id,
            normalized_session_id,
            session_scope,
            agent_nickname: self.agent_nickname.clone(),
            agent_role: self.agent_role.clone(),
            normalization_status,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexSessionIdentity {
    pub parent_session_id: Option<String>,
    pub normalized_session_id: String,
    pub session_scope: CodexSessionScope,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
    pub normalization_status: ToolSessionNormalizationStatus,
}

pub(crate) fn codex_fallback_session_id(path: &str) -> String {
    let basename = path
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".jsonl"))
        .filter(|name| !name.is_empty())
        .unwrap_or("session");
    if let Some(session_id) = codex_rollout_filename_session_id(basename) {
        return session_id;
    }
    format!("fallback-{basename}-{}", stable_hash(path))
}

#[cfg(test)]
pub(crate) fn codex_filename_session_id(path: &Path) -> Option<String> {
    let basename = path.file_stem()?.to_str()?;
    codex_rollout_filename_session_id(basename)
}

pub(crate) fn codex_project_name(project_path: &str) -> String {
    project_path
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("Codex")
        .to_string()
}

fn parent_thread_id_from_session_meta(payload: &serde_json::Value) -> Option<String> {
    string_payload_field(payload, "parent_thread_id").or_else(|| {
        payload
            .get("source")
            .and_then(|value| value.get("subagent"))
            .and_then(|value| value.get("thread_spawn"))
            .and_then(|value| value.get("parent_thread_id"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn string_payload_field(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn codex_rollout_filename_session_id(basename: &str) -> Option<String> {
    let session_id = basename.rsplit('-').take(5).collect::<Vec<_>>();
    if session_id.len() != 5 {
        return None;
    }
    let candidate = session_id.into_iter().rev().collect::<Vec<_>>().join("-");
    if is_uuid_like(&candidate) {
        Some(candidate)
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_resolves_direct_subagent_parent() {
        let mut metadata = CodexSessionMetadata::default();
        metadata.merge_session_meta(&serde_json::json!({
            "thread_source": "subagent",
            "parent_thread_id": "parent-session",
            "agent_nickname": "Jason",
            "agent_role": "default"
        }));

        let identity = metadata.identity_for_session("child-session");

        assert_eq!(
            identity.parent_session_id.as_deref(),
            Some("parent-session")
        );
        assert_eq!(identity.normalized_session_id, "parent-session");
        assert_eq!(identity.session_scope, CodexSessionScope::Subagent);
        assert_eq!(identity.agent_nickname.as_deref(), Some("Jason"));
        assert_eq!(identity.agent_role.as_deref(), Some("default"));
        assert_eq!(
            identity.normalization_status,
            ToolSessionNormalizationStatus::Resolved
        );
    }

    #[test]
    fn metadata_resolves_nested_subagent_parent() {
        let mut metadata = CodexSessionMetadata::default();
        metadata.merge_session_meta(&serde_json::json!({
            "thread_source": "subagent",
            "source": {
                "subagent": {
                    "thread_spawn": {
                        "parent_thread_id": "nested-parent"
                    }
                }
            }
        }));

        let identity = metadata.identity_for_session("nested-child");

        assert_eq!(identity.parent_session_id.as_deref(), Some("nested-parent"));
        assert_eq!(identity.normalized_session_id, "nested-parent");
        assert_eq!(identity.session_scope, CodexSessionScope::Subagent);
    }

    #[test]
    fn metadata_marks_missing_parent_subagent_as_unresolved_to_self() {
        let mut metadata = CodexSessionMetadata::default();
        metadata.merge_session_meta(&serde_json::json!({
            "thread_source": "subagent"
        }));

        let identity = metadata.identity_for_session("orphan-child");

        assert_eq!(identity.parent_session_id, None);
        assert_eq!(identity.normalized_session_id, "orphan-child");
        assert_eq!(identity.session_scope, CodexSessionScope::Subagent);
        assert_eq!(
            identity.normalization_status,
            ToolSessionNormalizationStatus::ParentMissing
        );
    }

    #[test]
    fn fallback_session_id_uses_full_path_for_same_basename() {
        let first = codex_fallback_session_id("/tmp/one/rollout.jsonl");
        let second = codex_fallback_session_id("/tmp/two/rollout.jsonl");

        assert_ne!(first, second);
        assert!(first.starts_with("fallback-rollout-"));
        assert!(second.starts_with("fallback-rollout-"));
    }

    #[test]
    fn fallback_session_id_extracts_uuid_from_rollout_filename() {
        let session_id = codex_fallback_session_id(
            "/tmp/rollout-2026-06-11T13-58-25-019eb542-a886-72e0-86fd-e5730054991c.jsonl",
        );

        assert_eq!(session_id, "019eb542-a886-72e0-86fd-e5730054991c");
    }

    #[test]
    fn project_name_uses_last_non_empty_path_component() {
        assert_eq!(codex_project_name("/tmp/NiuMaNotifier/"), "NiuMaNotifier");
        assert_eq!(codex_project_name(""), "Codex");
    }
}
