# 插件开发说明

本文档描述 NiumaNotifier 插件 v1 的接入方式。当前插件是由 `plugin.json` 描述、由 NiumaNotifier 启停的本机受信可执行程序；插件通过 Local API 与主程序通信。

当前支持以下插件形态：

| 类型 | `kind` | 主要能力 | 说明 |
| --- | --- | --- | --- |
| 工具监听插件 | `tool` | `event_watcher` | 监听 Codex、Claude Code、Cursor 等工具的原始状态，并转换为统一的 `NiumaEvent` 上报。 |
| 工具会话 provider 插件 | `tool` | `tool_session_list_provider`、`tool_session_detail_provider` | 解析工具原始 session 文件，把会话列表和归一化消息详情提供给宿主。 |
| 通知插件 | `notification` | `event_consumer`、`notification_test`，可选 `approval_handler` | 消费主程序事件流，自行决定是否发送 Bark、ntfy 等外部通知；只有声明 `approval_handler` 时才可以处理授权决策。 |
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
3. 启动：声明 `event_watcher`、`event_consumer`、`state_consumer`、`tool_session_list_provider` 或 `tool_session_detail_provider` 的插件会作为常驻子进程启动。
4. 运行：主程序向插件注入环境变量，并把插件工作目录设置为 `plugin.json` 所在目录。
5. 停止：插件停用、manifest 变化或插件被移除时，主程序会终止旧进程。
6. 重启：插件异常退出后，主程序会记录 `failed` 状态，并在短暂退避后重试启动。

插件进程应满足以下要求：

- 能响应 `SIGTERM` 或平台等效终止信号，并尽快退出。
- 不要假设自己只会启动一次；重启后应能从 `NIUMA_PLUGIN_DATA_DIR` 恢复本地去重状态。
- 不要把运行状态写入 NiumaNotifier 的内部持久化文件。
- `stdout` 和 `stderr` 当前不会作为用户可见日志展示；调试阶段建议自行写入插件数据目录中的日志文件。
- 声明 `tool_session_list_provider` 或 `tool_session_detail_provider` 的插件会使用 `stdout` 作为 provider JSON Lines RPC 通道；这类插件不能向 `stdout` 写普通日志，普通日志必须写 `stderr` 或插件数据目录。

监听开关行为：

- 带有 `event_watcher` 能力的 `tool` 插件由对应工具的监听开关控制。
- 如果同一个进程还声明了 `tool_session_list_provider` 和 `tool_session_detail_provider`，关闭工具监听时也会同时关闭 provider。
- 工具监听关闭后，宿主会清空该工具的 session snapshot 缓存和事件 cursor 状态。reader 插件应预期 `/api/v1/session_list` 和 `/api/v1/session_project_groups` 对该工具返回空 session。
- 如果 reader 插件请求的详情因 provider 被关闭或停止而不再可用，宿主会返回业务失败 envelope，而不是返回旧的会话正文。

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

