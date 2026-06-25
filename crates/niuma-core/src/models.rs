use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

// MVP-0 的核心模型集中放在这里，后续 CLI、Tauri 和 hook helper 共用同一套类型。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ToolId {
    Codex,
    ClaudeCode,
    Custom(String),
}

pub type ToolKind = ToolId;

impl ToolId {
    pub const CODEX: &'static str = "codex";
    pub const CLAUDE_CODE: &'static str = "claude_code";

    pub fn as_str(&self) -> &str {
        match self {
            ToolId::Codex => Self::CODEX,
            ToolId::ClaudeCode => Self::CLAUDE_CODE,
            ToolId::Custom(value) => value.as_str(),
        }
    }

    pub fn from_id(value: impl Into<String>) -> Self {
        let value = value.into();
        match value.as_str() {
            Self::CODEX => ToolId::Codex,
            Self::CLAUDE_CODE => ToolId::ClaudeCode,
            _ => ToolId::Custom(value),
        }
    }
}

impl Serialize for ToolId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ToolId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(ToolId::from_id(value))
    }
}

// 状态枚举使用产品文档中的命名，序列化时保持 API 友好的 snake_case。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStateStatus {
    Idle,
    Running,
    WaitingApproval,
    WaitingInput,
    Completed,
    Error,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeStateItem {
    pub tool: ToolKind,
    pub session_id: String,
    pub project_path: String,
    pub project_name: String,
    pub status: RuntimeStateStatus,
    pub last_event_id: Option<String>,
    pub last_activity_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AttentionItem {
    pub event_id: String,
    pub tool: ToolKind,
    pub session_id: String,
    pub status: RuntimeStateStatus,
    pub summary: String,
    // 用于把后续“已恢复运行”的事件精确匹配到某个待处理项。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_resolve_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl AttentionItem {
    pub fn from_event(event: &NiumaEvent, status: RuntimeStateStatus) -> Self {
        Self {
            event_id: event.id.clone(),
            tool: event.tool.clone(),
            session_id: event.session_id.clone(),
            status,
            summary: event.summary.clone(),
            attention_resolve_key: event.attention_resolve_key.clone(),
            created_at: event.created_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Allowed,
    Denied,
    ReturnedToCodex,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalProxyStatus {
    None,
    Active,
    Lost,
}

fn default_approval_proxy_status() -> ApprovalProxyStatus {
    ApprovalProxyStatus::None
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionKind {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalChannel {
    HookProxy,
    NiumaCodexRelay,
}

impl ApprovalChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalChannel::HookProxy => "hook_proxy",
            ApprovalChannel::NiumaCodexRelay => "niuma_codex_relay",
        }
    }
}

fn default_approval_channel() -> ApprovalChannel {
    ApprovalChannel::HookProxy
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalControlRef {
    pub wrapper_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_session_id: Option<String>,
    pub relay_request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool: ToolKind,
    pub session_id: String,
    pub turn_id: String,
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub project_path: String,
    pub project_name: String,
    pub status: ApprovalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub proxy_timeout_seconds: u64,
    #[serde(default = "default_approval_proxy_status")]
    pub proxy_status: ApprovalProxyStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_lost_at: Option<DateTime<Utc>>,
    #[serde(default = "default_approval_channel")]
    pub channel: ApprovalChannel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_ref: Option<ApprovalControlRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub request_id: String,
    pub decision: ApprovalDecisionKind,
    pub decided_by: String,
    pub decided_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub decided_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LatestActivity {
    pub event_id: Option<String>,
    // idle 没有归属工具；真实运行态事件必须携带工具，避免同 session_id 跨工具串扰。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolKind>,
    pub session_id: Option<String>,
    pub status: RuntimeStateStatus,
    pub updated_at: Option<DateTime<Utc>>,
}

impl LatestActivity {
    pub fn idle() -> Self {
        Self {
            event_id: None,
            tool: None,
            session_id: None,
            status: RuntimeStateStatus::Idle,
            updated_at: None,
        }
    }

    pub fn from_event(event: &NiumaEvent, status: RuntimeStateStatus) -> Self {
        Self {
            event_id: Some(event.id.clone()),
            tool: Some(event.tool.clone()),
            session_id: Some(event.session_id.clone()),
            status,
            updated_at: Some(event.created_at),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    SessionStarted,
    SessionIdled,
    ApprovalRequested,
    ApprovalResolved,
    ApprovalReturnedToCodex,
    InputRequested,
    TaskFailed,
    AssistantMessageCompleted,
    ManualDismissed,
    SessionStaled,
    SessionActivity,
}

// 完成原因仅用于通知和诊断，不参与状态层的 event_type 聚合。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionReason {
    Normal,
    Interrupted,
    RolledBack,
    AbortedUnknown,
}

// 失败原因保留工具侧的可诊断分类，状态层仍以 TaskFailed 作为主语义。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    Timeout,
    ContextWindowExceeded,
    UsageLimitReached,
    ServerOverloaded,
    PolicyBlocked,
    ResponseStreamFailed,
    ConnectionFailed,
    QuotaExceeded,
    InternalServerError,
    RetryLimit,
    SandboxError,
    Fatal,
    Unknown,
}

// 事件级会话范围用于让外部插件区分主会话与 subagent 事件。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSessionScope {
    Main,
    Subagent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NiumaEvent {
    pub id: String,
    pub dedupe_key: String,
    pub source: String,
    pub tool: ToolKind,
    pub session_id: String,
    // Codex subagent 会话会带父会话 ID；展示仍使用 session_id，跨来源仲裁可用它归一化。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_scope: Option<EventSessionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
    pub project_path: String,
    pub project_name: String,
    pub event_type: EventType,
    pub severity: String,
    pub summary: String,
    // 对外展示正文，供等待授权、等待输入和完成态优先使用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    // 错误详情，供失败态优先使用，避免把长错误塞进短摘要。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    // 工具适配器可填写该键，让状态层只清除对应的阻塞项。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_resolve_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_reason: Option<CompletionReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<FailureReason>,
    pub payload_ref: Option<String>,
    pub created_at: DateTime<Utc>,
}

// 旧状态机的内部快照，只服务状态转移测试和诊断日志。
// 对外展示状态统一使用 MainStatePayload，由 MainStateService 计算。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InternalStateSnapshot {
    pub status: RuntimeStateStatus,
    pub primary_session_id: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
    pub primary_event: Option<NiumaEvent>,
}
