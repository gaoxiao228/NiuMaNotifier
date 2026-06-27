# Session Control Channel 设计

## 背景

当前 group session 的可续写状态来自 `ToolSessionControl`。这个模型把控制能力和 `niuma codex` 托管会话绑定在一起，并且 provider 后续 snapshot 更新只替换内存缓存，不一定唤醒 `session_project_groups/stream`。结果是 session 列表里的可续写状态可能滞后。

后续续写能力不应只依赖 `niuma codex` 托管会话。系统需要支持多个续写实现，例如托管 relay、工具原生 resume、插件控制通道或外部 bridge。因此本设计把“session 是否可续写”抽象为通用的 Session Control Channel。

## 目标

- group session SSE 实时反映 session 当前是否可续写。
- 续写能力不与 `niuma_codex` 或 `wrapper_session_id` 绑定。
- `niuma_codex_managed` 作为第一个 control channel provider 接入。
- 断开、退出、绑定失败等状态必须实时通知 session stream。
- API 和 SSE 使用新的 channel 模型，不保留旧 `ToolSessionControl` 顶层字段兼容。
- 普通 JSON API 继续遵守统一 envelope；SSE 继续作为协议例外发送裸 payload。

## 非目标

- 第一版不新增持久化表。
- 第一版不实现第二种真实续写 provider。
- 第一版不支持旧请求体里的 `wrapper_session_id` 兼容。
- 第一版不把 SSE 改成增量 patch；仍发送完整快照。

## 概念模型

Session 本身只描述工具会话归属、项目、更新时间和运行态。可续写能力由 `control` 表示。

`control = null` 表示没有任何已知控制通道。

`control.resumable = true` 表示至少一个 channel 当前可用于主动续写。

`control.resumable = false` 表示系统知道这个 session 有控制通道，但当前没有任何 channel 可用。客户端应展示“不可续写”或具体不可用原因，而不是把它当成从未支持续写。

`control.preferred_channel_id` 由服务端计算。客户端默认使用该 channel 执行续写，避免在前端硬编码 provider 优先级。

## API 数据结构

`ToolSessionControl` 调整为聚合结构：

```rust
pub struct ToolSessionControl {
    pub resumable: bool,
    pub preferred_channel_id: Option<String>,
    pub channels: Vec<ToolSessionControlChannel>,
}
```

单个 channel：

```rust
pub struct ToolSessionControlChannel {
    pub id: String,
    pub provider: String,
    pub kind: String,
    pub available: bool,
    pub capabilities: Vec<String>,
    pub actions: Vec<ToolSessionControlAction>,
    pub unavailable_reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}
```

响应示例：

```json
{
  "control": {
    "resumable": true,
    "preferred_channel_id": "niuma_codex_managed:niuma_codex_xxx",
    "channels": [
      {
        "id": "niuma_codex_managed:niuma_codex_xxx",
        "provider": "niuma_codex",
        "kind": "managed_relay",
        "available": true,
        "capabilities": [
          "send_instruction",
          "interrupt",
          "answer_input",
          "approve",
          "reject"
        ],
        "actions": [
          {
            "type": "send_instruction",
            "transport": "local_api",
            "endpoint": "/api/v1/session-control/send-instruction"
          },
          {
            "type": "interrupt",
            "transport": "local_api",
            "endpoint": "/api/v1/session-control/interrupt"
          },
          {
            "type": "answer_input",
            "transport": "local_api",
            "endpoint": "/api/v1/session-control/answer-input"
          }
        ],
        "unavailable_reason": null,
        "updated_at": "2026-06-27T10:00:00Z"
      }
    ]
  }
}
```

不可用示例：

```json
{
  "control": {
    "resumable": false,
    "preferred_channel_id": null,
    "channels": [
      {
        "id": "niuma_codex_managed:niuma_codex_xxx",
        "provider": "niuma_codex",
        "kind": "managed_relay",
        "available": false,
        "capabilities": [
          "send_instruction",
          "interrupt"
        ],
        "actions": [],
        "unavailable_reason": "process_exited",
        "updated_at": "2026-06-27T10:05:00Z"
      }
    ]
  }
}
```

