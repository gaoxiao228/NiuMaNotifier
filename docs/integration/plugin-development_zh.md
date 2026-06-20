# 插件开发说明

本文档描述 NiumaNotifier 插件 v1 的接入方式。当前插件是由 `plugin.json` 描述、由 NiumaNotifier 启停的本机受信可执行程序；插件通过 Local API 与主程序通信。

当前支持三类插件：

| 类型 | `kind` | 主要能力 | 说明 |
| --- | --- | --- | --- |
| 工具监听插件 | `tool` | `event_watcher` | 监听 Codex、Claude Code、Cursor 等工具的原始状态，并转换为统一的 `NiumaEvent` 上报。 |
| 通知插件 | `notification` | `event_consumer`、`notification_test` | 消费主程序事件流，自行决定是否发送 Bark、ntfy 等外部通知，并把通知结果回写主程序。 |
| 状态指示插件 | `status_indicator` | `state_consumer` | 消费主状态流，用于外部指示灯、状态面板、桌宠等展示。 |

## 插件边界

- 插件是本机受信可执行程序，由用户安装并由 NiumaNotifier 启停。
- NiumaNotifier 不在 v1 中提供强沙箱、签名校验或插件市场。
- 插件不得直接写 NiumaNotifier 的持久化文件；所有状态事件和通知结果必须通过 Local API 上报。
- 状态指示插件不需要理解事件上报或通知回写协议，只消费 `/api/v1/state/stream` SSE 主状态流。
- Local API 默认只面向本机可信调用方，不内置鉴权；如果显式绑定到非 loopback 地址，应由外层网络策略保护。

## 开发流程总览

建议按以下顺序开发外部插件：

1. 编写 `plugin.json`，先确认 `id`、`kind`、`tool_id`、`capabilities` 和当前平台匹配。
2. 编写可独立启动的本机可执行程序，优先从环境变量读取 Local API 地址和插件 ID。
3. 工具监听插件先实现 `/api/v1/plugin-events` 上报，并保证 `dedupe_key` 稳定。
4. 通知插件先订阅 `/api/v1/events/stream`，再实现通知发送、本地去重和通知结果回写。
5. 状态指示插件只订阅 `/api/v1/state/stream`，不要自行写事件或通知历史。
6. 把插件目录放入用户插件目录，重启 NiumaNotifier，在插件管理列表中启用并观察运行状态。

最小可用插件应该先做到“能启动、能退出、能处理 Local API 失败”。复杂功能应建立在这个基础上逐步增加。

## 插件包结构

推荐结构：

```text
niuma-plugin-example/
  plugin.json
  bin/
    niuma-plugin-example
  assets/
    icon.png
```

仓库内提供了一个最小工具插件样例和一个状态指示插件样例：

```text
examples/plugins/niuma-plugin-demo/
examples/plugins/status-indicator-demo/
```

安装到本机插件目录：

```bash
mkdir -p "$HOME/Library/Application Support/NiumaNotifier/plugins"
cp -R examples/plugins/niuma-plugin-demo "$HOME/Library/Application Support/NiumaNotifier/plugins/"
```

重启 NiumaNotifier 后，监听列表应出现 `Demo Tool`。启用后，该插件会通过 `/api/v1/plugin-events` 上报一组稳定的测试事件。

状态指示插件样例安装方式相同，启用后会通过 `/api/v1/state/stream` 消费主状态并打印指示结果。

用户插件目录：

```text
~/Library/Application Support/NiumaNotifier/plugins/<plugin-id>/plugin.json
```

外部插件不能覆盖内置插件 ID，例如 `builtin-codex`、`builtin-bark`、`builtin-ntfy`。

## 运行生命周期

NiumaNotifier 会周期性发现插件目录中的 `plugin.json`，并根据启用状态管理常驻插件进程：