同时提供事件监听和 session provider 的工具插件示例：

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
  "capabilities": [
    "event_watcher",
    "tool_session_list_provider",
    "tool_session_detail_provider"
  ],
  "icon_url": "./assets/icon.png"
}
```

这种 combined tool 插件是当前内置 `builtin-codex` 的推荐形态：同一个进程可以监听工具事件，也可以通过 provider RPC 提供 session 列表和详情。实现时必须保证 provider RPC 独占 `stdout`；事件监听日志、debug 信息和普通运行日志不要写入 `stdout`。

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
| `capabilities` | 否 | 当前支持 `event_watcher`、`event_consumer`、`approval_handler`、`notification_test`、`state_consumer`、`tool_session_list_provider`、`tool_session_detail_provider`、`tool_session_list_reader`、`tool_session_detail_reader`。 |
| `icon_url` | 否 | 图标地址或相对资源路径。 |
| `config_schema` | 否 | 插件配置字段定义，供 UI 和配置接口使用。 |

约束：

- `tool` 插件必须提供 `tool_id`。
- 非 `tool` 插件不能声明 `event_watcher`。
- `event_watcher` 插件由工具监听开关控制启停。
- 如果同一个 `tool` 插件同时声明 `event_watcher` 和 session provider capability，工具监听开关关闭时，这个插件进程整体停止；对应工具的 session snapshot 和 detail provider 也会不可用。
- 无 `tool_id` 的插件由通用插件启用状态控制启停。
- 声明 `event_watcher`、`event_consumer`、`state_consumer`、`tool_session_list_provider` 或 `tool_session_detail_provider` 的插件会被运行管理器作为常驻子进程管理。
- `approval_handler` 是授权决策的补充能力，必须和 `event_consumer` 一起使用；单独声明 `approval_handler` 不是有效运行模式。
- `event_watcher`、`tool_session_list_provider` 和 `tool_session_detail_provider` 是独立能力；工具监听能力不隐含工具会话 provider 能力。
- 同一个 `tool_id` 下，每种上报能力只能由一个插件声明，例如只能有一个 `event_watcher`，也只能有一个 `tool_session_list_provider` 和一个 `tool_session_detail_provider`。
- 非 `tool` 插件不能声明 provider capability；`tool_session_detail_provider` 必须和 `tool_session_list_provider` 在同一个插件中声明。
- `tool_session_detail_reader` 表示插件可读取 AI 会话内容，插件管理 UI 会把它展示为敏感能力。当前 v1 该声明是开发契约和能力展示标记，不是服务端强鉴权边界。

能力说明：

| Capability | 可声明插件 | 说明 |
| --- | --- | --- |
| `event_watcher` | `tool` | 监听工具原始事件并通过 `/api/v1/plugin-events` 上报。 |
| `event_consumer` | `notification` | 消费 `/api/v1/events/stream` 事件流。 |
| `approval_handler` | `notification`，需同时声明 `event_consumer` | 可提交授权决策。 |
| `notification_test` | `notification` | 支持主界面测试通知按钮。 |
| `state_consumer` | `status_indicator` | 消费 `/api/v1/state/stream` 主状态流。 |
| `tool_session_list_provider` | `tool` | 向宿主提供该工具实际发现到的 session 列表。 |
| `tool_session_detail_provider` | `tool` | 按 `session_id` 向宿主提供归一化消息详情。 |
| `tool_session_list_reader` | 任意业务插件 | 读取宿主 `session_list` API。 |
| `tool_session_detail_reader` | 任意业务插件 | 通过宿主 `session_detail` API 读取 AI 会话内容；敏感能力。 |

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
- 事件、运行态条目、关注项和最新活动是主程序内存运行态；插件不能依赖数据库查询历史事件。
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

## 工具会话读取

第三方 reader 插件通过宿主 Local API 读取工具会话，不直接读取 Codex、Claude Code 等工具目录，也不直接调用 provider 插件。工具 session 视图和 Niuma 运行态是两个概念：`/api/v1/runtime_state_list` 返回 Niuma 状态机里的运行状态，工具原始 session 列表、项目分组和消息详情使用下面这些接口。

```http
GET /api/v1/session_list?tool=codex&include_subagents=false&active_only=false&limit=100
GET /api/v1/session_project_groups?tool=codex&project_path=/repo&include_subagents=false&page=1&page_size=20
GET /api/v1/session_detail?tool=codex&session_id=session-1&limit=100&cursor=cursor-1
```

`session_list` 读取宿主保存的最新 provider snapshot，不要求 reader 插件直接扫描磁盘。常用查询参数：

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `tool` | `all` | `codex`、`claude_code`、自定义工具 ID 或 `all`。 |
| `include_subagents` | `false` | 是否包含 subagent session。 |
| `active_only` | `false` | 是否只返回仍活跃的 session。 |
| `limit` | `100` | 返回数量上限。 |

`session_list` 成功响应：

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
        "is_subagent": false,
        "normalized_session_id": "session-1",
        "status": "active"
      }
    ]
  }
}
```

`session_project_groups` 按项目路径聚合 provider snapshot，返回结构是项目 -> 归一会话 -> 可选 raw session 明细。归一会话会把 subagent 归到 `normalized_session_id` 下；默认不展开 raw subagent 明细，但仍会计算父会话更新时间。每个归一会话里的 `updated_at` 表示最近 raw session 更新时间；如果 provider 已知首条用户消息，还会返回 `first_user_message_preview` / `first_user_message_at`。项目组计数字段中，`normalized_session_count` 表示归一会话数量，`raw_session_count` 表示 raw session 文件数量，`subagent_count` 表示其中由 subagent 产生的 raw session 数量。常用查询参数：

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `tool` | `all` | `codex`、`claude_code`、自定义工具 ID 或 `all`。 |
| `project_path` | 空 | 精确筛选项目路径。 |
| `include_subagents` | `false` | 是否在归一会话下展开 raw session 明细。 |
| `page` | `1` | 项目分组分页页码。 |
| `page_size` | `20` | 项目分组分页大小，最大 `100`。 |

