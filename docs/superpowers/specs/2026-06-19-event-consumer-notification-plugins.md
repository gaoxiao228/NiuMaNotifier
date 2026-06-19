# 事件消费者推送插件设计

## 背景

当前 NiumaNotifier 的插件体系主要覆盖工具事件监听插件。插件通过 `event_watcher`
能力监听 Codex 等工具，并将统一的 `NiumaEvent` 上报给主程序。主程序再根据事件计算主状态、
写入状态库，并通过 `/api/v1/state/stream` 向外广播聚合后的 `MainState`。

通知推送最初是主程序内置能力。Bark、ntfy 配置、发送逻辑、通知记录和测试发送都在主程序
内部实现。这导致通知渠道扩展需要修改主程序，也让插件体系只覆盖“事件生产者”，没有覆盖“事件消费者”。

本设计目标是将推送模块逐步插件化：主程序只广播事实事件，推送插件订阅事件流后自行判断是否推送。

## 目标

- 新增事件级 SSE 流 `/api/v1/events/stream`，只广播新增且已应用的 `NiumaEvent`。
- 新增插件能力 `event_consumer`，用于表示插件会消费事件流。
- 支持内置推送插件，例如 `builtin-bark`、`builtin-ntfy`，由统一插件运行管理器启动。
- 推送插件自行判断是否发送、生成通知内容、发送到具体渠道并做本地去重。
- 新增插件通知结果回写接口，让主程序能维护统一通知历史。
- 第一阶段先验证 Bark 插件化；ntfy 已按同一模型迁移为内置通知插件。

## 非目标

- 不在第一阶段实现事件 replay 或持久化消费 offset。
- 不在第一阶段实现插件市场、签名校验或强沙箱。
- 不要求外部推送插件立即稳定接入，先以内置 Bark/ntfy 插件验证链路。
- 不让 `/api/v1/state/stream` 承担事件通知职责。

## 当前接口规范约束

普通业务接口继续遵守统一 JSON 响应结构：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

`/api/v1/events/stream` 是 SSE 流式协议，和现有 `/api/v1/state/stream` 一样属于协议例外，
响应类型为 `text/event-stream`，不使用统一 JSON 包装。

业务失败、参数错误、路由不存在和系统异常仍遵循当前 API 规范。事件流握手失败属于协议或系统层问题，
由 HTTP 状态码表达。

## 总体架构

```text
event_watcher 插件
  -> POST /api/v1/plugin-events
  -> 主程序 append_events
  -> store NiumaEvent
  -> /api/v1/events/stream 广播 NiumaEvent
  -> /api/v1/state/stream 广播 MainState

event_consumer 插件
  -> GET /api/v1/events/stream
  -> 自行判断是否推送
  -> 发送到 Bark / ntfy / 其他渠道
  -> 本地记录去重 key
```

主程序只定义事件事实，不定义推送策略。推送策略属于推送插件。

## 事件流协议

新增路由：

```http
GET /api/v1/events/stream
Accept: text/event-stream
```

SSE 消息格式：

```text
event: event
id: <event.id>
data: <NiumaEvent JSON>
```

示例：

```text
event: event
id: event-1
data: {"id":"event-1","tool":"codex","session_id":"session-1","project_path":"/repo","project_name":"repo","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","content":"Bash: cargo test","created_at":"2026-06-19T12:00:00Z"}
```

广播规则：

- 只广播 `StateMutationService.append_events()` 返回的 `applied_events`。
- 重复上报但未实际应用的事件不广播。
- 一次提交多个新事件时逐条广播。
- 第一阶段连接建立后不补发历史事件。
- 第一阶段不实现 `Last-Event-ID` replay。

## 与状态流的分工

| 路径 | 粒度 | 消费者 | 内容 |
| --- | --- | --- | --- |
| `/api/v1/state/stream` | 聚合状态 | UI、状态面板、状态消费者 | `MainState` |
| `/api/v1/events/stream` | 原始事件 | 推送插件、审计插件、自动化插件 | `NiumaEvent` |

状态流用于展示“现在系统处于什么状态”。事件流用于处理“刚刚发生了什么事件”。

## 插件 manifest 扩展

当前插件默认是工具插件，强依赖 `tool_id`。推送插件不属于具体 AI 工具，因此需要扩展插件类型。

