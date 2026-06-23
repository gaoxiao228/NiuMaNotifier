use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use niuma_core::models::ToolKind;
use niuma_core::tool_session::{
    ToolSessionDetail, ToolSessionListItem, ToolSessionNormalizationStatus, ToolSessionScope,
    ToolSessionStatus,
};
use serde::Serialize;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const DEFAULT_PAGE: usize = 1;
const DEFAULT_PAGE_SIZE: usize = 20;
const MAX_PAGE_SIZE: usize = 100;

// 查询参数模型先服务内存 registry，Task 5 增加路由时可直接从 query string 映射。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolSessionListQuery {
    pub tool: Option<String>,
    pub include_subagents: bool,
    pub active_only: bool,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolSessionProjectGroupsQuery {
    pub tool: Option<String>,
    pub project_path: Option<String>,
    pub include_subagents: bool,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSessionProjectGroupPage {
    pub list: Vec<ProjectSessionGroup>,
    pub page: usize,
    pub page_size: usize,
    pub total: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProjectSessionGroup {
    pub tool: ToolKind,
    pub project_path: String,
    pub project_name: String,
    pub updated_at: DateTime<Utc>,
    pub normalized_session_count: usize,
    pub raw_session_count: usize,
    pub subagent_count: usize,
    pub sessions: Vec<NormalizedSessionSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NormalizedSessionSummary {
    pub normalized_session_id: String,
    pub primary_session_id: String,
    pub title: String,
    pub status: ToolSessionStatus,
    pub updated_at: DateTime<Utc>,
    pub latest_event_summary: Option<String>,
    pub subagent_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_sessions: Option<Vec<RawSessionSummary>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RawSessionSummary {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    pub normalized_session_id: String,
    pub session_scope: ToolSessionScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_status: Option<ToolSessionNormalizationStatus>,
    pub source_path: String,
    pub updated_at: DateTime<Utc>,
    pub is_active: bool,
    pub status: ToolSessionStatus,
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

    pub fn project_groups(
        &self,
        query: ToolSessionProjectGroupsQuery,
    ) -> Result<ToolSessionProjectGroupPage, String> {
        let page = capped_page(query.page)?;
        let page_size = capped_page_size(query.page_size)?;
        let snapshot_items = {
            let snapshots = self
                .snapshots
                .read()
                .expect("tool session registry lock poisoned");

            snapshots_for_tool(&snapshots, query.tool.as_deref())
                .into_iter()
                .flat_map(|sessions| sessions.iter().cloned())
                .collect::<Vec<_>>()
        };
        let project_path = query
            .project_path
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let mut projects = HashMap::<String, ProjectAccumulator>::new();

        for item in snapshot_items {
            if project_path
                .as_deref()
                .is_some_and(|expected| item.project_path != expected)
            {
                continue;
            }
            let key = format!("{}\0{}", item.tool.as_str(), item.project_path);
            projects
                .entry(key)
                .or_insert_with(|| ProjectAccumulator::new(&item))
                .push(item);
        }

        let mut groups = projects
            .into_values()
            .map(|project| project.into_group(query.include_subagents))
            .collect::<Vec<_>>();
        groups.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.tool.as_str().cmp(right.tool.as_str()))
                .then_with(|| left.project_path.cmp(&right.project_path))
        });

        let total = groups.len();
        let start = (page - 1).saturating_mul(page_size);
        let list = groups.into_iter().skip(start).take(page_size).collect();
        Ok(ToolSessionProjectGroupPage {
            list,
            page,
            page_size,
            total,
        })
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
        let detail = provider.detail(tool, session_id, limit, cursor)?;
        // provider 返回的详情归属必须与请求一致，避免错误实现造成跨工具或跨会话串读。
        if detail.tool != *tool || detail.session_id != session_id {
            return Err("provider 返回的 session detail 归属不匹配".to_string());
        }
        Ok(detail)
    }
}

struct ProjectAccumulator {
    tool: ToolKind,
    project_path: String,
    project_name: String,
    sessions: HashMap<String, Vec<ToolSessionListItem>>,
}

impl ProjectAccumulator {
    fn new(item: &ToolSessionListItem) -> Self {
        Self {
            tool: item.tool.clone(),
            project_path: item.project_path.clone(),
            project_name: item.project_name.clone(),
            sessions: HashMap::new(),
        }
    }

    fn push(&mut self, item: ToolSessionListItem) {
        if !item.project_name.trim().is_empty() {
            self.project_name = item.project_name.clone();
        }
        self.sessions
            .entry(normalized_session_id_for(&item))
            .or_default()
            .push(item);
    }

    fn into_group(self, include_subagents: bool) -> ProjectSessionGroup {
        let raw_session_count = self
            .sessions
            .values()
            .map(|sessions| sessions.len())
            .sum::<usize>();
        let mut sessions = self
            .sessions
            .into_iter()
            .map(|(normalized_session_id, raw_sessions)| {
                normalized_summary(normalized_session_id, raw_sessions, include_subagents)
            })
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.primary_session_id.cmp(&right.primary_session_id))
        });
        let updated_at = sessions
            .iter()
            .map(|session| session.updated_at)
            .max()
            .unwrap_or_else(Utc::now);
        let subagent_count = sessions
            .iter()
            .map(|session| session.subagent_count)
            .sum::<usize>();

        ProjectSessionGroup {
            tool: self.tool,
            project_path: self.project_path,
            project_name: self.project_name,
            updated_at,
            normalized_session_count: sessions.len(),
            raw_session_count,
            subagent_count,
            sessions,
        }
    }
}