`session_project_groups` 成功响应使用标准分页结构：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "tool": "codex",
        "project_path": "/repo",
        "project_name": "repo",
        "normalized_session_count": 1,
        "raw_session_count": 2,
        "subagent_count": 1,
        "sessions": [
          {
            "normalized_session_id": "session-1",
            "primary_session_id": "session-1",
            "title": "session-session",
            "status": "active",
            "updated_at": "2026-06-24T10:00:00Z",
            "first_user_message_preview": "总结这个项目",
            "first_user_message_at": "2026-06-24T09:30:00Z",
            "latest_event_summary": null,
            "subagent_count": 1
          }
        ]
      }
    ],
    "page": 1,
    "page_size": 20,
    "total": 1
  }
}
```

`session_project_groups/stream` 提供 SSE 快照流，查询参数与 `session_project_groups` 保持一致：

```http
GET /api/v1/session_project_groups/stream?tool=codex&project_path=/repo&include_subagents=true&page=1&page_size=20
```

该流发送 `event: session_project_groups` 帧。`data` payload 使用与 `session_project_groups` 相同的分页结构，并在每个归一会话上追加来自 Niuma 运行态的 overlay 字段：`runtime_status`、`runtime_last_event_id`、`runtime_last_activity_at`。当 `include_subagents=true` 时，raw session 也会包含同样的运行态字段。`status` 仍保留 provider 含义（`active`、`inactive` 或 `unknown`）；判断 Niuma 运行态应使用 `runtime_status`，例如 `running`、`waiting_approval`、`waiting_input`、`completed`、`error`、`idle` 或 `stale`。

连接建立后，服务端会立即发送一帧完整快照。当 Niuma 运行态变化时，服务端会用同一组查询参数重新计算并在序列化内容发生变化时再次发送完整快照。该流是展示状态接口，消费者应按 `/api/v1/state/stream` 的方式处理：重连时重新打开流并接受首帧快照，不依赖 SSE id 做断点恢复。

示例帧：

```text
event: session_project_groups
id: 2
data: {"list":[{"tool":"codex","project_path":"/repo","project_name":"repo","normalized_session_count":1,"raw_session_count":1,"subagent_count":0,"sessions":[{"normalized_session_id":"session-1","primary_session_id":"session-1","title":"session-session","status":"active","runtime_status":"waiting_approval","runtime_last_event_id":"event-1","runtime_last_activity_at":"2026-06-25T02:15:35Z","updated_at":"2026-06-25T02:15:35Z","first_user_message_preview":"总结这个项目","first_user_message_at":"2026-06-25T02:10:00Z","latest_event_summary":null,"subagent_count":0}]}],"page":1,"page_size":20,"total":1}
```

运行态 overlay 规则：

- `runtime_status = null` 表示 Niuma 当前没有该 session 的运行态记录。
- `status` 与 `runtime_status` 有意分离。不要用 provider `status` 判断 session 是否正在等待授权或输入。
- 归一会话的 `runtime_status` 会从同一 `normalized_session_id` 下的 raw session 聚合，优先级为：`waiting_approval` / `waiting_input`，然后是 `error`、`running`、`completed`、`idle`、`stale`、`null`。
- 当 `include_subagents=true` 时，`raw_sessions[]` 会包含每个 raw session 自己的运行态 overlay 字段；当 `include_subagents=false` 时，只返回归一会话摘要。

如果查询参数类型不合法，stream 接口会在建立 SSE 前返回标准错误 envelope，例如 `HTTP 400` 且 `code = 100003`。如果分页范围触发业务校验失败，则返回 `HTTP 200` 加非 0 业务错误码。

`session_detail` 按 `tool + session_id` 读取归一化消息详情。`messages` 倒序返回，`messages[0]` 是本页最新消息；`next_cursor` 用于继续读取更旧消息。第一版支持的消息角色包括 `user`、`assistant`、`system`、`tool_call`、`tool_result`、`event` 和 `unknown`。

`session_detail/stream` 为单个明确 session 提供 SSE 快照流：

```http
GET /api/v1/session_detail/stream?tool=codex&session_id=session-1&limit=100
```

该流必须同时指定 `tool` 和 `session_id`，不支持全局订阅。全局事件或过滤事件订阅应使用 `/api/v1/events/stream`；只有当 UI 正在展示某一个 session 详情面板时，才使用 `session_detail/stream`。

该流发送 `event: session_detail` 帧。`data` payload 使用与 `/api/v1/session_detail` 返回的 `data` 对象相同的结构。连接建立后会立即发送首帧。后续当匹配的运行时事件表明该 raw session 或 normalized session 可能变化时，服务端会重新计算详情，并且只在序列化后的详情快照发生变化时推送。

stream 接口有意不支持 `cursor`。它只监听最新页；历史分页继续使用 `/api/v1/session_detail?cursor=...`。

如果 `tool` 或 `session_id` 缺失或为空，stream 接口会在建立 SSE 前返回标准业务失败 envelope。如果 `limit` 类型不合法，则在建立 SSE 前返回标准 `HTTP 400` 参数类型错误 envelope。

成功响应仍使用统一 envelope：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "tool": "codex",
    "session_id": "session-1",
    "messages": []
  }
}
```

