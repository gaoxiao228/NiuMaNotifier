# 外部 SSE 接入说明

本文档面向需要从外部系统读取 NiumaNotifier 本机状态的集成方，例如状态栏、自动化脚本、测试控制台、桌面通知代理或局域网内的辅助面板。

## 项目目的

NiumaNotifier 是一个本机 AI 编程工具状态通知器。它监听 Codex、Claude Code 等工具的 hook、session 文件和运行日志，把不同工具的原始事件统一转换为 `NiumaEvent`，再聚合成一个稳定的主状态。

外部系统接入 SSE 的常见目的：

- 实时知道当前 AI 工具是否正在运行。
- 判断是否需要用户处理授权、输入或错误。
- 在任务完成时触发外部自动化、提示或通知。
- 在测试环境中构造、观察和重置本机状态流。

NiumaNotifier 默认只在本机暴露 Local API：

```text
http://127.0.0.1:27874
```

可以通过 `NIUMA_LOCAL_API_ADDR` 覆盖监听地址。只有在显式绑定到非 loopback 地址时，局域网或外部网络才可能访问。

## 接入边界

- Local API 默认面向本机可信调用方，不内置鉴权。
- JSON 接口和 SSE 响应会带 CORS 头，允许浏览器端本地页面直接访问。
- 如果把监听地址绑定到 `0.0.0.0` 或局域网 IP，应在外层增加网络访问控制。
- `NIUMA_DB_PATH` 会影响当前实例使用的 SQLite 通知历史数据库；调试通知历史时应确认目标实例。
- 当所有 AI 监听开关关闭时，主状态会对外固定展示为 `idle`。

## 接口总览

| 用途 | 方法 | 路径 | 响应类型 | 稳定性 |
| --- | --- | --- | --- | --- |
| 实时主状态 SSE | `GET` | `/api/v1/state/stream` | `text/event-stream` | 稳定 |
| 实时事件 SSE | `GET` | `/api/v1/events/stream` | `text/event-stream` | 实验 |
| 查询当前主状态 | `GET` | `/api/v1/main-state` | JSON envelope | 稳定 |
| 重置本机状态 | `POST` | `/api/v1/state/reset` | JSON envelope | 稳定 |

SSE 是流式协议例外，不使用统一 JSON envelope。普通 HTTP JSON 接口遵循：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

常见错误码：

| `code` | 含义 | 典型场景 |
| --- | --- | --- |
| `0` | 成功 | 请求已正常处理。 |
| `100004` | 参数格式错误 | JSON 请求体无法解析。 |
| `100101` | 业务校验失败 | reset 的 `confirm` 不正确。 |
| `900001` | 系统错误 | 读取或计算本机状态失败。 |
| `900005` | 路由不存在 | 请求了未注册路径。 |

## SSE 主状态流

请求：

```http
GET /api/v1/state/stream
Accept: text/event-stream
```

连接建立后，服务端会立即发送一次当前主状态。后续只有状态内容变化时才推送新的 `state` 事件；同时服务端保留 5 秒兜底刷新检查，用于覆盖完成态过期、跨进程写入和订阅丢失。

事件格式：

```text
event: state
id: 1
data: {"version":1,"status":"running","updated_at":"2026-06-13T12:00:00Z","session":{...},"detail":{...}}
```

说明：

- `event` 固定为 `state`。
- `id` 与 `data.version` 相同，表示展示版本。
- `id` 不是可恢复消费位点；重连后建议重新接受首个快照，或先调用 `/api/v1/main-state` 做一次同步。
- 服务端可能发送 SSE keep-alive 注释行，客户端应忽略非 `state` 事件。

## 事件 SSE 流

请求：

```http
GET /api/v1/events/stream
Accept: text/event-stream
```

事件流用于事件消费者插件。服务端只广播成功写入并进入状态机的新 `NiumaEvent`，不会补发历史事件，也不会广播重复上报但被去重的事件。推送插件需要自行判断是否发送通知。

可选查询参数可以缩小普通 `event` 帧范围：