1. 发现：主程序加载内置插件和用户插件目录中的外部插件。
2. 启用：`tool` 插件由工具监听开关控制；无 `tool_id` 的插件由通用插件启用状态控制。
3. 启动：声明 `event_watcher`、`event_consumer` 或 `state_consumer` 的插件会作为常驻子进程启动。
4. 运行：主程序向插件注入环境变量，并把插件工作目录设置为 `plugin.json` 所在目录。
5. 停止：插件停用、manifest 变化或插件被移除时，主程序会终止旧进程。
6. 重启：插件异常退出后，主程序会记录 `failed` 状态，并在短暂退避后重试启动。

插件进程应满足以下要求：

- 能响应 `SIGTERM` 或平台等效终止信号，并尽快退出。
- 不要假设自己只会启动一次；重启后应能从 `NIUMA_PLUGIN_DATA_DIR` 恢复本地去重状态。
- 不要把运行状态写入 NiumaNotifier 的内部持久化文件。
- `stdout` 和 `stderr` 当前不会作为用户可见日志展示；调试阶段建议自行写入插件数据目录中的日志文件。

## Manifest

工具监听插件示例：

```json
{
  "id": "niuma-plugin-codex",
  "kind": "tool",
  "tool_id": "codex",
  "display_name": "Codex",
  "version": "0.1.0",
  "command": "./bin/niuma-plugin-codex",
  "args": [],
  "env": {},
  "platforms": ["macos"],
  "capabilities": ["event_watcher"],
  "icon_url": "./assets/icon.png"
}
```

通知插件示例：

```json
{
  "id": "niuma-plugin-webhook",
  "kind": "notification",
  "display_name": "Webhook",
  "version": "0.1.0",
  "command": "./bin/niuma-plugin-webhook",
  "args": [],
  "env": {},
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["event_consumer", "notification_test"],
  "config_schema": [
    {
      "key": "url",
      "type": "url",
      "label": "Webhook URL",
      "required": true
    },
    {
      "key": "token",
      "type": "secret",
      "label": "Token",
      "required": false
    }
  ]
}
```

状态指示插件示例：

```json
{
  "id": "status-indicator-demo",
  "kind": "status_indicator",
  "display_name": "Status Indicator Demo",
  "version": "0.1.0",
  "command": "node",
  "args": ["./bin/status-indicator-demo.mjs"],
  "env": {},
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["state_consumer"],
  "config_schema": [
    {
      "key": "style",
      "type": "select",
      "label": "Display style",
      "required": false,
      "default": "indicator",
      "options": ["indicator", "pet", "panel"]
    }
  ]
}
```

字段说明：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `id` | 是 | 插件唯一 ID。外部插件 ID 不能与内置插件重复。 |
| `kind` | 否 | 插件类型。缺省为 `tool`。当前支持 `tool`、`notification`、`status_indicator`。 |
| `tool_id` | 工具插件必填 | 工具 ID，例如 `codex`、`claude_code`、`cursor`、`demo_tool`。 |
| `display_name` | 是 | UI 展示名称。 |
| `version` | 是 | 插件版本。 |
| `command` | 外部插件必填 | 插件启动命令。带路径的相对命令按 `plugin.json` 所在目录解析；裸命令按系统 `PATH` 查找。 |
| `args` | 否 | 启动参数。相对路径参数不会自动改写，但插件进程工作目录会设为 `plugin.json` 所在目录。 |
| `env` | 否 | 额外环境变量，会注入到插件进程。 |
| `platforms` | 否 | 支持平台，当前使用 `macos`、`windows`、`linux`。空数组表示不限平台。 |
| `capabilities` | 否 | 当前支持 `event_watcher`、`event_consumer`、`notification_test`、`state_consumer`。 |
| `icon_url` | 否 | 图标地址或相对资源路径。 |
| `config_schema` | 否 | 插件配置字段定义，供 UI 和配置接口使用。 |

约束：

- `tool` 插件必须提供 `tool_id`。
- 非 `tool` 插件不能声明 `event_watcher`。
- `event_watcher` 插件由工具监听开关控制启停。
- 无 `tool_id` 的插件由通用插件启用状态控制启停。
- 声明 `event_watcher`、`event_consumer` 或 `state_consumer` 的插件会被运行管理器作为常驻子进程管理。