当对应工具监听关闭时：

- `session_list` 和 `session_project_groups` 会对该工具返回成功的空列表，因为宿主会清空 provider snapshot。
- 如果请求的 session 不可用，`session_detail` 返回业务失败 envelope。当前实现可能在 snapshot 清空后返回 `session_id 不存在`，也可能在 provider 进程仍存活时返回 provider 专用信息，例如 `session_provider_disabled`。
- reader 插件应把这两类情况都视为“当前无法读取 session 内容”，不要回退到直接读取原始工具 session 文件。

当前 v1 没有插件 token 鉴权，`tool_session_list_reader` 和 `tool_session_detail_reader` 是插件开发契约、能力展示和后续鉴权预留。`tool_session_detail_reader` 涉及 AI 会话内容，应在插件管理 UI 中按敏感能力展示。插件仍应只连接本机可信 Local API。

## 工具会话 Provider RPC

声明 `tool_session_list_provider` 和 `tool_session_detail_provider` 的工具插件由宿主管理为常驻进程。宿主通过插件进程的 `stdin/stdout` 与 provider 通信，协议是 JSON Lines：

- 宿主向插件 `stdin` 写入一行 JSON 请求。
- 插件向 `stdout` 写入一行 JSON 响应。
- 插件也可以向 `stdout` 写入无 `id` 的通知，用于告诉宿主 snapshot 已变化。
- `stdout` 只能写 provider JSON Lines；普通日志必须写 `stderr` 或 `NIUMA_PLUGIN_DATA_DIR` 下的日志文件。
- 每一行必须是完整 JSON，不允许 pretty print、多行 JSON 或其他前缀文本。

请求结构：

```json
{
  "id": "req-1",
  "method": "session_snapshot",
  "params": {}
}
```

响应结构：

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

Provider RPC 失败属于 provider 层失败，不是 Local API envelope。reader 调用 `/api/v1/session_detail` 时，宿主会把 provider 错误转换成标准 Local API 响应结构。常见 provider 错误码：

| 错误码 | 含义 |
| --- | --- |
| `method_not_found` | 未知 provider method。 |
| `invalid_params` | 请求参数无法解析，或参数中的工具与 provider 不匹配。 |
| `session_not_found` | 请求的 raw `session_id` 不存在于 provider snapshot 或文件索引中。 |
| `stale_session_file` | 原始文件已变化、被截断，或不再匹配索引中的 session。 |
| `session_provider_disabled` | 对应工具监听已关闭，session 列表和详情按设计不可用。 |
| `provider_internal_error` | provider 内部未预期失败。 |

当前 provider method：

| Method | Params | Result | 说明 |
| --- | --- | --- | --- |
| `session_snapshot` | `{ "tool": "codex" }` | `{ "tool": "codex", "sessions": [...] }` | 返回当前 provider 发现到的轻量 session 列表。 |
| `session_detail` | `{ "tool": "codex", "session_id": "session-1", "limit": 100, "cursor": null }` | `{ "detail": {...} }` | 返回指定 raw `session_id` 的归一化消息详情。 |

当前 provider notification：

```json
{
  "method": "session_snapshot_updated",
  "params": {
    "tool": "codex",
    "sessions": []
  }
}
```

宿主收到 `session_snapshot_updated` 后会更新内存中的 session registry；`/api/v1/session_list` 和 `/api/v1/session_project_groups` 都读取这个最新 snapshot。provider 应只在 snapshot 语义发生变化时通知，避免高频重复刷新。

`ToolSessionListItem` 字段语义：

| 字段 | 说明 |
| --- | --- |
| `id` | provider 内部列表项 ID，建议为 `<tool>:<session_id>`。 |
| `tool` | 工具 ID，必须匹配插件 manifest 的 `tool_id`。 |
| `session_id` | 工具原始 raw session id；`session_detail` 使用这个 ID 定位详情。 |
| `project_path` / `project_name` | 项目路径和展示名称。未知时可为空字符串或工具名。 |
| `file_path` | 原始 session 文件路径；如果工具没有文件，可使用可诊断的来源标识。 |
| `modified_at` | 原始 session 最后修改时间或等价更新时间。 |
| `discovered_at` / `last_seen_at` | provider 发现和最近看到该 session 的时间。 |
| `is_active` / `status` | provider 对活跃状态的判断；无法判断时 `status` 可为 `unknown`。 |
| `is_subagent` | 是否为 subagent / 子代理会话。 |
| `parent_session_id` | 工具原始父会话 ID，可为空。 |
| `normalized_session_id` | Niuma 计算出的业务归一会话 ID；subagent 通常归到父会话或根会话。 |
| `session_scope` | `main` 或 `subagent`。 |
| `agent_nickname` / `agent_role` | 工具提供的 subagent 展示信息，可为空。 |
| `normalization_status` | `resolved`、`parent_missing` 或 `parent_unresolved`，只用于诊断。 |
| `first_user_message_preview` | 首条用户消息摘要，可为空。provider 应保持短文本；内置 Codex provider 限制为 200 字符。 |
| `first_user_message_at` | `first_user_message_preview` 对应的消息时间，可为空。 |

