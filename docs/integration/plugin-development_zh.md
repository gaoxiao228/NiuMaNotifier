# 插件开发说明

本文档描述 NiumaNotifier 外部工具插件 v1 的接入方式。插件用于监听 Codex、Claude Code、Cursor 等工具的原始运行状态，并把它们转换成统一的 `NiumaEvent` 上报给 NiumaNotifier。

## 插件边界

- 插件是本机受信可执行程序，由用户安装并由 NiumaNotifier 启停。
- NiumaNotifier 不在 v1 中提供强沙箱、签名校验或插件市场。
- 插件不得直接写 SQLite 状态库；所有状态事件必须通过 Local API 上报。
- 外部状态指示器不需要理解插件协议，只消费 `/api/v1/stream` SSE 主状态流。

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

用户插件目录：

```text
~/Library/Application Support/NiumaNotifier/plugins/<plugin-id>/plugin.json
```

第一版 manifest：

```json
{
  "id": "niuma-plugin-codex",
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

字段说明：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `id` | 是 | 插件唯一 ID。 |
| `tool_id` | 是 | 工具 ID，例如 `codex`、`claude_code`、`cursor`。 |
| `display_name` | 是 | UI 展示名称。 |
| `version` | 是 | 插件版本。 |
| `command` | 外部插件必填 | 插件启动命令；相对路径按 `plugin.json` 所在目录解析。 |
| `args` | 否 | 启动参数。 |
| `env` | 否 | 额外环境变量。 |
| `platforms` | 否 | 支持平台，当前使用 `macos`、`windows`、`linux`。空数组表示不限平台。 |
| `capabilities` | 否 | 当前支持 `event_watcher`。 |
| `icon_url` | 否 | 工具图标地址或相对资源路径。 |

## 启动环境变量

NiumaNotifier 启动外部插件时会注入：

| 变量 | 说明 |
| --- | --- |
| `NIUMA_LOCAL_API_URL` | Local API 地址，例如 `http://127.0.0.1:27874`。 |
| `NIUMA_PLUGIN_ID` | 当前插件 ID。 |
| `NIUMA_TOOL_ID` | 当前插件对应的工具 ID。 |
| `NIUMA_STATE_PATH` | 当前实例使用的 SQLite 状态文件路径，仅用于诊断，不应直接写入。 |

## 事件上报

插件通过 Local API 上报事件：

```http
POST /api/v1/plugin-events
Content-Type: application/json
```

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
      "payload_ref": null,
      "created_at": "2026-06-18T12:00:00Z"
    }
  ]
}
```

约束：

- `plugin_id` 必须匹配已发现插件。
- `event.tool` 必须等于插件 manifest 中的 `tool_id`。
- `dedupe_key` 必须稳定，重复扫描同一原始事件时应保持一致。
- 成功上报后，NiumaNotifier 会通过 `StateMutationService` 写入状态并触发 SSE 更新。

## SSE 展示边界

插件只负责生成 `NiumaEvent`。状态优先级、完成态保留时间、阻塞项清理和 SSE 推送由主程序统一处理。

外部状态指示器应只依赖：

```text
GET /api/v1/stream
GET /api/v1/main-state
```

不要根据插件 ID、工具原始日志或 `event_type` 自行推导主状态。