## Channel ID

`channel_id` 是所有控制动作的唯一控制引用。外部 API 不再接收 `wrapper_session_id`。

当前 `niuma codex` 托管通道使用：

```text
niuma_codex_managed:<wrapper_session_id>
```

`wrapper_session_id` 是 provider 内部细节。API handler 根据 `channel_id` 解析 provider 和 provider-specific id，再分发到对应控制实现。

后续新增续写方式时，只需要新增新的 `kind` 和 provider 实现，例如：

- `tool_native_resume:<tool>:<session_id>`
- `plugin_control:<plugin_id>:<channel_key>`
- `external_bridge:<bridge_id>:<session_id>`

## 控制接口

主动续写：

```http
POST /api/v1/session-control/send-instruction
```

```json
{
  "tool": "codex",
  "session_id": "session-1",
  "channel_id": "niuma_codex_managed:niuma_codex_xxx",
  "content": "继续"
}
```

中断：

```http
POST /api/v1/session-control/interrupt
```

```json
{
  "tool": "codex",
  "session_id": "session-1",
  "channel_id": "niuma_codex_managed:niuma_codex_xxx"
}
```

等待输入：

```http
POST /api/v1/session-control/answer-input
```

```json
{
  "tool": "codex",
  "session_id": "session-1",
  "channel_id": "niuma_codex_managed:niuma_codex_xxx",
  "request_id": "request-1",
  "answers": {
    "mode": ["继续"]
  }
}
```

普通 JSON API 仍返回统一 envelope。channel 不存在、channel 当前不可用、capability 不支持、session 不匹配都属于业务失败，返回 `HTTP 200 + 非 0 code`。JSON 解析失败或参数类型错误返回 `HTTP 400 + 统一 envelope`。

## Approval 和 Input 交互

审批与等待输入也使用 channel 语义，不再暴露 `wrapper_session_id`。

交互 payload 中的 `control_ref` 调整为：

```json
{
  "channel_id": "niuma_codex_managed:niuma_codex_xxx",
  "provider": "niuma_codex",
  "kind": "managed_relay",
  "request_id": "request-1"
}
```

`approval-decisions` 继续保留现有路由。handler 根据 `control_ref.channel_id` 找到控制通道，再向对应 provider 回写审批结果。

## niuma_codex_managed 映射

`niuma_codex_managed` 从 managed registry 计算 channel 状态：

| Managed 状态 | Channel 状态 |
| --- | --- |
| `Created` | `available=false`, `unavailable_reason="binding_pending"` |
| `WaitingFirstUserMessage` | `available=false`, `unavailable_reason="binding_pending"` |
| `BindingPending` | `available=false`, `unavailable_reason="binding_pending"` |
| `Bound` 且 pid 存活 | `available=true` |
| `Ambiguous` | `available=false`, `unavailable_reason="binding_ambiguous"` |
| `Exited` | `available=false`, `unavailable_reason="process_exited"` |
| 控制 socket 不可用 | `available=false`, `unavailable_reason="socket_unavailable"` |
| 绑定失败 | `available=false`, `unavailable_reason="binding_failed"` |

`updated_at` 使用 registry 中对应状态变更时间。第一版应在 `ManagedCodexSession` 中新增 `state_changed_at` 字段，并在状态从 created、binding、bound、ambiguous、exited 之间转换时更新。历史 registry 没有该字段时，读取时回退到 `bound_at`、`first_user_message_submitted_at` 或 `started_at`。实现时必须保证 `updated_at` 存在，避免客户端无法排序或解释状态新旧。

## 实时事件

新增通用 runtime event：

```rust
RuntimeEvent::ToolSessionControlChanged {
    version: u64,
    tool: ToolKind,
    session_id: Option<String>,
    normalized_session_id: Option<String>,
    channel_id: Option<String>,
    reason: ToolSessionControlChangeReason,
}
```