`ToolSessionDetail` 复用同一套 identity 字段，并额外包含：

| 字段 | 说明 |
| --- | --- |
| `messages` | 当前页消息，倒序返回，最新消息在前。 |
| `next_cursor` | 读取更旧消息的 cursor。为空表示没有下一页。 |

provider 实现建议：

- snapshot 应保存轻量索引，不要长期把完整会话正文放在内存中。
- `session_detail` 应按 `limit` 分页读取，`limit` 已由宿主归一化，provider 不需要自行扩大上限。
- cursor 应稳定指向文件行号、消息序号或工具原始 offset，避免追加新消息后分页重复或漏消息。
- 原始文件被截断、替换或内容变化时，provider 应重建索引；不能用旧 cursor 读取错误 session。
- subagent 的 `session_id` 保持 raw id，不要直接改成 parent id；聚合使用 `normalized_session_id`。
- 关闭对应工具监听后，宿主会停止 provider 进程并清空该工具 snapshot；reader 插件不应假设关闭监听后仍能读取旧 session 列表。
- 如果 provider 进程在 combined watcher/provider 运行时中仍然存活，但监听处于关闭状态，`session_snapshot` 应返回空 session 列表，`session_detail` 应返回 `session_provider_disabled`。
- combined watcher/provider 插件应尽量共享同一个文件 repository 或等价缓存，但必须把事件投影和 session 读取保持为两条职责。provider 代码不应直接发出 `NiumaEvent`；watcher 代码仍应通过 `/api/v1/plugin-events` 上报事件。

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
      "parent_session_id": null,
      "normalized_session_id": "session-1",
      "session_scope": "main",
      "agent_nickname": null,
      "agent_role": null,
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
| `parent_session_id` | 否 | 工具侧父会话 ID。subagent 场景下用于表达 raw parent 关系。 |
| `normalized_session_id` | 否 | Niuma 计算出的业务归一会话 ID。主会话通常等于 `session_id`，subagent 通常指向父会话或根会话。 |
| `session_scope` | 否 | 会话范围，当前建议使用 `main` 或 `subagent`。 |
| `agent_nickname` | 否 | 工具提供的 subagent 展示昵称。 |
| `agent_role` | 否 | 工具提供的 subagent 角色。 |
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
| `approval_resolved` | 授权已由某个消费者同意或拒绝，状态恢复为 `running`。 |
| `approval_returned_to_codex` | Niuma 代处理窗口结束，仍保持 `waiting_approval`，用户需要回到 Codex 操作。 |
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
- 不要把 `curl -N` 能看到事件当作完整接入验证。插件自己的 SSE 客户端必须能解析完整的 `data: JSON` 载荷，并实际派发到事件处理逻辑。
- 当前 v1 事件流里，单个事件通常是一行完整 JSON。客户端可以在收到 `data:` 行后先尝试解析；如果解析失败，应继续累积后续 `data:` 行，直到空行 frame 边界后再次解析。
- 连接断开后自动重连，推荐使用 2 到 5 秒固定间隔或指数退避。
- 不要假设 SSE 会补发历史数据；断线期间错过的事件不应通过数据库回查补偿。

SSE 当前没有鉴权。部分流提供查询过滤参数，用于缩小推送帧范围。外部插件应只连接本机可信 Local API，不要把该接口暴露给公网。

## 通知插件事件消费

`event_consumer` 通知插件应订阅实时事件流：

```http
GET /api/v1/events/stream
Accept: text/event-stream
```

该流支持用可选查询参数过滤普通 `event` 帧：

```http
GET /api/v1/events/stream?tool=codex&session_id=session-1&event_type=approval_requested
GET /api/v1/events/stream?normalized_session_id=main-session&project_path=/repo
```

支持的过滤字段包括 `tool`、`session_id`、`normalized_session_id`、`project_path`、`event_type` 和 `severity`。多个过滤条件之间是 AND 关系。这些过滤只作用于普通 `event` 帧；`notification_test` 是插件测试控制事件，不绑定具体 session。