```http
GET /api/v1/events/stream?tool=codex&session_id=s1&event_type=approval_requested
GET /api/v1/events/stream?normalized_session_id=main-session&project_path=/repo
```

支持的过滤字段包括 `tool`、`session_id`、`normalized_session_id`、`project_path`、`event_type` 和 `severity`。多个过滤条件之间是 AND 关系。这些过滤不作用于 `notification_test` 控制帧。

事件格式：

```text
event: event
id: event-1
data: {"id":"event-1","tool":"codex","session_id":"s1","project_path":"/repo","project_name":"repo","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","created_at":"2026-06-19T12:00:00Z"}
```

Codex subagent 事件可能额外包含 `parent_session_id`。`session_id` 始终表示事件所属的真实会话；`parent_session_id` 仅表示父会话关系，消费者不应把它当作当前事件的会话 ID。

## 主状态字段

`data` 是一个 `MainStatePayload`：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `version` | number | SSE 展示版本。状态内容变化时递增。普通查询中可能为 `0`。 |
| `status` | string | 当前主状态。详见“状态含义”。 |
| `updated_at` | string/null | 当前主状态对应事件时间，ISO 8601 格式。 |
| `session` | object/null | 当前主状态关联的 session。`idle` 时通常为 `null`。 |
| `detail` | object/null | 当前主状态关联的事件详情。`idle` 时通常为 `null`。 |

`session`：

```json
{
  "id": "session-id",
  "tool": "codex",
  "project_name": "NiuMaNotifier",
  "project_path": "/path/to/project"
}
```

`detail`：

```json
{
  "event_id": "event-id",
  "event_type": "approval_requested",
  "severity": "urgent",
  "summary": "Bash: cargo test",
  "content": "Bash: cargo test",
  "error_message": null,
  "payload_ref": null,
  "completion_reason": null,
  "failure_reason": null
}
```

`detail` 字段说明：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `event_id` | string | 关联的 `NiumaEvent` ID。 |
| `event_type` | string | 原始事件类型名称，例如 `approval_requested`、`task_failed`。 |
| `severity` | string | 展示严重级别，常见值为 `info`、`urgent`、`error`。 |
| `summary` | string | 面向用户展示的短摘要。 |
| `content` | string/null | 可展示的正文或命令内容。 |
| `error_message` | string/null | 错误详情。`error` 状态下应优先展示。 |
| `payload_ref` | string/null | 可选的大 payload 引用。 |
| `completion_reason` | string/null | 完成原因。 |
| `failure_reason` | string/null | 失败原因。 |

外部系统应直接使用 `status` 判断主状态，不要根据 `event_type` 自行推导。

## 状态含义

| 状态 | 含义 | 外部系统建议 |
| --- | --- | --- |
| `idle` | 当前没有需要展示的活动。内部 `stale` 也会对外展示为 `idle`。 | 可认为当前无活跃任务或无可处理事项。 |
| `running` | AI 工具任务正在运行，最近仍有活动。 | 可展示“运行中”，通常不需要打断用户。 |
| `waiting_approval` | 工具正在等待用户授权，例如命令执行、权限提升或外部访问。 | 应高优先级提示用户处理。 |
| `waiting_input` | 工具正在等待用户输入。 | 应提示用户回到工具或主界面继续输入。 |
| `completed` | 最近任务已完成。该状态默认只保留 1 分钟，随后变为 `idle`。 | 可用于触发完成通知或外部自动化。 |
| `error` | 工具任务失败或出现需要关注的错误。 | 应高优先级提示，并优先展示 `detail.error_message`。 |

主状态优先级：

1. 最早的 `waiting_approval` / `waiting_input`。
2. 最早的 `error`。
3. 最近活动 `running` / `completed`。
4. 无活动时为 `idle`。

## 当前状态查询

请求：

```http
GET /api/v1/main-state
```

