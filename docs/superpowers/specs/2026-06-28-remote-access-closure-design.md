# NiuMaNotifier 远程访问闭环设计

## 背景

远程访问方案已经拆成多个模块设计和计划：远程服务端、RemoteAgent、Web 控制台、设备在线、信令、WebRTC、relay 和 E2EE RPC。当前桌面登录绑定已经完成并通过手动验收：本机设置页点击登录后，浏览器完成账号登录，NiuMaNotifier 设置页变为已绑定。

下一阶段的目标不是继续并行扩展模块，而是把用户可感知的主链路打通：

```text
外部 Web 客户端登录
  -> 查看已绑定设备
  -> 看到设备在线
  -> 点击进入设备控制台
  -> 建立到本机 RemoteAgent 的连接
  -> 调用最小 RPC
  -> 读取真实 NiuMaNotifier 状态
  -> 后续执行会话控制动作
```

本文档定义完成闭环的实施顺序、边界和验收方式。

## 目标

- 外部 Web 客户端登录同一账号后能看到已绑定设备。
- 本机 RemoteAgent 在线时，Web 控制台能准确显示在线状态。
- Web 控制台点击设备后能创建连接，并把连接邀请转发到本机 RemoteAgent。
- 本机 RemoteAgent 能接受或拒绝连接邀请。
- 第一条可用传输通道先以稳定可验收为目标，优先跑通 relay ping/pong；WebRTC DataChannel 在 relay 闭环之后接入。
- 最小 RPC 先实现 `rpc.ping`、`state.get`、`session.list`。
- Web 控制台能展示真实主状态和会话列表。
- 后续扩展到会话详情、发送指令、中断任务、处理授权和回答输入。
- 服务端只负责账号、设备、在线状态、连接、信令和密文转发，不直接代理本机 Local API。

## 非目标

- 这一阶段不做团队共享设备。
- 这一阶段不做远程文件浏览、远程 shell 或插件管理。
- 这一阶段不做完整审计后台和管理后台。
- 这一阶段不把本机 Local API 暴露成公网 HTTP 代理。
- 这一阶段不追求一次性完成全部 E2EE、安全审计和 WebRTC 优化；这些作为闭环跑通后的加固阶段。

## 推荐路径

采用“端到端最小控制闭环优先”的路径。

不采用“基础设施完整优先”，因为 WebRTC、relay、E2EE、RPC 和完整控制台一次性铺开会导致验收周期过长。

不采用“Web 控制台 UI 优先”，因为没有真实连接时，控制台只会变成静态外壳。

第一阶段的主线是：每完成一个切片，用户都能通过浏览器或设置页看到可验证结果。

## 闭环验收路径

完整 MVP 的手动验收路径如下：

```text
1. Docker 启动 remote-server。
2. 本机 NiuMaNotifier 设置页已绑定远程账号。
3. RemoteAgent 连接远程服务端 /ws/device。
4. 外部 Web 控制台登录同一账号。
5. /devices 页面展示当前设备。
6. 设备状态显示 online。
7. 点击设备进入 /devices/:device_id。
8. Web 控制台调用 POST /api/v1/connections/create。
9. Web 控制台连接 /ws/client。
10. 服务端通过 /ws/device 向本机发送 connection.invite。
11. 本机根据远程开关返回 connection.accept 或 connection.reject。
12. Web 控制台显示连接已接受。
13. Web 与本机通过 relay 发送 ping frame。
14. 本机返回 pong frame。
15. Web 发起 rpc.ping，收到 pong。
16. Web 发起 state.get，展示真实主状态。
17. Web 发起 session.list，展示真实会话列表。
```

其中第 13 步先使用 relay，是为了让第一条通道稳定可测。WebRTC DataChannel 在 relay ping/pong 和最小 RPC 通过后接入，并作为首选传输；relay 保留为 fallback。

## 阶段划分

### 阶段一：设备在线控制台闭环

目标：外部 Web 登录后能看到真实绑定设备和在线状态。

需要完成：

- Web 控制台基础应用骨架。
- 邮箱密码登录页面。
- access token 保存与请求封装。
- `/devices` 页面。
- 设备列表 API 对在线状态的返回。
- RemoteAgent `/ws/device` 心跳稳定。

验收：

- 本机运行时，Web 控制台设备显示 `online`。
- 关闭本机或断开 RemoteAgent 后，设备显示 `offline` 或最后在线时间变化。

### 阶段二：连接创建与信令闭环

目标：Web 点击“连接设备”后，服务端能创建连接并把邀请发送到本机。

需要完成：

- `POST /api/v1/connections/create`。
- `GET /api/v1/connections/ice-config`。
- `/ws/client` 鉴权与连接绑定。
- 服务端向已在线设备转发 `connection.invite`。
- 本机 RemoteAgent 返回 `connection.accept` 或 `connection.reject`。
- Web 控制台展示连接状态。

验收：

- 点击连接后生成 `connection_id`。
- 本机 RemoteAgent 收到连接邀请。
- Web 控制台能显示 accepted、rejected 或 timeout。