过滤参数说明：

| 参数 | 匹配目标 | 说明 |
| --- | --- | --- |
| `tool` | `event.tool` | 工具 ID，例如 `codex`、`claude_code` 或自定义工具 ID。 |
| `session_id` | `event.session_id` | 匹配产生事件的 raw session。当插件只绑定一个具体 session 时使用。 |
| `normalized_session_id` | `event.normalized_session_id` | 匹配归一化后的分组/主 session ID。项目分组 UI 如果需要同时接收主 session 和 subagent 事件，优先使用该参数。没有 `normalized_session_id` 的事件不会命中该过滤。 |
| `project_path` | `event.project_path` | 精确路径匹配。路径中有空格或非 ASCII 字符时需要 URL encode。 |
| `event_type` | `event.event_type` | snake_case 事件类型，例如 `approval_requested`、`input_requested` 或 `assistant_message_completed`。如果传入无效枚举值，服务端会在建立 SSE 前返回标准 `HTTP 400` 参数类型错误。 |
| `severity` | `event.severity` | 精确字符串匹配。当前内置来源常见值包括 `urgent`，但插件不应假设它是封闭枚举。 |

当消费者职责很窄时，建议使用过滤流减少插件侧处理。例如只处理授权的消费者可以订阅：

```http
GET /api/v1/events/stream?event_type=approval_requested
```

只关心某个分组 session 的详情面板可以订阅：

```http
GET /api/v1/events/stream?normalized_session_id=main-session
```

过滤不是权限边界。它只影响当前 SSE 连接收到哪些事件，不授予或限制其他 Local API 的访问能力。

普通事件格式：

```text
event: event
id: event-1
data: {"id":"event-1","tool":"codex","session_id":"session-1","project_path":"/repo","project_name":"repo","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","created_at":"2026-06-18T12:00:00Z"}
```

Codex subagent 事件可能额外包含 `parent_session_id`、`normalized_session_id`、`session_scope`、`agent_nickname` 和 `agent_role`。`session_id` 保持为真实事件来源会话；业务聚合、授权仲裁和默认通知策略应优先参考 `normalized_session_id` 与 `session_scope`。

测试通知事件格式：

```text
event: notification_test
id: manual-test:builtin-ntfy:1
data: {"test_id":"manual-test:builtin-ntfy:1","plugin_id":"builtin-ntfy","title":"测试通知","body":"这是一条测试通知","created_at":"2026-06-18T12:00:00Z"}
```

消费约束：

- `/api/v1/events/stream` 只广播成功写入的新事件，不补发历史事件。
- 重复上报但被去重的事件不会进入该流。
- 如果携带过滤参数，不匹配的事件会被跳过，不会为了后续重连缓存。
- 插件应自行判断哪些事件需要发送通知。
- 默认通知策略应跳过 `session_scope = "subagent"` 的 `assistant_message_completed`，避免子代理完成被误报为主任务完成；`approval_requested` 仍应通知，因为子代理授权也需要用户处理。
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

## 授权消费者

具备授权处理能力的消费者必须同时声明 `event_consumer` 和 `approval_handler`。实时授权弹窗必须只由 `/api/v1/events/stream` 中 `event: event` 且 `event_type = approval_requested` 的事件触发。没有 `approval_handler` 的事件消费者可以通知用户有授权等待处理，但不应展示授权操作，也不应提交授权决策。

`/api/v1/state/stream` 和 `/api/v1/main-state` 只表示展示状态。它们可以展示当前处于 `waiting_approval`，但不能作为“同意/拒绝”交互触发源。`GET /api/v1/approval-requests?status=pending` 仅用于插件启动恢复场景，是否使用由插件自行决定。

授权处理插件可以继续使用 `kind = notification`。`notification_test` 不是必需能力，只有插件需要支持主界面“测试通知”按钮时才声明。

授权处理插件 manifest 示例：

```json
{
  "id": "niuma-plugin-approval-demo",
  "kind": "notification",
  "display_name": "Approval Demo",
  "version": "0.1.0",
  "command": "node",
  "args": ["./bin/approval-demo.mjs"],
  "env": {},
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["event_consumer", "approval_handler"]
}
```

`approval_requested` 事件示例：

```text
event: event
id: event-approval-1
data: {"id":"event-approval-1","dedupe_key":"approval:codex:s1:t1:Bash:abc123","source":"codex-hook","tool":"codex","session_id":"session-1","project_path":"/repo","project_name":"repo","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","content":"Bash: cargo test","error_message":null,"attention_resolve_key":"approval:codex:s1:t1:Bash:abc123","payload_ref":"approval:codex:s1:t1:Bash:abc123","created_at":"2026-06-18T12:00:00Z"}
```

