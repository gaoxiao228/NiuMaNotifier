# NiuMaNotifier 本机 RemoteAgent 设计

## 背景

远程访问总方案已经确定：本机 NiumaNotifier 主动连接远程服务端，Web 控制台通过服务端发现在线设备，并在 WebRTC 或 relay fallback 通道上使用端到端加密 RPC 控制本机。服务端和 Web 控制台已有独立设计，但本机 RemoteAgent 需要作为宿主内置模块单独细化，避免实施时被误放进插件系统或绕过现有状态架构。

## 目标

- RemoteAgent 是 NiumaNotifier 宿主内置模块，不插件化。
- 本机不监听公网端口，只主动连接远程服务端。
- 本机设置页点击登录后打开系统浏览器完成账号登录和设备绑定。
- 本机保存远程服务配置、账号摘要、`device_id` 和 `device_token`。
- RemoteAgent 使用 `device_token` 连接远程服务端 `/ws/device`。
- RemoteAgent 接收 Web 客户端连接邀请，参与 WebRTC 信令和 relay fallback。
- RemoteAgent 承载 E2EE RPC server，服务端不可见 RPC 明文。
- RemoteAgent 复用现有主状态、session-control 和 Local API 能力，不绕过 `StateMutationService` 或状态机。
- RemoteAgent 执行每个远程控制 RPC 前做本机权限校验，并写本机审计日志。

## 非目标

- 第一版不做插件化 RemoteAgent。
- 第一版不提供公网 Local API 代理。
- 第一版不开放远程文件浏览。
- 第一版不开放远程 shell。
- 第一版不让服务端解析业务正文。
- 第一版不做跨账号设备共享。

## 模块位置

建议本机侧新增模块：

```text
crates/niuma-core/src/remote/
  mod.rs
  config.rs
  credentials.rs
  device_identity.rs
  rpc_protocol.rs
  rpc_router.rs
  permission.rs
  audit.rs
  crypto.rs

src-tauri/src/remote/
  mod.rs
  agent.rs
  login_flow.rs
  device_socket.rs
  signaling.rs
  transport.rs
  webrtc_transport.rs
  relay_transport.rs
```

职责边界：

- `niuma_core::remote::config`：远程服务地址、远程开关、设备信息的数据模型。
- `niuma_core::remote::credentials`：凭据引用、序列化安全边界和清理逻辑。
- `niuma_core::remote::device_identity`：本机随机设备安装 ID 和 device fingerprint 派生。
- `niuma_core::remote::rpc_protocol`：远程 RPC envelope、错误、事件类型。
- `niuma_core::remote::rpc_router`：RPC 方法到本机能力的分发契约。
- `niuma_core::remote::permission`：本机远程访问和远程控制权限判断。
- `niuma_core::remote::audit`：本机远程审计记录模型。
- `src-tauri::remote::agent`：RemoteAgent 生命周期和状态机。
- `src-tauri::remote::login_flow`：打开浏览器、desktop-login start/poll、绑定完成。
- `src-tauri::remote::device_socket`：`/ws/device` 常驻连接。
- `src-tauri::remote::signaling`：offer/answer/ICE candidate 转发。
- `src-tauri::remote::transport`：WebRTC 与 relay 抽象。

平台差异能力放入 `niuma_core::platform` 或 Tauri 壳层，不散落在业务逻辑里。

## 为什么不插件化

当前插件系统适合工具监听、通知、状态消费和会话 provider。RemoteAgent 是远程访问的信任根，负责设备凭据、常驻网络入口、远程 RPC 权限校验和本机审计，不适合以普通插件进程运行。

第一版决策：

- RemoteAgent 不使用 `plugin.json`。
- RemoteAgent 不声明插件 capability。
- RemoteAgent 不进入插件运行管理器。
- RemoteAgent 不把 `device_token` 放入插件数据目录。
- RemoteAgent 配置和凭据由宿主管理。

未来如果要扩展远程能力，可以单独设计 remote-specific capability；不进入当前 MVP。

## 本机配置与凭据

### 远程配置

配置保存内容：