### 阶段三：relay 最小传输闭环

目标：Web 与本机之间先有一条稳定可测的帧通道。

需要完成：

- `/ws/relay` 服务端帧转发。
- Web relay transport。
- 本机 relay transport。
- 帧格式包含 `connection_id`、`seq`、`payload`。
- ping/pong 帧测试。

验收：

- Web 控制台显示连接方式为 `relay`。
- Web 发送 ping frame，本机返回 pong frame。
- 服务端日志只能看到连接 ID、帧序号和方向，不记录业务正文。

### 阶段四：最小 RPC 闭环

目标：Web 能调用本机能力，但不直接访问本机 Local API。

首批 RPC：

- `rpc.ping`
- `state.get`
- `session.list`

需要完成：

- 共享 RPC envelope。
- Web `RemoteRpcClient`。
- 本机 `RemoteRpcRouter`。
- 本机只通过既有主状态和会话查询边界读取数据。
- RPC 超时和错误响应。

验收：

- Web 控制台能展示真实主状态。
- Web 控制台能展示本机会话列表。
- 断开本机后，RPC 返回明确连接错误。

### 阶段五：设备控制台 MVP

目标：把“能连通”变成可用控制台。

需要完成：

- 顶部设备状态栏。
- 会话列表。
- 会话详情。
- 指令输入框。
- 中断按钮。
- pending action 区域。

追加 RPC：

- `session.detail`
- `session.send_instruction`
- `session.interrupt`
- `interaction.list_pending`
- `interaction.answer_input`
- `interaction.decide_approval`

验收：

- 能查看会话详情。
- 能发送一条指令到本机工具会话。
- 能中断正在运行的会话。
- 能处理授权或等待输入。

### 阶段六：WebRTC 与安全加固

目标：把 MVP 从可用变成适合长期使用。

需要完成：

- WebRTC DataChannel 作为优先传输。
- WebRTC 失败后自动切换 relay。
- E2EE 握手。
- 设备身份签名校验。
- AES-GCM 加密帧。
- RPC request id、timeout、重复请求防护。
- RemotePermissionGuard。
- 本机 RemoteAuditLog。
- 服务端连接审计摘要。
- token revoke 后主动断开设备和连接。

验收：

- 网络允许时使用 WebRTC。
- WebRTC 不可用时自动切到 relay。
- 服务端无法看到 RPC 明文。
- 关闭本机远程控制开关后，控制类 RPC 被本机拒绝。

## API 与协议边界

HTTP API 遵守项目统一规范：

- 成功返回 `HTTP 200` 和 `{ code: 0, message: "ok", data: {} }`。
- 业务失败返回 `HTTP 200` 和非 0 `code`。
- 协议层参数错误才返回 `HTTP 400`。
- 业务参数使用 GET query 或 POST body。
- 后端 API 不使用路径动态参数。

WebSocket 消息不套 HTTP envelope，统一结构为：

```json
{
  "version": 1,
  "type": "connection.invite",
  "id": "msg_001",
  "data": {}
}
```

远程 RPC 不复用本机 Local API 的 HTTP 路径作为外部契约。Web 端调用远程 RPC 方法，本机 RemoteAgent 再映射到现有状态、会话控制和交互处理能力。

## 当前已有基础

已经具备：

- remote-server Docker 启动。
- 邮箱注册和登录。
- desktop-login 浏览器绑定页面。
- 本机设置页点击登录并完成设备绑定。
- `/ws/device` 基础连接。
- RemoteAgent 设置页状态。
- 服务端设备、连接、信令、relay、E2EE RPC 的分散设计和计划文档。

当前缺口：

- Web 控制台尚未形成可登录、可查看设备的主入口。
- 设备列表到连接创建还没有端到端验收。
- `/ws/client`、relay、Web 侧 transport 和本机 relay transport 尚未形成可测 ping/pong。
- 最小 RPC 尚未通过真实外部 Web 客户端调用。
- 完整设备控制台尚未建立。

## 实施原则

- 每个阶段都必须能手动验收。
- 优先完成真实链路，再补视觉精细度。
- relay 先跑通，WebRTC 后接入。
- `rpc.ping`、`state.get`、`session.list` 先于复杂控制动作。
- 服务端不解析业务明文。
- 本机 RemoteAgent 不绕过 `StateMutationService`、`MainStateService`、`SqliteStateStore` 等现有边界。
- 新增 UI 文案必须同步补齐简体中文、繁体中文、英语、日文、韩文、德文。

## 下一份实施计划

下一份计划应保存为：

```text
docs/superpowers/plans/2026-06-28-remote-access-closure-plan.md
```

计划应从阶段一到阶段四开始，先交付可验收 MVP：

1. Web 控制台登录和设备列表。
2. 设备在线状态联调。
3. 连接创建和 `/ws/client`。
4. 本机 invite/accept。
5. relay ping/pong。
6. `rpc.ping`。
7. `state.get`。
8. `session.list`。

阶段五和阶段六可以在 MVP 通过后拆成后续计划，避免单份计划过大。
