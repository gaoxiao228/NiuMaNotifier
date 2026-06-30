# NiuMaNotifier 外部客户端接入接口

本文档面向新的外部客户端实现方。外部客户端可以是 Web、iOS、Android、Windows 桌面端或其他桌面端，不限定运行平台。

外部客户端的核心职责：

- 登录远程服务端账号。
- 获取当前账号已绑定设备列表。
- 选择在线设备并创建远程连接。
- 通过 Relay 或 WebRTC DataChannel 与本机 NiuMaNotifier 通信。
- 通过统一 RPC 请求格式访问本机 Local API，例如实时获取 session 列表。

## 1. 基础约定

### 1.1 服务端地址

下文用 `{BASE_URL}` 表示外网远程服务端地址：

```text
https://remote.example.com
```

WebSocket 地址由服务端返回，也可以按协议从 HTTP 地址转换：

```text
https -> wss
http  -> ws
```

### 1.2 外部客户端部署入口

`remote-server` 内置 Web 页面是后台管理页面 `remote-admin`，用于服务部署者或管理员查看服务状态、设备状态和设备 token 管理。它不是终端用户使用的外部远程客户端。

第一版本地和 Docker 默认入口：

| 入口 | 定位 | 说明 |
| --- | --- | --- |
| `http://127.0.0.1:27880/` | `remote-admin` | `remote-server` 内置后台管理页面。 |
| `http://127.0.0.1:27883/` | `remote-client-web` | 独立外部 Web 客户端，面向终端用户连接在线设备和查看远程 session。 |

`remote-client-web` 是独立应用，通过构建期变量 `VITE_REMOTE_SERVER_URL` 指向远程服务端 API 地址。Docker Compose 默认在构建时写入：

```text
VITE_REMOTE_SERVER_URL=http://127.0.0.1:27880
```

注意：`VITE_REMOTE_SERVER_URL` 会被 Vite 在构建期内联到静态资源中，不是容器运行期环境变量。修改该值后需要重新构建 `remote-client-web`。

Web、iOS、Android、Windows 桌面端和其他客户端实现均复用本文档定义的 HTTP / WebSocket / Relay / WebRTC / RPC 协议。`remote-client-web` 只是官方第一版 Web 形态，移动端和桌面端可以按同一协议独立实现。

### 1.3 HTTP 统一响应结构

远程服务端业务 HTTP 接口统一返回：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

约定：

- `code = 0` 表示成功。
- `code != 0` 表示业务失败。
- 业务失败通常仍是 HTTP `200`，客户端必须优先判断 `code`。
- `Authorization` 使用 `Bearer <access_token>`。

协议例外：

- `/desktop-login` 返回 HTML 页面。
- `/ws/client`、`/ws/relay`、`/ws/device` 是 WebSocket 协议，不使用 HTTP JSON envelope。
- 本机 Local API SSE 通过远程 RPC 转成 `local_api.stream.event` notification，不直接暴露原始 SSE 给外部客户端。

### 1.4 常见错误码

| code | 含义 |
| --- | --- |
| `0` | 成功 |
| `100101` | 业务参数校验失败 |
| `200001` | 未登录 |
| `200002` | Token 无效 |
| `200003` | Token 过期 |
| `200401` | 账号不存在 |
| `200402` | 密码错误 |
| `200501` | 邮箱已注册 |
| `210401` | 设备不存在 |
| `210404` | 设备离线 |
| `220401` | 连接不存在 |
| `220402` | 连接已过期 |
| `220403` | 连接权限不足 |
| `220404` | 远程设备不可连接 |

## 2. 账号接口

### 2.1 注册

```http
POST /api/v1/auth/register
Content-Type: application/json
```

请求体：

```json
{
  "email": "user@example.com",
  "password": "11111111"
}
```

字段约束：