fn normalized_summary(
    normalized_session_id: String,
    mut raw_sessions: Vec<ToolSessionListItem>,
    include_subagents: bool,
) -> NormalizedSessionSummary {
    raw_sessions.sort_by(|left, right| {
        session_scope_sort_rank(left)
            .cmp(&session_scope_sort_rank(right))
            .then_with(|| right.modified_at.cmp(&left.modified_at))
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    let primary = raw_sessions
        .iter()
        .find(|item| session_scope_for(item) == ToolSessionScope::Main)
        .or_else(|| {
            raw_sessions
                .iter()
                .find(|item| item.session_id == normalized_session_id)
        })
        .unwrap_or(&raw_sessions[0]);
    let updated_at = raw_sessions
        .iter()
        .map(|item| item.modified_at)
        .max()
        .unwrap_or(primary.modified_at);
    let subagent_count = raw_sessions.iter().filter(|item| item.is_subagent).count();
    let status = if raw_sessions.iter().any(|item| item.is_active) {
        ToolSessionStatus::Active
    } else {
        primary.status.clone()
    };
    let raw_summaries = include_subagents.then(|| {
        raw_sessions
            .iter()
            .map(raw_session_summary)
            .collect::<Vec<_>>()
    });

    NormalizedSessionSummary {
        normalized_session_id: normalized_session_id.clone(),
        primary_session_id: primary.session_id.clone(),
        title: session_title(primary),
        status,
        updated_at,
        latest_event_summary: None,
        subagent_count,
        raw_sessions: raw_summaries,
    }
}

fn raw_session_summary(item: &ToolSessionListItem) -> RawSessionSummary {
    RawSessionSummary {
        session_id: item.session_id.clone(),
        parent_session_id: item.parent_session_id.clone(),
        normalized_session_id: normalized_session_id_for(item),
        session_scope: session_scope_for(item),
        agent_nickname: item.agent_nickname.clone(),
        agent_role: item.agent_role.clone(),
        normalization_status: item.normalization_status.clone(),
        source_path: item.file_path.clone(),
        updated_at: item.modified_at,
        is_active: item.is_active,
        status: item.status.clone(),
    }
}

fn normalized_session_id_for(item: &ToolSessionListItem) -> String {
    item.normalized_session_id
        .clone()
        .or_else(|| {
            item.is_subagent
                .then(|| item.parent_session_id.clone())
                .flatten()
        })
        .unwrap_or_else(|| item.session_id.clone())
}

fn session_scope_for(item: &ToolSessionListItem) -> ToolSessionScope {
    item.session_scope.clone().unwrap_or_else(|| {
        if item.is_subagent {
            ToolSessionScope::Subagent
        } else {
            ToolSessionScope::Main
        }
    })
}

fn session_scope_sort_rank(item: &ToolSessionListItem) -> usize {
    match session_scope_for(item) {
        ToolSessionScope::Main => 0,
        ToolSessionScope::Subagent => 1,
    }
}

fn session_title(item: &ToolSessionListItem) -> String {
    let short_id = item.session_id.chars().take(8).collect::<String>();
    format!("session-{short_id}")
}

pub(crate) fn capped_limit(limit: Option<usize>) -> Result<usize, String> {
    match limit.unwrap_or(DEFAULT_LIMIT) {
        0 => Err("limit 必须大于 0".to_string()),
        value => Ok(value.min(MAX_LIMIT)),
    }
}

fn capped_page(page: Option<usize>) -> Result<usize, String> {
    match page.unwrap_or(DEFAULT_PAGE) {
        0 => Err("page 必须大于 0".to_string()),
        value => Ok(value),
    }
}

fn capped_page_size(page_size: Option<usize>) -> Result<usize, String> {
    match page_size.unwrap_or(DEFAULT_PAGE_SIZE) {
        0 => Err("page_size 必须大于 0".to_string()),
        value => Ok(value.min(MAX_PAGE_SIZE)),
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