## 配置 Schema

`config_schema` 支持以下字段类型：

| 类型 | JSON 值类型 | 说明 |
| --- | --- | --- |
| `string` | string | 普通文本。 |
| `secret` | string | 密钥、token、device key 等敏感文本。 |
| `url` | string | URL 文本。 |
| `number` | number | 数字。 |
| `boolean` | boolean | 开关。 |
| `select` | string | 枚举值；可通过 `options` 限制允许值。 |

配置字段结构：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `key` | 是 | 配置键。不能为空，同一插件内不能重复。 |
| `type` | 是 | 配置类型。 |
| `label` | 是 | UI 展示名称。不能为空。 |
| `required` | 否 | 是否必填。 |
| `default` | 否 | 默认值。 |
| `options` | 否 | `select` 可选值列表。 |

配置保存时会按 `config_schema` 做基础类型校验和必填校验。未知配置键当前不会被拒绝，但插件不应依赖未声明字段。

## 启动环境变量

NiumaNotifier 启动插件时会注入：

| 变量 | 说明 |
| --- | --- |
| `NIUMA_LOCAL_API_URL` | Local API 地址，例如 `http://127.0.0.1:27874`。 |
| `NIUMA_PLUGIN_ID` | 当前插件 ID。 |
| `NIUMA_TOOL_ID` | 当前插件对应的工具 ID；仅 `tool` 插件有该变量。 |
| `NIUMA_PLUGIN_CONFIG_PATH` | 插件配置文件路径。当前内置 Bark/ntfy 通知插件会由主程序写入该文件；外部插件应优先通过配置 API 获取配置。 |
| `NIUMA_PLUGIN_DATA_DIR` | 插件数据目录，插件可用于保存本地去重等运行时状态。 |
| `NIUMA_PARENT_PID` | 主 App 进程 PID。插件可定时检测该进程是否仍存在；如果不存在，应主动退出，避免主 App 闪退后遗留插件进程。 |
| `NIUMA_DB_PATH` | 当前实例使用的 SQLite 通知历史数据库路径，仅用于诊断，不应直接写入。 |

建议外部插件把 `NIUMA_PARENT_PID` 作为自清理信号使用。该变量缺失或格式错误时，插件应保持兼容并继续运行；只有确认父进程不存在时才主动退出。

## 配置与本地数据

插件开发时需要区分三类数据：

| 数据 | 推荐位置 | 说明 |
| --- | --- | --- |
| 插件配置 | `/api/v1/plugins/config` | 由主程序根据 `config_schema` 校验并持久化。外部插件运行时应通过 Local API 读取。 |
| 插件本地运行数据 | `NIUMA_PLUGIN_DATA_DIR` | 插件自行维护，例如通知去重记录、重连状态、窗口位置或调试日志。 |
| 通知历史 | Local API 回写 | 真实通知和测试通知结果通过回写接口交给主程序保存。 |

约束：

- 外部插件不应直接读写 `config.json`、`plugin-configs`、`niuma.sqlite` 或其他主程序内部文件。
- `NIUMA_DB_PATH` 只用于诊断路径，不是插件扩展点。
- 事件、会话、关注项和最新活动是主程序内存运行态；插件不能依赖数据库查询历史事件。
- 通知插件应把“是否已经给某个事件发过通知”的本地去重记录保存在 `NIUMA_PLUGIN_DATA_DIR`，再把发送结果通过 Local API 回写主程序。

## Local API 约定

除 SSE 流外，插件相关 JSON 接口都使用统一响应结构：

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
- 业务校验失败通常返回 `HTTP 200 + 非 0 code`。
- JSON 解析失败等协议层错误返回 `HTTP 400 + 非 0 code`。
- 系统错误返回 `HTTP 500 + 非 0 code`。
- SSE 流是协议例外，不使用 JSON envelope。

常见错误码：

| `code` | 含义 |
| --- | --- |
| `0` | 成功。 |
| `100004` | 请求体无法解析或参数格式错误。 |
| `100101` | 业务校验失败，例如未知插件、插件类型不匹配、配置校验失败。 |
| `900001` | 系统错误。 |
| `900005` | 路由不存在。 |

