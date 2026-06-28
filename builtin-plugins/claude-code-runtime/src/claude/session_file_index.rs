use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClaudeSessionFileIndex {
    pub(crate) signature: ClaudeSessionFileSignature,
    // 只保存可展示消息的原始 JSONL 行位置，provider 不长期持有完整对话正文。
    pub(crate) message_lines: Vec<ClaudeMessageLineIndex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClaudeMessageLineIndex {
    pub(crate) line_index: usize,
    pub(crate) byte_start: u64,
    pub(crate) byte_end: u64,
    pub(crate) content_hash: u64,
}

impl ClaudeMessageLineIndex {
    pub(crate) fn new(line_index: usize, byte_start: u64, line_bytes: &[u8]) -> Self {
        Self {
            line_index,
            byte_start,
            byte_end: byte_start + line_bytes.len() as u64,
            content_hash: stable_bytes_hash(line_bytes),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClaudeSessionFileSignature {
    pub(crate) modified_system_time: SystemTime,
    pub(crate) size_bytes: u64,
    pub(crate) content_hash: u64,
}

pub(crate) fn session_file_signature(path: &Path) -> std::io::Result<ClaudeSessionFileSignature> {
    let metadata = std::fs::metadata(path)?;
    let content = std::fs::read(path)?;
    Ok(ClaudeSessionFileSignature {
        modified_system_time: metadata.modified()?,
        size_bytes: metadata.len(),
        content_hash: stable_bytes_hash(&content),
    })
}

pub(crate) fn read_indexed_line(
    file_path: &str,
    line_index: &ClaudeMessageLineIndex,
) -> Result<String, String> {
    if line_index.byte_end < line_index.byte_start {
        return Err(format!(
            "Claude Code session 索引已过期，第 {} 行范围非法",
            line_index.line_index + 1
        ));
    }
    let mut file = File::open(file_path)
        .map_err(|error| format!("打开 Claude Code session 文件失败：{error}"))?;
    file.seek(SeekFrom::Start(line_index.byte_start))
        .map_err(|error| format!("读取 Claude Code session 文件失败：{error}"))?;
    let byte_len = (line_index.byte_end - line_index.byte_start) as usize;
    let mut buffer = vec![0u8; byte_len];
    file.read_exact(&mut buffer)
        .map_err(|error| format!("读取 Claude Code session 文件失败：{error}"))?;
    if stable_bytes_hash(&buffer) != line_index.content_hash {
        return Err(format!(
            "Claude Code session 索引已过期，第 {} 行内容已变化",
            line_index.line_index + 1
        ));
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

pub(crate) fn trim_jsonl_line_bytes(buffer: &[u8]) -> &[u8] {
    buffer.strip_suffix(b"\n").unwrap_or(buffer)
}

fn stable_bytes_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}
