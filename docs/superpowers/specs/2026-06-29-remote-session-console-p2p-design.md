# 远程控制台直连优先传输设计

## 背景

远程访问第一阶段已经完成 Relay 基础闭环：外部 Web 客户端登录后可以看到绑定设备，连接在线设备，并通过服务端 Relay 与本机 NiuMaNotifier 建立 RPC 通道。远程会话列表已经通过通用 Local API RPC 访问本机接口：

```json
{
  "method": "GET",
  "path": "/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20",
  "body": null
}
```

这个闭环证明账号、设备绑定、连接邀请、Relay、RPC、SSE 转发和 Web 控制台展示都可用。下一阶段要回到原先优先方案：当 Relay 和 WebRTC DataChannel 都可用时，业务数据优先走 WebRTC 直连；但连接启动阶段不应死等 WebRTC，Relay 可以先快速承载首屏数据，WebRTC 在后台协商成功后接管后续数据面。

## 目标

- 外部 Web 客户端连接后，Relay 可以先快速可用，保证首屏会话列表尽快出现。
- WebRTC DataChannel 后台协商；当 Relay 和 WebRTC 都可用时，优先通过 WebRTC 直连本机 NiuMaNotifier。
- WebRTC 不可用、断开或发送失败时，自动继续使用现有 Relay。
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

### 数据面直连优先

最终优先路径：

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

启动策略不是“WebRTC 成功前不工作”，而是：

```text
Relay 先可用则先承载业务数据
WebRTC 后台协商
WebRTC 可用后成为首选下行/上行通道
Relay 保留为热备
```

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

Web 侧还需要一个 `RemoteMessageBus`：

```text
RelayTransport.onPayload
  -> RemoteMessageBus
WebRtcTransport.onPayload
  -> RemoteMessageBus
RemoteMessageBus
  -> PlainRpcClient / RemoteLocalApiClient
```

`PlainRpcClient` 和 `RemoteLocalApiClient` 继续复用，只依赖统一的发送入口和统一的接收入口，不直接知道 payload 来自 Relay 还是 WebRTC。

### 业务 stream 与 transport 解耦

远程 SSE 订阅不应该等同于某一条 transport 连接。以 session group SSE 为例，理想模型是：

```text
本机 Local Group SSE
  -> RemoteStreamSession(stream_id)
  -> TransportRouter
      -> WebRTC DataChannel，open 时优先
      -> Relay，WebRTC 不可用时兜底
```

本机只订阅一次本地 SSE。WebRTC 从不可用变成可用时，不重新订阅本地 SSE；下一条 stream event 自动通过 WebRTC 发送。WebRTC 断开时，也不重建本地 SSE；下一条 event 自动回到 Relay。

## 连接流程

1. Web 用户在设备列表点击连接。
2. Web 调用服务端创建连接，请求 `transport_preference = "auto"`。
3. 服务端创建 `connection_id`、`connection_token`，通知目标设备。
4. 本机 RemoteAgent 接受连接邀请。
5. Web 立即启动 Relay 连接，Relay open 后可以先发起远程 Local API stream。
6. Web 同时创建 `RTCPeerConnection` 和 DataChannel。
7. Web 生成 offer，通过服务端信令发给设备。
8. 本机收到 offer，创建 answer，通过服务端信令回给 Web。
9. 双方通过服务端交换 ICE candidate。
10. DataChannel open 后，Web 和本机都将 WebRTC 标记为可用。
11. 新的普通 RPC 请求优先通过 WebRTC 发送。
12. 已存在的业务 stream 不重新订阅本机 SSE；本机 `TransportRouter` 后续优先通过 WebRTC 推送 stream event。
13. WebRTC 失败或断开时，Relay 继续承载后续请求和 stream event。

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
- `trying_relay`
- `trying_webrtc`
- `webrtc_open`
- `webrtc_failed`
- `relay_open`
- `relay_failed`

Web 侧应单独维护每个 transport 的状态，而不是只有一个全局 transport 状态：

```ts
type TransportAvailability = {
  relay: 'idle' | 'connecting' | 'open' | 'failed' | 'closed'
  webrtc: 'idle' | 'connecting' | 'open' | 'failed' | 'closed'
}
```

选择规则：

```text
如果 WebRTC open，新请求优先走 WebRTC
否则如果 Relay open，新请求走 Relay
否则等待连接或展示错误
```

用户界面文案：

- 当前通道：直连
- 当前通道：Relay
- 备用通道：Relay 已连接
- 正在尝试直连
- 已直连
- 直连失败，继续使用 Relay
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
- 为每个 connection 维护 `TransportRouter`，统一选择下行响应和 stream event 的发送通道。

本机 RPC 处理仍复用：

- `rpc.ping`
- `state.get`
- `local_api.request`
- `local_api.stream`
- `local_api.stream.close`

### RemoteStreamSession

本机收到 `local_api.stream` 后创建业务 stream session，而不是创建“属于 Relay”或“属于 WebRTC”的 stream：

```text
stream_id
local_api_path
local_sse_subscription
last_event
next_seq
closed
```

本机本地 SSE 每产生一条事件，写入：

```json
{
  "version": 1,
  "type": "notification",
  "method": "local_api.stream.event",
  "params": {
    "stream_id": "stream_1",
    "seq": 12,
    "event": "session_project_groups",
    "data": {}
  }
}
```

`seq` 是每个 stream 内单调递增的序号。Web 端按 `stream_id + seq` 去重和防乱序：

