# NiuMaNotifier 远程 Web 控制台设计

## 背景

第一版外部客户端定为 Web 网页端。用户通过远程服务端账号登录后查看已绑定设备，设备在线时进入设备控制台，通过 WebRTC DataChannel 或 relay fallback 建立到本机 RemoteAgent 的端到端加密 RPC 通道。

`/Users/niuma/code/ContinueWork/` 是当前可参考的本地会话控制台原型。它提供了项目分组、会话列表、会话详情、消息流、pending action 和发送指令输入框。远程 Web 控制台应继承这个核心体验，但不能照搬本地 BFF 代理模式；远程场景必须增加登录、设备列表、远程连接状态、E2EE RPC 和设备管理。

## 目标

- Web 控制台随 `remote-server` 一起部署，代码放在 `remote-server/web/`。
- 用户登录后先看到已绑定设备列表。
- 在线设备可进入远程设备控制台。
- 设备控制台以 ContinueWork 的会话控制台为核心参考。
- Web 端通过 E2EE RPC 访问本机 RemoteAgent，不直接访问本机 Local API。
- 支持 WebRTC 优先，失败自动切换 relay fallback。
- 支持设备管理、连接状态、主状态、会话列表、会话详情、发送指令、中断任务、处理授权和回答输入。
- 所有页面文案支持简体中文、繁体中文、英语、日文、韩文、德文。

## 非目标

- 第一版不做远程文件浏览。
- 第一版不做远程 shell。
- 第一版不做插件管理。
- 第一版不做通知渠道配置。
- 第一版不做多设备同时控制。
- 第一版不做团队协作或跨账号共享设备。
- 第一版不完整复制本机桌面 UI。

## 代码位置

Web 控制台作为 `remote-server` 的一部分交付：

```text
remote-server/
  web/
    index.html
    src/
      main.tsx
      app.tsx
      i18n/
        index.ts
        zh-CN.ts
        zh-TW.ts
        en.ts
        ja.ts
        ko.ts
        de.ts
      api/
        httpClient.ts
        authApi.ts
        devicesApi.ts
        connectionsApi.ts
      auth/
        authStore.ts
        loginPage.tsx
        registerPage.tsx
      devices/
        deviceListPage.tsx
        deviceManagementPanel.tsx
      remote/
        remoteSessionClient.ts
        remoteRpcClient.ts
        cryptoSession.ts
        deviceConsolePage.tsx
        connectionStatusBar.tsx
        projectSidebar.tsx
        sessionDetail.tsx
        pendingActionMessage.tsx
        deviceStatusPanel.tsx
      audit/
        auditPage.tsx
      admin/
        serverStatusPage.tsx
      shared/
        envelope.ts
        errors.ts
        time.ts
        statusText.ts
        layout.tsx
```

`remote-server` 构建时将 `web/` 打包为静态资源，由 Fastify 托管。自托管用户只部署一个 `remote-server` 容器即可使用 Web 控制台。

## 技术栈

```text
React + TypeScript
Vite
Ant Design 或等价组件库
lucide-react 图标
WebRTC DataChannel
WebSocket relay fallback
Web Crypto API
```

ContinueWork 当前使用 React、Ant Design、Ant Design X 的 `Bubble` 和 `Sender`。远程 Web 控制台可以复用这种“消息流 + 指令输入”的交互，但颜色、布局和组件边界需要适配完整设备控制台。

## 页面与路由

前端路由可以使用动态路径；后端 API 仍遵守“不使用动态路径参数”的约束。

```text
/login
/register
/devices
/devices/:device_id
/settings/devices
/settings/account
/settings/server
/audit
```

### /login

功能：

- 邮箱密码登录。
- 登录成功后进入 `/devices`。
- 服务端未开放注册时隐藏注册入口。
- 展示当前服务端域名。
- 登录错误使用统一错误提示。

### /register

功能：

- 仅 `REGISTRATION_MODE=open` 时可用。
- 邮箱注册。
- 密码基础强度校验。
- 注册成功后跳转登录页。

`REGISTRATION_MODE=admin_invite` 或 `disabled` 时，页面展示“当前服务端未开放注册”，不展示注册表单。

### /devices

登录后的默认页面。

展示：

- 设备名。
- 在线 / 离线。
- 最后在线时间。
- Agent 协议版本。
- 是否支持 WebRTC。
- 是否支持 relay。
- 是否支持远程控制。
- 最近连接状态。

操作：

- 进入控制台。
- 重命名设备。
- 解绑设备。
- 吊销 device token。
- 刷新设备列表。

设备列表应是密集、可扫描的控制台列表，不做营销式卡片墙。设备较多时支持搜索和按在线状态过滤；这两个能力可在第一版后半段加入。

### /devices/:device_id

设备远程控制台。在线设备可以进入，离线设备展示离线状态和最近在线时间，不创建远程控制连接。

桌面布局：

