use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MANAGED_CLAUDE_CODE_CHANNEL_PREFIX: &str = "niuma_claude_managed:";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedClaudeCodeRegistry {
    pub version: u32,
    #[serde(default)]
    pub sessions: Vec<ManagedClaudeCodeSession>,
}

impl Default for ManagedClaudeCodeRegistry {
    fn default() -> Self {
        Self {
            version: 1,
            sessions: Vec::new(),
        }
    }
}

impl ManagedClaudeCodeRegistry {
    pub fn upsert(&mut self, session: ManagedClaudeCodeSession) {
        self.version = 1;
        self.sessions
            .retain(|item| item.wrapper_session_id != session.wrapper_session_id);
        self.sessions.push(session);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedClaudeCodeSessionState {
    Started,
    BindingPending,
    Bound,
    Exited,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedClaudeCodeSession {
    pub wrapper_session_id: String,
    pub state: ManagedClaudeCodeSessionState,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_socket: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_failure_reason: Option<String>,
}

pub fn managed_claude_code_channel_id(wrapper_session_id: &str) -> String {
    format!("{MANAGED_CLAUDE_CODE_CHANNEL_PREFIX}{wrapper_session_id}")
}

pub fn wrapper_session_id_from_channel_id(channel_id: &str) -> Option<&str> {
    channel_id.strip_prefix(MANAGED_CLAUDE_CODE_CHANNEL_PREFIX)
}

pub fn read_registry(path: &Path) -> Result<ManagedClaudeCodeRegistry, String> {
    if !path.exists() {
        return Ok(ManagedClaudeCodeRegistry::default());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 Claude Code managed registry 失败：{error}"))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("解析 Claude Code managed registry 失败：{error}"))
}

pub fn update_registry(
    path: &Path,
    mutate: impl FnOnce(&mut ManagedClaudeCodeRegistry),
) -> Result<ManagedClaudeCodeRegistry, String> {
    let mut registry = read_registry(path)?;
    mutate(&mut registry);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建 Claude Code managed registry 目录失败：{error}"))?;
    }
    let text = serde_json::to_string_pretty(&registry)
        .map_err(|error| format!("序列化 Claude Code managed registry 失败：{error}"))?;
    std::fs::write(path, text)
        .map_err(|error| format!("写入 Claude Code managed registry 失败：{error}"))?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_claude_channel_id_roundtrips_wrapper_session_id() {
        let channel_id = managed_claude_code_channel_id("niuma_claude_1");

        assert_eq!(channel_id, "niuma_claude_managed:niuma_claude_1");
        assert_eq!(
            wrapper_session_id_from_channel_id(&channel_id),
            Some("niuma_claude_1")
        );
    }

    #[test]
    fn registry_roundtrips_managed_claude_session() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("claude-code.json");
        let session = ManagedClaudeCodeSession {
            wrapper_session_id: "niuma_claude_1".to_string(),
            state: ManagedClaudeCodeSessionState::Started,
            cwd: "/repo".to_string(),
            pid: Some(123),
            control_socket: Some("/tmp/niuma-claude/1/control.sock".to_string()),
            started_at: chrono::Utc::now(),
            claude_session_id: Some("11111111-1111-4111-8111-111111111111".to_string()),
            transcript_path: None,
            bound_at: None,
            binding_failure_reason: None,
        };

        update_registry(&path, |registry| registry.upsert(session.clone())).unwrap();
        let loaded = read_registry(&path).unwrap();

        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(
            loaded.sessions[0].wrapper_session_id,
            session.wrapper_session_id
        );
    }
}
