# Codex session 文件归一化方案

本文档规划 NiumaNotifier 如何把 Codex 本地 session JSONL 文件转换为稳定的业务会话语义。目标是区分 Codex 原始数据、Niuma 计算字段和对外 API/SSE 字段，避免把内部观测字段误当成官方稳定协议。

## 背景

Codex watcher 当前按 `~/.codex/sessions/**/*.jsonl` 文件扫描事件。每个 JSONL 文件会被解析为 `NiumaEvent`，再进入主状态、通知插件、事件中心和授权仲裁。

这个模型在普通主会话里成立，但在 subagent 场景下会出现语义偏差：

- watcher 可能从 subagent session 文件识别授权请求，而 hook payload 使用父 session，导致同一授权请求 fingerprint 不一致。
- subagent 的 `task_complete` 会被解析为 `assistant_message_completed`，可能被默认通知误认为主任务完成。
- 多个 subagent 同时活动时，子会话事件可能抢占主状态展示。

因此需要把“Codex 原始 session 文件”归一化为“Niuma 业务会话视图”。

## 证据分级

### 官方文档确认

来自 OpenAI Codex 官方文档：

- Codex 支持 subagent workflow，subagent 是被委派的 agent thread。
- subagent activity 会在 Codex app 和 CLI 中展示。
- 交互式 CLI 中，inactive agent thread 的 approval request 也可能浮现给用户处理。
- `agents.max_depth` 说明 root session starts at 0。
- session transcripts 存放在 `$CODEX_HOME/sessions`，默认是 `~/.codex/sessions`。

参考：

- https://developers.openai.com/codex/subagents
- https://developers.openai.com/codex/cli
- https://developers.openai.com/codex/hooks

官方文档没有公开承诺以下 JSONL 内部字段：

- `session_meta`
- `thread_source`
- `parent_thread_id`
- `source.subagent.thread_spawn`
- `agent_nickname`
- `agent_role`
- `forked_from_id`
- `root_session_id`

因此这些字段只能按“本机观测到的 Codex 内部数据格式”使用，不能当成官方稳定 API。

### 本机样本确认

对本机 `~/.codex/sessions` 做只读扫描后，样本结果如下：

| 指标 | 数量 |
| --- | ---: |
| session 文件 | 83 |
| `session_meta` 行 | 238 |
| `thread_source = user` | 197 |
| `thread_source = subagent` | 41 |
| subagent 缺失 `parent_thread_id` | 0 |
| root-like 字段 | 0 |
| subagent `depth = 1` | 41 |

本机 subagent `session_meta.payload` 中稳定观测到：

- `id`
- `cwd`
- `thread_source = "subagent"`
- `parent_thread_id`
- `source.subagent.thread_spawn.parent_thread_id`
- `source.subagent.thread_spawn.depth`
- `agent_nickname`
- `agent_role`
- `multi_agent_version`

没有观测到：

- `root_session_id`
- `root_thread_id`

结论：root 不能从 Codex 原始字段直接读取，只能由 Niuma 基于 parent 关系计算。

## 术语

### 原始字段

原始字段来自 Codex JSONL，不改变含义：

| 字段 | 含义 |
| --- | --- |
| `session_id` | Codex 原始 session id，通常来自 `session_meta.payload.id` 或文件名兜底。 |
| `parent_session_id` | Codex 原始 parent id，优先来自 `payload.parent_thread_id`，兜底读取 `payload.source.subagent.thread_spawn.parent_thread_id`。 |
| `thread_source` | Codex 原始线程来源，当前观测到 `user` / `subagent`。 |
| `agent_nickname` | Codex 给 subagent 分配的展示昵称。 |
| `agent_role` | Codex subagent 角色，例如 `default` / `worker`。 |

### 计算字段

计算字段由 Niuma 生成：

| 字段 | 含义 |
| --- | --- |
| `normalized_session_id` | Niuma 业务归一会话 ID。普通主会话等于 `session_id`；subagent 尽量追溯到根主会话。 |
| `session_scope` | Niuma 业务范围，建议为 `main` / `subagent`。 |
| `normalization_status` | 可选诊断字段，表示归一化是否完整，例如 `resolved` / `parent_missing` / `parent_unresolved`。 |

## 对外事件字段

