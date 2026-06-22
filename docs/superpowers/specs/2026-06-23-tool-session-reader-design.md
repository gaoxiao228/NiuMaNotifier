# Tool Session Reader 设计

## 背景

第三方插件需要基于 AI 工具的真实 session 做业务决策。现有 `/api/v1/sessions` 返回的是 Niuma 状态机聚合后的运行态，不包含工具实际发现到的 session 文件、subagent 信息、文件修改时间、扫描时间或归一化消息内容。

本设计把“运行态”和“工具 session”彻底拆开：

- 运行态是 Niuma 对工具事件流的状态判断。
- 工具 session 是工具 provider 实际发现和解析的会话文件视图。

第三方 reader 插件只调用宿主 Local API，不直接读取 Codex、Claude Code 等工具目录，也不直接调用 provider 插件。

## 目标

- 立即把现有运行态模型从 session 命名中移出。
- 新增工具 session 列表和详情接口。
- 支持第三方插件读取宿主统一的 session 列表和消息详情。
- Codex session provider 作为独立 provider 插件能力实现，不合并到 `event_watcher`。
- 插件之间不形成依赖，宿主是唯一中介。
- 对话内容读取作为敏感能力展示，但第一版暂不做 token 鉴权。

## 非目标

- 不把 Codex、Claude Code 的原始 JSONL schema 暴露为公共 API。
- 不长期持久化完整对话内容。
- 不在 reader API 请求时扫描工具 session 目录。
- 不让 reader 插件直接调用 provider 插件。
- 不把工具 session 视图合并进运行态接口。
- 第一版不引入 `NIUMA_PLUGIN_ACCESS_TOKEN` 鉴权。

## 核心原则

```text
运行态不是 session
工具会话才是 session
插件之间没有依赖
所有第三方读取都经过宿主 API
```

## 运行态改名

现有概念立即硬切，不保留 `/api/v1/sessions` alias。

模型改名：

```text
NiumaSession -> RuntimeStateItem
SessionStatus -> RuntimeStateStatus
```

接口改名：

```http
GET /api/v1/runtime_state_list
```

响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "tool": "codex",
        "session_id": "session-1",
        "project_path": "/repo",
        "project_name": "repo",
        "status": "waiting_approval",
        "last_event_id": "event-1",
        "last_activity_at": "2026-06-22T08:00:00Z"
      }
    ]
  }
}
```

`RuntimeStateItem` 不再有 `id` 字段。这里的 `session_id` 是工具 session id，只用于关联工具会话。

`RuntimeStateStatus` 继续使用现有状态值：

```text
idle
running
waiting_approval
waiting_input
completed
error
stale
```

## 插件能力

新增 capability：

```text
tool_session_list_provider
tool_session_detail_provider
tool_session_list_reader
tool_session_detail_reader
```

能力含义：

- `tool_session_list_provider`：工具插件能向宿主提供该工具的 session 列表。
- `tool_session_detail_provider`：工具插件能按 `session_id` 向宿主提供归一化消息详情。
- `tool_session_list_reader`：业务插件声明会读取宿主的 session 列表 API。
- `tool_session_detail_reader`：业务插件声明会读取宿主的 session 消息详情 API。

`event_watcher`、`tool_session_list_provider`、`tool_session_detail_provider` 是独立 provider 能力。`event_watcher` 不隐含 session provider 能力。

同一个 `tool_id` 下，以下 provider 能力每种只能有一个插件声明：

```text
event_watcher
tool_session_list_provider
tool_session_detail_provider
```

唯一性是注册和安装阶段硬约束，不是运行时冲突处理。

额外约束：

- 非 `tool` 插件不能声明 provider capability。
- provider capability 必须绑定 `tool_id`。
- `tool_session_detail_provider` 必须和 `tool_session_list_provider` 在同一个插件里。
- reader capability 不做唯一性限制。
- 第一版 reader capability 只用于 manifest 契约、插件管理 UI 展示和未来鉴权预留。

插件管理 UI 文案：

```text
event_watcher: 事件监听
tool_session_list_provider: 提供 AI 会话列表
tool_session_detail_provider: 提供 AI 会话解析
tool_session_list_reader: 读取 AI 会话列表
tool_session_detail_reader: 可读取 AI 会话内容
```

`tool_session_detail_reader` 应展示为敏感能力。

## 插件拆分

Codex 第一版拆成两个独立插件：

```text
builtin-codex-watcher
  capabilities: ["event_watcher"]

