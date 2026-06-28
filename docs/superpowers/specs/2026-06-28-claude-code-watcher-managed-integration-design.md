# Claude Code watcher 与受管会话集成设计

日期：2026-06-28

## 背景

NiuMaNotifier 现有 Codex 内置插件使用 session/log 文件 watcher 作为基础事件源，并通过 provider RPC 提供 session 列表和详情。`niuma codex` 在此基础上提供受管控制能力，包括续写、中断、授权和等待输入。

Claude Code 集成需要达到同级体验，但不能把 Claude hooks 作为唯一事件源。hooks 只是一种增强方式；即使用户没有安装 hooks，NiuMaNotifier 也必须能从 Claude Code 的原始 session 文件中触发基础事件流。

## 已验证事实

在 Claude Code `2.1.195` 上使用无 hooks 的会话验证了以下行为：

- `~/.claude/projects/<project-key>/<session-id>.jsonl` 会在会话运行期间实时增量写入，不需要等 Claude 进程退出。
- 纯文本会话会写入 `user`、`assistant thinking`、`assistant text` 和 `last-prompt`。
- 成功工具调用会写入 `assistant.content[].type = "tool_use"`，随后写入配对的 `user.content[].type = "tool_result"`，并包含 `tool_result.tool_use_id` 与 `is_error = false`。
- 失败工具调用会写入 `tool_result.is_error = true`，内容包含退出码或错误说明。
- 长时间运行的 Bash 工具在执行期间已经写入 `tool_use`，直到工具结束才追加 `tool_result`。
- 交互式权限确认等待期间已经写入 `tool_use`，但尚未写入 `tool_result`。用户取消后会追加 `tool_result.is_error = true`，内容说明用户拒绝了工具使用。

这些事实说明 Claude session watcher 能可靠承担基础事件流。但单靠 JSONL 无法区分“长任务正在运行”和“权限弹窗正在等待用户确认”，二者在文件层都表现为存在未配对 `tool_use`。

同版本 `claude --help` 已确认存在 `--session-id <uuid>`、`-r/--resume [value]`、`--remote-control [name]`、`--input-format stream-json` 和 `--output-format stream-json`。第一版只使用已验证行为；`--remote-control` 和 active turn 流式控制作为后续增强点，不作为 watcher 基础功能前提。

## 目标

- 新增 Claude Code 内置工具插件，基础事件流必须由 session watcher 保证。
- 不依赖 hooks 也能展示 Claude session 列表、详情、运行态和基础通知。
- 对通过 `niuma claude` 启动的会话提供受管控制入口，包括已验证场景下的续写、中断、权限处理和等待输入处理；未验证或不稳定的动作不暴露为可用 action。
- hooks 只作为可选增强源，用于补充更精确的权限、通知或会话启动事件。
- 保持现有 Local API 统一 envelope 规范；SSE 和 provider RPC 继续作为协议例外。

## 非目标

第一版不做：

- 直接接管任意已经运行的原生 Claude TUI 进程。
- 仅依靠 hooks 实现事件流。
- 在非受管原生 Claude 会话中提供可操作批准、拒绝、续写或中断。
- 新增长期审计型受管会话数据库表。
- 重写现有 session-control API 路由。

## 整体架构

Claude Code 集成采用三层结构：

```text
Claude Code session JSONL
  -> ClaudeSessionWatcher
  -> NiumaEvent + session provider snapshot/detail

niuma claude 托管启动
  -> wrapper registry + control channel
  -> send instruction / interrupt / approval / input

Claude hooks
  -> 可选增强事件
  -> 不作为基础事件依赖
```

职责边界：

- session JSONL 是基础事实来源。
- watcher/provider 负责基础事件、session 列表和详情。
- 受管 wrapper 负责控制能力和可操作交互。
- hooks 只补充更精确、更实时的工具原生事件，不改变 watcher 的必要性。

## 内置插件

新增内置工具插件：

```json
{
  "id": "builtin-claude-code",
  "kind": "tool",
  "tool_id": "claude_code",
  "display_name": "Claude Code",
  "version": "0.1.0",
  "command": "niuma-claude-code-plugin",
  "args": [],
  "platforms": ["macos", "windows", "linux"],
  "capabilities": [
    "event_watcher",
    "tool_session_list_provider",
    "tool_session_detail_provider"
  ],
  "source": "builtin"
}
```

第一版可以先支持 macOS 路径发现；manifest 仍声明跨平台，运行时按平台路径能力降级。

## Runtime 文件结构

建议新增 crate：

```text
builtin-plugins/claude-code-runtime/
  Cargo.toml
  src/main.rs
  src/lib.rs
  src/claude/mod.rs
  src/claude/discovery.rs
  src/claude/session_event_cursor.rs
  src/claude/session_file_index.rs
  src/claude/session_protocol/current.rs
  src/claude/session_repository.rs
  src/claude/session_watcher.rs
  src/session_messages.rs
  src/session_provider.rs
```

实现上优先复用 Codex runtime 的模式：