新增字段：

```json
{
  "kind": "tool"
}
```

兼容规则：

- `kind` 缺省时默认为 `tool`。
- `kind = "tool"` 时必须提供 `tool_id`。
- `kind = "notification"` 时不要求 `tool_id`。
- `event_watcher` 只允许用于 `tool` 插件。
- `event_consumer` 不绑定 `tool_id`，第一阶段主要用于 `notification` 插件。

Codex manifest 示例：

```json
{
  "id": "builtin-codex",
  "kind": "tool",
  "tool_id": "codex",
  "display_name": "Codex",
  "version": "0.1.0",
  "command": "niuma-codex-plugin",
  "args": [],
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["event_watcher"],
  "source": "builtin"
}
```

Bark manifest 示例：

```json
{
  "id": "builtin-bark",
  "kind": "notification",
  "display_name": "Bark",
  "version": "0.1.0",
  "command": "niuma-plugin-bark",
  "args": [],
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["event_consumer", "notification_test"],
  "source": "builtin"
}
```

## 插件启用状态

当前 `listener_config` 的语义是“是否监听某个工具”，不适合承载通知插件启用状态。

建议新增通用插件启用配置：

```json
{
  "builtin-codex": true,
  "builtin-bark": true,
  "builtin-ntfy": false
}
```

第一阶段折中规则：

- `tool` 插件继续沿用 `listener_config` 控制启用和运行。
- `notification` 插件使用 `plugin_enabled_map` 控制运行。

长期目标：

- `plugin_enabled_map` 控制插件进程是否运行。
- `listener_config` 控制工具事件是否参与主状态。

## 插件运行管理器

插件运行管理器需要启动所有常驻插件能力：

- `event_watcher`
- `event_consumer`

启用判断：

```text
tool + event_watcher:
  listener_config 中对应 tool 启用时运行

notification + event_consumer:
  plugin_enabled_map 中对应 plugin_id 启用时运行
```

运行状态仍复用现有 `starting`、`running`、`stopping`、`stopped`、`failed`。

## 插件环境变量

继续注入现有环境变量：

```text
NIUMA_LOCAL_API_URL
NIUMA_PLUGIN_ID
NIUMA_PARENT_PID
NIUMA_STATE_PATH
```

新增：

```text
NIUMA_PLUGIN_CONFIG_PATH
NIUMA_PLUGIN_DATA_DIR
```

说明：

- `NIUMA_PLUGIN_CONFIG_PATH` 指向主程序生成的插件配置文件。
- `NIUMA_PLUGIN_DATA_DIR` 指向插件可写数据目录，用于去重记录、临时状态和插件本地日志。

推送插件不得直接写主程序 SQLite 状态库。

## 内置通知插件 MVP

Bark/ntfy 插件启动流程：

1. 读取 `NIUMA_LOCAL_API_URL`。
2. 读取 `NIUMA_PLUGIN_CONFIG_PATH`。
3. 创建或读取 `NIUMA_PLUGIN_DATA_DIR`。
4. 连接 `${NIUMA_LOCAL_API_URL}/api/v1/events/stream`。
5. 收到 `NiumaEvent` 后自行判断是否推送。
6. 发送 Bark 或 ntfy 请求。
7. 成功发送后记录去重 key。

配置文件示例：

```json
{
  "enabled": true,
  "device_key": "xxx"
}
```

第一阶段通知配置迁移到插件配置存储：

```text
plugin_configs.plugin_id = builtin-bark
plugin_configs.payload = {"device_key":"..."}
plugin_configs.plugin_id = builtin-ntfy
plugin_configs.payload = {"topic":"..."}
```

主程序启动 `builtin-bark` 前从 `plugin_configs` 生成 `NIUMA_PLUGIN_CONFIG_PATH`。
主程序启动 `builtin-ntfy` 前同样从 `plugin_configs` 生成 `NIUMA_PLUGIN_CONFIG_PATH`。
旧 `notification_channels` 不再作为兼容读取来源，Bark/ntfy 配置权威来源仅为 `plugin_configs`。
Bark server、group 和 icon URL 不在插件管理中暴露；运行时使用默认 server/group，并优先使用工具图标。
ntfy 当前在插件管理中暴露 `Topic`，server 默认 `https://ntfy.sh`，token 不再从旧配置读取。