`NiumaEvent` 应保持向后兼容：

```ts
{
  session_id: string,
  parent_session_id?: string,
  normalized_session_id?: string,
  session_scope?: "main" | "subagent",
  agent_nickname?: string,
  agent_role?: string
}
```

兼容原则：

- `session_id` 不改语义，始终表示真实事件来源 session。
- 新字段全部是 optional。
- 旧插件忽略未知字段仍可正常工作。
- 文档必须说明 `normalized_session_id` 是 Niuma 计算字段，不是 Codex 原始字段。

## 归一化规则

### 读取 session identity

每个 Codex session 文件维护一个 `SessionIdentity`：

```rust
struct SessionIdentity {
    session_id: String,
    source_path: PathBuf,
    project_path: Option<String>,
    thread_source: Option<String>,
    parent_session_id: Option<String>,
    normalized_session_id: String,
    session_scope: SessionScope,
    agent_nickname: Option<String>,
    agent_role: Option<String>,
}
```

读取规则：

1. `session_id` 优先取第一个有效 `session_meta.payload.id`。
2. 如果没有有效 `session_meta.payload.id`，从 rollout 文件名提取 session id。
3. `parent_session_id` 优先取 `payload.parent_thread_id`。
4. 如果顶层 `parent_thread_id` 缺失，再读取 `payload.source.subagent.thread_spawn.parent_thread_id`。
5. `session_scope` 根据 `thread_source` 判断：
   - `thread_source = "subagent"` -> `subagent`
   - 其他情况 -> `main`
6. 后续重复出现的 `session_meta` 不覆盖文件自身 `session_id`，避免 subagent 文件被 parent meta 覆盖。

### 计算 normalized_session_id

计算规则：

```text
如果 session_scope = main:
  normalized_session_id = session_id

如果 session_scope = subagent 且 parent_session_id 已知:
  如果 registry 中能找到 parent 的 identity:
    normalized_session_id = parent.normalized_session_id
  否则:
    normalized_session_id = parent_session_id

如果 session_scope = subagent 但 parent_session_id 缺失:
  normalized_session_id = session_id
```

说明：

- 当前本机样本中所有 subagent 都是 `depth = 1`，所以 `parent_session_id` 等于根主会话。
- 未来如果 Codex 支持更深层 subagent，递归追溯规则可以自然支持。
- parent identity 可能晚于 child 被发现，因此 registry 需要允许后续修正。

## 业务消费规则

不同业务层应使用不同 ID：

| 场景 | 使用字段 |
| --- | --- |
| 文件定位、session 详情 | `session_id` |
| 事件中心原始事件展示 | `session_id` + `parent_session_id` + `session_scope` |
| 授权仲裁 fingerprint | `normalized_session_id`，缺失时回退 `parent_session_id`，再回退 `session_id` |
| 主状态聚合 | `session_scope` + `normalized_session_id` |
| 默认完成通知 | 默认只通知 `session_scope = main` 的完成事件 |
| 默认授权通知 | subagent 授权仍可通知，因为需要用户处理 |

### 完成事件

建议规则：

```text
main completed:
  更新主状态为 completed
  默认触发完成通知

subagent completed:
  保留事件，进入事件中心和 SSE
  默认不触发完成通知
  默认不覆盖主状态为 completed
```

### 授权事件

建议规则：

```text
main approval:
  正常进入授权仲裁和通知

subagent approval:
  使用 normalized_session_id 参与仲裁
  可以触发通知，因为它需要用户处理
```

### 失败事件

失败事件先保持保守策略：

```text
main failed:
  正常进入主状态和通知

subagent failed:
  保留事件
  是否触发默认通知后续单独讨论
```

原因：subagent 失败有时会被主会话汇总处理，直接提升为主失败可能造成误报。

## 兼容与降级

Codex JSONL 内部字段没有官方公开稳定协议，因此必须降级友好：

- `thread_source` 缺失时，按 `main` 处理。
- `parent_session_id` 缺失时，不猜测父会话。
- parent 链断裂时，`normalized_session_id` 回退到已知最近父 ID 或自身 ID。
- 任何归一化失败不得阻止基础事件上报。
- debug/trace 日志应记录 `session_id`、`parent_session_id`、`normalized_session_id`、`session_scope` 和 fallback 原因。