```json
{
  "server_url": "https://remote.example.com",
  "user": {
    "id": "usr_...",
    "email": "user@example.com",
    "role": "user"
  },
  "device": {
    "id": "dev_...",
    "name": "NiuMa MacBook"
  },
  "remote_access_enabled": true,
  "remote_control_enabled": true,
  "last_connected_at": "2026-06-28T12:00:00Z"
}
```

非敏感配置可以存入现有应用配置文件。敏感凭据不应明文存入普通 JSON 配置。

### 敏感凭据

敏感内容：

- `device_token`
- 本次 desktop login 的一次性私钥
- 未来可能加入的 refresh token 或其他设备凭据

存储策略：

- macOS：优先使用 Keychain。
- Windows：优先使用 Credential Manager。
- Linux：优先使用 Secret Service；不可用时提示用户或使用加权限保护的本地文件作为降级方案。

第一版如果跨平台凭据库尚未接入，必须至少保证：

- 凭据文件权限限制为当前用户。
- 凭据不写入日志。
- 退出账号和解绑设备时清理凭据。
- 服务端返回 token revoked 时清理凭据。

## 设备身份

本机不采集硬件序列号。设备身份使用随机安装 ID 派生。

流程：

```text
第一次启用远程功能
  -> 生成 128/256 bit device_install_id
  -> 保存到本机安全配置
  -> 按服务端 origin 派生 device_fingerprint
```

派生规则：

```text
device_fingerprint = sha256("niuma-device-v1" + remote_server_origin + device_install_id)
```

性质：

- 不同服务端 origin 得到不同 fingerprint。
- 官方服务和自托管服务不能通过 fingerprint 关联同一台机器。
- 清除本机配置或重装应用后会成为新设备。
- 服务端只保存 fingerprint 的加 pepper 哈希。

## 浏览器登录绑定流程

本机设置页点击登录：

```text
1. 生成 desktop login 一次性密钥对。
2. 调用 POST /api/v1/desktop-login/start。
3. 请求带 device_name、device_fingerprint、desktop_public_key、capabilities。
4. 服务端返回 request_id、poll_token、login_url。
5. 本机打开系统浏览器访问 login_url。
6. 本机用 request_id + poll_token 轮询 /api/v1/desktop-login/poll。
7. 浏览器完成登录后，服务端返回 encrypted_result。
8. 本机用一次性私钥解密 encrypted_result。
9. 本机保存 user 摘要、device_id、device_token。
10. 自动开启 remote_access_enabled 和 remote_control_enabled。
11. RemoteAgent 启动 /ws/device 常驻连接。
```

约束：

- 本机不内嵌邮箱密码表单。
- 本机不保存用户密码。
- `poll_token` 不放入浏览器 URL。
- 一次性私钥只保存在本机内存中。
- 绑定超时后销毁一次性私钥。
- 成功解密后立即销毁一次性私钥。

## RemoteAgent 生命周期

RemoteAgent 状态：

```text
disabled
not_configured
binding
connecting
online
reconnecting
token_revoked
server_unreachable
error
```

状态含义：

- `disabled`：远程访问总开关关闭。
- `not_configured`：未登录或没有 `device_token`。
- `binding`：浏览器登录绑定进行中。
- `connecting`：正在连接远程服务端。
- `online`：`/ws/device` 已连接且心跳正常。
- `reconnecting`：连接中断后退避重连。
- `token_revoked`：服务端拒绝 device token。
- `server_unreachable`：服务端不可达或 TLS 失败。
- `error`：其他不可恢复错误。

启动规则：

```text
NiumaNotifier 启动
  -> 读取远程配置
  -> 如果 remote_access_enabled=false，进入 disabled
  -> 如果缺少 device_token，进入 not_configured
  -> 否则连接 /ws/device
```

重连规则：

- 网络错误使用指数退避。
- 用户关闭远程访问时停止重连。
- 服务端返回 token revoked 时停止重连并清理本地 device token。
- 切换服务端时断开旧连接并进入 not_configured。

## /ws/device 连接

鉴权：

```text
Authorization: Device <device_token>
```

连接成功后 RemoteAgent 发送：