调用建议：

- 业务接口返回 `HTTP 200` 时仍必须检查 `code`，不能只看 HTTP 状态码。
- `GET` 请求把业务参数放在 query，例如 `/api/v1/plugins/config?plugin_id=...`。
- `POST` 请求使用 JSON body，失败时直接读取外层 `message` 作为诊断信息。
- 插件启动早于 Local API 可用时，应进行有限重试，而不是立刻永久退出。
- 调试时可先用 `curl` 验证 API，再接入插件进程。

读取插件配置示例：

```bash
curl "$NIUMA_LOCAL_API_URL/api/v1/plugins/config?plugin_id=$NIUMA_PLUGIN_ID"
```

上报工具事件示例：

```bash
curl -X POST "$NIUMA_LOCAL_API_URL/api/v1/plugin-events" \
  -H "Content-Type: application/json" \
  -d '{"plugin_id":"niuma-plugin-demo","events":[]}'
```

## 工具事件上报

`event_watcher` 工具插件通过 Local API 上报事件：

```http
POST /api/v1/plugin-events
Content-Type: application/json
```

请求体：

```json
{
  "plugin_id": "niuma-plugin-codex",
  "events": [
    {
      "id": "event-1",
      "dedupe_key": "codex:session-1:approval-1",
      "source": "plugin:niuma-plugin-codex",
      "tool": "codex",
      "session_id": "session-1",
      "project_path": "/path/to/project",
      "project_name": "project",
      "event_type": "approval_requested",
      "severity": "urgent",
      "summary": "Bash: cargo test",
      "content": "Bash: cargo test",
      "error_message": null,
      "attention_resolve_key": null,
      "completion_reason": null,
      "failure_reason": null,
      "payload_ref": null,
      "created_at": "2026-06-18T12:00:00Z"
    }
  ]
}
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "plugin_id": "niuma-plugin-codex",
    "event_count": 1,
    "applied_count": 1,
    "session_count": 1
  }
}
```

约束：

- `plugin_id` 必须匹配已发现插件。
- 插件必须有关联 `tool_id`，否则不能调用该接口上报工具事件。
- `event.tool` 必须等于插件 manifest 中的 `tool_id`。
- `dedupe_key` 必须稳定，重复扫描同一原始事件时应保持一致。
- 成功上报后，NiumaNotifier 会通过 `StateMutationService` 写入状态并触发 SSE 更新。
- 返回 `applied_count = 0` 通常表示事件已被去重；插件不需要因此重试同一事件。

## NiumaEvent 字段

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `id` | 是 | 事件唯一 ID。建议包含工具、session 和原始事件标识。 |
| `dedupe_key` | 是 | 去重键。重复扫描同一原始事件时必须保持稳定。 |
| `source` | 是 | 来源，建议使用 `plugin:<plugin-id>`。 |
| `tool` | 是 | 工具 ID，必须匹配插件 `tool_id`。 |
| `session_id` | 是 | 工具侧会话 ID。 |
| `project_path` | 是 | 项目路径。未知时可传空字符串。 |
| `project_name` | 是 | 项目名。未知时可传工具侧可读名称。 |
| `event_type` | 是 | 事件类型，见下表。 |
| `severity` | 是 | 展示严重级别，常用 `info`、`urgent`、`error`。 |
| `summary` | 是 | 短摘要。 |
| `content` | 否 | 可展示正文或命令内容。 |
| `error_message` | 否 | 错误详情，失败态优先展示。 |
| `attention_resolve_key` | 否 | 用于精确清理等待授权/输入等关注项。 |
| `completion_reason` | 否 | 完成原因。 |
| `failure_reason` | 否 | 失败原因。 |
| `payload_ref` | 否 | 大 payload 引用。当前主程序只保存引用，不读取内容。 |
| `created_at` | 是 | 事件发生时间，RFC 3339 / ISO 8601 格式。 |

`event_type` 支持：