- `email`：合法邮箱。
- `password`：8 到 128 位。

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "user": {
      "id": "usr_xxx",
      "email": "user@example.com",
      "role": "user",
      "status": "active"
    }
  }
}
```

说明：

- 服务端可能关闭开放注册，此时返回 `230402`。
- 注册成功后仍建议调用登录接口获取 token。

### 2.2 登录

```http
POST /api/v1/auth/login
Content-Type: application/json
```

请求体：

```json
{
  "email": "user@example.com",
  "password": "11111111"
}
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "access_token": "jwt...",
    "refresh_token": "rft...",
    "expires_at": "2026-06-30T12:00:00.000Z",
    "user": {
      "id": "usr_xxx",
      "email": "user@example.com",
      "role": "user",
      "status": "active"
    }
  }
}
```

客户端处理：

- 保存 `access_token`，后续 HTTP 请求放入 `Authorization: Bearer <access_token>`。
- 保存 `refresh_token`，access token 过期时调用刷新接口。
- 如果接口返回 `200001`、`200002`、`200003`，要求用户重新登录或尝试 refresh。

### 2.3 刷新 token

```http
POST /api/v1/auth/refresh
Content-Type: application/json
```

请求体：

```json
{
  "refresh_token": "rft..."
}
```

成功响应结构与登录接口相同，会返回新的 `access_token` 和 `refresh_token`。

### 2.4 当前用户

```http
GET /api/v1/auth/me
Authorization: Bearer <access_token>
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "user": {
      "id": "usr_xxx",
      "email": "user@example.com",
      "role": "user",
      "status": "active"
    }
  }
}
```

### 2.5 登出

```http
POST /api/v1/auth/logout
Content-Type: application/json
```

请求体：

```json
{
  "refresh_token": "rft..."
}
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

## 3. 设备接口

### 3.1 获取设备列表

```http
GET /api/v1/devices/list
Authorization: Bearer <access_token>
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "id": "dev_xxx",
        "name": "Desk Mac",
        "online": true,
        "last_seen_at": "2026-06-30T12:00:00.000Z",
        "capabilities": {
          "agent_protocol_version": 1,
          "rpc_protocol_version": 1,
          "supports_webrtc": true,
          "supports_relay": true,
          "supports_remote_control": true
        },
        "identity_public_key": {}
      }
    ]
  }
}
```

客户端处理：

- 设备列表页只展示设备，不应该自动建立连接。
- 只有 `online = true` 的设备才允许进入远程 session 页面或创建连接。
- `last_seen_at` 是服务端 presence 或数据库最后在线时间。

### 3.2 吊销设备 token

```http
POST /api/v1/devices/revoke-token
Authorization: Bearer <access_token>
Content-Type: application/json
```

请求体：

```json
{
  "device_id": "dev_xxx"
}
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

说明：

- 吊销后服务端会关闭该设备当前 WebSocket。
- 外部客户端应刷新设备列表。

## 4. 创建远程连接

外部客户端要访问某台在线设备的本机接口，必须先创建连接。

```http
POST /api/v1/connections/create
Authorization: Bearer <access_token>
Content-Type: application/json
```

请求体：

```json
{
  "device_id": "dev_xxx",
  "client_id": "ios-client-uuid-or-installation-id",
  "transport_preference": "relay_first"
}
```

字段说明：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `device_id` | 是 | 设备列表返回的设备 ID。 |
| `client_id` | 是 | 当前外部客户端实例的稳定 ID。Web 可用 localStorage，移动端/桌面端可用安装 ID。 |
| `transport_preference` | 否 | `webrtc_first`、`relay_first`、`relay_only`。当前 Web 实现默认用 `relay_first`，连接建立后 WebRTC 可用时优先走 WebRTC。 |

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "connection_id": "conn_xxx",
    "connection_token": "cnt_xxx",
    "expires_at": "2026-06-30T12:10:00.000Z",
    "expires_in": 600,
    "signaling_url": "wss://remote.example.com/ws/client",
    "relay_url": "wss://remote.example.com/ws/relay"
  }
}
```

连接 token 是短期 token，仅用于本次 `/ws/client` 和 `/ws/relay` 绑定。

### 4.1 ICE 配置

```http
GET /api/v1/connections/ice-config
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "ice_servers": [
      {
        "urls": ["turn:turn.example.com:3478"],
        "username": "user",
        "credential": "password"
      }
    ]
  }
}
```

没有 TURN 配置时 `ice_servers` 为空数组。

## 5. 信令 WebSocket

### 5.1 连接地址

外部客户端使用 `connections/create` 返回的 `signaling_url`：

```text
wss://remote.example.com/ws/client?connection_id=conn_xxx&connection_token=cnt_xxx
```

说明：

- 浏览器 WebSocket 不能稳定携带自定义 `Authorization` header，所以 client 侧 WebSocket 使用短期 `connection_token` 鉴权。
- iOS、Android、Windows 桌面端也建议使用同样方式，保持跨平台协议一致。

### 5.2 服务端转发逻辑

外部客户端连接 `/ws/client` 成功后，远程服务端会向在线设备发送：

```json
{
  "version": 1,
  "type": "connection.invite",
  "id": "msg_conn_xxx",
  "data": {
    "connection_id": "conn_xxx",
    "connection_token": "cnt_xxx",
    "client_id": "ios-client-uuid-or-installation-id",
    "transport_preference": "relay",
    "expires_at": "2026-06-30T12:10:00.000Z"
  }
}
```