```text
顶部状态栏：
  设备名 / 在线状态 / 连接方式 / 重连 / 返回设备列表

主体三栏：
  左侧：项目 + 会话列表
  中间：会话详情消息流 + 发送指令
  右侧：设备状态 / 主状态 / pending action / 快捷操作
```

移动布局：

```text
顶部：设备状态条
Tabs：
  会话
  详情
  操作
```

或在小屏下使用“会话列表 -> 会话详情”的单页钻取，底部固定发送指令。

### /settings/devices

偏管理视图，展示所有设备：

- 设备名。
- 在线状态。
- 最后在线时间。
- 最近连接摘要。
- 重命名。
- 解绑。
- 吊销 token。

### /settings/account

第一版功能：

- 当前账号邮箱。
- 退出登录。
- 退出所有 Web 会话。

修改密码可作为后续补充；第一版如果不做忘记密码，修改密码不是主链路。

### /settings/server

仅 admin 可见，第一版只读。

展示：

- 服务版本。
- `REGISTRATION_MODE`。
- `REMOTE_SERVER_PUBLIC_URL`。
- PostgreSQL 连接状态。
- Redis 连接状态。
- TURN 是否启用。
- relay 是否启用。
- 当前在线设备数。
- 当前活跃连接数。

不在 Web UI 修改 `.env` 和 Docker Compose 配置。

### /audit

服务端审计摘要，不展示 E2EE 明文。

展示事件：

- 登录成功 / 失败。
- 设备注册。
- 设备解绑。
- device token 吊销。
- 连接创建。
- WebRTC 建连成功。
- relay fallback 启用。
- 连接关闭。

不展示：

- 指令正文。
- 授权命令正文。
- 等待输入正文。
- RPC 明文 payload。

## 设备控制台结构

### 顶部状态栏

顶部状态栏固定展示：

- 设备名。
- 在线状态。
- 连接状态：未连接、连接中、WebRTC、relay、断开、重连中。
- 主状态：idle、running、waiting_approval、waiting_input、completed、error。
- 返回设备列表。
- 重新连接。

连接方式文案要明确：`WebRTC` 表示直连，`Relay` 表示服务端仅转发密文。

### 左侧项目与会话列表

参考 ContinueWork 的 `ProjectSidebar`：

- 按项目分组。
- 每组可折叠。
- 默认每组展示最近 5 个会话。
- 会话显示第一条用户消息预览或 session id。
- 会话显示 runtime status。
- 会话显示最近活动时间。

远程版需要增加：

- 设备断线时列表保持最后快照，但显示过期状态。
- 远程 RPC 未连接时禁止选择会话触发详情请求。
- 会话列表来自远程 RPC `session.list` 或订阅事件，不从浏览器侧自行推导主状态。

### 中间会话详情

参考 ContinueWork 的 `SessionDetail`：

- 以消息流展示会话内容。
- 用户消息靠右，助手和系统侧消息靠左。
- 支持加载更早消息。
- 接近底部时自动跟随新消息。
- 用户不在底部时显示“有新消息”按钮。
- `runtime_status = running` 时显示运行中三点动效。
- 底部固定发送指令输入框。

远程版需要增加：

- 详情请求走 E2EE RPC `session.detail`。
- 详情更新走 E2EE event，不走本机 SSE。
- 发送指令走 `session.send_instruction` RPC。
- 中断任务走 `session.interrupt` RPC。
- 控制请求超时后显示可重试错误。

### 右侧状态与操作面板

ContinueWork 没有这一栏；远程版需要新增。

展示：

- 当前设备主状态。
- 当前连接 transport。
- 最近心跳时间。
- 当前 pending action。
- 中断任务按钮。
- 重新连接按钮。
- 断开连接按钮。

pending action 在两个位置出现：

- 消息流中作为上下文消息展示。
- 右侧面板中作为当前阻塞项摘要展示。

用户可以从任一位置处理，但同一 action 的按钮状态必须同步，避免重复提交。

## Pending Action 交互

参考 ContinueWork 的 `PendingActionMessage`，保留两类：

- approval：允许 / 拒绝。
- input：表单回答。

远程版约束：

- approval 必须展示完整命令或权限正文。
- 不能只展示“允许”按钮。
- input 字段按服务端 schema 渲染。
- 提交走 E2EE RPC，不使用 Local API endpoint。
- 提交中禁用按钮。
- 成功后 action 标记为已处理。
- 失败后保留输入内容。

RPC 映射：

```text
approval -> interaction.decide_approval
input    -> interaction.answer_input
```

## API 与 RPC 客户端

### HTTP API client

用于远程服务端账号和设备接口：

- `auth.login`
- `auth.refresh`
- `auth.logout`
- `auth.me`
- `devices.list`
- `devices.rename`
- `devices.unbind`
- `devices.revoke-token`
- `connections.create`
- `connections.ice-config`

HTTP client 必须统一处理 envelope：

```ts
type ApiEnvelope<T extends object> = {
  code: number
  message: string
  data: T | null
}
```

规则：

- HTTP 200 也必须检查 `code`。
- `code !== 0` 抛业务错误。
- access token 过期时使用 refresh token 刷新。
- refresh 失败时回到登录页。

