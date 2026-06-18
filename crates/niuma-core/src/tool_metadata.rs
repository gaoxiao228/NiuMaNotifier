use serde_json::Value;

use crate::models::ToolKind;

const TOOL_NOTIFICATION_ICONS: &str = include_str!("../config/tool-notification-icons.json");

pub fn tool_notification_icon_url(tool: &ToolKind) -> Option<String> {
    tool_notification_icons()
        .get(tool_key(tool))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn tool_notification_icons() -> Value {
    serde_json::from_str(TOOL_NOTIFICATION_ICONS)
        .unwrap_or_else(|_| Value::Object(Default::default()))
}

fn tool_key(tool: &ToolKind) -> &'static str {
    match tool {
        ToolKind::Codex => "codex",
        ToolKind::ClaudeCode => "claude_code",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_icon_config_is_valid_json() {
        let value = tool_notification_icons();

        assert!(value.is_object());
    }

    #[test]
    fn returns_codex_notification_icon_url() {
        assert_eq!(
            tool_notification_icon_url(&ToolKind::Codex).as_deref(),
            Some("https://cdn.jsdelivr.net/npm/@lobehub/icons-static-png@latest/light/codex-color.png")
        );
    }

    #[test]
    fn returns_claude_code_notification_icon_url() {
        assert_eq!(
            tool_notification_icon_url(&ToolKind::ClaudeCode).as_deref(),
            Some("https://upload.wikimedia.org/wikipedia/commons/thumb/b/b0/Claude_AI_symbol.svg/1280px-Claude_AI_symbol.svg.png")
        );
    }
}