响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "state": {
      "version": 0,
      "status": "idle",
      "updated_at": null,
      "session": null,
      "detail": null
    }
  }
}
```

说明：

- 查询接口适合初始同步、断线重连后的兜底同步或调试。
- 实时接入应以 `/api/v1/state/stream` 为主。
- 普通查询中的 `version` 可以是 `0`；SSE 推送中的 `version` 才表示展示版本。

## 重置状态接口

reset 是正式恢复接口，用于主状态无法自行恢复时，把 NiumaNotifier 的本机聚合状态恢复到 `idle`。

请求：

```http
POST /api/v1/state/reset
Content-Type: application/json
```

```json
{
  "confirm": "RESET_NIUMA_STATE",
  "reason": "state_stuck"
}
```

字段：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `confirm` | string | 是 | 必须等于 `RESET_NIUMA_STATE`，用于避免误触。 |
| `reason` | string | 否 | 调用方记录的重置原因，例如 `state_stuck`、`operator_request`。 |

确认字段错误时返回业务失败：

```json
{
  "code": 100101,
  "message": "confirm 必须为 RESET_NIUMA_STATE",
  "data": null
}
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "reset": true,
    "reset_at": "2026-06-13T12:00:00Z",
    "event_count": 0,
    "session_count": 0,
    "state": {
      "version": 0,
      "status": "idle",
      "updated_at": null,
      "session": null,
      "detail": null
    }
  }
}
```

注意事项：

- 该接口会重置当前 Local API 使用的内存聚合状态。
- 调用成功后会通过运行时事件总线发布重置事件，已连接的 SSE 客户端会收到新的 `state` 事件。
- reset 只恢复 NiumaNotifier 的聚合状态，不会停止或修复 Codex、Claude Code 等底层工具本身。
- 如果底层工具仍在继续写 session/log，reset 后状态可能再次变为 `running`、`waiting_approval`、`waiting_input` 或 `error`。
- 调用前应确认目标 Local API 地址，避免重置错误实例。

## JavaScript 接入示例

```ts
const apiUrl = 'http://127.0.0.1:27874'
const stream = new EventSource(`${apiUrl}/api/v1/state/stream`)

stream.addEventListener('state', (event) => {
  // SSE 的 data 是裸 MainStatePayload，不包 JSON envelope。
  const state = JSON.parse(event.data)
  console.log(state.status, state.session, state.detail)
})

stream.onerror = () => {
  // 浏览器 EventSource 会自动重连；需要强一致同步时可额外调用 /api/v1/main-state。
  console.warn('NiumaNotifier SSE disconnected, browser will retry automatically')
}
```

Node.js 环境可以使用支持 EventSource 的库；重连后仍建议把首个 `state` 事件当作完整快照处理。

## curl 调试

监听 SSE：

```bash
curl -N http://127.0.0.1:27874/api/v1/state/stream
```

查询当前状态：

```bash
curl -s http://127.0.0.1:27874/api/v1/main-state
```

重置状态：

```bash
curl -s -X POST http://127.0.0.1:27874/api/v1/state/reset \
  -H 'Content-Type: application/json' \
  -d '{"confirm":"RESET_NIUMA_STATE","reason":"state_stuck"}'
```

## 排障建议

| 现象 | 建议 |
| --- | --- |
| 连接不上 `/api/v1/state/stream` | 确认 NiumaNotifier Local API 已启动，并检查 `NIUMA_LOCAL_API_ADDR`。 |
| 一直是 `idle` | 确认 AI 监听开关已启用，并确认请求的是目标实例的 Local API 地址。 |
| `completed` 很快消失 | 这是预期行为，完成态默认保留 1 分钟。 |
| reset 后又变成运行中或等待中 | 底层工具仍在写入新事件，应回到对应工具处理。 |
| 浏览器跨域失败 | 确认请求的是 Local API，且没有被代理或外层网关移除 CORS 响应头。 |

## 兼容性约定

- `event` 名固定为 `state`。
- `status` 枚举新增前必须更新本文档和 `docs/architecture/main-state-contract.md`。
- 外部系统应忽略未知字段，避免未来扩展导致兼容性问题。
- 外部系统应把 `session` 和 `detail` 视为可空字段。
- 外部系统不应根据 `event_type` 自行推导主状态，应直接使用 `status`。