```text
seq <= lastSeqByStream[stream_id]：丢弃
seq > lastSeqByStream[stream_id]：接受并更新 UI
```

因为 session group event 是完整快照，丢弃旧 seq 不会破坏 UI。

当 WebRTC 从 closed 变 open，或 Relay 从 closed 变 open，本机可以用新的 seq 主动重发 `last_event`，让新通道尽快同步当前快照。

## Relay fallback

Relay 不只是失败后的冷 fallback，也可以作为连接启动阶段的快速通道和 WebRTC 可用后的热备通道。

WebRTC 降级触发条件：

- WebRTC 初始化失败。
- offer / answer 信令超时。
- ICE 状态进入 `failed`。
- DataChannel 未在限定时间内 open。
- DataChannel open 后异常关闭，且当前连接仍有效。

第一版建议超时时间：

- WebRTC open 超时：5 秒。
- Relay open 超时：沿用现有 WebSocket 行为，不额外加复杂重试。

fallback 规则：

- Relay open 后可以立即承载业务 RPC 和 stream event。
- WebRTC open 后，新请求优先走 WebRTC。
- 下行 stream event 由本机 `TransportRouter` 选择当前最佳通道发送。
- 同一个 stream event 只从一个通道发送；切换边界用 `seq` 防止迟到旧消息覆盖新消息。
- WebRTC 失败后不需要重建本地 SSE，后续 event 自动回到 Relay。

## STUN / TURN 策略

第一版支持配置 STUN，但不强制 TURN。

原因：

- STUN 足够验证直连优先架构。
- TURN 部署、鉴权和成本更复杂。
- TURN 本身也是中继，和 Relay 的产品目标需要清楚区分。

配置建议：

- 服务端 `.env` 暴露默认 ICE servers。
- Web 创建连接时从服务端返回 ICE server 列表。
- 自托管用户可以配置自己的 STUN/TURN。

第一版默认可以使用空 ICE server 或公开 STUN；如果直连失败，Relay fallback 兜底。

## 验收标准

1. Relay 现有远程会话列表功能不回退。
2. Relay 先 open 时，Web 控制台可以先显示远程会话列表。
3. Web 控制台后台尝试 WebRTC。
4. WebRTC 成功时，新请求优先走 WebRTC。
5. WebRTC 成功后，本机不重新订阅本地 group SSE，后续 stream event 自动优先通过 WebRTC 下发。
6. Web 控制台明确显示当前主通道和备用通道。
7. WebRTC 失败或断开时，远程会话列表继续通过 Relay 更新。
8. Web 侧按 `stream_id + seq` 丢弃迟到事件，旧通道消息不能覆盖新快照。
9. 服务端日志可以区分信令消息和 Relay frame；WebRTC 成功后该连接的 stream event 不应继续稳定走 Relay。
10. 本机退出、页面关闭或连接重试时，WebRTC、Relay 和 RemoteStreamSession 资源都能被清理。
11. Web 侧测试覆盖 Relay 先可用、WebRTC 后接管、迟到 seq 被丢弃、WebRTC 断开回到 Relay。
12. Rust 侧测试覆盖 stream event 路由选择、seq 单调递增、last_event 重发、未知连接拒绝、资源清理。

## 实现顺序

1. Web 端抽出 `RemoteTransport` 和 `RemoteMessageBus`，将现有 Relay 封装成 `RelayTransport`。
2. 保持 Relay 路径测试全绿，确认抽象没有改变现有行为。
3. 服务端连接创建响应增加 ICE server 配置字段。
4. Web 端新增 `WebRtcTransport`，实现 offer、candidate、DataChannel 收发。
5. 服务端 `/ws/client` 和设备 WebSocket 补齐 `signal.offer`、`signal.answer`、`signal.ice_candidate` 转发测试。
6. 本机 RemoteAgent 接入 `signal.offer`，创建 answer 和 DataChannel。
7. 本机 DataChannel payload 接入现有 RPC runtime。
8. 本机增加 `TransportRouter`，统一选择 Relay / WebRTC 下行通道。
9. 本机将 `local_api.stream` 改为业务级 `RemoteStreamSession`，为事件增加 `seq` 和 `last_event`。
10. Web 端按 `stream_id + seq` 处理 stream event，丢弃迟到事件。
11. Web 控制台实现 Relay 快速可用、WebRTC 后台协商、直连可用后优先使用的状态机。
12. 控制台展示当前主通道和备用通道，并补齐 i18n。
13. 做端到端手动验收：Relay 先显示、WebRTC 后接管、WebRTC 断开回到 Relay。

## 风险

- 浏览器和 Rust WebRTC 库的 DataChannel 行为可能存在兼容差异。
- NAT 环境下不配置 TURN 时，直连成功率不稳定。
- 如果没有 `stream seq`，WebRTC / Relay 切换边界可能出现迟到事件覆盖新快照。
- 如果状态机没有清理旧资源，可能出现 WebRTC 和 Relay 双通道同时响应普通 RPC。
- 多标签页同时连接同一设备时，仍要遵守当前 busy / replace 规则。
- Safari 或部分浏览器 WebRTC 行为可能不同，第一版优先保证 Chrome。

## 与现有实现的关系

已经完成的 Relay 闭环不是废弃代码，而是快速首屏和热备通道。下一阶段的核心不是新增另一套业务通信协议，而是把已经跑通的 RPC payload 放到统一 transport 层上：Relay 先可用时先服务用户，WebRTC 可用后成为首选通道。
