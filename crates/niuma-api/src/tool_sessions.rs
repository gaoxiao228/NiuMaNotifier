use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use niuma_core::models::ToolKind;
use niuma_core::tool_session::ToolSessionListItem;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;

// 查询参数模型先服务内存 registry，Task 5 增加路由时可直接从 query string 映射。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolSessionListQuery {
    pub tool: Option<String>,
    pub include_subagents: bool,
    pub active_only: bool,
    pub limit: Option<usize>,
}

// registry 是宿主进程内的 snapshot 缓存，provider 每次上报时按 tool 整批替换。
#[derive(Clone, Default)]
pub struct ToolSessionRegistry {
    snapshots: Arc<RwLock<HashMap<ToolKind, Vec<ToolSessionListItem>>>>,
}

impl ToolSessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace_snapshot(&self, tool: ToolKind, sessions: Vec<ToolSessionListItem>) {
        let normalized = sessions
            .into_iter()
            .map(|mut session| {
                // snapshot 归属以 provider 注册工具为准，避免错误 payload 污染跨工具查询。
                session.tool = tool.clone();
                session
            })
            .collect();
        self.snapshots
            .write()
            .expect("tool session registry lock poisoned")
            .insert(tool, normalized);
    }

    pub fn list(&self, query: ToolSessionListQuery) -> Result<Vec<ToolSessionListItem>, String> {
        let limit = capped_limit(query.limit)?;
        let snapshots = self
            .snapshots
            .read()
            .expect("tool session registry lock poisoned");
        let mut items = snapshots_for_tool(&snapshots, query.tool.as_deref())
            .into_iter()
            .flatten()
            .filter(|item| query.include_subagents || !item.is_subagent)
            .filter(|item| !query.active_only || item.is_active)
            .cloned()
            .collect::<Vec<_>>();

        // 最新可见会话优先；last_seen 相同再按文件修改时间倒序。
        items.sort_by(|left, right| {
            right
                .last_seen_at
                .cmp(&left.last_seen_at)
                .then_with(|| right.modified_at.cmp(&left.modified_at))
        });
        items.truncate(limit);
        Ok(items)
    }

    pub fn find_session(&self, tool: &ToolKind, session_id: &str) -> Option<ToolSessionListItem> {
        self.snapshots
            .read()
            .expect("tool session registry lock poisoned")
            .get(tool)
            .and_then(|sessions| {
                sessions
                    .iter()
                    .find(|session| session.session_id == session_id)
                    .cloned()
            })
    }
}

fn capped_limit(limit: Option<usize>) -> Result<usize, String> {
    match limit.unwrap_or(DEFAULT_LIMIT) {
        0 => Err("limit 必须大于 0".to_string()),
        value => Ok(value.min(MAX_LIMIT)),
    }
}

fn snapshots_for_tool<'a>(
    snapshots: &'a HashMap<ToolKind, Vec<ToolSessionListItem>>,
    tool: Option<&str>,
) -> Vec<&'a Vec<ToolSessionListItem>> {
    match tool {
        Some("all") | None => snapshots.values().collect(),
        Some(tool_id) => snapshots
            .get(&ToolKind::from_id(tool_id))
            .into_iter()
            .collect(),
    }
}