设备接受后，外部客户端会在 `/ws/client` 收到：

```json
{
  "version": 1,
  "type": "connection.accept",
  "id": "msg_xxx",
  "data": {
    "connection_id": "conn_xxx",
    "transport": "relay"
  }
}
```

设备拒绝时：

```json
{
  "version": 1,
  "type": "connection.reject",
  "id": "msg_xxx",
  "data": {
    "connection_id": "conn_xxx",
    "reason": "reason_text"
  }
}
```

### 5.3 WebRTC 信令消息

外部客户端发 offer：

```json
{
  "version": 1,
  "type": "signal.offer",
  "id": "sig_1",
  "data": {
    "sdp": "v=0..."
  }
}
```

外部客户端发 ICE candidate：

```json
{
  "version": 1,
  "type": "signal.ice_candidate",
  "id": "sig_2",
  "data": {
    "candidate": "candidate:...",
    "sdp_mid": "0",
    "sdp_mline_index": 0
  }
}
```

设备返回 answer：

```json
{
  "version": 1,
  "type": "signal.answer",
  "id": "sig_3",
  "data": {
    "connection_id": "conn_xxx",
    "sdp": "v=0..."
  }
}
```

设备返回 ICE candidate：

```json
{
  "version": 1,
  "type": "signal.ice_candidate",
  "id": "sig_4",
  "data": {
    "connection_id": "conn_xxx",
    "candidate": "candidate:...",
    "sdp_mid": null,
    "sdp_mline_index": 0
  }
}
```

取消：

```json
{
  "version": 1,
  "type": "signal.cancel",
  "id": "sig_cancel",
  "data": {
    "reason": "client_closed"
  }
}
```

## 6. Relay 通道

Relay 是 WebSocket 双端转发通道。外部客户端和设备分别连接同一个 `connection_id`，服务端只转发 frame。

### 6.1 连接地址

外部客户端：

```text
wss://remote.example.com/ws/relay?connection_id=conn_xxx&connection_token=cnt_xxx&side=client
```

设备端：

```text
wss://remote.example.com/ws/relay?connection_id=conn_xxx&connection_token=cnt_xxx&side=device
```

当 client 和 device 两侧都连接后，双方都会收到：

```json
{
  "version": 1,
  "type": "relay.ready",
  "connection_id": "conn_xxx"
}
```

### 6.2 Relay frame

发送格式：

```json
{
  "version": 1,
  "type": "relay.frame",
  "id": "relay_1",
  "connection_id": "conn_xxx",
  "seq": 1,
  "ciphertext": "base64-json-payload"
}
```

当前实现说明：

- `ciphertext` 字段当前是 JSON payload 的 UTF-8 bytes 再 base64，尚不是最终端到端加密密文。
- `seq` 必须从 `1` 开始单调递增；服务端会拒绝重复或倒退序号。
- 服务端只转发 frame，不理解内部 RPC 业务。

编码示例：

```ts
const ciphertext = base64(utf8(JSON.stringify(payload)))
```

解码示例：

```ts
const payload = JSON.parse(utf8(base64Decode(frame.ciphertext)))
```

## 7. WebRTC DataChannel

WebRTC 可用时，外部客户端应优先通过 DataChannel 发送 RPC payload。

约定：

- DataChannel label：`niuma-e2ee`
- Payload：JSON 字符串。
- 与 Relay 内部 payload 使用同一套 RPC envelope。
- 当 Relay 和 WebRTC 都可用时，推荐发送优先级：`webrtc` > `relay`。
- 接收端应记录实际观测通道，便于调试和去重。

平台说明：

- Web：使用浏览器 `RTCPeerConnection`。
- iOS/Android：使用平台 WebRTC SDK。
- Windows/macOS/Linux 桌面端：可使用 libwebrtc、WebView 内核或其他兼容实现。
- 如果平台暂不实现 WebRTC，可以先实现 `relay_only` 或 `relay_first`。

## 8. 远程 RPC envelope

无论通过 Relay 还是 WebRTC，外部客户端与本机 NiuMaNotifier 的业务通信都使用同一套 plain RPC envelope。

请求：

```json
{
  "version": 1,
  "type": "request",
  "transport": {
    "kind": "relay"
  },
  "id": "rpc_1",
  "method": "local_api.request",
  "params": {}
}
```

响应成功：

```json
{
  "version": 1,
  "type": "response",
  "transport": {
    "kind": "relay"
  },
  "id": "rpc_1",
  "ok": true,
  "result": {}
}
```

