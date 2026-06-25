# niuma-codex 受管会话设计

## 背景

NiuMaNotifier 现有 Codex 内置插件负责两类能力：

- 监听 Codex session/log 文件，转换为统一 `NiumaEvent`。
- 通过 provider RPC 提供 Codex session 列表和详情。

这次新增的 `niuma-codex` 不替代 watcher。它作为 Rust wrapper 启动 Codex，并在 Codex app-server 与 TUI remote 之间加入 relay，使通过 `niuma-codex` 新启动的交互式 Codex session 可以由 NiuMaNotifier 处理等待输入、授权、追加指令和中断。

## 已确认范围

第一版采用最小 Rust 受管链路：

- 在本项目重新实现 `niuma-codex`，第一版可先暴露为 `niuma codex ...`。
- 后续可增加独立 `niuma-codex` 二进制，复用同一套 Rust 实现。
- 所有原生 Codex 参数都必须能透传。
- 只有新启动的交互式 TUI 会话进入受管模式。
- `resume`、`exec`、`app-server`、`--help`、`--version` 和未知子命令第一版直通真实 Codex，不保证可控。
- 受管会话用 `cwd + 第一条真实用户消息 hash + 10 秒时间窗口 + 唯一候选` 绑定 Codex session 文件。
- 短消息也允许绑定，只要候选唯一。
- 受管 session registry 第一版使用 JSON 文件。
- session 列表、session 详情和等待类事件展示都要体现可交互能力。
- 第一版不在状态栏增加操作按钮。
- 授权复用现有 `/api/v1/approval-decisions`，通过 `channel` 区分 hook 和 relay。
- Codex hook 不因 `niuma-codex` 受管会话做规避，仍按现有机制创建 `hook_proxy` approval。
- `niuma-codex` relay 捕获到 app-server approval request 时主动上报，`channel = niuma_codex_relay`；Local API 负责用仲裁规则避免 hook 与 relay 产生重复可操作授权。
- `request_user_input` 第一版不建 input store，也不由 relay 追加第二条 `InputRequested` 事件；watcher 负责产生等待输入事件，relay 只提供 pending request 和 control overlay。

## 非目标

第一版不做：

- `resume` 会话可控绑定。
- 普通 `codex` 启动的老会话接管。
- 状态栏里的允许、拒绝、输入按钮。
- 通用 interaction store。
- 多工具通用控制抽象。
- 长期审计型 managed session 持久化。

## 整体架构

受管新会话流程：

```text
用户
  -> niuma codex ...
  -> Rust wrapper 启动 codex app-server
  -> Rust relay 夹在 app-server 和 codex --remote 中间
  -> Codex 正常写 session 文件
  -> NiuMaNotifier Codex watcher 扫描 session 文件
  -> matcher 把 session 文件绑定到 managed registry
  -> UI/API 根据绑定结果显示可交互能力
```

职责边界：

- Codex session 文件仍是会话事实来源。
- watcher/provider 继续负责 session 列表、详情、运行状态事件和主状态聚合。
- relay 只负责控制能力，包括 approval、input、send instruction 和 interrupt。
- API 和 UI 不直接理解 socket 或 app-server WebSocket 细节。

## 命令入口与参数透传

第一版入口：

```bash
niuma codex ...
```

后续可增加：

```bash
niuma-codex ...
```

真实 Codex 查找顺序：

1. 优先使用 `NIUMA_REAL_CODEX=/absolute/path/to/codex`。
2. 扫描 `PATH` 中的 `codex`。
3. 跳过 NiuMa wrapper 自身或明显指向 NiuMa 的路径，避免递归调用。
4. 找不到真实 Codex 时直接报错，并提示设置 `NIUMA_REAL_CODEX`。

参数分类只做最小判断：

- 受管模式：无子命令的新建交互式 TUI 会话，例如 `niuma codex --model gpt-5`。
- 直通模式：`resume`、`exec`、`app-server`、`--help`、`--version` 和未知子命令。

直通模式原样调用真实 Codex，不创建 registry，不启动 relay，不显示可控能力。如果真实 Codex 仍写 session 文件，现有 watcher 照常展示。

## 受管启动流程

受管模式执行：

```text
1. 创建 wrapper_session_id。
2. 写入 registry，state = created 或 waiting_first_user_message。
3. 启动 codex app-server --listen real.sock。
4. 启动 Rust relay：relay.sock -> real.sock。
5. 启动 codex --remote relay.sock <原始参数...>。
6. relay 捕获第一条 turn/start 用户输入。
7. registry 写入 first_user_message_hash 和 first_user_message_submitted_at。
8. watcher/provider 扫描 session 文件并执行绑定。
```