builtin-codex-session-provider
  capabilities: [
    "tool_session_list_provider",
    "tool_session_detail_provider"
  ]
```

`builtin-codex-watcher` 只负责事件流和 `NiumaEvent` 上报。`builtin-codex-session-provider` 只负责工具 session 列表、消息索引和详情读取。

## 对外 API

所有业务接口继续返回统一 envelope：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

### Session 列表

```http
GET /api/v1/session_list?tool=codex&include_subagents=false&active_only=false&limit=100
```

查询参数：

| 参数 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `tool` | string | `all` | `codex`、`claude_code`、自定义工具 ID 或 `all`。 |
| `include_subagents` | boolean | `false` | 是否包含 subagent session。 |
| `active_only` | boolean | `false` | 是否只返回仍活跃的 session。 |
| `limit` | number | `100` | 返回数量上限，最大 500。 |

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "id": "codex:session-1",
        "tool": "codex",
        "session_id": "session-1",
        "project_path": "/repo",
        "project_name": "repo",
        "file_path": "/Users/me/.codex/sessions/2026/06/22/session-1.jsonl",
        "modified_at": "2026-06-22T08:00:00Z",
        "discovered_at": "2026-06-22T08:00:01Z",
        "last_seen_at": "2026-06-22T08:00:05Z",
        "is_active": true,
        "is_subagent": false,
        "parent_session_id": null,
        "status": "active"
      }
    ]
  }
}
```

`/api/v1/session_list` 只读取宿主保存的最新 snapshot，不触发磁盘扫描，不实时请求 provider。

### Session 详情

```http
GET /api/v1/session_detail?tool=codex&session_id=session-1&limit=100&cursor=cursor-1
```

查询参数：

