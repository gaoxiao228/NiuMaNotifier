# 远程访问一键诊断设计

## 背景

当前远程访问链路已经包含账号登录、设备绑定、设备 WebSocket、外部 Web 客户端连接、Relay、WebRTC、Plain RPC、远程会话列表等多个环节。用户看到“设备离线”“通道为空”“连接中”“无法读取远程会话”等现象时，需要快速判断问题断在哪一层。

本设计新增两个诊断入口：

- 外部 Web 客户端控制台的一键诊断，用于验证“当前浏览器能否访问这台设备”。
- 本机 NiumaNotifier 远程访问页的一键诊断，用于验证“本机是否准备好被外部客户端连接”。

第一版只做只读诊断和结构化结果展示，不做自动修复、不做历史记录、不做导出。

## 设计原则

1. 外部 Web 客户端负责完整远程链路诊断。
2. 本机 NiumaNotifier 只诊断本机可被访问条件，不主动模拟外部客户端连接自己。
3. Relay/WebRTC 诊断必须通过业务 RPC ping 证明链路可用，不能只依赖 socket open 或 DataChannel open。
4. WebRTC 失败但 Relay 可用时，整体结果应为降级可用，不应判定为失败。
5. 诊断报告使用统一结构，方便两个界面复用展示组件和后续扩展。

## 方案选择

采用主动链路诊断方案：

- 外部 Web 客户端点击诊断时，如果没有连接则建立连接；如果已有连接则复用连接；如果旧连接处于错误状态则关闭后重建。
- 本机 NiumaNotifier 点击诊断时，不建立远程 connection，只检查配置、绑定、服务端可达性、设备 socket 状态、当前连接状态和本机业务能力。

不采用纯状态读取方案，因为它只能展示现有状态，不能证明链路真实可用。

不采用本机端到端自测方案，因为本机同时扮演 device 和 client 会让语义复杂，而且无法代表真实外部浏览器的网络环境。

## 统一诊断模型

```ts
export type DiagnosticStepStatus = 'passed' | 'failed' | 'skipped' | 'running'

export type DiagnosticSeverity = 'info' | 'warning' | 'error'

export type DiagnosticStep = {
  key: string
  title: string
  status: DiagnosticStepStatus
  severity?: DiagnosticSeverity
  duration_ms?: number
  message?: string
  suggestion?: string
  detail?: unknown
}

export type DiagnosticReport = {
  scope: 'web_client' | 'local_agent'
  overall: 'passed' | 'degraded' | 'failed'
  summary: string
  started_at: string
  finished_at?: string
  steps: DiagnosticStep[]
}
```

字段说明：

- `message` 描述本步骤发生了什么。
- `suggestion` 给出用户下一步处理建议。
- `detail` 保存开发排查信息，第一版可以默认折叠或暂不展示。
- `overall` 由核心步骤结果计算，不由 UI 手写判断。

## 外部 Web 客户端诊断

入口位于设备控制台页，和“连接”按钮放在同一区域。设备列表页不建立连接，因此第一版不在设备列表页增加诊断入口。

诊断步骤：

1. `device_online`：检查设备是否在线。
2. `connection_create`：创建或复用 connection。
3. `device_accept`：等待设备通过 signaling 返回 accept。
4. `relay_open`：检查 Relay WebSocket 是否 open/ready。
5. `relay_rpc_ping`：通过 Relay 发送 `rpc.ping`。
6. `webrtc_offer_answer`：完成 WebRTC offer/answer 流程。
7. `webrtc_data_channel`：检查 DataChannel 是否 open。
8. `webrtc_rpc_ping`：通过 WebRTC 发送 `rpc.ping`。
9. `session_project_groups`：通过当前最佳通道请求 `/api/v1/session_project_groups?tool=codex`。

连接规则：

- 当前没有连接：点击诊断会建立连接。
- 当前已有可用连接：点击诊断复用当前连接。
- 当前正在连接：禁用诊断按钮，避免并发。
- 当前连接错误：关闭旧连接，重新建立一次诊断连接。

结果规则：

- `passed`：Relay ping、WebRTC ping、远程会话接口均通过。
- `degraded`：Relay ping 和远程会话接口通过，但 WebRTC 失败。
- `failed`：设备未响应、Relay/WebRTC 都不可用、或远程会话接口失败。

典型总结：