## 实施阶段

### 阶段一：身份字段补齐

- 扩展 Codex parser 的 session identity。
- 事件上带 `normalized_session_id`、`session_scope`、`agent_nickname`、`agent_role`。
- 保持 `session_id` 原语义不变。
- 补齐 SSE/插件文档。

### 阶段二：业务层消费

- 授权仲裁从 parent 优先升级为 normalized 优先。
- 主状态聚合识别 `session_scope`。
- 默认通知策略跳过 subagent completed。
- 日志统一打印 raw/parent/normalized/scope。

### 阶段三：session API

- session 列表返回 raw session 和 normalized session 信息。
- session 详情按 raw session id 定位 JSONL 文件。
- 提供按项目分组的业务会话视图，支持类似 Codex 主界面的项目 -> 会话结构。

## 落地设计

### 模块边界

归一化逻辑应放在 Codex adapter / watcher 内部，避免散落到 API handler 或前端：

| 模块 | 职责 |
| --- | --- |
| `builtin-plugins/codex-runtime/src/codex/session_protocol/current.rs` | 从 JSONL 行读取原始 identity 字段，并生成事件字段。 |
| `builtin-plugins/codex-runtime/src/codex/session_watcher.rs` | 管理文件扫描状态和 parser 生命周期。 |
| `builtin-plugins/codex-runtime/src/codex/session_identity.rs` | 建议新增，集中定义 `SessionIdentity`、`SessionScope` 和归一化计算。 |
| `crates/niuma-core/src/models.rs` | 定义对外事件字段，保持 optional 兼容。 |
| `crates/niuma-api/src/handlers.rs` | 授权仲裁只消费归一化结果，不解析 Codex 原始字段。 |
| `crates/niuma-core/src/main_state.rs` | 主状态聚合按 `session_scope` 决定是否采纳完成事件。 |
| `builtin-plugins/bark-runtime` / `builtin-plugins/ntfy-runtime` | 默认通知策略按 `session_scope` 过滤 subagent completed。 |

### 建议数据结构

`SessionScope` 建议放在 core model，方便 API、通知插件和前端共享语义：

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionScope {
    Main,
    Subagent,
}
```

`NiumaEvent` 建议扩展为：

```rust
pub struct NiumaEvent {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_scope: Option<SessionScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
}
```

`SessionIdentity` 可以先只存在于 Codex runtime：

```rust
pub struct SessionIdentity {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub normalized_session_id: String,
    pub session_scope: SessionScope,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
}
```

不建议第一版引入持久化表。Codex watcher 是实时进程，先在 parser/scanner 内存里维护即可；session API 后续需要历史查询时，再评估是否缓存或按需读取 JSONL。

### parser 行为

当前 parser 已经避免后续 `session_meta` 覆盖首个 session id。归一化实现应继续保持：

```text
第一个有效 session_meta 决定文件自身 session identity。
后续重复 session_meta 可以用于补字段，但不能替换 session_id。
```

补字段规则：

```text
如果 identity 中 cwd 为空，后续 session_meta 可补 cwd。
如果 identity 中 parent_session_id 为空，后续 subagent meta 可补 parent。
如果 identity 中 agent_nickname / agent_role 为空，后续 meta 可补。
```

不允许：

```text
child rollout 文件后续出现 parent session_meta 后，把 child session_id 改成 parent session_id。
```

### registry 行为

第一版可以不做全局复杂图，只做轻量 `HashMap<String, SessionIdentity>`：

```text
key = session_id
value = identity
```

更新时机：

1. parser 从当前文件读到有效 `session_meta` 时更新当前文件 identity。
2. scanner 成功解析文件后，把 identity 写入 registry。
3. 生成事件时，使用 registry 计算 `normalized_session_id`。

parent 未发现时：

```text
normalized_session_id = parent_session_id
normalization_status = parent_unresolved
```

后续发现 parent 后，新事件会得到更准确的 normalized id。旧事件不回写，避免引入复杂迁移；需要历史准确聚合时，由 session API 阶段按需重算。

### API/SSE 兼容

新增字段全部 optional，示例：

```json
{
  "session_id": "019ef255-bec1-7541-be83-0fd75f59b263",
  "parent_session_id": "019ef255-5292-7d23-983c-95343f8cbaf3",
  "normalized_session_id": "019ef255-5292-7d23-983c-95343f8cbaf3",
  "session_scope": "subagent",
  "agent_nickname": "Ramanujan",
  "agent_role": "default"
}
```

文档要明确：

- 插件做业务决策时优先看 `session_scope`。
- 插件需要按用户主任务聚合时使用 `normalized_session_id`。
- 插件需要定位原始会话详情时使用 `session_id`。

## 项目分组视图

session 归一化不仅解决父子 session 关系，也为项目维度的会话浏览提供基础。目标结构类似 Codex 主界面：

```text
ProjectGroup
  NormalizedSession
    RawSession / SubagentSession