- `notify` 监听目录变化。
- 活跃文件轮询弥补 macOS 追加写入事件不稳定。
- 低频目录扫描兜底发现最近 session 文件。
- provider 与 watcher 共享 repository，避免重复解析。

## Claude 路径发现

默认路径：

```text
~/.claude/projects/**/*.jsonl
~/.claude/projects/**/subagents/*.jsonl
```

项目 key 目录形态为路径转义，例如：

```text
~/.claude/projects/-Users-gaopengcheng-Code-NiuMa-NiuMaNotifier/<session-id>.jsonl
```

session 文件自身包含 `cwd` 和 `sessionId`，provider 以文件内容为准，不从目录名反推项目路径。

## Watcher 事件映射

基础映射：

| Claude JSONL 形态 | Niuma 事件 |
| --- | --- |
| 新 session 文件或首条 `user` | `session_started` |
| `assistant` 中 `thinking` | `session_activity` |
| `assistant` 中 `text` | `assistant_message_completed` |
| `assistant` 中 `tool_use` | `session_activity`，并记录 pending tool |
| `user` 中 `tool_result.is_error = false` | `session_activity`，清理对应 pending tool |
| `user` 中 `tool_result.is_error = true` | `task_failed` 或工具拒绝/失败活动 |
| 未配对 `tool_use` 持续存在 | `pending_tool` 运行态 |
| 长时间无更新且仍 pending/running | `stale` |

第一版不新增公开 `EventType` 时，`pending_tool` 可以只作为 session provider 的运行态和事件 summary 表达；如果现有枚举无法准确表达，应新增 `EventType::PendingTool` 或等价类型，并同步文档。

## Pending tool 与权限等待

Claude JSONL 中，长任务执行中和权限弹窗等待都表现为：

```text
assistant tool_use 已写入
对应 user tool_result 尚未写入
```

因此 watcher 不能仅凭 JSONL 把该状态标记为可操作 `waiting_approval`。第一版规则：

- 对未配对 `tool_use` 立即标记 session 为 `running` 或 `pending_tool`。
- 如果 tool 名称和 input 明显涉及文件写入、Bash、MCP 删除等高风险操作，可以在 summary 中提示“可能等待权限或工具结果”。
- 不生成 `interaction.handling = "niuma"` 的可操作 approval。
- 如果后续出现 `tool_result.is_error = true` 且内容包含用户拒绝、blocked、permission 等关键词，生成失败或拒绝类事件并清 pending。

可操作审批只由受管控制或 hooks/stream 增强产生。

## Session provider

`tool_session_list_provider` 返回 Claude session 列表，关键字段：

- `tool = "claude_code"`
- `session_id = Claude sessionId`
- `project_path = cwd`
- `project_name = cwd` 的末级目录
- `file_path = transcript JSONL path`
- `modified_at = session 文件 mtime`
- `first_user_message_preview`
- `first_user_message_at`
- `status`
- `control`

详情 provider 将 JSONL 转为 `ToolSessionDetail.messages`：

| Claude 内容 | 归一化角色 |
| --- | --- |
| `type = "user"` 且 content 为字符串 | `user` |
| `assistant content[].text` | `assistant` |
| `assistant content[].thinking` | `assistant` 或 `event`，默认详情中可折叠展示 |
| `assistant content[].tool_use` | `tool_call` |
| `user content[].tool_result` | `tool_result` |
| `attachment` / `file-history-snapshot` / `ai-title` | 第一版可忽略或作为 `event` |

分页、cursor 和 stale 文件处理沿用 Codex provider 的策略。

## 受管启动

新增命令：

```bash
niuma claude ...
niuma claude-sessions
niuma claude-send <wrapper_session_id> <message>
niuma claude-interrupt <wrapper_session_id>
```

真实 Claude 查找顺序：

1. `NIUMA_REAL_CLAUDE=/absolute/path/to/claude`
2. `PATH` 中的 `claude`
3. 跳过 NiuMa wrapper 自身，避免递归

受管启动第一版：

```text
1. 创建 wrapper_session_id。
2. 生成 Claude session_id，并通过 --session-id 传给真实 claude。
3. 写入 managed registry，state = started。
4. 启动真实 Claude。
5. watcher 发现对应 JSONL 后绑定 transcript path。
6. provider snapshot 增加 control channel。
7. 退出时标记 exited。
```

## Managed registry

路径：

```text
<app_data>/managed-sessions/claude-code.json
```

结构：

```json
{
  "version": 1,
  "sessions": [
    {
      "wrapper_session_id": "niuma_claude_xxx",
      "state": "bound",
      "cwd": "/repo",
      "pid": 12345,
      "control_socket": "/tmp/niuma-claude/xxx/control.sock",
      "started_at": "2026-06-28T00:00:00Z",
      "claude_session_id": "uuid",
      "transcript_path": "~/.claude/projects/.../uuid.jsonl",
      "bound_at": "2026-06-28T00:00:02Z",
      "binding_failure_reason": null
    }
  ]
}
```

状态：

- `started`
- `binding_pending`
- `bound`
- `exited`
- `unavailable`

