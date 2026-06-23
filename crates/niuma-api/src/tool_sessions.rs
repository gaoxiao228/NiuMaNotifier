use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use niuma_core::models::ToolKind;
use niuma_core::tool_session::{ToolSessionDetail, ToolSessionListItem};

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
    detail_providers: Arc<RwLock<HashMap<ToolKind, Arc<dyn ToolSessionDetailProvider>>>>,
}

// detail provider 由宿主 runtime 注册，API 层只通过统一 trait 获取归一化消息。
pub trait ToolSessionDetailProvider: Send + Sync {
    fn detail(
        &self,
        tool: &ToolKind,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<ToolSessionDetail, String>;
}

impl ToolSessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace_snapshot(&self, tool: ToolKind, sessions: Vec<ToolSessionListItem>) {
        let normalized = sessions
            .into_iter()
            .map(|mut session| {
                // snapshot 归属和 canonical id 都由宿主生成，避免错误 payload 污染查询结果。
                session.tool = tool.clone();
                session.id = canonical_tool_session_id(&tool, &session.session_id);
                session
            })
            .collect();
        self.snapshots
            .write()
            .expect("tool session registry lock poisoned")
            .insert(tool, normalized);
    }

    pub fn clear_snapshot(&self, tool: &ToolKind) {
        // provider 生命周期结束时只清理当前 tool，避免影响其他工具的会话缓存。
        self.snapshots
            .write()
            .expect("tool session registry lock poisoned")
            .remove(tool);
    }

    pub fn register_detail_provider(
        &self,
        tool: ToolKind,
        provider: Arc<dyn ToolSessionDetailProvider>,
    ) {
        self.detail_providers
            .write()
            .expect("tool session registry lock poisoned")
            .insert(tool, provider);
    }

    pub fn unregister_detail_provider(&self, tool: &ToolKind) {
        // 当前 registry 保证同一 tool 只有一个 detail provider，可按 tool 精确注销。
        self.detail_providers
            .write()
            .expect("tool session registry lock poisoned")
            .remove(tool);
    }

    pub fn list(&self, query: ToolSessionListQuery) -> Result<Vec<ToolSessionListItem>, String> {
        let limit = capped_limit(query.limit)?;
        let snapshot_items = {
            let snapshots = self
                .snapshots
                .read()
                .expect("tool session registry lock poisoned");

            // 读锁只保护 snapshot clone，排序和截断在锁外完成，避免阻塞 provider 写入。
            snapshots_for_tool(&snapshots, query.tool.as_deref())
                .into_iter()
                .flat_map(|sessions| sessions.iter().cloned())
                .collect::<Vec<_>>()
        };
        let mut items = snapshot_items
            .into_iter()
            .filter(|item| query.include_subagents || !item.is_subagent)
            .filter(|item| !query.active_only || item.is_active)
            .collect::<Vec<_>>();

        // 最新可见会话优先；时间相同后使用 canonical 字段升序，保证 all + limit 稳定。
        items.sort_by(|left, right| {
            right
                .last_seen_at
                .cmp(&left.last_seen_at)
                .then_with(|| right.modified_at.cmp(&left.modified_at))
                .then_with(|| left.tool.as_str().cmp(right.tool.as_str()))
                .then_with(|| left.session_id.cmp(&right.session_id))
                .then_with(|| left.id.cmp(&right.id))
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

    pub fn detail(
        &self,
        tool: &ToolKind,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<ToolSessionDetail, String> {
        let provider = self
            .detail_providers
            .read()
            .expect("tool session registry lock poisoned")
            .get(tool)
            .cloned()
            .ok_or_else(|| "session detail provider 尚未就绪".to_string())?;
        provider.detail(tool, session_id, limit, cursor)
    }
}

pub(crate) fn capped_limit(limit: Option<usize>) -> Result<usize, String> {
    match limit.unwrap_or(DEFAULT_LIMIT) {
        0 => Err("limit 必须大于 0".to_string()),
        value => Ok(value.min(MAX_LIMIT)),
    }
}

fn canonical_tool_session_id(tool: &ToolKind, session_id: &str) -> String {
    format!("{}:{session_id}", tool.as_str())
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