```

### 分层语义

| 层级 | 用途 |
| --- | --- |
| ProjectGroup | 按项目聚合，面向 UI 左侧项目列表和插件扫描入口。 |
| NormalizedSession | 用户可理解的一次主会话/任务，默认列表展示这一层。 |
| RawSession | Codex 原始 session 文件，包含 main session 和 subagent session。 |

不要把 subagent raw session 直接铺到主会话列表，否则同一任务下的多个子代理会污染用户视图。

### 建议响应模型

项目分组：

```ts
type ProjectSessionGroup = {
  tool: "codex" | "claude" | string
  project_path: string
  project_name: string
  updated_at: string
  normalized_session_count: number
  raw_session_count: number
  subagent_count: number
  sessions: NormalizedSessionSummary[]
}
```

归一会话：

```ts
type NormalizedSessionSummary = {
  normalized_session_id: string
  primary_session_id: string
  title: string
  status: string
  updated_at: string
  latest_event_summary: string | null
  subagent_count: number
  raw_sessions?: RawSessionSummary[]
}
```

原始 session：

```ts
type RawSessionSummary = {
  session_id: string
  parent_session_id?: string
  normalized_session_id: string
  session_scope: "main" | "subagent"
  agent_nickname?: string
  agent_role?: string
  source_path: string
  created_at?: string
  updated_at: string
}
```

### 查询接口草案

遵循项目 API 规范，查询类接口使用 GET，业务参数放查询参数，不使用路径动态参数。

```http
GET /api/v1/session-groups?tool=codex&include_subagents=false
```

返回：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "tool": "codex",
        "project_path": "/Users/niuma/code/NiuMaNotifier",
        "project_name": "NiuMaNotifier",
        "updated_at": "2026-06-23T10:00:00Z",
        "normalized_session_count": 3,
        "raw_session_count": 8,
        "subagent_count": 5,
        "sessions": []
      }
    ],
    "page": 1,
    "page_size": 20,
    "total": 1
  }
}
```

建议查询参数：

| 参数 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `tool` | string | 空 | 可筛选 `codex` / `claude` 等。 |
| `project_path` | string | 空 | 精确筛选项目路径。 |
| `include_subagents` | boolean | `false` | 是否在每个 normalized session 下返回 raw subagent 列表。 |
| `page` | number | `1` | 分页页码。 |
| `page_size` | number | `20` | 分页大小。 |

### session 详情接口草案

raw session 详情按查询参数传递：

```http
GET /api/v1/session-detail?tool=codex&session_id=019ef255-bec1-7541-be83-0fd75f59b263
```

返回内容应面向第三方插件做业务决策，至少包含：

```ts
type SessionDetail = {
  tool: string
  session_id: string
  parent_session_id?: string
  normalized_session_id?: string
  session_scope?: "main" | "subagent"
  project_path: string
  project_name: string
  source_path: string
  messages: SessionMessage[]
  events: SessionEventSummary[]
}
```

注意：

- `session_id` 查询的是 raw session，不是 normalized session。
- 如果后续支持按 normalized session 查询，应另加 `normalized_session_id` 查询参数，不要复用 `session_id`。
- 详情接口可能返回较大数据，第一版可限制最近 N 条消息或提供 `limit`。

### 排序规则

项目排序：

```text
project.updated_at = 项目下最新 normalized session 更新时间
按 updated_at 倒序
```

归一会话排序：

```text
normalized.updated_at = main raw session 和子 raw session 中最新事件时间
按 updated_at 倒序
```

raw session 排序：

```text
main session 优先
subagent 按 updated_at 倒序
```