| 参数 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `tool` | string | 无 | 必填，工具 ID。 |
| `session_id` | string | 无 | 必填，工具 session ID。 |
| `limit` | number | `100` | 返回消息数量上限，最大 500。 |
| `cursor` | string | 无 | 可选分页游标。 |

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "tool": "codex",
    "session_id": "session-1",
    "project_path": "/repo",
    "project_name": "repo",
    "is_subagent": false,
    "parent_session_id": null,
    "messages": [
      {
        "id": "message-newest",
        "role": "assistant",
        "content": "最新回复",
        "created_at": "2026-06-22T08:05:00Z",
        "metadata": {}
      }
    ],
    "next_cursor": "older-page-cursor"
  }
}
```

详情接口规则：

- `messages[0]` 永远是本页最新消息。
- `next_cursor` 表示继续读取更旧消息。
- `next_cursor = null` 表示没有更旧消息。
- cursor 是 provider 生成的不透明字符串，宿主和 reader 都不解析。
- 宿主先从 snapshot 确认 `tool + session_id` 存在，再向 provider 请求详情。

消息角色第一版支持：

```text
user
assistant
system
tool_call
tool_result
event
unknown
```

消息字段约束：

- `id` 必须在同一 session 内稳定。
- `content` 是面向第三方插件的文本内容。
- 无法稳定提取文本时，`content` 使用空字符串，并在 `metadata.reason` 保留最小原因。
- `created_at` 无法从原始协议得到时返回 `null`。
- `metadata` 只能放最小诊断字段，不能放完整原始 JSONL 行、完整 payload 或完整原始对象。

## Provider RPC

Session provider 插件由宿主启动并管理，通过 stdio JSON Lines RPC 通信。每一行 stdin/stdout 都是一条 JSON。

启动环境变量：

```text
NIUMA_PLUGIN_ID=builtin-codex-session-provider
NIUMA_TOOL_ID=codex
NIUMA_PROVIDER_MODE=session_provider
```

请求：

```json
{
  "id": "req-1",
  "method": "session_detail",
  "params": {}
}
```

成功响应：

```json
{
  "id": "req-1",
  "result": {}
}
```

失败响应：

```json
{
  "id": "req-1",
  "error": {
    "code": "session_not_found",
    "message": "session_id 不存在：session-1"
  }
}
```

通知：

```json
{
  "type": "notification",
  "method": "session_snapshot_updated",
  "params": {}
}
```

第一版 provider 方法：

```text
session_snapshot
session_detail
```

宿主启动 provider 后主动拉一次 snapshot：

```json
{
  "id": "req-1",
  "method": "session_snapshot",
  "params": {
    "tool": "codex"
  }
}
```

provider 返回：

```json
{
  "id": "req-1",
  "result": {
    "tool": "codex",
    "sessions": []
  }
}
```

provider 扫描到变化时，可以主动通知宿主：

```json
{
  "type": "notification",
  "method": "session_snapshot_updated",
  "params": {
    "tool": "codex",
    "sessions": []
  }
}
```

宿主请求详情：

```json
{
  "id": "req-2",
  "method": "session_detail",
  "params": {
    "tool": "codex",
    "session_id": "session-1",
    "limit": 100,
    "cursor": null
  }
}
```

provider 返回：

```json
{
  "id": "req-2",
  "result": {
    "tool": "codex",
    "session_id": "session-1",
    "project_path": "/repo",
    "project_name": "repo",
    "is_subagent": false,
    "parent_session_id": null,
    "messages": [],
    "next_cursor": null
  }
}
```

Provider 超时建议：

- `session_snapshot`: 5 秒。
- `session_detail`: 10 秒。

## Codex Provider 内部读取策略

Codex session provider 自己负责文件发现、索引和解析。宿主不理解 Codex JSONL，不读取 Codex session 文件。

Provider 内部 session 状态：

```rust
struct ProviderSessionState {
    session: ProviderSessionItem,
    message_index: Vec<MessageIndexEntry>,
    indexed_file_size: u64,
    indexed_modified_at: DateTime<Utc>,
}
```

消息索引项：

```rust
struct MessageIndexEntry {
    message_id: String,
    line_index: u64,
    byte_start: u64,
    byte_end: u64,
    role: ToolSessionMessageRole,
    created_at: Option<DateTime<Utc>>,
}
```

索引只存定位和少量元信息，不存完整 `content`。

读取流程：

1. provider 根据 `session_id` 找 `ProviderSessionState`。
2. 检查文件 `modified_at/file_size` 是否变化。
3. 文件追加时增量读取并补充 `message_index`。
4. 文件被截断或替换时重建该 session 索引。
5. 根据 cursor 定位分页位置。
6. 从 `message_index` 倒序选择 `limit` 条。
7. 只读取本页需要的 JSONL 行。
8. 解析为统一 `ToolSessionMessage`。
9. 返回最新消息在前。

排序依据：

```text
line_index desc
```

`created_at` 只作为展示字段，不作为主排序依据。

## Codex 消息映射

Codex JSONL 映射为统一消息，不暴露原始协议。

建议映射：

```text
session_meta
  -> 不作为 message 返回，只用于提取 session 元信息。

event_msg / task_started
  -> role = event
  -> content = "任务开始"

event_msg / task_complete
  -> role = assistant
  -> content = 完成消息或最后 assistant 文本

event_msg / turn_aborted
  -> role = event
  -> content = 中断或失败原因摘要

event_msg / thread_rolled_back
  -> role = event
  -> content = "会话已回滚"