响应失败：

```json
{
  "version": 1,
  "type": "response",
  "transport": {
    "kind": "relay"
  },
  "id": "rpc_1",
  "ok": false,
  "error": {
    "code": "method_not_found",
    "message": "unknown RPC method: xxx"
  }
}
```

通知：

```json
{
  "version": 1,
  "type": "notification",
  "transport": {
    "kind": "relay"
  },
  "method": "local_api.stream.event",
  "params": {}
}
```

客户端要求：

- `id` 在当前连接内唯一。
- 请求超时建议 10 到 15 秒。
- `transport.kind` 应标记实际发送通道：`relay` 或 `webrtc`。
- 接收 notification 时不要依赖请求响应顺序。

## 9. 访问本机 Local API

第一版远程访问采用通用 HTTP-like RPC，不为每个业务接口单独映射。

### 9.1 普通请求

RPC method：

```text
local_api.request
```

RPC params：

```json
{
  "method": "GET",
  "path": "/api/v1/session_project_groups?tool=codex&page=1&page_size=20",
  "headers": {},
  "body": null
}
```

完整 RPC 请求：

```json
{
  "version": 1,
  "type": "request",
  "transport": {
    "kind": "webrtc"
  },
  "id": "rpc_2",
  "method": "local_api.request",
  "params": {
    "method": "GET",
    "path": "/api/v1/session_project_groups?tool=codex&page=1&page_size=20",
    "headers": {},
    "body": null
  }
}
```

成功响应的 `result` 是 HTTP-like payload：

```json
{
  "status": 200,
  "headers": {
    "content-type": "application/json"
  },
  "body": {
    "code": 0,
    "message": "ok",
    "data": {
      "list": [],
      "page": 1,
      "page_size": 20,
      "total": 0
    }
  }
}
```

限制：

- `path` 必须以 `/api/` 开头。
- `path` 不能是完整 URL，不能包含 `://`。
- 第一版本机访问策略为 `allow: ["*"]`，即允许所有 `/api/` 路径；后续可改成白名单。
- 会过滤 hop-by-hop headers：`connection`、`upgrade`、`host`、`content-length`、`transfer-encoding`。

### 9.2 SSE 流请求

RPC method：

```text
local_api.stream
```

请求：

```json
{
  "version": 1,
  "type": "request",
  "transport": {
    "kind": "relay"
  },
  "id": "rpc_3",
  "method": "local_api.stream",
  "params": {
    "method": "GET",
    "path": "/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20",
    "headers": {},
    "body": null
  }
}
```

成功响应：

```json
{
  "version": 1,
  "type": "response",
  "transport": {
    "kind": "relay"
  },
  "id": "rpc_3",
  "ok": true,
  "result": {
    "stream_id": "stream_xxx"
  }
}
```

后续事件通过 notification 推送：

```json
{
  "version": 1,
  "type": "notification",
  "transport": {
    "kind": "relay"
  },
  "method": "local_api.stream.event",
  "params": {
    "stream_id": "stream_xxx",
    "seq": 1,
    "event": "session_project_groups",
    "id": "1",
    "data": {
      "list": [],
      "page": 1,
      "page_size": 20,
      "total": 0
    }
  }
}
```

关闭流：

```json
{
  "version": 1,
  "type": "request",
  "transport": {
    "kind": "relay"
  },
  "id": "rpc_4",
  "method": "local_api.stream.close",
  "params": {
    "stream_id": "stream_xxx"
  }
}
```

关闭通知：

```json
{
  "version": 1,
  "type": "notification",
  "transport": {
    "kind": "relay"
  },
  "method": "local_api.stream.closed",
  "params": {
    "stream_id": "stream_xxx",
    "reason": "closed"
  }
}
```

客户端处理：

- `seq` 单调递增，同一 `stream_id` 内可用于丢弃重复或乱序事件。
- 第一版 session 列表只需要订阅 `session_project_groups` 事件。
- 如果连接断开，应重新创建连接并重新订阅流。

## 10. Session 列表接口

外部客户端第一版推荐只实现“查看 session 列表”。

### 10.1 实时 session 列表

通过远程 RPC 调本机：

```json
{
  "method": "GET",
  "path": "/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20",
  "body": null
}
```

对应 RPC method：

```text
local_api.stream
```

事件名：

```text
session_project_groups
```

事件 data 示例：