## 推送判断

推送插件自行决定是否推送。Bark MVP 为了保持现有行为，应支持当前规则：

推送：

- `approval_requested`
- `input_requested`
- `task_failed`
- `assistant_message_completed` 且 `completion_reason` 为 `normal` 或 `aborted_unknown`

跳过：

- `assistant_message_completed` 且 `completion_reason` 为 `interrupted` 或 `rolled_back`
- 其他非通知事件

通知规则、标题正文生成和第三方请求构造由通知插件本地实现；插件 runtime 不依赖
`niuma-core` 等主工程 crate，只依赖 Local API、SSE 事件、环境变量和插件配置文件这些协议。

后续如果需要抽 SDK，也应是独立协议 SDK，不能反向依赖主工程。

## 去重策略

插件本地去重 key：

```text
<plugin_id>:<event.id>
```

如果一个插件内部支持多个发送目标：

```text
<plugin_id>:<target_id>:<event.id>
```

第一阶段建议用文件存储：

```text
<NIUMA_PLUGIN_DATA_DIR>/sent-events.jsonl
```

每行示例：

```json
{"key":"builtin-bark:event-1","sent_at":"2026-06-19T12:00:00Z"}
```

去重写入策略：

- 发送前检查 key 是否存在。
- 发送成功后写入 key。

## 通知结果回写

通知插件发送成功或失败后回写主程序：

```http
POST /api/v1/plugins/notification-results
```

请求体：

```json
{
  "plugin_id": "builtin-bark",
  "event_id": "event-1",
  "status": "sent",
  "title": "需要处理",
  "body": "项目：demo",
  "reason": "approval_requested",
  "error_message": null,
  "sent_at": "2026-06-19T12:00:00Z"
}
```

约束：

- `status` 只接受 `sent` 和 `failed`。
- 主程序校验 `plugin_id` 必须是 `notification` 插件。
- 主程序校验 `event_id` 必须存在于公开事件表。
- 插件不回写 skipped，避免通知历史退化为事件审计日志。
- 存储以 `plugin_id + event_id` 幂等 upsert，允许失败后后续成功覆盖。
- 发送失败不写入 key，允许后续事件或手动重启后再次尝试。

## 旧通知 runtime 迁移

迁移期间必须避免重复推送：

```text
旧 notification_runtime 发送 Bark
builtin-bark 插件也发送 Bark
```

第一阶段建议：

- Bark 迁移到 `builtin-bark` 后，旧 notification runtime 不再发送 Bark。
- ntfy 迁移到 `builtin-ntfy` 后，旧 notification runtime 不再发送 ntfy。
- 旧 notification runtime 暂时保留手动测试和历史记录写入能力。

当前实现选择明确边界：旧 runtime 的实时事件处理永久跳过 Bark 和 ntfy；如果对应
通知插件未启用，实时推送就不会发生。手动测试发送仍作为配置测试保留在主程序内，但读取插件配置。

## 可行性判断

结论：可行，适合分阶段落地。

可行依据：

- 项目已有统一 `NiumaEvent` 模型。
- 项目已有 Local API 和 SSE 基础设施。
- 插件运行管理器已支持内置和外部可执行插件。
- 推送插件只需要订阅事件流，不要求主程序主动调用插件。
- 第一阶段不做结果回写和 replay，可以控制实现复杂度。

风险评估：

| 项目 | 风险 | 说明 |
| --- | --- | --- |
| `/api/v1/events/stream` | 低 | 复用现有 SSE 基础设施 |
| `event_consumer` capability | 中低 | 主要是 manifest 和筛选逻辑扩展 |
| `PluginKind` 改造 | 中 | 需要处理旧插件兼容和 `tool_id` 可选 |
| 通用插件启用配置 | 中 | 需要避免和 `listener_config` 语义混淆 |
| Bark 插件化 | 中高 | 涉及配置注入、HTTP 发送、去重 |
| 旧 runtime 迁移 | 高 | 最大风险是重复推送或漏推 |

## 实施阶段

### 阶段 1：事件流基础设施

- 新增 `/api/v1/events/stream`。
- 只广播 `applied_events`。
- 为 CORS、初始连接、事件广播、多客户端订阅补测试。
- 不改通知发送逻辑。

### 阶段 2：插件模型扩展