| 值 | 状态语义 |
| --- | --- |
| `session_started` | 会话开始，状态为 `running`。 |
| `session_idled` | 会话空闲，状态为 `idle`。 |
| `approval_requested` | 等待授权，状态为 `waiting_approval`。 |
| `input_requested` | 等待输入，状态为 `waiting_input`。 |
| `task_failed` | 任务失败，状态为 `error`。 |
| `assistant_message_completed` | 助手回复完成，状态为 `completed`。 |
| `manual_dismissed` | 手动忽略，状态为 `completed` 并清理关注项。 |
| `session_staled` | 会话过期，内部清理态。 |
| `session_activity` | 普通活动，状态为 `running`。 |

`completion_reason` 支持：

```text
normal
interrupted
rolled_back
aborted_unknown
```

`failure_reason` 支持：

```text
timeout
context_window_exceeded
usage_limit_reached
server_overloaded
policy_blocked
response_stream_failed
connection_failed
quota_exceeded
internal_server_error
retry_limit
sandbox_error
fatal
unknown
```

## SSE 客户端实现要求

通知插件和状态指示插件都通过 SSE 消费主程序实时数据。建议实现统一的 SSE 客户端逻辑：

- 请求头带上 `Accept: text/event-stream`。
- 忽略以 `:` 开头的 keep-alive 注释行。
- 按空行分隔 SSE frame，支持同一个 frame 中出现多行 `data:`。
- 根据 `event:` 区分事件类型；未知事件类型应忽略。
- 连接断开后自动重连，推荐使用 2 到 5 秒固定间隔或指数退避。
- 不要假设 SSE 会补发历史数据；断线期间错过的事件不应通过数据库回查补偿。

SSE 当前没有鉴权和订阅参数。外部插件应只连接本机可信 Local API，不要把该接口暴露给公网。

## 通知插件事件消费

`event_consumer` 通知插件应订阅实时事件流：

```http
GET /api/v1/events/stream
Accept: text/event-stream
```

普通事件格式：

```text
event: event
id: event-1
data: {"id":"event-1","tool":"codex","session_id":"session-1","project_path":"/repo","project_name":"repo","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","created_at":"2026-06-18T12:00:00Z"}
```

测试通知事件格式：

```text
event: notification_test
id: manual-test:builtin-ntfy:1
data: {"test_id":"manual-test:builtin-ntfy:1","plugin_id":"builtin-ntfy","title":"测试通知","body":"这是一条测试通知","created_at":"2026-06-18T12:00:00Z"}
```

消费约束：

- `/api/v1/events/stream` 只广播成功写入的新事件，不补发历史事件。
- 重复上报但被去重的事件不会进入该流。
- 插件应自行判断哪些事件需要发送通知。
- 插件应在 `NIUMA_PLUGIN_DATA_DIR` 中保存本地去重状态，避免重连后重复发送。
- SSE keep-alive 注释行应忽略。

推荐通知处理流程：

1. 从 SSE 收到 `event`。
2. 依据 `event_type`、`severity`、项目名或插件配置判断是否需要通知。
3. 使用 `plugin_id + event.id` 或更细的业务键检查本地去重记录。
4. 调用外部通知服务。
5. 无论成功或失败，都调用 `/api/v1/plugins/notification-results` 回写结果。
6. 成功发送后更新本地去重记录；失败时可按插件策略重试，但应避免无限重试刷屏。

通知插件不需要查询最近事件列表，也不需要直接写通知历史数据库。

## 状态指示插件主状态消费

`state_consumer` 状态指示插件应订阅主状态流：

```http
GET /api/v1/state/stream
Accept: text/event-stream
```

主状态事件格式：

```text
event: state
id: 12
data: {"version":12,"status":"waiting_approval","updated_at":"2026-06-18T12:00:00Z","session":{"id":"session-1","tool":"codex","project_name":"repo","project_path":"/repo"},"detail":{"event_id":"event-1","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","content":"Bash: cargo test","error_message":null,"payload_ref":null,"completion_reason":null,"failure_reason":null}}
```

