use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use niuma_core::models::NiumaEvent;

use crate::codex::session_protocol::current::CodexJsonlParser;
use crate::codex::session_repository::CodexSessionRepository;

pub(crate) struct CodexSessionScanner {
    repository: Arc<Mutex<CodexSessionRepository>>,
}

impl CodexSessionScanner {
    pub(crate) fn new(codex_home: PathBuf) -> Self {
        Self::with_repository(Arc::new(Mutex::new(CodexSessionRepository::new(
            codex_home,
        ))))
    }

    pub(crate) fn with_repository(repository: Arc<Mutex<CodexSessionRepository>>) -> Self {
        Self { repository }
    }
}

impl Default for CodexSessionScanner {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

#[derive(Clone, Default)]
pub(crate) struct CodexEventCursor {
    pub(crate) offset: u64,
    pub(crate) last_len: u64,
    pub(crate) file_identity: Option<CodexFileIdentity>,
    pub(crate) parser: CodexJsonlParser,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CodexFileIdentity {
    pub(crate) dev: u64,
    pub(crate) ino: u64,
}

impl CodexSessionScanner {
    #[allow(dead_code)]
    pub(crate) fn scan_file_tail(
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

        self.repository
            .lock()
            .map_err(|_| "Codex session repository lock poisoned".to_string())?
            .store_event_cursor(
                path,
                CodexEventCursor {
                    offset: file_len,
                    last_len: file_len,
                    file_identity: file_identity(&metadata),
                    parser,
                },
            );
        Ok(events)
    }

    pub(crate) fn prime_file_to_end(&mut self, path: &Path) -> Result<(), String> {
        self.repository
            .lock()
            .map_err(|_| "Codex session repository lock poisoned".to_string())?
            .prime_event_cursor_in_index(path)
    }

    pub(crate) fn scan_file(&mut self, path: &Path) -> Result<Vec<NiumaEvent>, String> {
        let (read, mut parser) = {
            let repository = self
                .repository
                .lock()
                .map_err(|_| "Codex session repository lock poisoned".to_string())?;
            let read = repository.read_new_event_lines_for_path(path)?;
            let should_reset_parser =
                read.reset_parser || read.file_replaced || read.file_truncated;
            let parser = if should_reset_parser {
                CodexJsonlParser::default()
            } else {
                repository
                    .event_cursor(path)
                    .map(|cursor| cursor.parser.clone())
                    .unwrap_or_default()
            };
            (read, parser)
        };

        let mut events = Vec::new();
        debug_assert!(!read.ended_with_partial_line || read.next_offset <= read.file_len);
        for event_line in read.lines {
            debug_assert!(event_line.byte_end >= event_line.byte_start);
            debug_assert!(event_line.byte_end <= read.next_offset);
            let line = event_line.line;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(event) = parser.parse_line(&line, &path.to_string_lossy())? {
                events.push(event);
            }
        }
        self.repository
            .lock()
            .map_err(|_| "Codex session repository lock poisoned".to_string())?
            .store_event_cursor(
                path,
                CodexEventCursor {
                    offset: read.next_offset,
                    last_len: read.file_len,
                    file_identity: read.file_identity,
                    parser,
                },
            );
        Ok(events)
    }
}

#[cfg(unix)]
pub(crate) fn file_identity(metadata: &std::fs::Metadata) -> Option<CodexFileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Some(CodexFileIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

#[cfg(not(unix))]
pub(crate) fn file_identity(_metadata: &std::fs::Metadata) -> Option<CodexFileIdentity> {
    None
}
