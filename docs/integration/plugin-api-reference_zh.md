# 插件 API 参考

本文档集中整理 NiumaNotifier 当前对插件暴露的接口。这里的“插件”包括由 `plugin.json` 描述、由宿主启动的本机可执行程序，以及读取 Local API 的外部辅助面板或 reader 插件。

相关入口：

- 插件进程通过环境变量 `NIUMA_LOCAL_API_URL` 获取 Local API 基址，例如 `http://127.0.0.1:27874`。
- 插件进程通过环境变量 `NIUMA_PLUGIN_ID` 获取当前插件 ID。
- 工具插件额外通过 `NIUMA_TOOL_ID` 获取当前工具 ID。
- 工具 session provider 使用宿主写入插件 `stdin`、插件写回 `stdout` 的 JSON Lines RPC，不走 HTTP。

## 通用约定

除 SSE 流和 provider RPC 外，Local API 使用统一 JSON 响应结构：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

约定：

- `code = 0` 表示成功。
- `code != 0` 表示失败。
- 业务失败返回 `HTTP 200 + 非 0 code`。
- JSON 解析失败、查询参数类型错误等协议层错误返回 `HTTP 400 + 非 0 code`。
- 系统错误返回 `HTTP 500 + 非 0 code`。
- SSE 是协议例外，成功建立连接后发送 `text/event-stream` 帧，不包统一 envelope；详细规则见“[SSE 协议总览](#sse-协议总览)”。
- provider RPC 是插件进程内 JSON Lines 协议，不使用 Local API envelope。

常见错误码：

| `code` | 含义 |
| --- | --- |
| `0` | 成功 |
| `100003` | 查询参数类型错误 |
| `100004` | 请求体无法解析或参数格式错误 |
| `100101` | 业务参数校验失败 |
| `900001` | 系统异常 |
| `900005` | 路由不存在 |

插件调用建议：

- 不要只判断 HTTP 状态码，必须同时检查 `code`。
- `GET` 请求把业务参数放 query。
- `POST` 请求使用 JSON body。
- Local API 当前面向本机可信调用方，v1 没有插件 token 鉴权；不要把接口暴露到公网。

## 插件能力与接口关系

| Capability | 插件类型 | 主要接口 |
| --- | --- | --- |
| `event_watcher` | `tool` | `POST /api/v1/plugin-events` |
| `event_consumer` | `notification` | `GET /api/v1/events/stream` |
| `approval_handler` | `notification`，且需同时声明 `event_consumer` | `POST /api/v1/approval-decisions`，可选读取 `GET /api/v1/approval-requests` |
| `notification_test` | `notification` | 消费 `events/stream` 中的 `notification_test`，回写 `POST /api/v1/plugins/notification-test-results` |
| `state_consumer` | `status_indicator` | `GET /api/v1/state/stream` |
| `tool_session_list_provider` | `tool` | Provider RPC `session_snapshot` / `session_snapshot_updated` |
| `tool_session_detail_provider` | `tool` | Provider RPC `session_detail` |
| `tool_session_list_reader` | 任意业务插件 | `GET /api/v1/session_list`、`GET /api/v1/session_project_groups` |
| `tool_session_detail_reader` | 任意业务插件 | `GET /api/v1/session_detail`、`GET /api/v1/session_detail/stream` |

## 事件上报 API

### `POST /api/v1/plugin-events`

工具监听插件上报统一 `NiumaEvent`。

请求体：

```json
{
  "plugin_id": "niuma-plugin-codex",
  "events": []
}
```

关键规则：

- `plugin_id` 必须是已发现插件。
- 插件必须有关联 `tool_id`。
- 每个 `event.tool` 必须等于 manifest 中的 `tool_id`。
- `dedupe_key` 必须稳定；重复扫描同一原始事件时保持一致。
- 授权类 Codex watcher 事件会经过授权仲裁，可能延迟、抑制或立即写入。

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `plugin_id` | 实际接收的插件 ID |
| `event_count` | 当前主状态事件数量 |
| `applied_count` | 本次实际应用的新事件数量 |
| `session_count` | 当前运行态 session 数量 |
| `delayed_count` | 本次被授权仲裁延迟的事件数 |
| `suppressed_count` | 本次被授权仲裁抑制的事件数 |

## 事件与状态消费 API

## SSE 协议总览

插件当前可消费的 SSE 流如下：

| Path | `event:` 类型 | 主要消费者 | Payload 形态 |
| --- | --- | --- | --- |
| `/api/v1/events/stream` | `event`、`notification_test` | 通知插件、授权处理插件 | 单个 `NiumaEvent` 或测试通知请求 |
| `/api/v1/state/stream` | `state` | 状态指示插件、外部状态面板 | `MainStatePayload` |
| `/api/v1/session_project_groups/stream` | `session_project_groups` | 会话列表 reader、外部面板 | 与 `GET /api/v1/session_project_groups` 的 `data` 相同 |
| `/api/v1/session_detail/stream` | `session_detail` | 会话详情 reader、外部面板 | 与 `GET /api/v1/session_detail` 的 `data` 相同 |
| `/api/v1/session_detail/stream` | `session_detail_error` | 会话详情 reader、外部面板 | 建连后详情重算发生系统错误时的标准错误 envelope |

SSE 成功建立连接后使用 `text/event-stream`，不再包 `code/message/data`。如果在建立连接前发生参数类型错误或业务校验失败，则仍返回标准 Local API envelope：

```json
{
  "code": 100003,
  "message": "查询参数类型错误（event_type）：...",
  "data": null
}
```

建连前错误规则：

| 场景 | HTTP 状态 | `code` |
| --- | --- | --- |
| 查询参数类型错误，例如 `event_type`、`limit`、`page_size` 类型非法 | `400` | `100003` |
| 必填业务参数缺失，例如 `session_detail/stream` 缺少 `tool` 或 `session_id` | `200` | `100101` |
| session 不存在、分页范围不合法、provider 不可用等业务失败 | `200` | `100101` |

SSE 帧格式示例：

```text
event: event
id: event-1
data: {"id":"event-1","event_type":"approval_requested"}

```

客户端实现要求：

- 请求头建议带 `Accept: text/event-stream`。
- 按空行分隔 frame。
- 支持 `event:`、`id:` 和一行或多行 `data:`。
- 忽略以 `:` 开头的 keep-alive 注释行。
- `data:` 内容是 JSON 字符串；如果出现多行 `data:`，应按 SSE 规范拼接后再解析。
- 连接断开后重新建立连接；当前流不提供历史补偿或断点续传。
- 不要假设 `id` 全局连续。`events/stream` 使用事件 ID 或测试 ID；状态和 session 快照流使用运行时版本或递增版本作为去重提示。
- 过滤参数只减少当前连接收到的帧，不是权限边界。
- 解析失败时应丢弃当前 frame 并记录插件日志，不要阻塞后续 frame。

各流的推送语义：

| Path | 首帧 | 后续推送 |
| --- | --- | --- |
| `/api/v1/events/stream` | 不补发历史事件 | 新 `NiumaEvent` 写入后推送；测试通知请求单独推送 |
| `/api/v1/state/stream` | 建连后立即推送当前完整主状态 | 主状态内容变化后推送完整快照 |
| `/api/v1/session_project_groups/stream` | 建连后立即推送当前完整项目分组快照 | 运行态、关注项、state reset、插件配置或 session control 变化后重新计算，内容变化才推送 |
| `/api/v1/session_detail/stream` | 建连后立即推送当前最新页详情 | 匹配 raw session 或 normalized session 的事件、状态重置、控制通道变化后重新计算，内容变化才推送 |

`session_detail/stream` 建连后如果详情重算遇到系统错误，会发送 `event: session_detail_error`，`data` 是标准错误 envelope，例如：

```text
event: session_detail_error
data: {"code":900001,"message":"session detail 序列化失败：...","data":null}
```

### `GET /api/v1/events/stream`

通知插件消费实时事件和测试通知事件。

可选过滤参数：

| 参数 | 说明 |
| --- | --- |
| `tool` | 按工具 ID 过滤 |
| `session_id` | 按 raw session ID 过滤 |
| `normalized_session_id` | 按归一化 session ID 过滤 |
| `project_path` | 按项目路径精确过滤 |
| `event_type` | 按事件类型过滤，类型非法时返回 `HTTP 400` |
| `severity` | 按严重级别精确过滤 |

普通事件帧：

```text
event: event
id: event-1
data: {"id":"event-1","event_type":"approval_requested"}
```

测试通知帧：

```text
event: notification_test
id: manual-test:builtin-ntfy:1
data: {"test_id":"manual-test:builtin-ntfy:1","plugin_id":"builtin-ntfy","title":"测试通知","body":"这是一条测试通知","created_at":"2026-06-18T12:00:00Z"}
```

消费规则：

- 该流只广播成功写入的新事件，不补发历史事件。
- 重连后插件应重新建立 SSE 连接，不要依赖 SSE id 做断点恢复。
- `notification_test` 不写入公开事件历史，只用于通知插件测试。
- 有授权处理能力的插件必须通过事件中的 `interaction` 判断是否可操作。

### `GET /api/v1/state/stream`

状态指示插件消费主状态快照。

成功帧：

```text
event: state
id: 1
data: {"version":1,"state":{}}
```

规则：

- 建连后立即发送当前完整状态快照。
- 主状态变化后发送新的完整状态快照。
- 该流用于展示状态，不应作为授权“同意/拒绝”的触发来源。

### `GET /api/v1/main-state`

读取当前主状态，适合调试或非实时展示。

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `state` | 当前主状态 payload |

## 通知结果回写 API

### `POST /api/v1/plugins/notification-results`

通知插件回写真正通知发送结果。

请求体：

```json
{
  "plugin_id": "builtin-ntfy",
  "event_id": "event-1",
  "status": "sent",
  "title": "标题",
  "body": "正文",
  "reason": null,
  "error_message": null,
  "sent_at": "2026-06-18T12:00:00Z"
}
```

字段规则：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `plugin_id` | 是 | 通知插件 ID |
| `event_id` | 是 | 被通知的事件 ID |
| `status` | 是 | 仅支持 `sent` 或 `failed` |
| `title` / `body` | 否 | 实际发送内容 |
| `reason` | 否 | 插件侧原因 |
| `error_message` | 否 | 失败详情 |
| `sent_at` | 否 | RFC 3339 时间；`sent` 且为空时宿主使用当前时间 |

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `saved` | 是否已保存 |
| `record_id` | 通知记录 ID |

### `POST /api/v1/plugins/notification-test-results`

通知插件回写测试通知发送结果。

请求体：

```json
{
  "plugin_id": "builtin-ntfy",
  "test_id": "manual-test:builtin-ntfy:1",
  "status": "sent",
  "title": "测试通知",
  "body": "这是一条测试通知",
  "error_message": null,
  "sent_at": "2026-06-18T12:00:00Z"
}
```

规则与 `notification-results` 基本一致，差异是使用 `test_id` 而不是 `event_id`。

## 授权处理 API

### `GET /api/v1/approval-requests`

读取授权请求列表，主要用于插件启动恢复 pending 授权。

查询参数：

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `status` | 不过滤 | 可选：`pending`、`allowed`、`denied`、`returned_to_codex`、`resolved_in_tool` |

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `list` | 授权请求列表 |

### `GET /api/v1/approval-decisions`

读取单个授权请求当前决策状态。

查询参数：

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `request_id` | 是 | 授权请求 ID |

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `request_id` | 授权请求 ID |
| `status` | 当前状态 |
| `decision` | `allow`、`deny` 或 `null` |
| `decided_by` | 决策人 |
| `decided_source` | 决策来源 |
| `reason` | 决策原因 |
| `proxy_status` | hook proxy 状态 |

### `POST /api/v1/approval-decisions`

提交授权决策。

请求体：

```json
{
  "request_id": "codex:s1:t1:Bash:abc123",
  "decision": "allow",
  "decided_by": "plugin:builtin-ntfy",
  "decided_source": "notification",
  "reason": "用户在通知中同意"
}
```

字段规则：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `request_id` | 是 | 来自事件 `interaction.request_id` |
| `decision` | 是 | 仅支持 `allow` 或 `deny` |
| `decided_by` | 是 | 决策主体，例如 `plugin:<plugin-id>` |
| `decided_source` | 是 | 决策来源 |
| `reason` | 否 | 决策原因 |

成功响应与 `GET /api/v1/approval-decisions` 类似，并额外返回：

| 字段 | 说明 |
| --- | --- |
| `accepted` | 本次决策是否被接收并产生状态变化 |

### 授权内部协作接口

以下接口主要供 Codex hook、托管 relay 或宿主内部协作使用，不建议普通第三方插件直接调用：

| Method | Path | 说明 |
| --- | --- | --- |
| `POST` | `/api/v1/approval-requests` | 创建授权请求 |
| `POST` | `/api/v1/approval-requests/return` | 将授权处理权退回 Codex |
| `POST` | `/api/v1/approval-requests/tool-resolved` | 标记授权已在原工具中处理 |
| `POST` | `/api/v1/approval-requests/heartbeat` | hook proxy 心跳 |

## 工具会话读取 API

### `GET /api/v1/session_list`

读取宿主保存的最新 provider session snapshot。

查询参数：

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `tool` | `all` | 工具 ID；空值等价于未传 |
| `include_subagents` | `false` | 是否包含 subagent session |
| `active_only` | `false` | 是否只返回活跃 session |
| `limit` | `100` | 返回数量上限 |

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `list` | `ToolSessionListItem[]` |

### `GET /api/v1/session_project_groups`

按项目路径聚合 session snapshot。

查询参数：

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `tool` | `all` | 工具 ID；空值等价于未传 |
| `project_path` | 不过滤 | 精确项目路径 |
| `include_subagents` | `false` | 是否展开 raw subagent 明细 |
| `page` | `1` | 页码 |
| `page_size` | `20` | 每页数量，最大 `100` |

成功响应 `data` 使用标准分页结构：

| 字段 | 说明 |
| --- | --- |
| `list` | 项目分组列表 |
| `page` | 当前页码 |
| `page_size` | 每页数量 |
| `total` | 总数 |

列表接口只提供轻量状态，不提供授权弹框或等待输入表单。插件在列表中判断 session 当前状态时使用 `runtime_status`：

```json
{
  "normalized_session_id": "session-1",
  "primary_session_id": "session-1",
  "status": "active",
  "runtime_status": "waiting_approval"
}
```

字段边界：

| 字段 | 说明 |
| --- | --- |
| `status` | provider 源状态，例如 `active`、`inactive` 或 `unknown`，表示工具侧 session 文件/进程状态 |
| `runtime_status` | Niuma 运行态，例如 `running`、`waiting_approval`、`waiting_input`、`completed`、`error`、`idle`、`stale` 或 `null` |
| `runtime_last_event_id` | 触发当前运行态的最新事件 ID，可为空 |
| `runtime_last_activity_at` | 当前运行态的最新活动时间，可为空 |

列表 UI 如果发现 `runtime_status = "waiting_approval"` 或 `"waiting_input"`，应进入详情接口读取 `pending_action`。不要从 `/api/v1/events/stream` 自己拼会话列表状态；事件流用于通知和增量触发，列表快照由宿主统一聚合。

能力边界：通知型 `approval_handler` 仍应消费 `/api/v1/events/stream` 来弹出全局授权通知；会话列表/详情 reader 在展示某个 session 时，应使用 `session_detail.pending_action` 作为该 session 当前交互的唯一详情来源。

### `GET /api/v1/session_project_groups/stream`

按与 `session_project_groups` 相同的查询参数订阅项目分组快照。

成功帧：

```text
event: session_project_groups
id: 2
data: {"list":[],"page":1,"page_size":20,"total":0}
```

规则：

- 建连后发送完整快照。
- 运行态、关注项、session control 状态变化时重新计算并推送完整快照。
- payload 是 `session_project_groups` 的 `data` 对象，不包 Local API envelope。
- payload 与 `session_project_groups` 一样只包含列表轻量状态。实时授权弹框和等待输入详情不放在列表流里，应由 `session_detail` 或 `session_detail/stream` 的 `pending_action` 提供。

### `GET /api/v1/session_detail`

读取指定 raw session 的归一化消息详情。

查询参数：

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `tool` | 是 | 工具 ID |
| `session_id` | 是 | raw session ID |
| `limit` | 否 | 每页消息数 |
| `cursor` | 否 | 读取更旧消息的 cursor |

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `tool` | 工具 ID |
| `session_id` | raw session ID |
| `project_path` / `project_name` | 项目信息 |
| `messages` | 当前页消息，倒序返回，最新消息在前 |
| `next_cursor` | 下一页 cursor；为空表示没有下一页 |
| `control` | 可选会话控制能力 |
| `pending_action` | 当前最高优先级待处理交互；无交互时固定返回 `null` |

`control` 表示宿主当前掌握的最新控制通道状态。即使 provider detail 内部缓存较旧，Local API 也会用当前 session snapshot 的 `control` 覆盖详情返回值；托管 session 关闭后，`control.resumable` 应随 snapshot 刷新变为 `false`。

`pending_action` 由宿主根据当前运行态和原始事件 `interaction` 计算，不要求 provider 在 `session_detail` RPC 中返回。优先级：

1. Niuma 可直接处理的授权 `approval`
2. Niuma 可直接处理的输入 `input`
3. 只能回到工具处理的授权提示
4. 只能回到工具处理的输入提示

同优先级取最早创建的等待项。

无待处理动作时：

```json
{
  "tool": "codex",
  "session_id": "session-1",
  "messages": [],
  "pending_action": null
}
```

授权动作示例：

```json
{
  "pending_action": {
    "type": "approval",
    "title": "需要授权",
    "description": "Codex 请求执行 cargo test",
    "actionable": true,
    "created_at": "2026-06-18T12:00:00Z",
    "source_event_id": "event-approval-1",
    "actions": [
      {
        "id": "allow",
        "label": "允许",
        "submit": {
          "method": "POST",
          "url": "/api/v1/approval-decisions",
          "body": {
            "request_id": "request-1",
            "decision": "allow"
          }
        }
      },
      {
        "id": "deny",
        "label": "拒绝",
        "submit": {
          "method": "POST",
          "url": "/api/v1/approval-decisions",
          "body": {
            "request_id": "request-1",
            "decision": "deny"
          }
        }
      }
    ],
    "fields": [],
    "submit": null
  }
}
```

提交授权时，插件使用按钮里的 `submit.body` 作为基础，并补充决策来源字段：

```json
{
  "request_id": "request-1",
  "decision": "allow",
  "decided_by": "plugin:my-plugin",
  "decided_source": "session_detail_panel",
  "reason": "用户在会话详情中同意"
}
```

等待输入示例：

```json
{
  "pending_action": {
    "type": "input",
    "title": "等待输入",
    "description": "Codex 需要你补充信息",
    "actionable": true,
    "created_at": "2026-06-18T12:00:00Z",
    "source_event_id": "event-input-1",
    "actions": [],
    "fields": [
      {
        "id": "mode",
        "label": "运行形态",
        "question": "你希望主要以什么形态运行？",
        "type": "single_select",
        "required": true,
        "options": [
          {
            "label": "托盘常驻",
            "description": "适合长期后台监控"
          }
        ]
      }
    ],
    "submit": {
      "method": "POST",
      "url": "/api/v1/session-control/answer-input",
      "body": {
        "tool": "codex",
        "session_id": "session-1",
        "channel_id": "niuma_codex_managed:xxx",
        "request_id": "input-1"
      }
    }
  }
}
```

提交输入时，插件使用 `submit.body` 作为基础，并补充 `answers`：

```json
{
  "tool": "codex",
  "session_id": "session-1",
  "channel_id": "niuma_codex_managed:xxx",
  "request_id": "input-1",
  "answers": {
    "mode": ["托盘常驻"]
  }
}
```

如果 `actionable = false`，表示当前只是展示“工具中有等待项”，插件不能直接提交本地操作。此时应展示提示，让用户回到原工具处理。

### `GET /api/v1/session_detail/stream`

订阅单个 session 的详情快照。

查询参数：

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `tool` | 是 | 工具 ID |
| `session_id` | 是 | raw session ID |
| `limit` | 否 | 最新页消息数 |

成功帧：

```text
event: session_detail
id: 2
data: {"tool":"codex","session_id":"session-1","messages":[]}
```

规则：

- 不支持 `cursor`；历史分页继续使用 `GET /api/v1/session_detail`。
- 建连后发送最新页完整快照。
- 匹配 raw session 或 normalized session 的事件变化时重新计算。
- `data` 中包含与 `session_detail` 相同的 `pending_action` 字段。列表页面需要实时刷新弹框内容时，应在用户选中或展开某个 session 后订阅该 session 的详情流。
- 建连后系统错误会以 `event: session_detail_error` 推送，业务失败仍在建连前以标准 envelope 返回。

## 会话控制 API

这些接口主要用于有 `control` 字段的托管 Codex 会话。调用方应优先读取 `session_list`、`session_project_groups` 或 `session_detail` 返回的 `control.channels[].actions[]`，不要按工具名称硬编码。

### `POST /api/v1/session-control/send-instruction`

向可控会话发送续写指令。

请求体：

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "channel_id": "niuma_codex_managed:niuma_codex_xxx",
  "content": "继续"
}
```

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `sent` | 是否已发送 |
| `channel_id` | 实际通道 ID |
| `result` | provider 返回结果 |

### `POST /api/v1/session-control/interrupt`

中断可控会话。

请求体：

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "channel_id": "niuma_codex_managed:niuma_codex_xxx"
}
```

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `interrupted` | 是否已发送中断 |
| `channel_id` | 实际通道 ID |
| `result` | provider 返回结果 |