- 新增 `PluginCapability::EventConsumer`。
- 新增 `PluginKind`。
- 让 `tool_id` 对 `notification` 插件可选。
- 更新 manifest 校验和管理列表展示。

### 阶段 3：通用插件启用配置

- 新增 `plugin_enabled_map` 存储。
- `notification` 插件按 `plugin_enabled_map` 启停。
- `tool` 插件暂时继续按 `listener_config` 启停。

### 阶段 4：Bark 插件 MVP

- 新增 `builtin-bark` manifest。
- 新增 Bark runtime。
- 订阅 `/api/v1/events/stream`。
- 按现有通知规则发送 Bark。
- 使用 `NIUMA_PLUGIN_DATA_DIR` 做本地去重。
- 关闭旧 runtime 的 Bark 分支。

### 阶段 5：ntfy 迁移

- 新增 `builtin-ntfy` 插件。
- 迁移 ntfy 发送逻辑。
- 主通知设置页不再承载 ntfy 配置，ntfy 配置改由插件管理 schema 渲染。
- 再评估是否移除旧 notification runtime 的手动测试职责。

## 验收标准

- `/api/v1/events/stream` 能在新增事件后广播 `event` SSE。
- 重复上报但未应用的事件不会再次广播。
- `/api/v1/state/stream` 行为不受影响。
- `builtin-bark` 能在启用后订阅事件流。
- `builtin-ntfy` 能在启用后订阅事件流。
- Codex 审批事件产生后，Bark 插件能发送推送。
- Codex 审批事件产生后，ntfy 插件能发送推送。
- 关闭 `builtin-bark` 后不再发送 Bark。
- 关闭 `builtin-ntfy` 后不再发送 ntfy。
- 同一 `event.id` 不会重复推送。
- 旧 notification runtime 不会和 Bark/ntfy 插件重复发送。
- `cargo test -p niuma-api` 通过。
- 前端测试通过。

## 待确认问题

- 第一阶段是否接受插件离线期间事件不补发。
- Bark 插件失败后是否需要退避重试，还是等待下一次事件或重启。
- `plugin_enabled_map` 是否需要在第一阶段进入 UI，还是先内部默认启用内置 Bark。
- 旧 notification history 在通知插件化后是否隐藏、标记为旧模式，还是暂时保留但不承诺准确。

## 当前实施进展

- 已新增 `/api/v1/events/stream`，按 `applied_events` 广播实时 `NiumaEvent`。
- 已新增 `event_consumer`、`notification` 插件类型和 `plugin_enabled_map`。
- 插件管理页已通过通用插件启停接口控制工具插件和通知插件；工具插件继续写
  `listener_config`，通知插件写 `plugin_enabled_map`。
- 已注册内置 `builtin-bark`、`builtin-ntfy` manifest，并新增 `niuma-plugin-bark`、`niuma-plugin-ntfy` runtime。
- `builtin-bark`、`builtin-ntfy` manifest 已声明 `config_schema`，插件管理页按 schema 渲染配置表单。
- Bark/ntfy 配置权威存储已迁到 `plugin_configs`；旧 `notification_channels` 配置兼容读取已移除。
- `builtin-bark` 和 `builtin-ntfy` 已能订阅事件流、读取插件配置、按插件本地通知规则发送推送，并使用
  `<NIUMA_PLUGIN_DATA_DIR>/sent-events.jsonl` 做本地成功发送去重。
- `builtin-bark` 和 `builtin-ntfy` runtime 已移除对 `niuma-core` 的 Rust 依赖；事件解析、通知决策和
  Bark/ntfy 请求构造都在插件进程内部完成。
- 主通知设置页已移除 Bark/ntfy 配置块，配置统一迁到插件管理。
- 桌面端启动时会为内置插件解析真实二进制路径；开发和打包构建会将内置插件同步到
  `src-tauri/resources/bin`，Tauri 打包资源统一映射为 `resource_dir/bin`。
- 旧 `notification_runtime` 的实时事件处理已永久跳过 Bark 和 ntfy 渠道，避免插件未启用时仍由旧逻辑发送。
- 已新增 `/api/v1/plugins/notification-results`，Bark/ntfy 插件会回写发送成功或失败结果。
- 通知历史读取接口已合并旧手动测试记录和插件回写结果。
- 事件 replay 仍未实现。
