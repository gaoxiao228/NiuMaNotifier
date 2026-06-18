use serde::Deserialize;

pub mod current;

use crate::models::NiumaEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexProtocolFamily {
    Current,
    Unsupported,
}

pub trait CodexSessionProtocolParser {
    fn parse_line(&mut self, line: &str, fallback_path: &str)
        -> Result<Option<NiumaEvent>, String>;
}

#[derive(Deserialize)]
struct ProbeRow {
    #[serde(rename = "type")]
    row_type: String,
    payload: serde_json::Value,
}

pub fn detect_session_protocol_family(line: &str) -> Result<CodexProtocolFamily, String> {
    let row: ProbeRow =
        serde_json::from_str(line).map_err(|error| format!("解析 Codex JSONL 失败：{error}"))?;
    if is_current_session_row(&row) {
        Ok(CodexProtocolFamily::Current)
    } else {
        Ok(CodexProtocolFamily::Unsupported)
    }
}

fn is_current_session_row(row: &ProbeRow) -> bool {
    if row.row_type == "session_meta" {
        return row.payload.get("id").is_some() || row.payload.get("cwd").is_some();
    }
    if !matches!(row.row_type.as_str(), "event_msg" | "response_item") {
        return false;
    }
    row.payload
        .get("type")
        .and_then(|value| value.as_str())
        .is_some()
}