这样 subagent 正在运行时，其父归一会话会被顶到项目会话列表前面，但 subagent 不会单独占据主列表。

### 标题规则

会话标题建议按优先级生成：

1. 第一条明确用户 prompt。
2. 最近的主会话摘要。
3. 最近事件摘要。
4. `session_id` 截断。

subagent 标题建议优先使用：

1. `agent_nickname` + `agent_role`。
2. subagent 初始 prompt 摘要。
3. `session_id` 截断。

### UI 与插件消费规则

UI 默认行为：

- 左侧按项目展示。
- 项目内默认展示 normalized session。
- subagent 默认折叠，只在展开或详情页显示。
- subagent completed 不作为主会话完成提示。

插件默认行为：

- 做项目级扫描时使用 `/api/v1/session-groups`。
- 要定位原始 transcript 时使用 `/api/v1/session-detail?session_id=...`。
- 要按用户主任务聚合时使用 `normalized_session_id`。
- 要识别是否是子代理事件时使用 `session_scope`。

## 测试矩阵

### Codex parser 测试

- 普通 `thread_source=user` session：
  - `session_scope = main`
  - `normalized_session_id = session_id`
  - `parent_session_id = None`
- subagent session：
  - 读取顶层 `parent_thread_id`
  - 读取嵌套 `source.subagent.thread_spawn.parent_thread_id`
  - `session_scope = subagent`
  - `normalized_session_id = parent_session_id`
- 缺失 `thread_source`：
  - 降级为 `main`
- 缺失 `parent_thread_id`：
  - 不猜测 parent
  - `normalized_session_id = session_id`
- 同文件多个 `session_meta`：
  - 首个 session id 不被覆盖
  - 后续 meta 可补充缺失字段

### 授权仲裁测试

- hook 父 session + watcher 子 session：
  - 使用 `normalized_session_id` 后 fingerprint 一致
  - watcher delayed event 被 suppress
- 无 normalized 字段的旧事件：
  - 回退到 `parent_session_id`
  - 再回退到 `session_id`
- 同一父会话下两个不同命令：
  - 不应互相 suppress

### 主状态测试

- `main completed` 更新主状态为 completed。
- `subagent completed` 不覆盖主状态为 completed。
- `subagent approval_requested` 仍进入 waiting_approval。
- `main waiting_approval` 与 `subagent completed` 同时存在时，主状态保持 waiting_approval。

### 通知插件测试

- Bark/ntfy 默认通知 `main completed`。
- Bark/ntfy 默认跳过 `subagent completed`。
- Bark/ntfy 仍通知 `subagent approval_requested`。
- 旧事件没有 `session_scope` 时维持旧行为。

### API 文档测试

- SSE 示例包含可选字段说明。
- 插件开发文档说明 `session_id` 与 `normalized_session_id` 的区别。
- 若新增 session API，必须遵守统一响应结构：`code`、`message`、`data`。
- session group API 按项目分组，默认不把 subagent 铺到主列表。
- session detail API 通过查询参数传递 `session_id`，不使用路径参数。

## 验收标准

实现完成后，应满足：

- subagent 授权请求不会因为 watcher 子 session / hook 父 session 不一致而重复通知。
- subagent 完成事件仍进入事件中心，但默认不发送“任务完成”通知。
- subagent 完成事件不把主状态错误切换成 completed。
- 主会话完成仍正常通知和更新主状态。
- `session_id` 仍可用于定位原始 session 文件。
- 第三方插件可以用 `session_scope` 和 `normalized_session_id` 做业务决策。
- 第三方插件可以按项目获取归一会话列表，并按 raw session id 获取详情。
- Codex JSONL 缺少内部字段时，基础事件仍可上报。

## 待确认问题

- 是否对外暴露 `thread_source` 原始字段，还是只暴露 `session_scope`。
- `normalized_session_id` 命名是否比 `root_session_id` 更合适。当前建议使用 `normalized_session_id`，避免误导为 Codex 原始 root 字段。
- subagent failed 是否默认通知。
- 事件中心是否需要显示 agent nickname / role。
- session API 是否同时支持 raw session 和 normalized session 查询。
- 项目分组接口是否命名为 `/api/v1/session-groups`，还是复用并扩展现有 `/api/v1/sessions`。
