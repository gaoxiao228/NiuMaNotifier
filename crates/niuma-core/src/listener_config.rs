use serde::{Deserialize, Serialize};

use crate::models::ToolKind;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListenerConfig {
    #[serde(default)]
    pub codex_listening_enabled: bool,
    #[serde(default)]
    pub claude_code_listening_enabled: bool,
}

impl ListenerConfig {
    pub fn is_any_ai_listening_enabled(&self) -> bool {
        self.is_tool_enabled(&ToolKind::Codex) || self.is_tool_enabled(&ToolKind::ClaudeCode)
    }

    pub fn is_tool_enabled(&self, tool: &ToolKind) -> bool {
        match tool {
            ToolKind::Codex => self.codex_listening_enabled,
            ToolKind::ClaudeCode => self.claude_code_listening_enabled,
        }
    }

    pub fn with_tool_enabled(mut self, tool: &ToolKind, enabled: bool) -> Self {
        match tool {
            ToolKind::Codex => self.codex_listening_enabled = enabled,
            ToolKind::ClaudeCode => self.claude_code_listening_enabled = enabled,
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::ListenerConfig;
    use crate::models::ToolKind;

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
        };

        assert!(config.is_tool_enabled(&ToolKind::Codex));
        assert!(!config.is_tool_enabled(&ToolKind::ClaudeCode));
    }

    #[test]
    fn tool_listener_update_preserves_other_tools() {
        let config = ListenerConfig {
            codex_listening_enabled: true,
            claude_code_listening_enabled: false,
        }
        .with_tool_enabled(&ToolKind::ClaudeCode, true);

        assert!(config.codex_listening_enabled);
        assert!(config.claude_code_listening_enabled);
        assert!(config.is_any_ai_listening_enabled());
    }
}