### `POST /api/v1/session-control/answer-input`

回答可控会话的结构化等待输入。

请求体：

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "channel_id": "niuma_codex_managed:niuma_codex_xxx",
  "request_id": "codex-input:niuma_codex_xxx:9",
  "answers": {
    "app_form": ["托盘常驻 (Recommended)"]
  }
}
```

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `answered` | 是否已提交答案 |
| `channel_id` | 实际通道 ID |
| `request_id` | 已回答的输入请求 ID |
| `state_cleared` | 是否追加清理等待输入状态的事件 |
| `result` | provider 返回结果 |

控制接口规则：

- 当前只支持 `tool = "codex"`。
- `session_id` 必须和 `channel_id` 绑定到同一个托管会话。
- 会话过期、进程退出或 control socket 不可用时返回业务失败 envelope。

## 插件管理与配置 API

这些接口主要供前端插件管理 UI 调用，外部插件运行时可以读取自己的配置，但不应依赖管理接口控制其他插件。

### `GET /api/v1/plugins`

读取插件管理列表。

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `list` | `PluginManagementItem[]` |

`PluginManagementItem` 主要字段：

| 字段 | 说明 |
| --- | --- |
| `id` | 插件 ID |
| `kind` | `tool`、`notification` 或 `status_indicator` |
| `tool_id` | 工具插件对应工具 ID |
| `display_name` / `version` | 展示信息 |
| `source` | `builtin` 或 `external` |
| `enabled` | 是否启用 |
| `runtime_status` | `starting`、`running`、`stopping`、`stopped` 或 `failed` |
| `last_error` | 最近运行错误 |
| `capabilities` | 插件能力列表 |
| `management_actions` | 宿主生成的受控管理动作 |
| `config_schema` | 配置 schema |
| `install_path` | 外部插件安装路径 |

### `GET /api/v1/plugins/config`

读取插件配置。外部插件运行时建议用该接口读取最终合并默认值后的配置。

查询参数：

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `plugin_id` | 是 | 插件 ID |

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `plugin_id` | 插件 ID |
| `config` | 合并默认值后的配置对象 |
| `config_schema` | 插件配置 schema |

### `POST /api/v1/plugins/config`

保存插件配置。

请求体：

```json
{
  "plugin_id": "builtin-ntfy",
  "config": {}
}
```

规则：

- `config` 必须是对象。
- 宿主按 manifest `config_schema` 校验基础类型和必填字段。
- 成功后发布插件配置变化事件，运行管理器可据此刷新插件。

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `saved` | 是否保存 |
| `plugin_id` | 插件 ID |
| `config` | 保存后的合并配置 |
| `config_schema` | 插件配置 schema |

### 管理类接口

| Method | Path | 说明 |
| --- | --- | --- |
| `POST` | `/api/v1/plugins/import` | 从本机目录导入外部插件 |
| `POST` | `/api/v1/plugins/remove` | 移除外部插件；内置插件不可移除 |
| `POST` | `/api/v1/plugins/enabled` | 启用或停用插件 |
| `POST` | `/api/v1/plugins/actions` | 执行宿主 allowlist 中的管理动作；当前仅内置 Codex Hook 安装/移除 |

## 工具监听配置 API

这些接口主要供宿主 UI 使用，但工具插件调试时可读取当前监听状态。

### `GET /api/v1/listener-config`

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `codex_listening_enabled` | Codex 监听开关 |
| `tool_listening_enabled` | 工具 ID 到启用状态的映射 |
| `tools` | 可监听工具列表 |

### `POST /api/v1/listener-config/save`

保存监听开关。

请求体二选一：

```json
{
  "tool_listening_enabled": {
    "codex": true
  }
}
```

或兼容旧字段：

```json
{
  "codex_listening_enabled": true
}
```

成功响应 `data`：

| 字段 | 说明 |
| --- | --- |
| `saved` | 是否保存 |
| `codex_listening_enabled` | Codex 监听开关 |
| `tool_listening_enabled` | 工具开关映射 |
| `tools` | 可监听工具列表 |

## 诊断读取 API

以下接口是 Local API 的公开路由，但不建议插件把它们作为核心运行依赖：

| Method | Path | 说明 |
| --- | --- | --- |
| `GET` | `/api/v1/events` | 读取最近事件，query `limit` 默认 `50` |
| `GET` | `/api/v1/runtime_state_list` | 读取 Niuma 运行态列表 |
| `GET` | `/api/v1/notification-records` | 读取最近通知记录，当前固定返回最近 `20` 条 |

## 内部或测试接口

以下接口对插件不是稳定扩展契约：

| Method | Path | 说明 |
| --- | --- | --- |
| `POST` | `/api/v1/events` | 直接上报单个 `NiumaEvent`，主要用于内部和测试；插件应使用 `/api/v1/plugin-events` |
| `POST` | `/api/v1/blocker/dismiss` | 清理当前阻塞关注项，主要供 UI 使用 |
| `POST` | `/api/v1/state/reset` | 重置状态，需 `confirm = "RESET_NIUMA_STATE"` |
| `POST` | `/api/v1/manual-test/scenario` | 手动测试场景 |

## Provider JSON Lines RPC

声明 `tool_session_list_provider` 和 `tool_session_detail_provider` 的工具插件使用 provider RPC。宿主通过插件 `stdin` 写请求，插件通过 `stdout` 写响应或通知。

传输规则：

- 每一行必须是一条完整 JSON。
- 不允许 pretty print、多行 JSON 或日志前缀。
- `stdout` 只能写 provider RPC；普通日志写 `stderr` 或 `NIUMA_PLUGIN_DATA_DIR`。
- 请求和响应用 `id` 对应；通知没有 `id`。

请求结构：

```json
{
  "id": "req-1",
  "method": "session_snapshot",
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

### `session_snapshot`

宿主请求 provider 返回轻量 session 列表。

Params：

```json
{
  "tool": "codex"
}
```

Result：

```json
{
  "tool": "codex",
  "sessions": []
}
```

`sessions[]` 使用 `ToolSessionListItem` 结构，关键字段包括：

| 字段 | 说明 |
| --- | --- |
| `id` | provider 内部列表项 ID |
| `tool` | 工具 ID |
| `session_id` | raw session ID |
| `project_path` / `project_name` | 项目信息 |
| `file_path` | 原始来源路径或可诊断来源标识 |
| `modified_at` / `discovered_at` / `last_seen_at` | 时间戳 |
| `is_active` / `status` | provider 对活跃状态的判断 |
| `is_subagent` | 是否子代理会话 |
| `parent_session_id` | raw 父会话 ID |
| `normalized_session_id` | 业务归一会话 ID |
| `session_scope` | `main` 或 `subagent` |
| `first_user_message_preview` | 首条用户消息摘要 |
| `control` | 可选会话控制信息 |

### `session_detail`

宿主请求 provider 返回指定 raw session 的消息详情。

Params：

```json
{
  "tool": "codex",
  "session_id": "session-1",
  "limit": 100,
  "cursor": null
}
```

Result：

```json
{
  "detail": {
    "tool": "codex",
    "session_id": "session-1",
    "messages": [],
    "next_cursor": null
  }
}
```

`messages[]` 字段：

| 字段 | 说明 |
| --- | --- |
| `id` | 消息 ID |
| `role` | `user`、`assistant`、`system`、`tool_call`、`tool_result`、`event` 或 `unknown` |
| `content` | 消息内容 |
| `created_at` | 消息时间 |
| `metadata` | provider 可携带的工具特有字段 |

### `session_snapshot_updated`

provider 主动通知宿主 snapshot 已变化。

通知结构：

```json
{
  "method": "session_snapshot_updated",
  "params": {
    "tool": "codex",
    "sessions": []
  }
}
```

宿主收到后会更新内存 session registry；`session_list`、`session_project_groups` 和相关 stream 会基于最新 snapshot 返回。

常见 provider 错误码：

| 错误码 | 含义 |
| --- | --- |
| `method_not_found` | 未知 provider method |
| `invalid_params` | 参数无法解析或工具不匹配 |
| `session_not_found` | session 不存在 |
| `stale_session_file` | 原始 session 文件已变化或失效 |
| `session_provider_disabled` | 对应工具监听关闭 |
| `provider_internal_error` | provider 内部异常 |

## 接口范围说明

当前代码中所有 Local API 路由都位于 `crates/niuma-api/src/routes.rs`。本文档按插件开发稳定性分为：

- 稳定插件契约：事件上报、事件/状态消费、通知回写、授权决策、session 读取、provider RPC。
- 管理和诊断接口：插件管理、监听配置、最近事件、运行态、通知历史。
- 内部或测试接口：直接事件写入、状态重置、手动测试等。

如果后续新增插件能力或 Local API，必须同步更新本文档，并保持统一响应结构、错误码和参数传递规范。