事件的 `payload_ref` 和 `attention_resolve_key` 会使用 `approval:<request_id>` 格式。消费者应优先从 `payload_ref` 解析；如果它为空，再从 `attention_resolve_key` 解析。没有 `approval:` 前缀的事件不能当作授权请求处理。

解析规则示例：

```js
function approvalRequestId(event) {
  // payload_ref 优先；没有时再使用 attention_resolve_key 兜底。
  const value = event.payload_ref || event.attention_resolve_key || ''
  return value.startsWith('approval:') ? value.slice('approval:'.length) : null
}
```

提交决策：

```http
POST /api/v1/approval-decisions
Content-Type: application/json
```

```json
{
  "request_id": "codex:s1:t1:Bash:abc123",
  "decision": "allow",
  "decided_by": "niuma-plugin-approval-demo",
  "decided_source": "plugin",
  "reason": "用户在通知中同意"
}
```

字段约定：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `request_id` | 是 | 从 `approval:<request_id>` 中解析出的授权请求 ID。 |
| `decision` | 是 | `allow` 或 `deny`。 |
| `decided_by` | 是 | 推荐使用环境变量 `NIUMA_PLUGIN_ID`。 |
| `decided_source` | 是 | 推荐使用稳定来源标识，例如 `plugin`、`notification`、`menu_bar`、`webhook`、`mobile`。 |
| `reason` | 否 | 用户可读原因，便于其他消费者展示处理来源。 |

成功并赢得决策时：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "request_id": "codex:s1:t1:Bash:abc123",
    "accepted": true,
    "status": "allowed",
    "decision": "allow",
    "decided_by": "niuma-plugin-approval-demo",
    "decided_source": "plugin",
    "reason": "用户在通知中同意",
    "proxy_status": "active"
  }
}
```

已有其他消费者先处理时：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "request_id": "codex:s1:t1:Bash:abc123",
    "accepted": false,
    "status": "denied",
    "decision": "deny",
    "decided_by": "dashboard",
    "decided_source": "ui",
    "reason": "用户在主界面拒绝",
    "proxy_status": "active"
  }
}
```

业务失败示例：

```json
{
  "code": 200001,
  "message": "request_id 不能为空",
  "data": null
}
```

接口进入业务处理后通常返回 HTTP 200，插件必须检查外层 `code`。`code = 0` 才表示业务成功；`accepted=true` 表示当前消费者赢得本次决策；`accepted=false` 表示已有其他消费者或桌面 UI 先处理，消费者应把本地按钮置为已处理状态，不要重试覆盖。

消费者可以在启动时可选恢复待处理授权，也可以轮询决策状态：

```http
GET /api/v1/approval-requests?status=pending
GET /api/v1/approval-decisions?request_id=codex:s1:t1:Bash:abc123
```

待处理列表返回示例：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "id": "codex:s1:t1:Bash:abc123",
        "tool": "codex",
        "session_id": "session-1",
        "turn_id": "turn-1",
        "tool_name": "Bash",
        "command": "cargo test",
        "description": "是否允许执行 cargo test？",
        "project_path": "/repo",
        "project_name": "repo",
        "status": "pending",
        "decided_by": null,
        "decided_source": null,
        "reason": null,
        "proxy_status": "active"
      }
    ]
  }
}
```

查询单个授权决策返回示例：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "request_id": "codex:s1:t1:Bash:abc123",
    "status": "allowed",
    "decision": "allow",
    "decided_by": "niuma-plugin-approval-demo",
    "decided_source": "plugin",
    "reason": "用户在通知中同意",
    "proxy_status": "active"
  }
}
```

`GET /api/v1/approval-decisions` 只用于查询状态，不包含 `accepted` 字段；只有 `POST /api/v1/approval-decisions` 的提交结果会返回 `accepted`。

恢复推荐流程：

1. 启动后读取 `NIUMA_LOCAL_API_URL` 和 `NIUMA_PLUGIN_ID`。
2. 建立 `/api/v1/events/stream` SSE 连接。
3. 如果插件需要启动恢复，可选调用 `GET /api/v1/approval-requests?status=pending` 做一次补偿恢复。
4. 对 SSE 事件和 pending 列表中的 `request_id` 做本地去重。
5. 展示来自 `approval_requested` 事件的待处理授权；如果启用了启动恢复，也展示启动时恢复到的 pending 列表。
6. 收到 `approval_resolved` 后移除或禁用本地按钮。
7. 收到 `approval_returned_to_codex` 后禁用按钮并提示回 Codex 操作。
8. 提交决策后根据 `accepted` 判断是否赢得决策。