response_item / message / role=user
  -> role = user
  -> content = 用户文本

response_item / message / role=assistant
  -> role = assistant
  -> content = 助手文本

response_item / function_call
  -> role = tool_call
  -> content = 工具名和参数摘要

response_item / function_call_output
  -> role = tool_result
  -> content = 输出文本摘要

无法识别但有文本
  -> role = unknown
  -> content = 提取到的文本

无法识别且无文本
  -> role = unknown
  -> content = ""
  -> metadata.reason = "unsupported_payload"
```

`metadata` 白名单示例：

```json
{
  "source": "codex_session_file",
  "codex_row_type": "response_item",
  "codex_item_type": "function_call",
  "tool_name": "exec_command",
  "call_id": "call-1",
  "truncated": false,
  "reason": "unsupported_payload"
}
```

禁止字段：

```text
raw
payload
raw_line
jsonl
original
```

## 错误语义

Provider 内部错误码：

```text
invalid_request
unsupported_tool
session_not_found
cursor_invalid
cursor_expired
index_not_ready
parse_failed
io_failed
internal_error
```

宿主对外映射：

| 场景 | HTTP | API code |
| --- | ---: | ---: |
| 业务失败 | 200 | `100101` |
| 参数类型或格式错误 | 400 | `100003` 或 `100004` |
| provider 业务错误 | 200 | `100101` |
| provider 超时 | 200 | `100101` |
| provider 崩溃或协议断开 | 500 | `900001` |
| 宿主内部异常 | 500 | `900001` |

`limit` 规则：

```text
默认 100
最小 1
最大 500
超过 500 截断为 500
```

## 缓存策略

宿主保存：

- 每个 tool 的最新 session snapshot。
- 短期 `session_detail` 页缓存。

详情缓存 key：

```text
tool + session_id + cursor + limit + snapshot_revision
```

宿主不长期保存完整会话内容。Provider 内部索引不保存完整消息内容，只保存文件偏移和少量元信息。

## 第一版实现顺序

1. 改运行态命名和 `/api/v1/runtime_state_list`。
2. 扩展 manifest capability 和 provider 唯一性校验。
3. 增加插件管理 UI 能力标签。
4. 实现 session provider stdio JSON Lines RPC runtime。
5. 新增 `/api/v1/session_list` 和 `/api/v1/session_detail`。
6. 实现独立 Codex session provider。
7. 补测试和文档。

## 测试要求

- `/api/v1/runtime_state_list` 替代旧 `/api/v1/sessions`。
- `RuntimeStateItem` 不再返回 `id`，改为 `session_id`。
- manifest 能解析新增 provider/reader capability。
- 非 `tool` 插件不能声明 provider capability。
- 同一 `tool_id` 下 provider capability 不能重复。
- `tool_session_detail_provider` 缺少 `tool_session_list_provider` 时 manifest 无效。
- `/api/v1/session_list` 支持 `tool`、`include_subagents`、`active_only`、`limit`。
- `/api/v1/session_detail` 支持 `tool`、`session_id`、`limit`、`cursor`。
- `/api/v1/session_detail` 返回倒序消息，最新消息为第一条。
- 不支持的工具或不存在的 session 返回业务失败 envelope。
- provider 超时、崩溃、非法 JSON 有稳定错误响应。
- Codex provider 能把 fixture session 文件解析成稳定 session 列表和 message 列表。
- 详情接口不返回原始完整 JSONL 行或完整 payload。

## 风险与后续扩展

- 第一版不做 token 鉴权，reader capability 不是强安全边界。后续可通过 header token 识别调用方，再复用现有 capability 做服务端鉴权。
- Stdio RPC 需要处理 provider stdout 日志污染。provider 必须把协议消息写 stdout，日志写 stderr。
- 大 session 文件需要依赖增量索引保证性能，不能在详情请求时全量解析。
- Claude Code 第一版可以只保留扩展空间，列表为空或详情返回不支持。