退出时：

- 标记 registry `state = exited`。
- 清理 socket。
- 不影响已经写入的 Codex session 文件。

## Managed Session Registry

第一版 registry 是运行时 JSON 文件，不作为长期历史数据。

路径由 `niuma_core::platform::paths` 提供，例如：

```text
<app_data>/managed-sessions/codex.json
```

示例结构：

```json
{
  "version": 1,
  "sessions": [
    {
      "wrapper_session_id": "niuma_codex_abc123",
      "state": "binding_pending",
      "cwd": "/repo",
      "pid": 12345,
      "real_socket": "...",
      "relay_socket": "...",
      "control_socket": "...",
      "started_at": "2026-06-25T10:00:00Z",
      "first_user_message_hash": "sha256...",
      "first_user_message_preview": "第一条用户消息预览",
      "first_user_message_submitted_at": "2026-06-25T10:00:08Z",
      "codex_session_id": null,
      "codex_session_file_path": null,
      "bound_at": null,
      "binding_failure_reason": null
    }
  ]
}
```

状态：

- `created`：wrapper 已启动，socket 准备中。
- `waiting_first_user_message`：TUI 已打开，但还没看到用户第一条真实输入。
- `binding_pending`：已捕获第一条用户消息，等待 watcher/provider 扫到 session 文件。
- `bound`：已绑定到 Codex session id 和 session 文件。
- `ambiguous`：出现多个候选，暂不提供控制能力。
- `exited`：wrapper 退出或控制通道不可用。

写入策略：

- 每次更新前重新读取 JSON。
- 写入使用临时文件和原子 rename。
- 更新按 `wrapper_session_id` 或 `codex_session_id` 精确定位。
- 写入失败不影响 Codex 继续运行，只降级为不可控。
- 定期清理 pid 不存在或 socket 不存在的记录。

## 绑定规则

受管新会话绑定使用：

```text
cwd + first_user_message_hash + 10 秒时间窗口 + 唯一候选
```

候选条件：

1. `managed.cwd` 与 `session.project_path` 规范化后一致。
2. `managed.first_user_message_hash == session.first_user_message_hash`。
3. `abs(session.first_user_message_at - managed.first_user_message_submitted_at) <= 10 秒`。
4. managed session 仍存活。

绑定结果：

- 候选数为 1：写入 `codex_session_id`、`codex_session_file_path`、`bound_at`，并设置 `state = bound`。
- 候选数为 0：保持 `binding_pending`。
- 候选数大于 1：设置 `state = ambiguous`，不提供控制能力。

短消息也允许绑定，只要候选唯一。

当前对外已有 `first_user_message_preview` 和 `first_user_message_at`。绑定需要内部额外解析完整第一条真实用户消息并计算 `first_user_message_hash`。对外仍只暴露 preview，不暴露完整消息。

## Approval 渠道分流

所有可由 NiuMa 处理的授权都进入现有 approval store 和事件流，但通过 `channel` 区分处理方式：

- `hook_proxy`：现有 hook approval proxy。
- `niuma_codex_relay`：`niuma-codex` relay 捕获并回包。

Codex hook 不对 `niuma-codex` 做特殊规避。如果用户安装了 NiuMa Codex hook，Codex 仍会按原有配置调用：

```bash
niuma internal hook codex --source niuma-notifier
```

hook helper 继续按现有流程调用 `/api/v1/approval-requests` 创建 `channel = hook_proxy` 的授权请求，并轮询 `/api/v1/approval-decisions`。这样用户已经安装并信任的 hook 行为保持稳定，不因为使用 `niuma-codex` 启动而改变。

对于同一个受管 session，如果 hook 已经接管了授权，用户仍通过现有 `/api/v1/approval-decisions` 决策。hook helper 轮询到允许或拒绝后向 Codex 输出 hook decision。此时 relay 不需要成为该授权的实际回包渠道。

### Relay 上报

Rust relay 捕获 app-server 发给 TUI 的 server request：

```text
item/commandExecution/requestApproval
```

relay 主动调用：

```http
POST /api/v1/approval-requests
```

创建 request 时传入：

```json
{
  "channel": "niuma_codex_relay",
  "control_ref": {
    "wrapper_session_id": "niuma_codex_abc123",
    "codex_session_id": "session_xxx",
    "relay_request_id": "jsonrpc-id",
    "turn_id": "turn_xxx",
    "item_id": "item_xxx"
  }
}
```

如果当时还没有绑定 `codex_session_id`，可以先为空。绑定成功后不改 `request_id`。

relay 上报前后都必须接受 Local API 的授权仲裁结果：

