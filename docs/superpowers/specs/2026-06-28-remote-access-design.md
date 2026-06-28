# NiuMaNotifier 远程访问与完整远程控制设计

## 背景

当前 NiuMaNotifier 的 Local API 默认只监听 `127.0.0.1:27874`，面向本机可信调用方，不内置公网鉴权。外部系统可以通过 SSE 读取主状态，但只有在用户显式修改监听地址时才可能被局域网或外网访问。直接把 Local API 暴露到公网会扩大攻击面，也会绕开当前“本机状态源由 `NiumaStore`、`StateMutationService`、`RuntimeEventBus` 维护”的架构边界。

本设计新增一套远程访问层，让外网客户端可以发现本机 NiumaNotifier、判断在线和可用状态，并在连接建立后执行完整远程控制。远程层不把 Local API 做成公网镜像，而是通过受控的端到端加密 RPC 暴露明确能力。

## 目标

- 本机服务不监听公网端口，只主动连接外网服务端。
- 外网客户端通过账号登录外网服务端，发现同账号设备。
- 登录账号后，远程访问和远程控制默认开启，不弹首次提醒。
- 支持 Web 网页端作为第一版外网客户端。
- 支持完整远程控制：查看状态、会话列表、会话详情、发送指令、中断任务、处理授权、回答输入。
- 优先使用 WebRTC DataChannel 直连；无法直连时通过服务端 relay 转发密文。
- 服务端负责账号、设备目录、在线状态、信令和 relay，但不可见会话正文、指令正文、授权正文。
- 允许用户自托管远程服务端；默认使用官方服务。
- 远程能力必须复用现有主状态、Local API、session-control 和状态转移边界，不直接修改主状态。

## 非目标

- 第一版不做多人共享设备。
- 第一版不做临时访问链接。
- 第一版不做团队组织、RBAC、计费或多区域部署。
- 第一版不做远程 shell、文件浏览或文件传输。
- 第一版不让服务端解析业务正文或保存业务正文审计。
- 第一版不把本机 Local API 直接代理成公网 HTTP API。

## 总体架构

```text
Web 控制台
  - 登录账号
  - 查看设备列表
  - 建立远程连接
  - 通过 E2EE RPC 控制本机
        |
        | HTTPS / WSS / WebRTC 信令 / 密文 relay
        v
外网服务端
  - 账号认证
  - 设备目录
  - 在线状态
  - 连接权限
  - WebRTC 信令
  - relay fallback
        |
        | WSS 设备控制连接 / WebRTC DataChannel / 密文 relay
        v
本机 NiumaNotifier Remote Agent
  - 主动连接服务端
  - 上报心跳和能力
  - 承载 E2EE RPC 服务端
  - 复用本机 Local API / 主状态服务 / session-control
```

Remote Agent 是本机新增的远程编排层。它不解析工具原始事件，不绕过 `StateMutationService` 写状态，也不直接修改 `NiumaStore`。远程读写能力必须进入当前已有的主状态、会话控制和交互处理边界。

## 服务端选择与自托管

远程服务默认指向官方服务。用户可以在设置页切换到自托管服务端地址，例如 `https://remote.example.com`。

服务端的登录方式、token 模型、数据存储、Redis 状态、WebSocket 协议和 Docker 自托管部署详见 [NiuMaNotifier 远程服务端设计](./2026-06-28-remote-server-design.md)。

切换服务端等价于切换远程域：

- 官方账号与自托管账号不互通。
- 官方设备列表与自托管设备列表不互通。
- 官方设备 token 与自托管设备 token 不互通。
- 切换服务端时，本机断开旧服务端连接。
- 切换服务端后，本机需要重新登录、重新注册设备并获取新的设备 token。
- Web 控制台必须访问同一个服务端，才能看到对应设备。

自托管服务端第一版需要提供：

- HTTPS 和 WSS。
- 账号登录、刷新 token、退出登录。
- 设备注册、设备列表、设备解绑。
- Web 控制台静态资源或部署入口。
- WebRTC 信令。
- relay fallback。
- TURN 配置或外部 TURN 服务配置。
- 管理员初始化方式，例如首个管理员账号或环境变量创建管理员。

## 登录与默认开启策略

本机登录远程账号成功后：

```text
登录成功
  -> 自动注册或绑定当前设备
  -> 自动开启远程访问
  -> 自动开启远程控制
  -> RemoteAgent 建立到服务端的 WSS 连接
  -> Web 端同账号登录后可直接控制该设备
```