原因枚举：

```rust
pub enum ToolSessionControlChangeReason {
    ChannelRegistered,
    ChannelAvailable,
    ChannelUnavailable,
    ChannelRemoved,
    SnapshotRefreshed,
}
```

触发点：

- `niuma codex` 创建 wrapper 后注册 channel。
- 绑定到真实 Codex session 成功后通知可用。
- resume 启动并绑定成功后通知可用。
- relay 退出、启动失败或 mark exited 后通知不可用。
- 控制 socket 写入失败或 pid 不存在时通知不可用。
- provider `session_snapshot_updated` 成功替换 snapshot 后通知刷新，作为兜底。

## SSE 行为

`session_project_groups/stream` 和 `session_detail/stream` 都监听 `ToolSessionControlChanged`。

收到事件后：

1. 使用原查询参数重新计算完整快照。
2. 与上一次发送内容比较。
3. 只有序列化内容发生变化时发送 SSE frame。

SSE `id` 继续只表示展示版本，不作为可恢复消费位点。客户端重连后应接受首帧完整快照。

## Provider 边界

第一版不引入通用 provider registry。先在现有 Codex session repository/provider 内生成 `niuma_codex_managed` channel。

接口 handler 分发时只支持 `channel_id` 前缀 `niuma_codex_managed:`。其他前缀返回业务失败。

后续新增第二种续写方式时，再抽象 `ToolSessionControlProvider` trait，避免现在过度设计。

## 不兼容变更

- `ToolSessionControl.available` 顶层字段删除。
- `ToolSessionControl.provider` 顶层字段删除。
- `ToolSessionControl.wrapper_session_id` 顶层字段删除。
- `ToolSessionControl.capabilities` 顶层字段删除。
- `ToolSessionControl.actions` 顶层字段删除。
- `/api/v1/session-control/send-instruction` 不再接受 `wrapper_session_id`。
- `/api/v1/session-control/interrupt` 不再接受 `wrapper_session_id`。
- `/api/v1/session-control/answer-input` 不再接受 `wrapper_session_id`。
- approval/input `control_ref` 不再暴露 `wrapper_session_id`。

## 测试范围

需要覆盖：

- `session_project_groups` 返回 `control.resumable` 和 `channels[]`。
- `session_detail` 返回同样的 channel 模型。
- managed `Bound` 会话生成 `available=true` channel。
- managed `Exited` 会话生成 `available=false` channel，且 `control.resumable=false`。
- provider snapshot notification 成功替换后发布 control changed event。
- `session_project_groups/stream` 在 control changed 后推送新快照。
- `session_detail/stream` 在 control changed 后推送新快照。
- send-instruction 使用 `channel_id` 成功分发。
- interrupt 使用 `channel_id` 成功分发。
- answer-input 使用 `channel_id` 成功分发并清理等待输入。
- channel 不存在、channel 不可用、capability 不支持、session 不匹配均返回统一业务失败 envelope。
- 旧 `wrapper_session_id` 请求体不再通过。

## 文档更新

需要同步更新：

- `docs/integration/plugin-development.md`
- `docs/integration/plugin-development_zh.md`
- `docs/integration/sse-external-integration.md`
- `docs/integration/sse-external-integration_zh.md`
- `docs/integration/niuma-codex-managed.md`
- `docs/integration/niuma-codex-managed_zh.md`

文档必须说明 `control = null`、`control.resumable=false` 和 `channel.available=false` 的区别。

## 验收标准

- 通过 `niuma codex` 启动或 resume 的 session 绑定成功后，group session SSE 能实时推送 `resumable=true`。
- 托管 relay 退出或进程不可用后，group session SSE 能实时推送 `resumable=false`。
- session detail SSE 与 group session SSE 对同一 session 的 control 状态一致。
- 外部客户端只依赖 `channel_id` 执行续写、中断、审批和等待输入。
- 普通 API 返回结构符合统一 envelope；SSE 作为协议例外保持裸 payload。
