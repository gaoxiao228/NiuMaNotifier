use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::models::NiumaEvent;
pub use crate::tools::codex::log_protocol::current::parse_codex_log_row;
use crate::tools::codex::log_protocol::{detect_log_protocol_family, CodexProtocolFamily};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexLogRow {
    pub id: i64,
    pub ts: i64,
    pub ts_nanos: i64,
    pub level: String,
    pub target: String,
    pub feedback_log_body: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Default)]
pub struct CodexLogScanner {
    last_id: i64,
}

impl CodexLogScanner {
    pub fn prime_to_end(&mut self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            self.last_id = 0;
            return Ok(());
        }
        let connection = open_read_only(path)?;
        self.last_id = connection
            .query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| format!("读取 Codex 日志游标失败：{error}"))?;
        Ok(())
    }

    pub fn scan_file(&mut self, path: &Path) -> Result<Vec<NiumaEvent>, String> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let connection = open_read_only(path)?;
        let upper_id = connection
            .query_row("SELECT COALESCE(MAX(id), 0) FROM logs", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| format!("读取 Codex 日志游标失败：{error}"))?;
        if upper_id <= self.last_id {
            return Ok(Vec::new());
        }
        let mut statement = connection
            .prepare(
                "SELECT id, ts, ts_nanos, level, target, feedback_log_body, thread_id
                 FROM logs
                 WHERE id > ?1
                   AND id <= ?2
                   AND (target LIKE 'codex_otel.%' OR target = 'codex_core::session::turn' OR target = 'log')
                   AND (
                     (
                       feedback_log_body LIKE '%\"type\":\"invalid_request_error\"%'
                       AND feedback_log_body LIKE '%\"code\":\"context_too_large\"%'
                       AND (
                         target <> 'log'
                         OR (
                           feedback_log_body LIKE '%Received message {\"type\":\"error\"%'
                           AND feedback_log_body LIKE '%\"status\":400%'
                         )
                       )
                     )
                     OR (
                       target = 'codex_core::session::turn'
                       AND feedback_log_body LIKE '%Turn error:%'
                     )
                   )
                 ORDER BY id ASC",
            )
            .map_err(|error| format!("准备 Codex 日志查询失败：{error}"))?;
        let rows = statement
            .query_map([self.last_id, upper_id], |row| {
                Ok(CodexLogRow {
                    id: row.get(0)?,
                    ts: row.get(1)?,
                    ts_nanos: row.get(2)?,
                    level: row.get(3)?,
                    target: row.get(4)?,
                    feedback_log_body: row.get(5)?,
                    thread_id: row.get(6)?,
                })
            })
            .map_err(|error| format!("读取 Codex 日志失败：{error}"))?;

        let mut events = Vec::new();
        for row in rows {
            let row = row.map_err(|error| format!("读取 Codex 日志行失败：{error}"))?;
            if let Some(event) = parse_codex_log_row(&row, &path.to_string_lossy()) {
                events.push(event);
            }
        }
        // SQL 已在当前 upper_id 之前完成预过滤；无关行也应推进游标，避免大库反复重扫。
        self.last_id = upper_id;
        Ok(events)
    }
}

pub fn codex_internal_log_path(codex_home: &Path) -> std::path::PathBuf {
    codex_home.join("logs_2.sqlite")
}

pub fn codex_log_schema_available(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let connection = open_read_only(path)?;
    let mut statement = connection
        .prepare("PRAGMA table_info(logs)")
        .map_err(|error| format!("读取 Codex 日志 schema 失败：{error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("读取 Codex 日志 schema 失败：{error}"))?;

    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(|error| format!("读取 Codex 日志 schema 行失败：{error}"))?);
    }
    if columns.is_empty() {
        return Ok(false);
    }
    Ok(detect_log_protocol_family(columns) == CodexProtocolFamily::Current)
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("打开 Codex 内部日志失败：{error}"))
}

#[cfg(test)]
mod tests;