## Control channel

Claude 受管通道使用通用 session control channel：

```text
niuma_claude_managed:<wrapper_session_id>
```

channel 示例：

```json
{
  "id": "niuma_claude_managed:niuma_claude_xxx",
  "provider": "niuma_claude",
  "kind": "managed_process",
  "available": true,
  "capabilities": [
    "send_instruction",
    "interrupt"
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
    }
  ],
  "unavailable_reason": null
}
```

第一版控制能力分级：

- 对仍在前台 TUI 中等待权限的会话，wrapper 可以通过 PTY 发送按键实现拒绝或中断，但批准/永久批准需要谨慎设计，默认不做静默自动批准。
- 对已退出或 idle 的会话，续写可通过 `claude --resume <session_id> <message>` 实现新进程续写。
- 对 active turn 的续写如果没有稳定协议，不承诺第一版支持；UI 应根据 channel capability 展示。

后续如果 Claude `--remote-control` 提供稳定控制协议，再把 channel `kind` 升级或新增为 `remote_control`。

## Hooks 增强

hooks 插件可以作为可选增强包提供：

- `SessionStart`：加速 registry 绑定。
- `PreToolUse` / `PermissionRequest`：补充精确 approval request。
- `PermissionDenied`：补充拒绝原因。
- `Notification`：补充“Claude needs your permission”等通知。
- `Stop` / `SessionEnd`：补充退出事件。

约束：

- hooks 不影响 watcher 的基础功能。
- hooks 上报必须使用稳定 `dedupe_key`，避免和 watcher 事件重复。
- hooks 不能向 stdout 写普通日志。
- hooks 不可用时插件不降级为失败，只少一层增强。

## API 契约

不新增基础 session-control 路由，继续复用：

```http
POST /api/v1/session-control/send-instruction
POST /api/v1/session-control/interrupt
POST /api/v1/session-control/answer-input
POST /api/v1/approval-decisions
```

请求继续使用 `tool`、`session_id`、`channel_id`，禁止路径动态参数。业务失败返回 `HTTP 200 + 非 0 code`，协议层 JSON 或参数类型错误返回 `HTTP 400 + 统一 envelope`。

第一版如果 Claude channel 不支持某个动作，返回业务失败：

```json
{
  "code": 100101,
  "message": "Claude Code control channel 当前不支持 active turn 续写",
  "data": {
    "channel_id": "niuma_claude_managed:niuma_claude_xxx"
  }
}
```

## UI 能力分级

UI 应区分三种能力：

| 启动方式 | 事件流 | 列表/详情 | 权限状态 | 控制 |
| --- | --- | --- | --- | --- |
| 原生 `claude` | 支持 | 支持 | 只能推断 pending tool | 不支持 |
| 原生 `claude` + hooks | 支持 | 支持 | 更精确 | 不支持 |
| `niuma claude` | 支持 | 支持 | 可操作能力按 channel 暴露 | 支持可用 actions |

前端不应从工具名称硬编码能力，应读取 `session.control.channels[].actions[]`。

## 测试计划

Runtime 单元测试：

- 解析纯文本 session。
- 解析成功 `tool_use/tool_result` 配对。
- 解析失败 `tool_result.is_error = true`。
- 未配对 `tool_use` 形成 pending tool。
- 用户取消权限后清 pending。
- session snapshot 排序、分页和 stale 文件处理。

集成测试：

- 构造临时 Claude home，验证 watcher 增量扫描。
- 验证长任务执行中 watcher 先看到 `tool_use`，结束后看到 `tool_result`。
- provider stdout 只输出 JSON Lines RPC。
- 监听关闭后 snapshot 返回空列表，detail 返回 provider disabled。

手动验证：

- 原生 Claude 无 hooks 会话仍出现在 NiuMa session 列表。
- 交互式权限等待时 UI 显示 pending tool 或可能等待权限。
- 用户拒绝权限后 UI 清理 pending 并显示失败/拒绝结果。
- `niuma claude` 启动的会话显示 control channel。

## 风险与缓解

- Claude JSONL 格式可能变化：实现协议族探测，未知格式跳过并记录诊断，不阻塞插件。
- 权限等待无法仅靠 watcher 精确识别：使用 pending tool 保守表达，可操作能力交给受管控制或 hooks。
- active turn 续写协议不稳定：第一版只暴露已验证的 actions，未验证能力不展示。
- `~/.claude/projects` 文件数量较多：沿用 Codex 的最近文件限制、活跃文件 TTL 和低频全量兜底。
- 交互式 PTY 控制脆弱：默认优先中断、resume 续写；批准/永久批准需要单独验证后再开放。

## 结论

Claude Code watcher 可以作为与 Codex watcher 同级的基础事件源，覆盖 session 发现、活动、工具调用、工具结果、失败、完成、实时 pending 和 stale 处理。它不能单独提供与 Codex relay 等价的精确可操作权限处理，因此最终设计必须保持 watcher-first，同时通过 `niuma claude` 和可选 hooks 提供增强控制能力。