首次登录不弹框、不阻断、不额外确认。设置页需要提供可见的状态与撤销入口：

- 当前服务端地址。
- 当前账号。
- 当前设备名。
- 远程访问开关。
- 远程控制开关。
- 退出账号。
- 解绑设备。
- 查看远程审计日志。

## 本机组件

### RemoteAgent

RemoteAgent 运行在 Rust 后端侧，负责：

- 读取远程服务配置和设备凭证。
- 使用设备 token 建立服务端 WSS 设备控制连接。
- 上报设备在线心跳、应用版本和能力摘要。
- 接收 Web 客户端连接邀请。
- 参与 WebRTC offer、answer、ICE candidate 信令交换。
- 在 WebRTC 失败时切换到 relay fallback。
- 为每个远程连接创建加密 RPC 会话。

RemoteAgent 不放在 Tauri 前端层，因为它需要持有设备 token、建立长期连接、执行远程控制和写审计日志。

### RemoteTransport

RemoteTransport 抽象底层通道：

- WebRTC DataChannel。
- 服务端 relay fallback。

RPC 层只看到有序消息帧，不关心底层是否直连。relay fallback 只转发端到端加密后的帧。

### RemoteRpcRouter

RemoteRpcRouter 负责：

- 解密后的 RPC envelope 解析。
- 按 `method` 分发到本机能力。
- 调用权限校验。
- 调用审计日志。
- 把本机执行结果编码为 RPC response。

远程协议是产品契约，Local API 是本机实现细节。Web 端不直接调用 `/api/v1/session-control/send-instruction` 等本地路径。

### RemotePermissionGuard

RemotePermissionGuard 在每次 RPC 执行前检查：

- 本机远程访问总开关是否开启。
- 本机远程控制开关是否开启。
- 当前连接账号是否匹配设备绑定账号。
- 当前 RPC 方法是否需要控制权限。
- 设备 token 或连接 token 是否仍有效。
- 请求是否超时、重复或已被取消。

服务端负责“谁能连”，本机端负责“这台机器现在是否允许被控制”。两边都通过后才执行控制动作。

### RemoteAuditLog

RemoteAuditLog 记录远程控制行为：

- 时间。
- 服务端域。
- 账号 ID。
- 客户端 ID。
- 设备 ID。
- RPC 方法名。
- 请求 ID。
- 执行结果。
- 失败错误码。

默认不记录指令正文、授权命令正文、等待输入正文，避免本机日志成为敏感内容副本。需要排查时可以后续设计显式的脱敏诊断模式。

## 服务端组件

### AuthService

负责账号登录、刷新 token、退出登录、退出所有 Web 会话、吊销 token。

### DeviceRegistry

保存账号与设备关系、设备在线状态、设备能力、设备 token 和最后心跳时间。

### SignalingService

负责 Web 客户端和 RemoteAgent 之间的 WebRTC 信令转发：

- offer。
- answer。
- ICE candidate。
- 连接取消。
- 连接超时。

### RelayService

当 WebRTC DataChannel 不可用时，RelayService 按连接 ID 转发密文帧。服务端只理解连接元数据、帧序号和路由目标，不解析 RPC payload。

### ConnectionPolicy

负责连接级权限判断：

- 账号是否有效。
- 客户端登录态是否有效。
- 设备是否属于该账号。
- 设备是否在线。
- 连接 token 是否过期。
- 账号是否被禁用或风控。

## Web 控制台组件

### DeviceListView

登录后展示设备列表，包括在线状态、设备名、版本、能力摘要和最后在线时间。

### RemoteSessionClient

负责创建远程会话：

- 获取连接 token。
- 建立 WebSocket 信令连接。
- 协商 WebRTC DataChannel。
- 失败时切换 relay fallback。
- 维护端到端加密会话密钥。

### RemoteRpcClient

提供类型化远程方法：

- `stateGet()`
- `stateSubscribe()`
- `sessionList()`
- `sessionDetail()`
- `sessionSendInstruction()`
- `sessionInterrupt()`
- `interactionListPending()`
- `interactionAnswerInput()`
- `interactionDecideApproval()`

### RemoteConsoleView

负责远程控制界面：

- 主状态。
- 会话列表。
- 会话详情。
- 指令输入框。
- 中断按钮。
- 授权处理面板。
- 等待输入回答面板。

远程处理 approval 时，Web 端必须展示完整授权内容，不能只展示“同意/拒绝”按钮。

## 远程 RPC 协议

