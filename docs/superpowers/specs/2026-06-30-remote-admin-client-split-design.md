# 远程后台与外部客户端拆分设计

## 背景

当前 `remote-server` 的 Web 页面最初承担了两类职责：

- 远程服务端后台管理：账号登录、设备列表、设备在线状态、设备 token 管理、服务端部署状态。
- 外部客户端：连接在线设备、建立 Relay/WebRTC 通道、查看远程 session 列表。

这两个职责面向的用户、部署方式和产品节奏不同。继续放在同一个 Web 应用里，会导致后台管理页面和终端用户客户端互相牵制：设备列表页到底是管理页还是使用页、连接状态到底是诊断信息还是用户主界面、Docker 中暴露出来的页面到底是服务后台还是外部客户端，都会变得模糊。

本设计修正早期 `remote-server/web` 同时作为外部 Web 控制台的方案。后续统一使用 React 技术栈，不使用 Vue。

## 决策

采用方案 A：

```text
remote-server 自带 Web
  -> 定位为 remote-admin，作为外部服务器后台管理页面

external web client
  -> 独立为 remote-client-web，作为终端用户使用的远程客户端
  -> 使用 React + Ant Design X
  -> 单独容器部署
```

最终前端统一使用 React：

- `remote-admin`：React + TypeScript，后台管理风格，优先轻量、密集、可扫描。
- `remote-client-web`：React + TypeScript + Ant Design X，面向远程 AI session 使用体验。

不再引入 Vue，也不使用 Ant Design Vue。

## 目标

- 明确 `remote-server` 容器暴露的 Web 页面是后台管理，不是终端用户外部客户端。
- 新增独立 `remote-client-web` 应用和 Docker 镜像，专门接入外部服务器 API。
- 保持外部客户端协议平台无关：Web、iOS、Android、Windows 客户端都依赖同一套 HTTP / WebSocket / Relay / WebRTC / RPC 协议。
- 抽离可复用的远程客户端协议模块，避免后台管理和外部客户端重复造轮子。
- 第一版外部 Web 客户端只实现闭环：登录、设备列表、连接在线设备、实时查看 session 列表。

## 非目标

- 第一版不重做完整后台用户管理系统。
- 第一版不做 session 详情、发送指令、中断任务、授权/输入处理。
- 第一版不做移动端原生客户端实现，只保证协议文档可供移动端实现。
- 第一版不要求移除现有所有 `remote-server/web` 代码；先完成职责收敛和新客户端拆分。
- 第一版不实现新的后端业务接口，优先复用当前已有 API。

## 产品边界

### remote-server

`remote-server` 是外部服务器后端和控制面，负责：

- 账号注册、登录、刷新 token、登出。
- 设备绑定记录、设备 presence、设备 token 吊销。
- 连接创建、连接 token 颁发、WebSocket 信令。
- 应用层 WebSocket Relay。
- WebRTC ICE 配置。
- 托管后台管理静态页面。

它不应该包含终端用户远程 session 使用界面的核心体验。

### remote-admin

`remote-admin` 随 `remote-server` 容器部署，面向服务部署者或管理员。

第一版页面：

- 登录页。
- 设备管理页：
  - 设备名。
  - 在线 / 离线。
  - 最后在线时间。
  - capabilities。
  - 吊销 device token。
  - 刷新列表。
- 服务状态页：
  - 服务版本。
  - `REMOTE_SERVER_PUBLIC_URL`。
  - 注册模式。
  - TURN 是否启用。
  - PostgreSQL / Redis 基础状态。
- 账号页：
  - 当前账号。
  - 登出。

明确不做：

- 不在设备列表页建立远程连接。
- 不显示 session 列表。
- 不显示 Relay/WebRTC 用户连接状态。
- 不承担终端用户远程控制台职责。

### remote-client-web

`remote-client-web` 是独立外部 Web 客户端，面向终端用户。

第一版页面：

- 登录页。
- 设备列表页。
- 远程 session 列表页。

第一版能力：

- 登录远程服务端账号。
- 获取设备列表。
- 用户点击在线设备后创建远程连接。
- 同时支持 Relay 和 WebRTC DataChannel。
- Relay 可先可用，WebRTC 探活成功后优先使用 WebRTC。
- 通过 `local_api.stream` 订阅本机 `/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20`。
- 远程 session 页面以 session 列表为主，连接信息统一放在顶部紧凑状态栏。

## 技术栈