```json
{
  "version": 1,
  "type": "device.hello",
  "id": "msg_001",
  "data": {
    "device_id": "dev_...",
    "agent_protocol_version": 1,
    "rpc_protocol_version": 1,
    "capabilities": {
      "supports_webrtc": true,
      "supports_relay": true,
      "supports_remote_control": true
    }
  }
}
```

心跳：

- RemoteAgent 周期发送 heartbeat。
- 服务端也可 ping RemoteAgent。
- 心跳失败进入 `reconnecting`。

接收消息：

- 连接邀请。
- WebRTC offer / answer / ICE candidate。
- relay 启动请求。
- token revoked。
- server shutdown。

## Transport 抽象

RemoteTransport 提供统一接口：

```text
send(frame)
onFrame(callback)
close(reason)
state()
```

实现：

- `WebRtcTransport`：DataChannel 直连。
- `RelayTransport`：通过 `/ws/relay` 转发密文帧。

选择策略：

```text
收到连接邀请
  -> 尝试 WebRTC
  -> WebRTC 成功，使用 WebRtcTransport
  -> WebRTC 超时或失败，切换 RelayTransport
  -> relay 失败，连接失败
```

RemoteAgent 不应把业务 RPC 绑定到具体 transport。RPC 层只处理加密后的有序帧。

## E2EE RPC Server

RPC envelope：

```json
{
  "version": 1,
  "type": "request",
  "id": "req-001",
  "method": "session.send_instruction",
  "params": {}
}
```

RemoteAgent 解密后执行：

```text
校验协议版本
校验 request id 未重复
校验方法存在
执行 RemotePermissionGuard
写 RemoteAuditLog start
调用 RemoteRpcRouter
写 RemoteAuditLog result
加密 response
发送 response
```

RPC 必须有超时。连接关闭时，所有进行中的请求标记为 cancelled。

## RPC 方法映射

只读方法：

| RPC | 本机能力 |
| --- | --- |
| `device.get_health` | RemoteAgent 运行状态和版本。 |
| `device.get_capabilities` | RemoteAgent 能力摘要。 |
| `state.get` | `MainStateService::current_state`。 |
| `state.subscribe` | `RuntimeEventBus` 触发后重新读取主状态。 |
| `session.list` | 现有会话列表 API 或 `ToolSessionRegistry` 查询。 |
| `session.detail` | 现有 session detail 能力。 |
| `interaction.list_pending` | 当前主状态和 pending action 快照。 |

控制方法：

| RPC | 本机能力 |
| --- | --- |
| `session.send_instruction` | 复用现有 `/api/v1/session-control/send-instruction` 对应服务能力。 |
| `session.interrupt` | 复用现有 `/api/v1/session-control/interrupt` 对应服务能力。 |
| `interaction.answer_input` | 复用现有 `/api/v1/session-control/answer-input` 对应服务能力。 |
| `interaction.decide_approval` | 复用现有 approval decision 能力。 |

实现要求：

- RemoteAgent 不重新实现工具控制协议。
- 如果能力当前只在 Local API handler 中，应先抽出可复用 service，再由 Local API 和 RemoteAgent 共用。
- 不允许 RemoteAgent 手写修改主状态。

## 权限模型

本机二次校验优先于服务端连接权限。

规则：

```text
remote_access_enabled=false
  -> 拒绝所有 RPC，并主动关闭远程连接。

remote_control_enabled=false
  -> 允许只读 RPC。
  -> 拒绝控制 RPC。

device_token 缺失或被吊销
  -> 断开 /ws/device。
  -> 清理本机凭据。

server_url 与当前绑定不一致
  -> 拒绝连接。
```

控制 RPC 包括：

- `session.send_instruction`
- `session.interrupt`
- `interaction.answer_input`
- `interaction.decide_approval`

只读 RPC 不应触发状态写入。

## 本机审计日志

服务端看不到 E2EE RPC 明文，所以本机必须记录控制类行为摘要。

字段：

```text
id
server_url
account_id
account_email
device_id
client_id
connection_id
rpc_method
request_id
result
error_code
created_at
```

默认不记录：

- 指令正文。
- 授权命令正文。
- 等待输入正文。
- 会话消息正文。