- 如果没有等价 pending approval，Local API 创建 `channel = niuma_codex_relay` 的新授权请求，后续决策通过 relay control socket 回包。
- 如果已存在等价 `hook_proxy` pending approval，Local API 返回已有 request，并标记 relay 上报被抑制或合并；relay 不创建第二个可操作授权。
- 如果已存在等价 `niuma_codex_relay` pending approval，Local API 返回已有 request，避免 relay 重复上报。

等价判断沿用现有 approval arbitration 思路，至少使用 project path、session 归一化标识、命令或说明文本生成 fingerprint。relay 上报需要提供足够字段，让 Local API 可以和 hook approval 生成同一类 fingerprint。

### ApprovalRequest 模型

`ApprovalRequest` 增加：

```rust
channel: ApprovalChannel
control_ref: Option<ApprovalControlRef>
```

旧客户端不传 `channel` 时默认 `hook_proxy`，保持兼容。

relay approval request id 使用稳定前缀：

```text
codex-relay:<wrapper_session_id>:<turn_id>:<item_id 或 jsonrpc_id>
```

### 统一决策 endpoint

用户允许或拒绝仍调用：

```http
POST /api/v1/approval-decisions
```

服务端以 store 中 `ApprovalRequest.channel` 为准分发，不信任客户端传入的 channel。

分发逻辑：

```text
channel = hook_proxy:
  沿用现有逻辑，更新 store，hook helper 轮询到结果后输出 Codex hook decision。

channel = niuma_codex_relay:
  1. 找到 managed session/control_socket。
  2. 先把 allow/deny 发送给 relay。
  3. relay 通过同一条 TUI WebSocket 回包给 app-server。
  4. 回包成功后，再更新 approval store 为 allowed/denied。
  5. 追加 approval_resolved 事件。
```

如果 control socket 失败：

- 不更新为 allowed/denied。
- 返回业务失败。
- UI 仍可显示 pending，并提示控制通道不可用。

如果同一授权已经由 `hook_proxy` 接管，即使 session 是 `niuma-codex` 受管 session，也仍按 `hook_proxy` 流程处理，不强制切换到 relay channel。

## Request User Input

`request_user_input` 第一版不新增 input store，并且不让 relay 单独追加 `InputRequested` 事件。避免出现 app-server relay 和 session watcher 对同一次等待输入各写一条事件。

relay 捕获：

```text
item/tool/requestUserInput
```

relay 捕获后只记录 pending input request，并通过 control socket 暴露 request id、questions、options 等可回答信息。session watcher 继续从 Codex session JSONL 生成唯一的 `InputRequested` 事件。

UI 或 API 展示 watcher 生成的 `InputRequested` 事件时，根据 managed registry 和 relay pending request 动态叠加可操作交互信息。叠加后的展示形态示例：

```json
{
  "kind": "input",
  "handling": "niuma",
  "actionable": true,
  "request_id": "codex-input:niuma_codex_abc123:42",
  "actions": ["answer"],
  "endpoint": "/api/v1/tool-session-control/answer-input"
}
```

如果 watcher 还没扫描到对应 session 文件事件，但 relay 已经捕获到 pending input，第一版可以只在 session 详情的“当前 pending request”区域展示，不追加事件流事件。这样保持事件事实来源单一。

提交答案接口：

```http
POST /api/v1/tool-session-control/answer-input
{
  "tool": "codex",
  "session_id": "session_xxx",
  "request_id": "codex-input:niuma_codex_abc123:42",
  "answers": {
    "question_id": ["answer"]
  }
}
```

如果 request 已不存在、relay 已退出或 control socket 断开，返回业务失败。

## 发送新指令与中断

发送新指令用于用户主动给当前 Codex session 追加指令，不等同于回答 input request。

```http
POST /api/v1/tool-session-control/send
{
  "tool": "codex",
  "session_id": "session_xxx",
  "content": "继续完成刚才的任务"
}
```

Local API 根据 `session_id` 找 bound registry，再通过 control socket 通知 relay。relay 内部连接 app-server：

- thread idle：调用 `turn/start`。
- thread active 且有 in-progress turn：调用 `turn/steer`。
- 其他状态：返回业务失败。

中断接口：

```http
POST /api/v1/tool-session-control/interrupt
{
  "tool": "codex",
  "session_id": "session_xxx"
}
```

relay 读取当前 loaded thread，找到 in-progress turn 后调用 `turn/interrupt`。没有运行中 turn 时返回业务失败。

## Control Socket 协议

Local API 和 `niuma-codex` 使用本地 JSON Lines control socket。示例：