### 统一前端栈

```text
React
TypeScript
Vite
Vitest
Playwright 或浏览器人工验收
```

### remote-admin

后台管理页面不强依赖 Ant Design X，因为 Ant Design X 的价值主要在 AI 会话体验。第一版可以继续使用当前 React 实现并逐步收敛职责。

允许使用：

- React。
- 普通 CSS 或后续引入 Ant Design。
- lucide-react 图标。

### remote-client-web

外部 Web 客户端使用：

```text
React
Ant Design X
Ant Design
Vite
```

使用 Ant Design X 的原因：

- 外部客户端未来会扩展到 AI 会话详情、消息流、用户输入、授权处理。
- Ant Design X 的 Bubble、Sender、Conversations 等组件更适合远程 session 客户端。
- 当前阶段只做 session 列表时，不强行做聊天页，但组件体系先按 AI 客户端方向选型。

## 代码结构

推荐在当前仓库内拆成多应用，保持 API 和协议演进同步。

```text
remote-server/
  src/
    modules/
    ws/
  admin-web/
    index.html
    package.json
    src/
      api/
      auth/
      devices/
      server/
      shared/
  Dockerfile

remote-client-web/
  index.html
  package.json
  Dockerfile
  nginx.conf
  src/
    api/
      authApi.ts
      devicesApi.ts
      connectionsApi.ts
      httpClient.ts
    remote/
      connectionClient.ts
      relayTransport.ts
      webrtcTransport.ts
      remoteTransport.ts
      plainRpcClient.ts
      remoteLocalApiClient.ts
      remoteDeviceSessionController.ts
    auth/
    devices/
    sessions/
    i18n/
    shared/

remote-client-sdk/
  package.json
  src/
    api/
    transport/
    rpc/
    types/
```

第一版可以先不创建独立 `remote-client-sdk` 包，而是把协议模块放在 `remote-client-web/src/remote`。当 iOS、Android、Windows 客户端开始接入时，再把协议类型和测试抽成 `remote-client-sdk` 或文档化协议包。

## 复用与迁移

从当前 `remote-server/web` 迁移到 `remote-client-web` 的代码：

- `authApi.ts`
- `devicesApi.ts`
- `connectionsApi.ts`
- `httpClient.ts`
- `clientId.ts`
- `connectionClient.ts`
- `relayTransport.ts`
- `webrtcTransport.ts`
- `remoteTransport.ts`
- `plainRpcClient.ts`
- `remoteLocalApiClient.ts`
- `remoteDeviceSessionController.ts`
- `remoteSessionTypes.ts`
- `RemoteSessionGroupsView.tsx`

迁移后，`remote-server/admin-web` 不再依赖：

- `remoteDeviceSessionController`
- `relayTransport`
- `webrtcTransport`
- `remoteLocalApiClient`
- `RemoteSessionGroupsView`

## API 依赖

`remote-client-web` 只依赖外部客户端接入文档定义的协议：

```text
HTTP:
  POST /api/v1/auth/register
  POST /api/v1/auth/login
  POST /api/v1/auth/refresh
  POST /api/v1/auth/logout
  GET  /api/v1/auth/me
  GET  /api/v1/devices/list
  POST /api/v1/connections/create
  GET  /api/v1/connections/ice-config

WebSocket:
  GET /ws/client
  GET /ws/relay

RPC:
  rpc.ping
  local_api.request
  local_api.stream
  local_api.stream.close
```

第一版 session 列表只使用：

```text
local_api.stream
GET /api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20
```

## 部署设计

### remote-server 容器

职责：

- Fastify API。
- WebSocket 信令。
- WebSocket Relay。
- 静态托管 `remote-admin`。

示例：

```text
https://remote.example.com/
  -> remote-admin

https://remote.example.com/api/v1/*
  -> remote-server API

wss://remote.example.com/ws/*
  -> remote-server WebSocket
```

### remote-client-web 容器

职责：

- 托管外部 Web 客户端静态资源。
- 通过环境变量配置远程服务端地址。

示例：

```text
https://client.example.com/
  -> remote-client-web

VITE_REMOTE_SERVER_URL=https://remote.example.com
```

Docker 部署可以使用两个容器：

```text
niuma-remote-server
niuma-remote-client-web
```

也可以由同一个反向代理分发：

```text
https://remote.example.com/admin  -> remote-admin
https://remote.example.com/client -> remote-client-web
```