审计记录可存入本机 SQLite 或单独远程审计表。UI 设置页提供查看入口。

## 设置页能力

本机设置页新增远程访问区域：

- 当前服务端地址。
- 选择官方服务或自定义服务端。
- 登录按钮。
- 当前账号。
- 当前设备名。
- 远程连接状态。
- 远程访问开关。
- 远程控制开关。
- 退出账号。
- 解绑设备。
- 查看远程审计日志。

行为：

- 点击登录打开系统浏览器。
- 登录中展示绑定进度。
- 登录成功后不弹首次提醒。
- 退出账号清理本机 `device_token` 并断开远程连接。
- 解绑设备调用服务端接口，成功后清理本机凭据。

所有新增 UI 文案必须补齐简中、繁中、英语、日文、韩文、德文。

## 服务端切换

用户切换服务端地址时：

```text
断开当前 /ws/device
清理当前服务端 device_token
保留或重新派生 device_install_id
按新 server_url 派生新的 device_fingerprint
进入 not_configured
等待用户重新登录绑定
```

官方服务账号和自托管账号不互通。

## 威胁模型与缓解

### 服务端被动监听

风险：服务端 relay 或 signaling 看到流量。

缓解：

- RPC payload E2EE。
- relay 只转发 ciphertext。
- 服务端不保存业务正文。

### 账号被盗

风险：攻击者登录同账号后控制在线设备。

缓解：

- 服务端支持退出所有 Web 会话。
- 服务端支持设备解绑和 token 吊销。
- 本机有远程访问/远程控制开关。
- 本机记录远程控制审计。

### device token 泄露

风险：攻击者伪装设备连接服务端。

缓解：

- device token 存系统凭据库或受限文件。
- 服务端只保存 token hash。
- 解绑或吊销后立即断开设备连接。
- 本机日志不输出 token。

### 浏览器绑定会话被截获

风险：攻击者拿到 `request_id`。

缓解：

- `poll_token` 不进入 URL。
- poll 需要 `request_id + poll_token`。
- 绑定结果用本机一次性公钥加密。
- 会话短 TTL。

### Web 客户端 XSS

风险：攻击者读取 Web 登录态或发起控制。

缓解：

- Web 端减少 token 暴露，优先使用 httpOnly secure cookie 保存 refresh token。
- access token 短 TTL。
- 严格 CSP。
- 不在 DOM 中渲染未转义命令正文。

## 测试策略

第一版至少覆盖：

- 未配置时 RemoteAgent 进入 `not_configured`。
- 远程访问关闭时 RemoteAgent 进入 `disabled`。
- desktop-login/start 请求包含 `desktop_public_key`。
- desktop-login/poll 完成后能解密 `encrypted_result`。
- 成功绑定后保存 `device_id` 和 `device_token`。
- `device_token` 被吊销后 RemoteAgent 断开并清理本机凭据。
- `/ws/device` 连接失败后指数退避重连。
- 远程控制关闭时拒绝控制 RPC，允许只读 RPC。
- `state.subscribe` 首帧返回当前主状态。
- 主状态变化后 `state.subscribe` 推送新快照。
- `session.send_instruction` 复用现有 session-control service。
- `interaction.decide_approval` 写本机审计日志。
- WebRTC 失败时切换 relay transport。
- 连接关闭时 pending RPC 被取消。
- 服务端切换后旧凭据不再使用。

## 实施拆分建议

1. 增加 remote 配置模型和本机设置页状态字段。
2. 实现 device_install_id 和 device_fingerprint 派生。
3. 实现敏感凭据存储抽象。
4. 实现浏览器登录绑定流程。
5. 实现 RemoteAgent 生命周期状态机。
6. 实现 `/ws/device` 连接和心跳。
7. 实现 signaling 消息处理。
8. 实现 RemoteTransport 抽象和 relay transport。
9. 实现 WebRTC transport。
10. 实现 E2EE RPC protocol 和 crypto。
11. 实现 RemotePermissionGuard。
12. 实现 RemoteRpcRouter 只读方法。
13. 抽取 session-control service 并实现控制方法。
14. 实现本机 RemoteAuditLog。
15. 完成本机设置页远程访问区域和 i18n。