`status` 支持：

```text
idle
running
waiting_approval
waiting_input
completed
error
```

消费约束：

- `/api/v1/state/stream` 连接建立后会先发送一次当前主状态快照，后续仅在主状态内容变化时发送。
- 状态指示插件不应上报事件、不应写通知历史，也不应直接写 NiumaNotifier 的持久化文件。
- 状态展示应以 `status` 为准；不要根据插件 ID、工具原始日志或 `event_type` 自行推导主状态。
- 插件可以使用 `NIUMA_PLUGIN_DATA_DIR` 保存窗口位置、展示样式等本地运行状态。
- SSE keep-alive 注释行应忽略，断线后插件应自行重连。

## 通知结果回写

通知插件发送真实事件通知后，应回写发送结果：

```http
POST /api/v1/plugins/notification-results
Content-Type: application/json
```

请求体：

```json
{
  "plugin_id": "niuma-plugin-webhook",
  "event_id": "event-1",
  "status": "sent",
  "title": "需要授权",
  "body": "项目：repo\n工具：Codex\n事件：需要授权\n内容：Bash: cargo test",
  "reason": "approval_requested",
  "error_message": null,
  "sent_at": "2026-06-18T12:00:03Z"
}
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "saved": true,
    "record_id": "plugin_notification:niuma-plugin-webhook:event-1"
  }
}
```

字段说明：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `plugin_id` | 是 | 通知插件 ID。插件必须是 `kind = notification`。 |
| `event_id` | 是 | 被通知的 `NiumaEvent.id`。 |
| `status` | 是 | 当前只支持 `sent` 和 `failed`。 |
| `title` | 否 | 实际发送标题。 |
| `body` | 否 | 实际发送正文。 |
| `reason` | 否 | 发送原因，例如 `approval_requested`。 |
| `error_message` | 否 | 发送失败原因。 |
| `sent_at` | 否 | 发送成功时间。`status = sent` 且未传时，主程序使用当前时间。 |

约束：

- `event_id` 必须对应已存在事件。
- 非通知插件调用会返回业务校验失败。
- `status = failed` 时不会保存 `sent_at`。
- 同一个 `plugin_id + event_id` 会覆盖同一条插件通知记录，适合插件在失败后重试并回写最终结果。

## 测试通知结果回写

通知插件收到 `notification_test` SSE 事件并处理后，应回写测试结果：

```http
POST /api/v1/plugins/notification-test-results
Content-Type: application/json
```

请求体：

```json
{
  "plugin_id": "niuma-plugin-webhook",
  "test_id": "manual-test:niuma-plugin-webhook:1",
  "status": "sent",
  "title": "测试通知",
  "body": "这是一条测试通知",
  "error_message": null,
  "sent_at": "2026-06-18T12:00:03Z"
}
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "saved": true,
    "record_id": "plugin_notification_test:niuma-plugin-webhook:manual-test:niuma-plugin-webhook:1"
  }
}
```

## 插件管理 API

插件管理 API 主要供主界面使用，也可用于本地调试。

### 查询插件列表

```http
GET /api/v1/plugins
```

响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "id": "builtin-codex",
        "kind": "tool",
        "tool_id": "codex",
        "display_name": "Codex",
        "version": "0.1.0",
        "source": "builtin",
        "enabled": true,
        "runtime_status": "running",
        "last_error": null,
        "icon_url": null,
        "capabilities": ["event_watcher"],
        "config_schema": [],
        "install_path": null
      }
    ]
  }
}
```

`runtime_status` 支持：

```text
starting
stopped
stopping
running
failed
```

### 导入外部插件

```http
POST /api/v1/plugins/import
Content-Type: application/json
```

请求体：

```json
{
  "source_dir": "/path/to/niuma-plugin-example"
}
```

主程序会复制整个目录到用户插件目录，目标目录名为 manifest 中的 `id`。

### 移除外部插件

```http
POST /api/v1/plugins/remove
Content-Type: application/json
```

请求体：

```json
{
  "plugin_id": "niuma-plugin-example"
}
```

内置插件不能移除。

### 启用或停用插件

```http
POST /api/v1/plugins/enabled
Content-Type: application/json
```

请求体：

```json
{
  "plugin_id": "niuma-plugin-example",
  "enabled": true
}
```

说明：

- `tool` 插件会写入工具监听配置。
- 无 `tool_id` 的插件会写入通用插件启用状态。
- 启用状态变化会唤醒插件运行管理器。

### 读取插件配置

```http
GET /api/v1/plugins/config?plugin_id=niuma-plugin-example
```

响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "plugin_id": "niuma-plugin-example",
    "config": {
      "url": "https://example.com/webhook"
    },
    "config_schema": [
      {
        "key": "url",
        "type": "url",
        "label": "Webhook URL",
        "required": true,
        "default": null,
        "options": []
      }
    ]
  }
}
```