```json
{
  "list": [
    {
      "tool": "codex",
      "project_name": "NiuMaNotifier",
      "project_path": "/Users/niuma/code/NiuMaNotifier",
      "sessions": [
        {
          "normalized_session_id": "session-1",
          "primary_session_id": "session-1",
          "title": "实现远程连接",
          "runtime_status": "running",
          "status": "active",
          "first_user_message_preview": "继续",
          "latest_event_summary": null,
          "subagent_count": 0
        }
      ]
    }
  ],
  "page": 1,
  "page_size": 20,
  "total": 1
}
```

### 10.2 一次性 session 列表

如果不需要实时更新，可以使用：

```json
{
  "method": "GET",
  "path": "/api/v1/session_project_groups?tool=codex&page=1&page_size=20",
  "body": null
}
```

对应 RPC method：

```text
local_api.request
```

## 11. 推荐连接流程

### 11.1 通用流程

1. `POST /api/v1/auth/login` 获取 `access_token`。
2. `GET /api/v1/devices/list` 展示设备列表。
3. 用户选择在线设备。
4. `POST /api/v1/connections/create` 创建连接。
5. 连接 `/ws/client` 等待 `connection.accept`。
6. 同时准备 Relay 和 WebRTC：
   - Relay：连接 `/ws/relay?side=client`，等待 `relay.ready`。
   - WebRTC：通过 `/ws/client` 交换 offer/answer/ICE，等待 DataChannel open。
7. 先用更快可用的通道发 RPC；当 Relay 和 WebRTC 都可用时，优先使用 WebRTC。
8. 用 `local_api.stream` 订阅 `/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20`。
9. 根据 `local_api.stream.event` 更新 session 列表 UI。
10. 页面退出或切换设备时关闭 stream、Relay/WebRTC、信令 socket。

### 11.2 Relay-only 最小实现

移动端或桌面端如果第一版不想接 WebRTC，可以实现最小链路：

1. 登录。
2. 设备列表。
3. 创建连接，`transport_preference = "relay_only"` 或 `"relay_first"`。
4. 连接 `/ws/client` 等待设备 accept。
5. 连接 `/ws/relay?side=client`。
6. 收到 `relay.ready` 后，通过 Relay frame 发送 plain RPC。
7. 订阅 session stream。

这个实现不需要 WebRTC，但所有业务流量会经过远程服务端 Relay 转发。

## 12. 客户端状态建议

外部客户端建议维护以下状态：

```ts
type RemoteConnectionState = {
  deviceId: string
  connectionId: string | null
  connectionStatus: 'idle' | 'connecting' | 'accepted' | 'rejected' | 'closed' | 'error'
  relayStatus: 'idle' | 'connecting' | 'open' | 'closed' | 'error'
  webRtcStatus: 'idle' | 'connecting' | 'open' | 'closed' | 'error'
  activeTransport: 'idle' | 'relay' | 'webrtc'
  sessionStreamId: string | null
}
```

UI 建议：

- 设备列表页只展示设备，不自动建连接。
- 进入某个设备后才创建连接。
- session 页面以 session 列表为主。
- 连接状态、当前通道、Relay/WebRTC 状态统一放在顶部紧凑栏。
- Token 失效时直接要求重新登录。

## 13. 平台差异

### Web

- WebSocket 不能可靠设置自定义 Authorization header，因此 `/ws/client` 和 client 侧 `/ws/relay` 只能依赖 `connection_token` query。
- 可使用浏览器原生 `RTCPeerConnection` 和 DataChannel。
- `client_id` 可存在 localStorage。

### iOS / Android

- HTTP token 存 Keychain / Keystore。
- `client_id` 建议使用安装级 UUID。
- WebSocket 和 WebRTC 使用平台 SDK。
- 即使平台 WebSocket 支持 header，也建议保持 query token 方式，避免协议分叉。

### Windows / macOS / Linux 桌面端

- HTTP token 存系统凭据管理器或应用安全存储。
- WebRTC 可选；第一版可以先 Relay-only。
- 如果使用 WebView，行为基本等同 Web。

## 14. 与当前实现的注意点

- 远程服务端 HTTP API 基本遵循统一 envelope。
- `/desktop-login` 是 HTML 页面例外。
- WebSocket close reason 里可能是 JSON 字符串，例如 `{"code":220403,"message":"连接权限不足"}`；客户端应尽量解析。
- 当前 Relay 的 `ciphertext` 字段还不是最终 E2EE 密文，只是 base64 JSON payload；后续协议升级时应保持 frame 外层结构不变。
- 当前本机 Local API 远程访问策略为 allow all `/api/` 路径，后续可能改成白名单。新客户端不应依赖非必要本机接口。