远程 RPC 不复用 HTTP 路径，统一使用 envelope。

请求：

```json
{
  "version": 1,
  "type": "request",
  "id": "req-001",
  "method": "session.send_instruction",
  "params": {}
}
```

响应：

```json
{
  "version": 1,
  "type": "response",
  "id": "req-001",
  "ok": true,
  "data": {}
}
```

错误：

```json
{
  "version": 1,
  "type": "response",
  "id": "req-001",
  "ok": false,
  "error": {
    "code": "permission_denied",
    "message": "远程控制未开启"
  }
}
```

推送事件：

```json
{
  "version": 1,
  "type": "event",
  "topic": "state",
  "data": {}
}
```

第一版方法：

| 方法 | 能力 | 说明 |
| --- | --- | --- |
| `device.get_health` | 只读 | 读取本机远程代理健康状态。 |
| `device.get_capabilities` | 只读 | 读取远程支持的协议版本和功能。 |
| `state.get` | 只读 | 读取当前主状态快照。 |
| `state.subscribe` | 只读 | 订阅主状态变化，建连后先推送首帧快照。 |
| `session.list` | 只读 | 读取会话列表。 |
| `session.detail` | 控制开关开启 | 读取会话详情和当前可控动作。 |
| `session.send_instruction` | 控制开关开启 | 向受控会话发送指令。 |
| `session.interrupt` | 控制开关开启 | 中断受控会话当前 turn。 |
| `interaction.list_pending` | 控制开关开启 | 读取待处理授权或输入。 |
| `interaction.answer_input` | 控制开关开启 | 回答等待输入。 |
| `interaction.decide_approval` | 控制开关开启 | 处理授权请求。 |

订阅类请求需要有显式取消机制，例如 `subscription.cancel` 或在连接关闭时自动取消。RPC 请求必须有超时，超时后不允许悬挂执行。

## 端到端加密

第一版采用临时会话密钥：

```text
Web 登录服务端
  -> 获取短期 connection_token
RemoteAgent 已通过设备 token 连接服务端
  -> 双方通过服务端交换临时公钥
  -> 协商会话密钥
  -> 后续 RPC payload 全部加密
  -> 断线后重新协商
```

服务端可见：

- 账号 ID。
- 设备 ID。
- 客户端 ID。
- 连接 ID。
- 在线状态。
- 连接建立、失败、断开。
- relay 流量统计。

服务端不可见：

- 会话详情正文。
- 指令正文。
- 授权命令正文。
- 等待输入内容。
- RPC payload 明文。

由于本设计采用同账号自动信任，E2EE 主要保护服务端不可见业务内容，不解决账号被盗后的远程控制风险。账号风险通过 token 管理、退出所有会话、设备解绑、本机远程开关和审计日志控制。

## 服务端 HTTP API 规范

普通业务接口遵循统一 JSON envelope：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

约束：

- `code = 0` 是唯一成功码。
- 业务失败使用 `HTTP 200 + 非 0 code`。
- 认证失败、权限失败、设备离线、设备不存在都属于业务失败。
- JSON 无法解析、参数类型导致无法进入业务处理时使用 `HTTP 400`，响应体仍保持统一结构。
- 路由不存在使用 `HTTP 404`，响应体仍保持统一结构。
- 系统异常使用 `HTTP 500`，响应体仍保持统一结构。
- 查询类接口使用 GET，创建、修改、删除和业务动作使用 POST。
- 禁止路径动态参数，业务参数通过 GET 查询参数或 POST 请求体传递。

建议接口：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| POST | `/api/v1/auth/login` | 登录。 |
| POST | `/api/v1/auth/refresh` | 刷新 token。 |
| POST | `/api/v1/auth/logout` | 退出当前会话。 |
| POST | `/api/v1/auth/logout-all` | 退出所有 Web 会话。 |
| POST | `/api/v1/devices/register` | 本机注册设备。 |
| GET | `/api/v1/devices/list` | Web 端读取设备列表。 |
| POST | `/api/v1/devices/unbind` | 解绑设备。 |
| POST | `/api/v1/devices/revoke-token` | 吊销设备 token。 |
| POST | `/api/v1/connections/create` | Web 端创建远程连接。 |
| GET | `/api/v1/connections/ice-config` | 获取 STUN/TURN 配置。 |

协议流例外：

- 设备控制 WSS。
- Web 客户端信令 WSS。
- WebRTC DataChannel RPC。
- relay fallback 密文帧。

