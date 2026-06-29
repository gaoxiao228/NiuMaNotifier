# 远程控制台 WebRTC-first 设计

## 背景

远程访问第一阶段已经完成 Relay 基础闭环：外部 Web 客户端登录后可以看到绑定设备，连接在线设备，并通过服务端 Relay 与本机 NiuMaNotifier 建立 RPC 通道。远程会话列表已经通过通用 Local API RPC 访问本机接口：

```json
{
  "method": "GET",
  "path": "/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20",
  "body": null
}
```

这个闭环证明账号、设备绑定、连接邀请、Relay、RPC、SSE 转发和 Web 控制台展示都可用。下一阶段要回到原先优先方案：数据面优先走 WebRTC DataChannel 直连，服务端只做控制面和信令；Relay 保留为 fallback。

## 目标

- 外部 Web 客户端优先通过 WebRTC DataChannel 直连本机 NiuMaNotifier。
- WebRTC 失败、超时或不可用时，自动降级到现有 Relay。
- 现有 `PlainRpcClient`、`RemoteLocalApiClient` 和本机 `LocalApiBridge` 不重写业务逻辑。
- 控制台明确展示当前传输方式，方便确认是否真的绕过服务端 Relay。
- 第一版只保证现有远程会话列表实时更新继续可用。

第一版不做：

- TURN 强依赖。
- 远程会话详情。
- 远程输入。
- 授权 allow / deny。
- interrupt / stop。
- 文件访问。
- 服务端存储 session 内容。

## 核心原则

### 控制面继续走外网服务端

服务端负责：

- 账号认证。
- 设备目录和在线状态。
- 连接权限校验。
- 短期 `connection_token`。
- WebRTC offer / answer / ICE candidate 信令转发。
- Relay fallback 路由。

服务端不负责：

- 解析 RPC payload。
- 解析本机 Local API 请求体。
- 存储 session 列表、项目路径、会话正文或授权正文。

### 数据面 WebRTC 优先

优先路径：

```text
外部 Web 控制台
  <-> WebRTC DataChannel
  <-> 本机 NiuMaNotifier
```

兜底路径：

```text
外部 Web 控制台
  <-> 外网服务端 Relay
  <-> 本机 NiuMaNotifier
```

RPC 层只看到有序 payload，不关心底层是 WebRTC 还是 Relay。

### 传输层抽象先行

Web 控制台不应继续直接依赖 `RelayClient`。下一阶段先抽统一传输接口：

```ts
type RemoteTransport = {
  send(value: unknown): void
  close(): void
}
```

配套事件：

- `onOpen`
- `onPayload`
- `onClose`
- `onError`

实现：

- `RelayTransport`：封装当前 Relay WebSocket。
- `WebRtcTransport`：封装 `RTCPeerConnection` 和 DataChannel。

`PlainRpcClient` 和 `RemoteLocalApiClient` 继续复用，只依赖 `RemoteTransport.send(...)`。

## 连接流程

1. Web 用户在设备列表点击连接。
2. Web 调用服务端创建连接，请求 `transport_preference = "auto"`。
3. 服务端创建 `connection_id`、`connection_token`，通知目标设备。
4. 本机 RemoteAgent 接受连接邀请。
5. Web 创建 `RTCPeerConnection` 和 DataChannel。
6. Web 生成 offer，通过服务端信令发给设备。
7. 本机收到 offer，创建 answer，通过服务端信令回给 Web。
8. 双方通过服务端交换 ICE candidate。
9. DataChannel open 后，Web 控制台将当前 transport 标记为 `webrtc`。
10. 现有 RPC 和远程 Local API stream 通过 DataChannel 发送。
11. 如果 WebRTC 在超时时间内未 open，或 ICE / DataChannel 进入失败状态，Web 自动启动 Relay fallback。
12. Relay fallback 成功后，当前 transport 标记为 `relay`，业务功能继续可用。

## 信令协议

现有 Rust 信令模型已经包含：

- `connection.invite`
- `connection.accept`
- `connection.reject`
- `signal.offer`
- `signal.answer`
- `signal.ice_candidate`
- `signal.cancel`

下一阶段沿用这些消息类型，不新增 HTTP 动态路径。Web 客户端通过已建立的 `/ws/client` 信令连接发送 offer 和 ICE；设备通过设备 WebSocket 回传 answer 和 ICE。

信令消息仍使用统一 envelope：

```json
{
  "version": 1,
  "type": "signal.offer",
  "id": "msg_xxx",
  "data": {
    "connection_id": "conn_xxx",
    "sdp": "..."
  }
}
```

ICE candidate：

```json
{
  "version": 1,
  "type": "signal.ice_candidate",
  "id": "msg_xxx",
  "data": {
    "connection_id": "conn_xxx",
    "candidate": "...",
    "sdp_mid": "0",
    "sdp_mline_index": 0
  }
}
```

## Web 端状态机

控制台连接状态拆成两层。

连接控制面：

