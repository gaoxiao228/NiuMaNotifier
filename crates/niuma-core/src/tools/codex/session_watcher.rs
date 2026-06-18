use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::models::NiumaEvent;

pub use crate::tools::codex::session_protocol::current::CodexJsonlParser;

const PRIME_METADATA_MAX_BYTES: u64 = 64 * 1024;

#[derive(Default)]
pub struct CodexSessionScanner {
    files: HashMap<PathBuf, FileScanState>,
}

#[derive(Default)]
struct FileScanState {
    offset: u64,
    last_len: u64,
    identity: Option<FileIdentity>,
    parser: CodexJsonlParser,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
}

impl CodexSessionScanner {
    pub fn scan_file_tail(
        &mut self,
        path: &Path,
        max_bytes: u64,
    ) -> Result<Vec<NiumaEvent>, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("读取 Codex session 文件信息失败：{error}"))?;
        let file_len = metadata.len();
        let read_size = file_len.min(max_bytes.max(1));
        let read_start = file_len.saturating_sub(read_size);

        let mut file =
            File::open(path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
        file.seek(SeekFrom::Start(read_start))
            .map_err(|error| format!("定位 Codex session 文件失败：{error}"))?;

        let mut buffer = String::new();
        file.read_to_string(&mut buffer)
            .map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;

        // 从文件中间读取时，第一行可能是不完整 JSON，先跳到下一个换行。
        let text = if read_start > 0 {
            buffer
                .find('\n')
                .map(|index| &buffer[index + 1..])
                .unwrap_or("")
        } else {
            buffer.as_str()
        };

        let mut parser = CodexJsonlParser::default();
        let mut events = Vec::new();
        for segment in text.split_inclusive('\n') {
            if !segment.ends_with('\n') {
                break;
            }
            let line = segment.trim_end_matches('\n');
            if line.trim().is_empty() {
                continue;
            }
            if let Some(event) = parser.parse_line(line, &path.to_string_lossy())? {
                events.push(event);
            }
        }

        let state = self.files.entry(path.to_path_buf()).or_default();
        state.offset = file_len;
        state.last_len = file_len;
        state.identity = file_identity(&metadata);
        state.parser = parser;
        Ok(events)
    }

    pub fn prime_file_to_end(&mut self, path: &Path) -> Result<(), String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("读取 Codex session 文件信息失败：{error}"))?;
        let mut parser = CodexJsonlParser::default();
        prime_parser_metadata(path, &mut parser)?;
        let state = self.files.entry(path.to_path_buf()).or_default();
        // 旧 session 文件首次纳入监听时跳到尾部，但保留 session_meta 中的项目上下文。
        state.offset = metadata.len();
        state.last_len = metadata.len();
        state.identity = file_identity(&metadata);
        state.parser = parser;
        Ok(())
    }

    pub fn scan_file(&mut self, path: &Path) -> Result<Vec<NiumaEvent>, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("读取 Codex session 文件信息失败：{error}"))?;
        let identity = file_identity(&metadata);
        let state = self.files.entry(path.to_path_buf()).or_default();
        let file_replaced =
            state.identity.is_some() && identity.is_some() && state.identity != identity;
        let file_truncated = metadata.len() < state.last_len || state.offset > metadata.len();

        let mut parser = if file_replaced || file_truncated {
            CodexJsonlParser::default()
        } else {
            state.parser.clone()
        };
        let mut next_offset = if file_replaced || file_truncated {
            0
        } else {
            state.offset
        };

        let mut file =
            File::open(path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
        file.seek(SeekFrom::Start(next_offset))
            .map_err(|error| format!("定位 Codex session 文件失败：{error}"))?;

        let mut buffer = String::new();
        file.read_to_string(&mut buffer)
            .map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;

        let mut events = Vec::new();
        for segment in buffer.split_inclusive('\n') {
            // 最后一段未落盘换行时不推进 offset，等待下次补齐后再解析。
            if !segment.ends_with('\n') {
                break;
            }
            next_offset += segment.as_bytes().len() as u64;
            let line = segment.trim_end_matches('\n');
            if line.trim().is_empty() {
                continue;
            }
            if let Some(event) = parser.parse_line(line, &path.to_string_lossy())? {
                events.push(event);
            }
        }
        state.parser = parser;
        state.offset = next_offset;
        state.last_len = metadata.len();
        state.identity = identity;
        Ok(events)
    }
}

fn prime_parser_metadata(path: &Path, parser: &mut CodexJsonlParser) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut read_bytes = 0_u64;
    let fallback_path = path.to_string_lossy();

    while read_bytes < PRIME_METADATA_MAX_BYTES {
        line.clear();
        let count = reader
            .read_line(&mut line)
            .map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;
        if count == 0 {
            break;
        }
        read_bytes += count as u64;
        if !line.ends_with('\n') {
            break;
        }
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.trim().is_empty() {
            continue;
        }
        let _ = parser.parse_line(trimmed, &fallback_path);
        if parser.has_session_metadata() {
            break;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn file_identity(metadata: &std::fs::Metadata) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Some(FileIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn file_identity(_metadata: &std::fs::Metadata) -> Option<FileIdentity> {
    None
}

pub fn codex_session_dirs(codex_home: &Path, now: chrono::DateTime<chrono::Utc>) -> Vec<PathBuf> {
    let today = now.date_naive();
    [today, today - chrono::Duration::days(1)]
        .iter()
        .map(|day| {
            codex_home
                .join("sessions")
                .join(day.format("%Y").to_string())
                .join(day.format("%m").to_string())
                .join(day.format("%d").to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests;
