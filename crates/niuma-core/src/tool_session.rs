use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::ToolKind;

// 工具会话状态来自 provider snapshot，unknown 用于兼容无法判断活跃性的工具。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSessionStatus {
    Active,
    Inactive,
    Unknown,
}

// 会话范围用于区分用户主会话和 Codex/Claude 等工具派生出来的子代理会话。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSessionScope {
    Main,
    Subagent,
}

// 归一化状态只做诊断提示，不参与会话定位或状态转移。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSessionNormalizationStatus {
    Resolved,
    ParentMissing,
    ParentUnresolved,
}

// control 描述某个工具会话是否能被 Niuma 通过外部通道继续写入或控制。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolSessionControl {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

// 列表项是宿主保存的轻量会话索引，供后续 session_list API 直接返回。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolSessionListItem {
    pub id: String,
    pub tool: ToolKind,
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub file_path: String,
    pub modified_at: DateTime<Utc>,
    pub discovered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub is_active: bool,
    pub is_subagent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_scope: Option<ToolSessionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_status: Option<ToolSessionNormalizationStatus>,
    // 列表摘要只保留首条用户消息预览，避免 session 列表接口携带完整对话内容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_user_message_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<ToolSessionControl>,
    pub status: ToolSessionStatus,
}

// 消息角色覆盖主流 AI 工具会话记录，unknown 用于保留 provider 无法映射的原始角色。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSessionMessageRole {
    User,
    Assistant,
    System,
    ToolCall,
    ToolResult,
    Event,
    Unknown,
}

// 详情消息保留 metadata，方便 provider 携带工具特有字段而不阻塞统一 API。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSessionMessage {
    pub id: String,
    pub role: ToolSessionMessageRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: Value,
}

// 会话详情模型由后续 provider RPC 填充；registry 当前只保存列表 snapshot。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSessionDetail {
    pub tool: ToolKind,
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub is_subagent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_scope: Option<ToolSessionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_status: Option<ToolSessionNormalizationStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<ToolSessionControl>,
    pub messages: Vec<ToolSessionMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}
