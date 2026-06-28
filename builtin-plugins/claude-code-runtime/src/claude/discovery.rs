use std::path::{Path, PathBuf};

pub(crate) fn claude_projects_dir(claude_home: &Path) -> PathBuf {
    claude_home.join("projects")
}

pub(crate) fn recent_jsonl_files(claude_home: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_jsonl_files(&claude_projects_dir(claude_home), &mut files);
    files.sort_by(|left, right| modified_time(right).cmp(&modified_time(left)));
    files.into_iter().take(limit).collect()
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn modified_time(path: &Path) -> std::time::SystemTime {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}