### 保存插件配置

```http
POST /api/v1/plugins/config
Content-Type: application/json
```

请求体：

```json
{
  "plugin_id": "niuma-plugin-example",
  "config": {
    "url": "https://example.com/webhook",
    "token": "secret-token"
  }
}
```

配置保存成功后会唤醒插件运行管理器。插件应在收到配置变化后自行重连或由主程序重启进程完成配置刷新。

## 调试建议

外部插件启动失败时，优先按以下顺序排查：

1. `plugin.json` 是否是合法 JSON，`id` 是否与内置插件冲突。
2. `platforms` 是否包含当前平台，或是否为空数组。
3. `command` 是否可执行；相对路径是否相对 `plugin.json` 所在目录。
4. 插件是否已经在界面或 `/api/v1/plugins/enabled` 中启用。
5. `/api/v1/plugins` 中的 `runtime_status` 和 `last_error` 是否指出启动错误。
6. 插件是否能访问 `NIUMA_LOCAL_API_URL`。
7. 通知插件是否在 `NIUMA_PLUGIN_DATA_DIR` 中保存了过旧或错误的去重记录。

运行中排查可以先调用：

```bash
curl "$NIUMA_LOCAL_API_URL/api/v1/plugins"
curl "$NIUMA_LOCAL_API_URL/api/v1/main-state"
curl "$NIUMA_LOCAL_API_URL/api/v1/notification-records"
```

## SSE 展示边界

插件只负责生成或消费 `NiumaEvent`。状态优先级、完成态保留时间、阻塞项清理和主状态 SSE 推送由主程序统一处理。

外部状态指示器应只依赖：

```text
GET /api/v1/state/stream
GET /api/v1/main-state
```

不要根据插件 ID、工具原始日志或 `event_type` 自行推导主状态。

## 开发检查清单

- `plugin.json` 能被 JSON 解析，且 `id` 不与内置插件重复。
- `platforms` 包含当前平台，或留空表示不限平台。
- `command` 在插件安装目录下可执行，或裸命令能被系统 `PATH` 找到。
- 工具插件声明 `kind = tool`、`tool_id` 和 `event_watcher`。
- 通知插件声明 `kind = notification`、`event_consumer`，需要测试通知时同时声明 `notification_test`。
- 状态指示插件声明 `kind = status_indicator` 和 `state_consumer`。
- 工具插件上报事件时，`event.tool` 与 manifest `tool_id` 完全一致。
- 状态指示插件只消费 `/api/v1/state/stream`，不自行推导主状态。
- `dedupe_key` 稳定，重复扫描不会制造重复状态。
- 插件退出时能响应 `SIGTERM` 或等效终止信号。
- 插件使用 `NIUMA_PARENT_PID` 做父进程退出自清理。
- 通知插件把发送去重状态保存到 `NIUMA_PLUGIN_DATA_DIR`。
- 外部插件通过 Local API 读取配置，不依赖主程序内部配置文件路径。
- 插件不直接读写 `niuma.sqlite`，也不通过数据库查询历史事件。
- SSE 断线后能自动重连，且不会把断线重连当成必须补发历史事件。
- 所有 JSON API 调用都检查外层 `code` 和 `message`。