这些协议不使用普通 HTTP JSON envelope，但必须定义协议层 envelope、版本号、错误码、连接关闭原因和超时语义。

## 典型数据流

### 本机启动

```text
NiumaNotifier 启动
  -> RemoteAgent 检查远程账号和设备 token
  -> 使用当前服务端地址建立 WSS
  -> 上报 device_id、版本、能力摘要
  -> 周期性心跳
  -> 服务端更新设备在线状态
```

### Web 端连接设备

```text
Web 登录账号
  -> GET /api/v1/devices/list
  -> 选择在线设备
  -> POST /api/v1/connections/create
  -> 服务端校验账号和设备关系
  -> Web 与 RemoteAgent 交换信令
  -> 尝试 WebRTC DataChannel
  -> 失败则走 relay fallback
  -> 双方协商临时会话密钥
  -> Web 发送远程 RPC
```

### 远程发送指令

```text
Web 发送 session.send_instruction 密文 RPC
  -> 服务端只转发密文
  -> RemoteAgent 解密
  -> RemotePermissionGuard 二次校验
  -> RemoteAuditLog 记录方法和结果
  -> RemoteRpcRouter 调用本机 session-control 能力
  -> 返回密文响应
```

### 远程订阅状态

```text
Web 发送 state.subscribe
  -> RemoteAgent 读取当前主状态快照
  -> 立即返回首帧 state event
  -> RemoteAgent 订阅 RuntimeEventBus 或本机状态流
  -> 主状态变化时推送后续 state event
```

## 失败处理

| 场景 | 行为 |
| --- | --- |
| 设备离线 | Web 端展示离线，不允许创建控制连接。 |
| WebRTC 失败 | 自动切换 relay fallback，RPC payload 仍端到端加密。 |
| relay 失败 | Web 端展示连接失败，允许重试。 |
| RemoteAgent 断线 | 服务端标记设备离线，Web 端订阅断开。 |
| RPC 超时 | 请求返回超时错误，请求 ID 标记完成，不继续执行。 |
| 本机远程访问关闭 | 拒绝所有远程 RPC。 |
| 本机远程控制关闭 | 只读 RPC 可继续，控制类 RPC 返回权限错误。 |
| 账号退出或设备 token 吊销 | RemoteAgent 主动断开服务端连接，现有远程连接失效。 |
| 本机 Local API 或 session-control 失败 | 转成远程 RPC 错误，保留错误类别，避免泄露不必要敏感细节。 |

## 与现有架构的关系

- 工具原始事件仍由对应 adapter、watcher 或 hook 转换为 `NiumaEvent`。
- 运行时进程内写入仍必须走 `StateMutationService`。
- 不允许 RemoteAgent 绕过 `SqliteStateStore` 状态转移或 `MainStateService` 直接修改主状态。
- 远程订阅状态应复用主状态服务或 RuntimeEventBus 的变化通知。
- 远程控制应复用现有 session-control 能力，不在远程层重新实现工具控制协议。
- 平台差异能力应放入 `niuma_core::platform`，不要散落在 RemoteAgent 内部。

## 测试策略

第一版至少覆盖：

- RemoteRpcRouter 方法分发。
- RemotePermissionGuard 权限矩阵。
- RPC envelope 编解码。
- 订阅首帧快照和后续推送。
- 控制类 RPC 审计日志。
- 远程访问关闭时拒绝所有 RPC。
- 远程控制关闭时拒绝控制类 RPC。
- WebRTC 失败时 relay fallback 状态机。
- 服务端 relay 只转发密文，不解析 RPC payload。
- 服务端普通 HTTP API 的统一响应 envelope。
- 设备 token 吊销后 RemoteAgent 断线。
- 切换服务端后旧连接失效并重新注册设备。

## 实施拆分建议

建议后续实施按以下阶段推进：

1. 服务端协议与本机远程配置模型。
2. 本机 RemoteAgent 设备注册、登录态、心跳和设置页状态。
3. 服务端设备目录、账号登录和 Web 控制台设备列表。
4. WebRTC 信令与 relay fallback 的连接骨架。
5. E2EE RPC envelope、密钥协商和基础只读方法。
6. 会话列表、会话详情和主状态订阅。
7. 远程发送指令、中断、授权处理和输入回答。
8. 审计日志、退出所有会话、设备解绑和 token 吊销。
9. 自托管部署文档和 TURN/relay 配置文档。

每个阶段都应保持 Local API 默认本机绑定，不把远程层做成公网 Local API 代理。
