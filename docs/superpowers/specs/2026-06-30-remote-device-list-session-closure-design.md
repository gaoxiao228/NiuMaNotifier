# 设备列表远程会话闭环设计

## 背景

当前外部 Web 控制台已经具备设备列表、设备控制台、relay、WebRTC DataChannel、plain RPC、远程 Local API bridge 和 session stream 能力。但实际使用中出现过多次状态不一致：

- 设备显示在线，但远程会话读取超时。
- Relay 或 WebRTC 显示已连接，但业务 RPC 没有返回。
- WebRTC 先 ready、relay 后 ready 时，首批读取可能进入超时状态。
- 用户只能进入设备控制台后观察状态，设备列表页不能直接确认 session 是否可读。

因此第一版闭环目标不是继续堆更多远程能力，而是把“设备在线、两个传输通道可用、远程 session 可读”变成一个页面可见、可测试、可手动验收的流程。

## 目标

打开外部 Web 控制台的设备列表页后，在线设备应能自动完成远程连接诊断，并在设备列表页展示 session 列表。

成功状态必须同时证明：

1. 设备 presence 在线。
2. relay 通道可建立并能完成 `rpc.ping`。
3. WebRTC DataChannel 可建立并能完成 `rpc.ping`。
4. session stream 可通过可用通道读取到首包，显示 session 列表或空状态。

如果失败，页面必须说明失败层级，例如设备离线、relay RPC 不通、WebRTC RPC 不通、session stream 读取失败。不能只暴露 `Plain RPC request timed out` 作为最终用户判断依据。

## 非目标

- 不新增后端业务接口。
- 不改变本机 Local API 的 session 数据结构。
- 不在第一版做多设备并发连接优化。
- 不要求 WebRTC 成功后关闭 relay；两条通道可以同时保持连接，用于热备和诊断。
- 不实现复杂权限白名单；沿用当前远程 Local API RPC 能力。

## 推荐方案

采用“共享远程设备会话客户端 + 设备列表自动验链”的方案。

新增一个前端远程设备会话客户端，把现在 `DeviceConsolePage` 中分散的连接、relay、WebRTC、plain RPC、session stream 启动逻辑收敛成可复用单元。设备列表页和设备控制台都通过同一套客户端得到状态。

设备列表页只对在线设备自动连接第一台设备。这样能满足当前手动验收目标，同时避免未来多设备列表一次性建立大量 WebSocket 和 WebRTC 连接。后续如需多设备并发，可以在这个客户端之上增加并发队列。

## 页面行为

设备列表页：

- 仍然先调用 `devicesApi.list()` 显示设备列表。
- 如果存在在线设备，自动选择第一台在线设备建立远程诊断连接。
- 在设备行下方或右侧显示远程 session 区域。
- session 区域显示：
  - relay 诊断状态。
  - WebRTC 诊断状态。
  - 当前 session 读取通道。
  - session 列表、空状态或错误状态。
- 用户点击设备连接按钮时，进入设备控制台；控制台展示同样的通道诊断和 session 列表。

设备控制台：

- 不再拥有一套独立的远程连接状态机。
- 复用共享远程设备会话客户端。
- 保留信令消息、Ping、远程状态、远程会话原始数据等调试信息。

## 远程连接状态模型

共享客户端对外暴露一个状态快照：

```ts
type RemoteDeviceSessionSnapshot = {
  connectionStatus: 'idle' | 'connecting' | 'accepted' | 'error'
  relay: {
    socket: 'idle' | 'connecting' | 'open' | 'closed' | 'error'
    rpc: 'idle' | 'checking' | 'ok' | 'timeout' | 'error'
    error?: string
  }
  webrtc: {
    socket: 'idle' | 'connecting' | 'open' | 'closed' | 'error'
    rpc: 'idle' | 'checking' | 'ok' | 'timeout' | 'error'
    error?: string
  }
  activeTransport: 'idle' | 'relay' | 'webrtc'
  sessions: {
    status: 'idle' | 'loading' | 'ready' | 'empty' | 'error'
    value: RemoteSessionProjectGroupPage | null
    error?: string
    transport?: 'relay' | 'webrtc'
  }
}
```

这里的 socket 状态只表示传输层连接；rpc 状态才表示业务请求真正可用。

## 数据流

1. 设备列表页加载设备。
2. 找到第一台 `online === true` 的设备。
3. 调用 `connectionsApi.create(device.id, clientId)` 创建连接。
4. 建立信令 WebSocket，等待 `accepted`。
5. 并行启动 relay 和 WebRTC。
6. relay 收到 `relay.ready` 后，强制通过 relay 发送 `rpc.ping`。
7. WebRTC DataChannel open 后，强制通过 WebRTC 发送 `rpc.ping`。
8. 任一通道业务 ping 成功后，可以启动 session stream。
9. 如果 WebRTC session stream 超时且 relay RPC 已 ok，则通过 relay 重试。
10. 当 relay 和 WebRTC 都完成业务 ping 后，设备列表页显示“两条通道可用”。

## 错误处理

- 设备离线：设备行显示离线，不自动连接。
- 登录失效：复用现有 unauthorized 处理，回到登录页。
- relay socket open 但 `relay.ready` 未到：relay socket 显示连接中，relay RPC 保持 idle。
- WebRTC DataChannel open 但 `rpc.ping` 超时：WebRTC RPC 显示 timeout，不把它算作可用通道。
- session stream 首包超时：如果另一个通道 RPC ok，则切换通道重试一次。
- 两个通道都无法 RPC ping：session 区域显示“远程通道不可用”，并展示两个通道各自错误。

## 验收标准

自动测试：

- 设备列表页加载在线设备后，会启动远程诊断连接。
- relay ready 后，会通过 relay 发送 `rpc.ping`。
- WebRTC probe 成功后，会通过 WebRTC 发送 `rpc.ping`。
- 两个 RPC ping 成功后，页面显示 relay 和 WebRTC 均可用。
- session stream 返回 `session_project_groups` 后，设备列表页显示 session 标题、项目路径和运行状态。
- WebRTC session 读取超时但 relay RPC ok 时，会通过 relay 重试。

手动验收：

1. 启动 Docker remote-server。
2. 启动本机 NiuMaNotifier。
3. 打开 `http://127.0.0.1:27880/`。
4. 登录后进入设备列表页。
5. 不点击设备，确认在线设备下方出现 session 列表。
6. 确认页面显示 relay RPC 可用。
7. 确认页面显示 WebRTC RPC 可用。
8. 点击设备进入控制台。
9. 确认控制台 session 列表与设备列表页一致。
10. 刷新页面重复一次，确认状态稳定。

## 浏览器验证要求

实现完成后必须由助手使用浏览器实际打开 `http://127.0.0.1:27880/` 验证。

验证时检查可见页面文本，而不是只依赖测试：

- 设备名称存在。
- 设备状态为在线。
- session 区域不是永久 loading。
- relay 状态显示业务可用。
- WebRTC 状态显示业务可用。
- session 列表显示真实数据或明确空状态。

如果浏览器中需要操作本机 Tauri 窗口来开启远程访问或确认绑定状态，可以使用 `computer-use` 技能辅助。

## 实施边界

第一版只自动诊断第一台在线设备。这个选择是有意的：当前目标是把单设备远程会话闭环跑通、测准、可验收。多设备并发诊断会引入连接数量、取消、限流、排序和 UI 密度问题，应在单设备闭环稳定后再做。
