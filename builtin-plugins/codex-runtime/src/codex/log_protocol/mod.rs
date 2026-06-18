use std::collections::HashSet;

pub mod current;

use crate::codex::log_watcher::CodexLogRow;
use niuma_core::models::NiumaEvent;

pub const REQUIRED_LOG_COLUMNS: &[&str] = &[
    "id",
    "ts",
    "ts_nanos",
    "level",
    "target",
    "feedback_log_body",
    "thread_id",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexProtocolFamily {
    Current,
    Unsupported,
}

#[allow(dead_code)]
pub trait CodexLogProtocolParser {
    fn parse_row(&self, row: &CodexLogRow, source_path: &str) -> Option<NiumaEvent>;
}

pub fn detect_log_protocol_family<I, S>(columns: I) -> CodexProtocolFamily
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let columns = columns
        .into_iter()
        .map(|column| column.as_ref().to_string())
        .collect::<HashSet<_>>();
    if REQUIRED_LOG_COLUMNS
        .iter()
        .all(|column| columns.contains(*column))
    {
        CodexProtocolFamily::Current
    } else {
        CodexProtocolFamily::Unsupported
    }
}