但推荐第一版用两个明确入口，避免后台和客户端路径混淆。

## 安全边界

- `remote-admin` 与 `remote-client-web` 都使用账号登录，但页面权限不同。
- 后端 API 仍由 `access_token` 鉴权。
- WebSocket client 侧使用短期 `connection_token`，不依赖浏览器无法稳定设置的 Authorization header。
- `remote-client-web` 不应获得服务端管理能力，例如用户管理、注册模式修改、审计配置。
- 远程 Local API 第一版仍是 allow all `/api/` 路径；后续需要引入白名单配置。
- Relay frame 当前字段名为 `ciphertext`，但实现仍是 base64 JSON payload；后续 E2EE 升级不能改变外部客户端的页面职责边界。

## UI 原则

### remote-admin

后台管理界面应密集、克制、适合扫描：

- 设备表格优先，不做卡片墙。
- 状态信息可排序、可过滤。
- 页面不主动建立设备连接。
- 不展示终端用户 session 内容。

### remote-client-web

外部客户端界面应以远程使用为核心：

- 设备列表页只负责选择设备。
- 远程 session 页以 session 列表为主。
- 连接状态、Relay 状态、WebRTC 状态、当前通道统一显示在顶部紧凑栏。
- 后续 session 详情页可以使用 Ant Design X 的 AI 会话组件。
- 所有 UI 文案必须支持简体中文、繁体中文、英语、日文、韩文、德文。

## 第一版实施范围

第一版只做拆分闭环：

1. 新建 `remote-client-web` React + Ant Design X 应用。
2. 迁移现有外部客户端通信模块。
3. 实现登录、设备列表、远程 session 列表。
4. `remote-server/web` 改名或重定位为 `admin-web`，移除远程 session 客户端职责。
5. Docker Compose 增加 `remote-client-web` 服务。
6. 更新接入文档和部署文档。

## 实施路径 / 最终路径

第一版实施路径以“职责先清晰、目录少搬迁”为原则：

- 保留 `remote-server/web` 作为 `remote-admin` 的源码目录，继续随 `remote-server` 构建和部署。
- `remote-server` 本地和 Docker 默认入口 `http://127.0.0.1:27880/` 只表达后台管理能力，不再承担外部客户端职责。
- 新增仓库根目录 `remote-client-web/`，作为独立外部客户端源码目录、构建上下文和 Docker 镜像入口。
- `remote-client-web` 本地开发入口使用 `http://127.0.0.1:27882/`，Docker host 入口使用 `http://127.0.0.1:27883/`。
- `remote-client-web` 通过构建期 `VITE_REMOTE_SERVER_URL` 指向远程服务端 API 地址；Docker Compose 默认写入 `http://127.0.0.1:27880`。

最终路径保持该职责边界：`remote-server/web` 是随服务端内置的 `remote-admin`，`remote-client-web/` 是终端用户外部客户端。后续即使重命名 `remote-server/web` 为 `remote-server/admin-web`，也只作为目录语义优化，不改变部署边界和协议边界。

## 测试策略

自动测试：

- HTTP client envelope 解包和 token 失效处理。
- 设备列表页不创建连接。
- 远程 session 页创建连接并订阅 session stream。
- Relay ready 后能发送 RPC。
- WebRTC ready 后优先走 WebRTC。
- session stream event 能更新列表。
- token 失效时回到登录页。

构建验证：

- `remote-server` build。
- `remote-admin` build。
- `remote-client-web` build。
- Docker 镜像构建。

手动验收：

1. 启动 `remote-server` Docker。
2. 启动本机 NiuMaNotifier，并绑定设备。
3. 打开 `remote-admin`，确认只能看到后台管理能力。
4. 打开 `remote-client-web`。
5. 登录账号。
6. 查看设备列表。
7. 点击在线设备。
8. 远程 session 页面显示 session 列表。
9. 顶部状态栏显示连接状态、当前通道、Relay/WebRTC 状态。
10. 如果 WebRTC 不可用，Relay-only 仍可显示 session 列表。

## 与旧设计的关系

本设计取代早期“`remote-server/web` 同时作为外部 Web 控制台”的设计。后续实现应以本设计为准：

- `remote-server/web` 不再继续扩展终端用户远程 session 能力。
- 外部 Web 客户端独立为 `remote-client-web`。
- 前端统一使用 React；不使用 Vue。
- 外部客户端使用 React + Ant Design X。