### RemoteSessionClient

负责远程连接生命周期：

```text
idle
  -> creating_connection
  -> signaling
  -> connecting_webrtc
  -> connected_webrtc
  -> connecting_relay
  -> connected_relay
  -> disconnected
  -> reconnecting
  -> failed
```

职责：

- 调用 `/api/v1/connections/create`。
- 获取 ICE 配置。
- 建立 `/ws/client` 信令连接。
- 尝试 WebRTC DataChannel。
- WebRTC 失败时连接 `/ws/relay`。
- 建立 E2EE 会话密钥。
- 暴露 send/subscribe/close。

### RemoteRpcClient

提供类型化 RPC：

```text
device.get_health
device.get_capabilities
state.get
state.subscribe
session.list
session.detail
session.send_instruction
session.interrupt
interaction.list_pending
interaction.answer_input
interaction.decide_approval
```

请求规则：

- 每个 request 有唯一 ID。
- 每个 request 有超时。
- 连接断开时 reject 所有 pending request。
- event 消息按 topic 分发。

### cryptoSession

负责：

- 生成临时密钥。
- 协商 E2EE session key。
- 加密 RPC payload。
- 解密 response 和 event。

服务端 relay 和 signaling 不可见 RPC 明文。

## 从 ContinueWork 迁移的设计点

可以迁移：

- 项目分组 + 会话列表布局。
- 会话详情消息流。
- pending action 消息组件。
- 发送指令输入框。
- 新消息按钮。
- 运行中三点动效。
- “列表流”和“详情流”分离的思想。

需要替换：

- ContinueWork 的 BFF `/api/session-detail` 调用替换为 E2EE RPC。
- ContinueWork 的 `EventSource` SSE 替换为远程 RPC event。
- ContinueWork 的 submit endpoint 映射替换为 RPC 方法映射。
- ContinueWork 的本地错误提示替换为远程连接错误和 RPC 错误。

不能迁移：

- 直接代理 NiumaNotifier Local API。
- 浏览器可见本机 Local API URL。
- 从 Local API endpoint path 推导可执行动作。

## 国际化

Web 控制台必须支持：

- 简体中文。
- 繁体中文。
- 英语。
- 日文。
- 韩文。
- 德文。

默认语言跟随浏览器语言；不在支持列表内时使用英语。

所有页面文案、按钮、状态标签、错误消息、表单 label 和空状态都必须进入 i18n 文件。不得新增不可配置的硬编码界面文案。

## 视觉与交互原则

- 这是操作控制台，不是营销页。
- 首屏登录后直接进入设备列表，不做 hero。
- 设备列表保持密集、可扫描。
- 设备控制台使用稳定三栏布局，避免卡片套卡片。
- 按钮使用图标加必要文字；常见操作优先用 lucide 图标。
- 状态标签短小清晰。
- 危险操作如解绑、吊销 token 需要确认。
- 文本不得溢出按钮、列表项或状态栏。
- 移动端改为 tabs 或钻取式布局，不强行压缩三栏。

## 错误与空状态

设备列表：

- 未绑定设备：展示空列表和“在 NiumaNotifier 设置页登录后会自动绑定设备”。
- 加载失败：展示重试。
- 登录过期：跳转登录。

设备控制台：

- 设备离线：展示最后在线时间，不创建远程连接。
- WebRTC 失败：自动尝试 relay，并提示当前正在使用密文 relay。
- relay 失败：展示重试按钮。
- RPC 超时：展示具体操作失败，允许重试。
- 远程控制被本机关闭：展示权限错误，不隐藏设备。

## 测试策略

第一版至少覆盖：

- 登录成功后进入设备列表。
- refresh token 失败后回到登录页。
- 设备列表展示在线和离线设备。
- 离线设备不能进入连接态。
- 在线设备创建连接后进入控制台。
- WebRTC 失败时切换 relay。
- RemoteRpcClient request/response 匹配。
- RemoteRpcClient event topic 分发。
- 连接断开时 pending request 被 reject。
- 会话列表按项目分组展示。
- 会话详情新消息不在底部时显示“有新消息”。
- pending approval 展示完整正文并可提交。
- pending input 渲染字段并提交 answers。
- 危险设备操作需要确认。
- 六种语言都有对应文案 key。

## 实施拆分建议

1. 初始化 `remote-server/web` React/Vite/TypeScript 项目结构。
2. 建立 i18n 机制和六种语言文件。
3. 实现 HTTP envelope client 和 auth store。
4. 实现登录、注册和登录态刷新。
5. 实现设备列表和设备管理动作。
6. 实现 RemoteSessionClient 连接状态机。
7. 实现 RemoteRpcClient 和 cryptoSession。
8. 实现设备控制台顶部状态栏。
9. 迁移 ContinueWork 风格的项目会话侧栏。
10. 迁移 ContinueWork 风格的会话详情和发送指令。
11. 实现 pending action 远程 RPC 提交。
12. 实现 admin 服务状态页和审计摘要页。
