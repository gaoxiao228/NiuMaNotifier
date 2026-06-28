use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

use niuma_core::models::NiumaEvent;

use crate::claude::session_protocol::current::ClaudeJsonlParser;
use crate::claude::session_repository::ClaudeSessionRepository;

pub(crate) struct ClaudeSessionScanner {
    repository: Arc<Mutex<ClaudeSessionRepository>>,
}

#[derive(Clone, Default)]
pub(crate) struct ClaudeEventCursor {
    pub(crate) offset: u64,
    pub(crate) last_len: u64,
    pub(crate) parser: ClaudeJsonlParser,
}

impl ClaudeSessionScanner {
    #[cfg(test)]
    pub(crate) fn with_repository(repository: ClaudeSessionRepository) -> Self {
        Self {
            repository: Arc::new(Mutex::new(repository)),
        }
    }

    pub(crate) fn with_shared_repository(repository: Arc<Mutex<ClaudeSessionRepository>>) -> Self {
        Self { repository }
    }

    pub(crate) fn scan_file(&mut self, path: &Path) -> Result<Vec<NiumaEvent>, String> {
        let previous_cursor = self
            .repository
            .lock()
            .map_err(|_| "Claude Code session repository lock poisoned".to_string())?
            .event_cursor_cloned(path);
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("读取 Claude Code session 文件信息失败：{error}"))?;
        let file_len = metadata.len();
        let file_truncated = previous_cursor
            .as_ref()
            .is_some_and(|cursor| cursor.offset > file_len || cursor.last_len > file_len);
        let read_start = if file_truncated {
            0
        } else {
            previous_cursor
                .as_ref()
                .map(|cursor| cursor.offset)
                .unwrap_or(0)
        };
        let mut parser = if file_truncated {
            ClaudeJsonlParser::default()
        } else {
            previous_cursor
                .map(|cursor| cursor.parser)
                .unwrap_or_default()
        };

        let mut file = File::open(path)
            .map_err(|error| format!("打开 Claude Code session 文件失败：{error}"))?;
        file.seek(SeekFrom::Start(read_start))
            .map_err(|error| format!("定位 Claude Code session 文件失败：{error}"))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|error| format!("读取 Claude Code session 文件失败：{error}"))?;

        let complete_len = complete_line_len(&buffer);
        let mut events = Vec::new();
        for line_bytes in buffer[..complete_len].split(|byte| *byte == b'\n') {
            let line_bytes = line_bytes.strip_suffix(b"\r").unwrap_or(line_bytes);
            if line_bytes.is_empty() {
                continue;
            }
            let line = String::from_utf8_lossy(line_bytes);
            if let Some(event) = parser.parse_line(&line, &path.to_string_lossy())? {
                events.push(event);
            }
        }
        let next_offset = read_start + complete_len as u64;
        self.repository
            .lock()
            .map_err(|_| "Claude Code session repository lock poisoned".to_string())?
            .store_event_cursor(
                path,
                ClaudeEventCursor {
                    offset: next_offset,
                    last_len: file_len,
                    parser,
                },
            );
        Ok(events)
    }

    #[allow(dead_code)]
    pub(crate) fn prime_file_to_end(&mut self, path: &Path) -> Result<(), String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("读取 Claude Code session 文件信息失败：{error}"))?;
        self.repository
            .lock()
            .map_err(|_| "Claude Code session repository lock poisoned".to_string())?
            .store_event_cursor(
                path,
                ClaudeEventCursor {
                    offset: metadata.len(),
                    last_len: metadata.len(),
                    parser: ClaudeJsonlParser::default(),
                },
            );
        Ok(())
    }
}

fn complete_line_len(buffer: &[u8]) -> usize {
    if buffer.ends_with(b"\n") {
        return buffer.len();
    }
    buffer
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}
