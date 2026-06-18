use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::models::{ToolId, ToolKind};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListenerConfig {
    #[serde(default)]
    pub codex_listening_enabled: bool,
    #[serde(default)]
    pub claude_code_listening_enabled: bool,
    #[serde(default)]
    pub tool_listening_enabled: BTreeMap<String, bool>,
}

impl ListenerConfig {
    pub fn is_any_ai_listening_enabled(&self) -> bool {
        self.tool_listening_enabled.values().any(|enabled| *enabled)
            || self.codex_listening_enabled
            || self.claude_code_listening_enabled
    }

    pub fn is_tool_enabled(&self, tool: &ToolKind) -> bool {
        self.tool_listening_enabled
            .get(tool.as_str())
            .copied()
            .unwrap_or_else(|| self.legacy_tool_enabled(tool))
    }

    pub fn with_tool_enabled(mut self, tool: &ToolKind, enabled: bool) -> Self {
        self.tool_listening_enabled
            .insert(tool.as_str().to_string(), enabled);
        self.sync_legacy_tool(tool, enabled);
        self
    }

    pub fn tool_enabled_map(&self) -> BTreeMap<String, bool> {
        let mut map = self.tool_listening_enabled.clone();
        map.entry(ToolId::CODEX.to_string())
            .or_insert(self.codex_listening_enabled);
        map.entry(ToolId::CLAUDE_CODE.to_string())
            .or_insert(self.claude_code_listening_enabled);
        map
    }

    fn legacy_tool_enabled(&self, tool: &ToolKind) -> bool {
        match tool {
            ToolKind::Codex => self.codex_listening_enabled,
            ToolKind::ClaudeCode => self.claude_code_listening_enabled,
            ToolKind::Custom(_) => false,
        }
    }

    fn sync_legacy_tool(&mut self, tool: &ToolKind, enabled: bool) {
        match tool {
            ToolKind::Codex => self.codex_listening_enabled = enabled,
            ToolKind::ClaudeCode => self.claude_code_listening_enabled = enabled,
            ToolKind::Custom(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ListenerConfig;
    use crate::models::ToolKind;
    use std::collections::BTreeMap;

    #[test]
    fn any_ai_listener_is_disabled_by_default() {
        assert!(!ListenerConfig::default().is_any_ai_listening_enabled());
    }

    #[test]
    fn any_ai_listener_is_enabled_when_codex_listener_is_enabled() {
        let config = ListenerConfig {
            codex_listening_enabled: true,
            ..ListenerConfig::default()
        };

        assert!(config.is_any_ai_listening_enabled());
    }

    #[test]
    fn tool_listener_state_is_read_by_tool_kind() {
        let config = ListenerConfig {
            codex_listening_enabled: true,
            claude_code_listening_enabled: false,
            ..ListenerConfig::default()
        };

        assert!(config.is_tool_enabled(&ToolKind::Codex));
        assert!(!config.is_tool_enabled(&ToolKind::ClaudeCode));
    }

    #[test]
    fn tool_listener_update_preserves_other_tools() {
        let config = ListenerConfig {
            codex_listening_enabled: true,
            claude_code_listening_enabled: false,
            tool_listening_enabled: BTreeMap::new(),
        }
        .with_tool_enabled(&ToolKind::ClaudeCode, true);

        assert!(config.codex_listening_enabled);
        assert!(config.claude_code_listening_enabled);
        assert!(config.is_any_ai_listening_enabled());
    }

    #[test]
    fn dynamic_tool_listener_state_supports_custom_tools() {
        let config = ListenerConfig::default()
            .with_tool_enabled(&ToolKind::Custom("cursor".to_string()), true);

        assert!(config.is_tool_enabled(&ToolKind::Custom("cursor".to_string())));
        assert!(config.is_any_ai_listening_enabled());
        assert_eq!(config.tool_enabled_map()["cursor"], true);
    }
}
