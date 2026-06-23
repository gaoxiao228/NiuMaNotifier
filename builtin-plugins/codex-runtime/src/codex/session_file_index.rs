use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub(crate) struct CodexSessionFileIndex {
    pub(crate) signature: CodexSessionFileSignature,
    pub(crate) session_meta_line: Option<CodexMessageLineIndex>,
    // 只保存可分页消息的原始 JSONL 行位置，不在 provider 内存中长期持有完整对话正文。
    pub(crate) message_lines: Vec<CodexMessageLineIndex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CodexMessageLineIndex {
    pub(crate) line_index: usize,
    pub(crate) byte_start: u64,
    pub(crate) byte_end: u64,
    pub(crate) content_hash: u64,
}

impl CodexMessageLineIndex {
    pub(crate) fn new(line_index: usize, byte_start: u64, line_bytes: &[u8]) -> Self {
        Self {
            line_index,
            byte_start,
            byte_end: byte_start + line_bytes.len() as u64,
            content_hash: stable_bytes_hash(line_bytes),
        }
    }

    fn byte_len(self) -> Result<usize, String> {
        if self.byte_end < self.byte_start {
            return Err(format!(
                "Codex session 索引已过期，第 {} 行范围非法",
                self.line_index + 1
            ));
        }
        Ok((self.byte_end - self.byte_start) as usize)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CodexSessionFileSignature {
    pub(crate) modified_system_time: SystemTime,
    pub(crate) size_bytes: u64,
    pub(crate) content_hash: u64,
}

impl CodexSessionFileSignature {
    pub(crate) fn fallback(modified_system_time: SystemTime, size_bytes: u64) -> Self {
        Self {
            modified_system_time,
            size_bytes,
            content_hash: 0,
        }
    }

    pub(crate) fn metadata_changed(self, path: &str) -> io::Result<bool> {
        let metadata = std::fs::metadata(path)?;
        Ok(metadata.modified()? != self.modified_system_time || metadata.len() != self.size_bytes)
    }
}

pub(crate) fn session_file_signature(path: &Path) -> io::Result<CodexSessionFileSignature> {
    let metadata = std::fs::metadata(path)?;
    session_file_signature_from_metadata(path, &metadata)
}

fn session_file_signature_from_metadata(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> io::Result<CodexSessionFileSignature> {
    // 签名只读取原始字节，不解析 JSONL，避免把缓存校验退化成重复逐行索引。
    let content = std::fs::read(path)?;
    Ok(CodexSessionFileSignature {
        modified_system_time: metadata.modified()?,
        size_bytes: metadata.len(),
        content_hash: stable_bytes_hash(&content),
    })
}

pub(crate) fn read_indexed_line(
    file_path: &str,
    line_index: &CodexMessageLineIndex,
) -> Result<String, String> {
    let mut file =
        File::open(file_path).map_err(|error| format!("打开 Codex session 文件失败：{error}"))?;
    let byte_len = line_index.byte_len()?;
    file.seek(SeekFrom::Start(line_index.byte_start))
        .map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;
    let mut buffer = vec![0u8; byte_len];
    file.read_exact(&mut buffer)
        .map_err(|error| format!("读取 Codex session 文件失败：{error}"))?;
    if stable_bytes_hash(&buffer) != line_index.content_hash {
        return Err(format!(
            "Codex session 索引已过期，第 {} 行内容已变化",
            line_index.line_index + 1
        ));
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

pub(crate) fn trim_jsonl_line_bytes(buffer: &[u8]) -> &[u8] {
    buffer.strip_suffix(b"\n").unwrap_or(buffer)
}

pub(crate) fn stable_bytes_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_index_keeps_signature_and_line_ranges() {
        let signature = CodexSessionFileSignature::fallback(SystemTime::UNIX_EPOCH, 10);
        let line = CodexMessageLineIndex::new(0, 0, b"first");
        let index = CodexSessionFileIndex {
            signature,
            session_meta_line: Some(line),
            message_lines: vec![line],
        };

        assert_eq!(index.signature, signature);
        assert_eq!(index.session_meta_line, Some(line));
        assert_eq!(index.message_lines, vec![line]);
    }

    #[test]
    fn indexed_line_rejects_changed_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        std::fs::write(&path, "first\nsecond\n").unwrap();
        let index = CodexMessageLineIndex::new(0, 0, b"first");

        std::fs::write(&path, "other\nsecond\n").unwrap();

        let error = read_indexed_line(&path.to_string_lossy(), &index).unwrap_err();
        assert!(error.contains("内容已变化"));
    }

    #[test]
    fn file_signature_detects_size_change() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.jsonl");
        std::fs::write(&path, "first\n").unwrap();
        let signature = session_file_signature(&path).unwrap();

        std::fs::write(&path, "first\nsecond\n").unwrap();

        assert!(signature.metadata_changed(&path.to_string_lossy()).unwrap());
    }
}