```json
{"type":"answer_input","request_id":"codex-input:...","answers":{"app_type":["桌面/本地 CLI"]}}
{"type":"send_instruction","content":"继续完成"}
{"type":"interrupt"}
{"type":"approval_decision","request_id":"codex-relay:...","decision":"allow"}
{"type":"requests"}
```

control socket 不暴露给 UI 或外部插件；外部只调用 Local API。

## Session、Event 与 UI

`ToolSessionListItem` 和 `ToolSessionDetail` 增加可选控制信息：

```json
{
  "control": {
    "available": true,
    "provider": "niuma_codex",
    "wrapper_session_id": "niuma_codex_abc123",
    "capabilities": [
      "send_instruction",
      "answer_input",
      "approve",
      "reject",
      "interrupt"
    ]
  }
}
```

普通或未绑定 Codex session：

```json
{
  "control": {
    "available": false
  }
}
```

历史 `NiumaEvent` 不回头改。展示层根据最新 registry 和 approval/input 状态叠加可操作能力：

- `approval_requested` 且 approval store 中 `channel = niuma_codex_relay`：显示允许/拒绝，endpoint 为 `/api/v1/approval-decisions`。
- `input_requested` 且 session 已绑定 `niuma-codex`，relay 仍有 pending input：显示输入或选项，endpoint 为 `/api/v1/tool-session-control/answer-input`。
- session 未绑定或 relay 不可用：显示“请回到 Codex 中操作”。

第一版 UI 范围：

- 事件中心里的 approval/input 操作。
- Codex session 详情里的发送新指令、中断和当前 pending approval/input 操作入口。

不做状态栏按钮。

所有新增界面文案必须补齐：

- 简体中文
- 繁体中文
- 英语
- 日文
- 韩文
- 德文

## API 规范

新增和修改 API 遵守统一响应结构：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

业务失败使用：

```text
HTTP 200 + code != 0
```

协议层 JSON 解析失败使用：

```text
HTTP 400 + 统一响应
```

不使用路径参数，业务参数全部放 query 或 POST body。

修改现有 approval API：

- `POST /api/v1/approval-requests`
- `POST /api/v1/approval-decisions`
- `GET /api/v1/approval-requests`
- `GET /api/v1/approval-decisions`

新增 control API：

- `POST /api/v1/tool-session-control/send`
- `POST /api/v1/tool-session-control/interrupt`
- `POST /api/v1/tool-session-control/answer-input`

失败场景包括：

- session 未绑定。
- session 非 `niuma-codex` 管理。
- control socket 不存在。
- request_id 不存在。
- Codex thread 状态不允许该操作。
- relay 回包失败。

这些都返回业务失败，不返回 4xx。

错误码第一版沿用项目现有 `ApiErrorCode::BusinessValidation`、`System`、`ParameterFormat`。不为了本功能大改错误码体系。

## 测试范围

Rust 单元测试：

- Codex 参数分类：受管/直通。
- 真实 Codex 路径解析，避免递归。
- registry JSON 读写、原子更新、清理。
- 第一条消息 normalize/hash。
- `cwd + hash + 10s + unique` 绑定规则。
- ambiguous 不绑定。
- approval channel 分发。
- hook 与 relay 上报等价 approval 时通过 fingerprint 仲裁，避免重复可操作授权。
- control socket 协议解析。

集成或模拟测试：

- relay 捕获 approval request 后创建 `channel = niuma_codex_relay` 的 approval request。
- `/api/v1/approval-decisions` 对 relay channel 先调用 control socket，成功后更新 store。
- watcher 生成 `InputRequested` 事件，relay pending input 只叠加可操作信息，不生成第二条事件。
- `answer-input` 通过 control socket 成功回包。
- session list/detail 带 control 信息。
- 未绑定 session 不显示可交互能力。

前端测试：

- 事件中心展示 approval 按钮。
- 事件中心展示 input 操作。
- session 详情展示发送新指令和中断。
- 不可控 session 不展示控制按钮。
- 新增文案所有语言齐全。

## 风险与验证点

- 需要实测 hook 已安装时，Codex app-server 是否还会向 TUI/relay 发出同一 approval request；如果会发出，Local API 必须用 fingerprint 仲裁抑制重复授权。
- 需要实测 Rust relay 对 Codex app-server WebSocket frame 的解析和回包是否与当前 Codex 版本一致。
- 需要确认 `codex --remote relay.sock <交互式参数>` 对常用交互参数的兼容性。
- 绑定窗口默认 10 秒，后续可根据实际日志调整。
- JSON registry 并发写需要严格使用原子写入，避免坏文件导致控制能力整体失效。