- 全部通过：远程访问正常，当前优先使用 WebRTC。
- Relay 通过、WebRTC 失败：远程访问可用，但直连失败，当前降级 Relay。
- 通道通过、会话接口失败：连接链路正常，但本机业务接口失败。
- 设备未响应：服务端能创建连接，但本机没有 accept，重点检查本机 device socket、远程控制开关或忙碌状态。

## 本机 NiumaNotifier 诊断

入口位于设置页的远程访问面板，放在保存按钮附近或远程状态区域下方。

诊断步骤：

1. `server_url`：检查 server_url 格式和规范化结果。
2. `remote_access_enabled`：检查远程访问开关。
3. `remote_control_enabled`：检查远程控制开关。
4. `account_bound`：检查账号绑定摘要。
5. `device_bound`：检查设备绑定摘要。
6. `credential_present`：检查本地 credential/device_token 是否存在。
7. `server_health`：请求服务端 `/api/v1/health`。
8. `device_socket_status`：读取当前 agent status。
9. `active_connection`：读取 active_connection_id、available_transports、selected_transport。
10. `local_session_project_groups`：直接调用本机现有 service/helper，确认本机能生成会话项目组数据。

本机诊断不主动创建远程 connection。没有 active connection 是正常状态，应显示“当前无外部客户端连接”，不判定为失败。

结果规则：

- `passed`：配置完整、已绑定、凭据存在、服务端可达、device socket online、本机业务接口正常。
- `degraded`：本机大体可用，但远程控制关闭，或当前无外部连接等非致命状态。
- `failed`：未绑定、凭据缺失、server_url 不可达、device socket offline/error、或本机业务接口失败。

## UI 展示

第一版使用“总结 + 步骤列表”的轻量展示，不做复杂日志面板。

外部 Web 示例：

```text
诊断结果：远程访问可用，当前使用 WebRTC

设备在线              通过
连接创建              通过  32ms
设备响应              通过  104ms
Relay 业务 Ping       通过  55ms
WebRTC 业务 Ping      通过  180ms
远程会话接口          通过  41ms
```

本机示例：

```text
诊断结果：本机远程访问已就绪，等待外部客户端连接

配置检查              通过
绑定状态              通过
凭据检查              通过
服务端健康检查        通过  18ms
设备连接状态          通过  online
当前远程连接          跳过  当前无外部客户端连接
本机会话接口          通过  3 个项目组
```

## 代码边界

外部 Web：

- 扩展 `remoteDeviceSessionController`，新增或整理 `runDiagnostics()`。
- 复用已有 connection、relay、webrtc、Plain RPC、session group 读取逻辑。
- UI 只负责触发诊断和渲染 `DiagnosticReport`，不直接拼接连接流程。

本机 NiumaNotifier：

- 新增 Tauri command：`run_remote_access_diagnostics`。
- Rust 侧复用 store、credential store、remote agent status 和本机会话 service/helper。
- 前端设置页只调用 command 并展示报告。

共享：

- 两端可各自维护类型定义，字段语义保持一致。
- 多语言覆盖 `zh-CN`、`zh-TW`、`en`、`ja`、`ko`、`de`。

## 错误处理

- 单个步骤失败后，依赖它的后续步骤标记为 `skipped`。
- 非致命失败用 `degraded`，例如 WebRTC 失败但 Relay 和业务接口可用。
- 失败信息面向用户，保留开发细节到 `detail`。
- 诊断过程中的超时应有明确 message，例如“等待 connection.accept 超时”。

## 测试策略

外部 Web：

- 当前无连接时，诊断会创建连接。
- 当前已有连接时，诊断复用连接。
- 连接错误时，诊断重建连接。
- Relay ping 通过、WebRTC ping 失败时，overall 为 `degraded`。
- 通道通过但 session group 失败时，overall 为 `failed`。

本机 NiumaNotifier：

- 未绑定时返回 failed，并提示登录绑定。
- 凭据缺失时返回 failed，并提示重新绑定。
- 服务端 health 失败时返回 failed。
- 无 active connection 时步骤为 skipped/info，不影响本机就绪判断。
- 本机会话 service/helper 失败时返回 failed。

## 非目标

第一版不做：

- 自动修复。
- 自动重新登录。
- 自动清理绑定。
- 诊断历史记录。
- 诊断报告导出。
- 设备列表页建立诊断连接。
- 本机模拟外部客户端做端到端自测。