- `connecting`
- `accepted`
- `rejected`
- `closed`
- `error`

数据传输面：

- `idle`
- `trying_webrtc`
- `webrtc_open`
- `webrtc_failed`
- `trying_relay`
- `relay_open`
- `relay_failed`

用户界面文案：

- 正在尝试直连
- 已直连
- 直连失败，正在切换 Relay
- Relay 已连接
- 连接失败

所有新增文案必须补齐简体中文、繁体中文、英语、日文、韩文、德文。

## 本机 RemoteAgent

本机侧需要把现有 WebRTC 骨架接进设备信令状态机：

- 收到 `signal.offer` 后校验 `connection_id` 是否属于当前已接受连接。
- 创建 WebRTC answer。
- 绑定 DataChannel 收发。
- 将 DataChannel payload 送入现有 `RelayRuntime` / RPC router 等价处理路径。
- 本机生成 ICE candidate 时通过设备 WebSocket 发回服务端。
- 连接结束或失败时清理该 connection 的 WebRTC 资源。

本机 RPC 处理仍复用：

- `rpc.ping`
- `state.get`
- `local_api.request`
- `local_api.stream`
- `local_api.stream.close`

## Relay fallback

fallback 触发条件：

- WebRTC 初始化失败。
- offer / answer 信令超时。
- ICE 状态进入 `failed`。
- DataChannel 未在限定时间内 open。
- DataChannel open 后异常关闭，且当前连接仍有效。

第一版建议超时时间：

- WebRTC open 超时：5 秒。
- Relay open 超时：沿用现有 WebSocket 行为，不额外加复杂重试。

fallback 规则：

- WebRTC 未成功前，不向业务 RPC 层暴露 open。
- WebRTC 失败后启动 Relay。
- Relay 成功后复用同一个 `PlainRpcClient` 创建新请求。
- 旧 WebRTC 资源必须关闭，避免双通道同时回包。

## STUN / TURN 策略

第一版支持配置 STUN，但不强制 TURN。

原因：

- STUN 足够验证 WebRTC-first 架构。
- TURN 部署、鉴权和成本更复杂。
- TURN 本身也是中继，和 Relay 的产品目标需要清楚区分。

配置建议：

- 服务端 `.env` 暴露默认 ICE servers。
- Web 创建连接时从服务端返回 ICE server 列表。
- 自托管用户可以配置自己的 STUN/TURN。

第一版默认可以使用空 ICE server 或公开 STUN；如果直连失败，Relay fallback 兜底。

## 验收标准

1. Relay 现有远程会话列表功能不回退。
2. Web 控制台优先尝试 WebRTC。
3. WebRTC 成功时，远程会话列表实时更新可用。
4. Web 控制台明确显示当前 transport 为 WebRTC。
5. WebRTC 失败时自动切换 Relay。
6. Relay fallback 成功时，远程会话列表仍可用。
7. 服务端日志可以区分信令消息和 Relay frame；WebRTC 成功后不应持续出现该连接的 Relay frame。
8. 本机退出、页面关闭或连接重试时，WebRTC 和 Relay 资源都能被清理。
9. Web 侧测试覆盖 WebRTC 成功、WebRTC 失败 fallback、Relay 失败错误态。
10. Rust 侧测试覆盖 offer 校验、answer 生成、未知连接拒绝、资源清理。

## 实现顺序

1. Web 端抽出 `RemoteTransport` 接口，将现有 Relay 封装成 `RelayTransport`。
2. 保持 Relay 路径测试全绿，确认抽象没有改变现有行为。
3. 服务端连接创建响应增加 ICE server 配置字段。
4. Web 端新增 `WebRtcTransport`，实现 offer、candidate、DataChannel 收发。
5. 服务端 `/ws/client` 和设备 WebSocket 补齐 `signal.offer`、`signal.answer`、`signal.ice_candidate` 转发测试。
6. 本机 RemoteAgent 接入 `signal.offer`，创建 answer 和 DataChannel。
7. 本机 DataChannel payload 接入现有 RPC runtime。
8. Web 控制台实现 WebRTC-first 状态机和 Relay fallback。
9. 控制台展示当前 transport，并补齐 i18n。
10. 做端到端手动验收：WebRTC 成功路径、强制失败 fallback 路径。

## 风险

- 浏览器和 Rust WebRTC 库的 DataChannel 行为可能存在兼容差异。
- NAT 环境下不配置 TURN 时，直连成功率不稳定。
- 如果状态机没有清理旧资源，可能出现 WebRTC 和 Relay 双通道同时响应。
- 多标签页同时连接同一设备时，仍要遵守当前 busy / replace 规则。
- Safari 或部分浏览器 WebRTC 行为可能不同，第一版优先保证 Chrome。

## 与现有实现的关系

已经完成的 Relay 闭环不是废弃代码，而是 WebRTC-first 的 fallback 基座。下一阶段的核心不是新增另一套业务通信协议，而是把已经跑通的 RPC payload 放到更合适的传输层上。