处理事件规则：

- 收到 `approval_resolved`：禁用本地同意/拒绝，显示已由 `decided_by` / `decided_source` 处理。
- 收到 `approval_returned_to_codex`：禁用本地同意/拒绝，提示用户回到 Codex 中操作。
- 只有 `pending` 授权可以提交决策；`allowed`、`denied`、`returned_to_codex` 都视为已处理。

最小 Node.js 消费者示例：

```js
const apiUrl = process.env.NIUMA_LOCAL_API_URL
const pluginId = process.env.NIUMA_PLUGIN_ID

if (!apiUrl || !pluginId) {
  throw new Error('NIUMA_LOCAL_API_URL 和 NIUMA_PLUGIN_ID 必须存在')
}

function approvalRequestId(event) {
  // 授权事件通过 approval:<request_id> 暴露可提交决策的请求 ID。
  const value = event.payload_ref || event.attention_resolve_key || ''
  return value.startsWith('approval:') ? value.slice('approval:'.length) : null
}

async function decide(requestId, decision) {
  // 当前 v1 通过本机 Local API 提交决策，业务成功必须检查外层 code。
  const response = await fetch(`${apiUrl}/api/v1/approval-decisions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      request_id: requestId,
      decision,
      decided_by: pluginId,
      decided_source: 'plugin',
      reason: `用户在 ${pluginId} 中选择 ${decision}`
    })
  })
  const body = await response.json()
  if (body.code !== 0) {
    throw new Error(body.message)
  }
  return body.data
}

async function connect() {
  // 事件消费者通过 SSE 接收授权申请、已处理、回到 Codex 等事件。
  const response = await fetch(`${apiUrl}/api/v1/events/stream`, {
    headers: { Accept: 'text/event-stream' }
  })
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let currentEventName = 'message'
  let currentDataLines = []

  function dispatchCurrentData() {
    if (currentEventName !== 'event' || currentDataLines.length === 0) return
    const dataText = currentDataLines.join('\n')
    const event = JSON.parse(dataText)
    if (event.event_type !== 'approval_requested') return
    const requestId = approvalRequestId(event)
    if (!requestId) return
    console.log(`[approval] ${event.project_name}: ${event.summary}`)
    console.log(`调用 decide("${requestId}", "allow") 或 decide("${requestId}", "deny")`)
  }

  function resetCurrentFrame() {
    currentEventName = 'message'
    currentDataLines = []
  }

  function tryDispatchCurrentData() {
    try {
      dispatchCurrentData()
      resetCurrentFrame()
    } catch (error) {
      if (!(error instanceof SyntaxError)) throw error
    }
  }

  while (true) {
    const { value, done } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    const lines = buffer.split(/\r?\n/)
    buffer = lines.pop() || ''
    for (const line of lines) {
      if (line.startsWith(':')) continue
      if (line === '') {
        // 空行表示 SSE frame 结束；如果 data 跨行，结束时再兜底派发一次。
        tryDispatchCurrentData()
        resetCurrentFrame()
        continue
      }
      if (line.startsWith('event:')) {
        currentEventName = line.slice(6).trim()
        continue
      }
      if (line.startsWith('data:')) {
        currentDataLines.push(line.slice(5).trimStart())
        // v1 通常用一行 data 发送完整 JSON，此处收到后立即尝试派发。
        tryDispatchCurrentData()
      }
    }
  }
}

connect().catch((error) => {
  console.error(error)
  process.exit(1)
})
```

当前 v1 没有插件 token 鉴权，主程序不会把 `decided_by` 当成安全身份认证。`approval_handler` 是插件开发契约和能力展示依据，不是服务端强安全边界。插件仍应只连接本机可信 Local API。

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
- `/api/v1/state/stream` 和 `/api/v1/main-state` 只用于展示状态，不能触发授权交互 UI。
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

不要根据插件 ID、工具原始日志或 `event_type` 自行推导主状态。授权交互属于 `/api/v1/events/stream`，必须由 `approval_requested` 事件触发，不能由展示状态接口触发。

## 开发检查清单

- `plugin.json` 能被 JSON 解析，且 `id` 不与内置插件重复。
- `platforms` 包含当前平台，或留空表示不限平台。
- `command` 在插件安装目录下可执行，或裸命令能被系统 `PATH` 找到。
- 工具插件声明 `kind = tool`、`tool_id` 和 `event_watcher`。
- 通知插件声明 `kind = notification`、`event_consumer`，需要测试通知时同时声明 `notification_test`。
- 具备授权处理能力的消费者必须同时声明 `event_consumer` 和 `approval_handler`；单独声明 `approval_handler` 不是有效运行模式。
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
