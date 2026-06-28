# NiuMaNotifier 远程服务端设计

## 背景

远程访问总方案已经确定：本机 NiumaNotifier 通过 Remote Agent 主动连接外网服务端，Web 控制台登录同账号后发现设备，并通过端到端加密 RPC 完整控制本机。服务端只负责账号、设备目录、在线状态、WebRTC 信令和密文 relay，不解析会话正文、指令正文或授权正文。

本文档补齐服务端实现细节，包括登录方式、token 模型、模块架构、数据存储、Redis 状态、WebSocket 连接、Docker 自托管部署和 API 规范。

## 目标

- 第一版服务端使用 Node.js / TypeScript 技术栈。
- 第一版登录方式为邮箱 + 密码。
- 普通 HTTP API 遵循统一 `{ code, message, data }` 响应结构。
- 支持官方服务和自托管服务使用同一套协议。
- 自托管优先支持 Docker Compose 部署。
- PostgreSQL 持久化账号、设备、token、连接记录和服务端可见审计摘要。
- Redis 保存在线状态、连接协商状态、relay 路由和限流计数。
- 服务端不保存 E2EE RPC 明文，不提供公网 Local API 代理。

## 非目标

- 第一版不做 OAuth/OIDC。
- 第一版不做 Magic Link。
- 第一版不做邮箱验证码确认和忘记密码。
- 第一版不做多因素认证。
- 第一版不做团队组织、跨账号共享设备或计费。
- 第一版不把 relay 单独拆成微服务。
- 第一版不要求多区域部署。

## 技术栈

```text
Runtime: Node.js LTS
Language: TypeScript
HTTP: Fastify
WebSocket: @fastify/websocket 或 ws
Database: PostgreSQL
Query / ORM: Drizzle ORM + drizzle-kit
Cache / Presence: Redis
Validation: Zod
Password hash: Argon2id
Access token: JWT
Refresh token: 随机高熵 token + 服务端哈希
Device token: 随机高熵 token + 服务端哈希
Web 控制台: React + Vite
Container: Docker + Docker Compose
TURN: coturn 或外部 TURN 服务
```

服务端采用模块化单体。这样第一版部署简单，代码边界清楚，后续如果 relay 或 signaling 压力变大，再拆分独立服务。

## 仓库结构

建议在当前仓库新增 `remote-server/`：

```text
remote-server/
  Dockerfile
  docker-compose.yml
  .env.example
  package.json
  tsconfig.json
  drizzle.config.ts
  src/
    app.ts
    server.ts
    config.ts
    db/
      client.ts
      schema.ts
      migrate.ts
    shared/
      response.ts
      errors.ts
      validation.ts
      crypto.ts
      time.ts
    modules/
      auth/
        auth.routes.ts
        auth.service.ts
        auth.schemas.ts
        password.service.ts
        token.service.ts
      devices/
        devices.routes.ts
        devices.service.ts
        device-token.service.ts
        presence.service.ts
      connections/
        connections.routes.ts
        connection.service.ts
        signaling.service.ts
        relay.service.ts
      admin/
        admin.routes.ts
        bootstrap.service.ts
        settings.service.ts
    ws/
      device-socket.ts
      client-socket.ts
      relay-socket.ts
  web/
    index.html
    src/
      main.tsx
      app.tsx
      api/
      auth/
      devices/
      remote/
      audit/
      admin/
      pages/
      i18n/
      shared/
  migrations/
```

职责边界：

- `shared/response.ts`：统一 API envelope。
- `shared/errors.ts`：错误码和错误消息台账。
- `shared/validation.ts`：Zod 校验和协议层参数错误转换。
- `modules/auth`：账号、密码、access token、refresh token。
- `modules/devices`：设备注册、设备 token、设备列表、设备在线状态。
- `modules/connections`：连接创建、ICE 配置、信令、relay 路由。
- `ws/`：WSS 连接生命周期，不放业务规则。
- `web/`：Web 控制台，不直接访问数据库；页面与组件细节见 [NiuMaNotifier 远程 Web 控制台设计](./2026-06-28-remote-web-client-design.md)。

## 账号与登录

第一版支持邮箱 + 密码登录。

支持能力：

- 邮箱注册。
- 邮箱 + 密码登录。
- access token。
- refresh token。
- refresh token 轮换。
- 退出当前会话。
- 退出所有会话。
- 自托管 bootstrap 管理员初始化。

第一版不支持：

- 邮箱验证码确认。
- 忘记密码。
- OAuth/OIDC。
- Magic Link。
- 多因素认证。

### 注册模式

服务端通过 `REGISTRATION_MODE` 控制注册策略：

```text
open          允许任意用户注册，适合官方服务 MVP 或内网测试。
admin_invite 只允许管理员创建用户，适合默认自托管。
disabled      禁止新用户注册，适合完全私有部署。
```

默认值：

```text
官方服务: open
自托管 docker-compose 示例: admin_invite
```

### 密码存储

数据库不保存明文密码。`users` 表保存：

```text
password_hash
password_algo
password_updated_at
```

密码哈希算法：

- 第一版使用 Argon2id。
- 参数通过配置控制，默认使用适合服务端交互式登录的成本。
- `password_algo` 预留后续升级空间。

## Token 模型

### access token

用途：

- Web 控制台普通 HTTP API。
- Web 控制台 WSS 信令连接。
- 浏览器登录设备绑定流程中的用户身份校验。

属性：

- JWT。
- 短有效期，建议 15 分钟。
- payload 包含 `sub`、`session_id`、`role`、`iat`、`exp`。
- 使用非对称签名，服务端配置 `JWT_PRIVATE_KEY` 和 `JWT_PUBLIC_KEY`。

### refresh token

用途：

- Web 控制台刷新登录态。
- 浏览器 Web 登录态刷新。

属性：

- 随机高熵字符串，不使用 JWT。
- 服务端只保存 token 哈希。
- 长有效期，建议 30 天。
- 每次刷新时轮换，旧 token 标记为 revoked。
- 支持退出当前会话和退出所有会话。

### device token

用途：

- Remote Agent 常驻 WSS 设备连接。

属性：

- 随机高熵字符串，不使用 JWT。
- 服务端只保存 token 哈希。
- 长有效期，直到退出账号、解绑设备或吊销。
- 只能用于 `/ws/device`，不能调用 Web 用户 API。
- 设备 token 被吊销后，服务端主动关闭对应设备连接。

### connection token

用途：

- Web 客户端和设备建立某一次远程连接。
- relay fallback 鉴权。

属性：

- 短有效期，建议 2 分钟内完成建连。
- 绑定 `connection_id`、`user_id`、`device_id`、`client_id`。
- 建连后可换成连接内短期 session secret。
- 不可用于账号 API、设备 API 或其他连接。

### desktop login token

用途：

- 本机 NiumaNotifier 设置页点击登录后，创建一次性桌面绑定会话。
- 桌面端轮询绑定结果。

属性：

- 随机高熵字符串，不使用 JWT。
- 服务端只保存 token 哈希。
- 短有效期，建议 10 分钟。
- 只能用于查询对应 `desktop_login_sessions` 的完成状态。
- 不能调用账号 API、设备 API、WebSocket 或 relay。

## PostgreSQL 数据模型

### users

```text
id uuid primary key
email text unique not null
password_hash text not null
password_algo text not null
role text not null
status text not null
created_at timestamptz not null
updated_at timestamptz not null
password_updated_at timestamptz not null
```

`role`：

```text
admin
user
```

`status`：

```text
active
disabled
```

### refresh_tokens

```text
id uuid primary key
user_id uuid not null references users(id)
token_hash text unique not null
client_id text not null
user_agent text
ip text
expires_at timestamptz not null
revoked_at timestamptz
rotated_from_id uuid references refresh_tokens(id)
created_at timestamptz not null
```

### devices

```text
id uuid primary key
user_id uuid not null references users(id)
name text not null
fingerprint_hash text not null
token_hash text unique not null
status text not null
last_seen_at timestamptz
capability_json jsonb not null
created_at timestamptz not null
updated_at timestamptz not null
revoked_at timestamptz
```

`status`：

```text
active
revoked
```

`capability_json` 保存服务端可见能力摘要，例如：

```json
{
  "agent_protocol_version": 1,
  "rpc_protocol_version": 1,
  "supports_webrtc": true,
  "supports_relay": true,
  "supports_remote_control": true
}
```

### remote_connections

```text
id uuid primary key
user_id uuid not null references users(id)
device_id uuid not null references devices(id)
client_id text not null
status text not null
transport_preference text not null
transport_selected text
expires_at timestamptz not null
created_at timestamptz not null
connected_at timestamptz
closed_at timestamptz
close_reason text
```

`status`：

```text
pending
signaling
connected
closed
expired
failed
```

`transport_selected`：

```text
webrtc
relay
```

### desktop_login_sessions

用于“本机点击登录，浏览器完成账号登录并绑定设备”的一次性流程。

```text
id uuid primary key
request_id text unique not null
poll_token_hash text unique not null
desktop_public_key text not null
device_name text not null
fingerprint_hash text not null
capability_json jsonb not null
status text not null
user_id uuid references users(id)
device_id uuid references devices(id)
encrypted_result_json jsonb
expires_at timestamptz not null
completed_at timestamptz
consumed_at timestamptz
created_at timestamptz not null
```

`status`：

```text
pending
completed
consumed
expired
cancelled
```

服务端只保存 `poll_token`、`device_fingerprint` 和最终 `device_token` 的哈希。绑定完成时，服务端用 `desktop_public_key` 把包含 `device_token` 的结果加密后保存到 `encrypted_result_json`；桌面端 poll 拿到密文后本地解密。`device_token` 明文不持久化。

### audit_events

服务端审计只记录服务端可见摘要，不记录 E2EE RPC 明文。

```text
id uuid primary key
user_id uuid references users(id)
device_id uuid references devices(id)
client_id text
event_type text not null
result text not null
ip text
user_agent text
metadata_json jsonb not null
created_at timestamptz not null
```

事件示例：

```text
auth.register
auth.login
auth.logout
auth.logout_all
device.register
device.rename
device.unbind
device.revoke_token
connection.create
connection.signaling_started
connection.webrtc_connected
connection.relay_started
connection.closed
admin.bootstrap_created
```

### server_settings

用于保存自托管运行时可见配置。

```text
key text primary key
value_json jsonb not null
updated_at timestamptz not null
```

第一版可以只读环境变量，不提供 UI 修改；表结构预留给后续管理后台。

## Redis 状态模型

Redis 只保存短期状态和在线状态，不作为持久状态源。

### 设备在线

```text
presence:device:{device_id}
```

值：

```json
{
  "user_id": "usr_...",
  "device_id": "dev_...",
  "socket_id": "sock_...",
  "server_instance_id": "srv_...",
  "last_seen_at": "2026-06-28T12:00:00Z",
  "capabilities": {}
}
```

TTL 建议为心跳间隔的 3 倍。Remote Agent 断线或心跳过期后，设备视为离线。

### 连接协商

```text
connection:{connection_id}
```

值：

```json
{
  "user_id": "usr_...",
  "device_id": "dev_...",
  "client_id": "web_...",
  "status": "signaling",
  "created_at": "2026-06-28T12:00:00Z",
  "expires_at": "2026-06-28T12:02:00Z"
}
```

TTL 建议 2 到 5 分钟。过期后不能继续交换信令。

### 桌面登录绑定

```text
desktop_login:{request_id}
```

值：

```json
{
  "status": "pending",
  "expires_at": "2026-06-28T12:10:00Z"
}
```

该 key 用于加速浏览器登录页和桌面轮询状态判断，持久状态仍以 PostgreSQL 的 `desktop_login_sessions` 为准。

### relay 路由

```text
relay:{connection_id}
```

值：

```json
{
  "client_socket_id": "sock_client",
  "device_socket_id": "sock_device",
  "server_instance_id": "srv_...",
  "started_at": "2026-06-28T12:00:30Z"
}
```

### 限流

```text
rate_limit:auth_login:ip:{ip}
rate_limit:auth_login:email:{email_hash}
rate_limit:connection_create:user:{user_id}
```

第一版至少对登录和连接创建做限流，避免官方服务被暴力尝试或连接风暴打穿。

## HTTP API 规范

所有普通业务接口返回统一 JSON envelope：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

规则：

- `code = 0` 是唯一成功码。
- 业务失败使用 `HTTP 200 + 非 0 code`。
- 认证失败、权限失败、账号不存在、设备离线、连接过期都属于业务失败。
- 协议层 JSON 解析失败或参数类型错误使用 `HTTP 400`，响应体仍保持 envelope。
- 路由不存在使用 `HTTP 404`，响应体仍保持 envelope。
- 系统异常使用 `HTTP 500`，响应体仍保持 envelope。
- 查询类接口使用 GET。
- 创建、修改、删除、业务动作使用 POST。
- 禁止路径动态参数，业务参数通过 GET 查询参数或 POST 请求体传递。

### 错误码

通用：

```text
100001 协议层参数错误
100002 协议层缺少必填参数
100003 协议层参数类型错误
100004 协议层参数格式错误
100101 业务参数校验失败
900001 系统异常
900002 数据库异常
900003 下游服务异常
900004 服务不可用
900005 接口不存在
```

认证与账号：

```text
200001 未登录
200002 Token 无效
200003 Token 已过期
200004 无权限访问
200101 邮箱格式错误
200102 密码格式错误
200401 账号不存在
200402 密码错误
200403 账号已禁用
200501 邮箱已注册
```

设备：

```text
210401 设备不存在
210402 设备已解绑
210403 设备不属于当前账号
210404 设备离线
210405 设备 token 无效
210406 设备 token 已吊销
```

桌面登录绑定：

```text
240401 桌面登录会话不存在
240402 桌面登录会话已过期
240403 桌面登录轮询 token 无效
240404 桌面登录会话尚未完成
240405 桌面登录会话已被消费
```

连接：

```text
220401 连接不存在
220402 连接已过期
220403 连接权限不足
220404 远程设备不可连接
220405 信令会话不存在
220406 relay 会话不存在
```

管理：

```text
230401 管理员权限不足
230402 注册模式不允许当前操作
230501 Bootstrap 管理员已存在
```

## HTTP API 设计

### POST /api/v1/auth/register

用途：注册账号。受 `REGISTRATION_MODE` 控制。

请求：

```json
{
  "email": "user@example.com",
  "password": "password"
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "user": {
      "id": "usr_...",
      "email": "user@example.com",
      "role": "user"
    }
  }
}
```

### POST /api/v1/auth/login

请求：

```json
{
  "email": "user@example.com",
  "password": "password"
}
```

该接口只由浏览器 Web 登录页调用。本机 NiumaNotifier 设置页不内嵌邮箱密码表单，也不直接调用该接口。

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "access_token": "jwt",
    "refresh_token": "opaque_refresh_token",
    "expires_in": 900,
    "user": {
      "id": "usr_...",
      "email": "user@example.com",
      "role": "user"
    }
  }
}
```

### POST /api/v1/auth/refresh

请求：

```json
{
  "refresh_token": "opaque_refresh_token"
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "access_token": "new_jwt",
    "refresh_token": "new_opaque_refresh_token",
    "expires_in": 900
  }
}
```

旧 refresh token 必须在刷新成功后标记为 revoked。

### POST /api/v1/auth/logout

请求：

```json
{
  "refresh_token": "opaque_refresh_token"
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

### POST /api/v1/auth/logout-all

请求：

```json
{}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

该接口吊销当前用户全部 refresh token，不吊销 device token。设备解绑或退出本机账号时才吊销 device token。

### GET /api/v1/auth/me

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "user": {
      "id": "usr_...",
      "email": "user@example.com",
      "role": "user"
    }
  }
}
```

### POST /api/v1/desktop-login/start

用途：本机 NiumaNotifier 设置页点击“登录”后创建一次性浏览器登录绑定会话。该接口不需要账号登录态。

请求：

```json
{
  "device_name": "NiuMa MacBook",
  "device_fingerprint": "stable-local-fingerprint",
  "desktop_public_key": "base64-public-key",
  "capabilities": {
    "agent_protocol_version": 1,
    "rpc_protocol_version": 1,
    "supports_webrtc": true,
    "supports_relay": true,
    "supports_remote_control": true
  }
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "request_id": "dlr_...",
    "poll_token": "opaque_desktop_login_poll_token",
    "login_url": "https://remote.example.com/desktop-login?request_id=dlr_...",
    "expires_in": 600
  }
}
```

本机随后打开系统浏览器访问 `login_url`，并使用 `poll_token` 轮询绑定结果。`poll_token` 不放入浏览器 URL。`desktop_public_key` 是本次绑定会话的一次性公钥，对应私钥只保存在本机内存中，用于解密 poll 返回的绑定结果。

设备指纹生成规则：

```text
第一次启用远程功能时，本机生成随机 device_install_id。
device_install_id 明文只保存在本机应用配置。
device_fingerprint = sha256("niuma-device-v1" + remote_server_origin + device_install_id)
```

服务端保存 `device_fingerprint` 的加 pepper 哈希，不保存原始 fingerprint。不同远程服务端 origin 会得到不同 fingerprint，避免官方服务和自托管服务之间关联同一台机器。

### GET /desktop-login?request_id=...

用途：浏览器页面入口，不是 JSON API。

行为：

```text
如果浏览器未登录:
  展示邮箱密码登录页。

如果浏览器已登录:
  自动完成该 request_id 对应的设备绑定。
  展示“设备已绑定，可以返回 NiumaNotifier”的结果页。
```

首次登录不要求本机弹框确认，也不要求额外确认绑定。用户点击本机登录按钮并在浏览器完成账号登录，即视为授权绑定当前设备。

### POST /api/v1/desktop-login/complete

用途：浏览器登录态下完成桌面绑定。该接口由 Web 页面调用，需要 access token。

请求：

```json
{
  "request_id": "dlr_..."
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

服务端处理：

```text
校验 request_id 存在且未过期。
读取 desktop_login_sessions 中的 device_name、fingerprint_hash、capability_json。
按当前 user_id + fingerprint_hash 查找 active device。
如果存在，更新设备名和 capabilities。
如果不存在，创建 devices 记录。
生成新的 device_token，保存 token_hash 到 devices。
用 desktop_public_key 加密 user、device 和 device_token 结果，保存到 desktop_login_sessions.encrypted_result_json。
标记会话为 completed。
写入 audit_events: device.register 或 device.rebind。
```

### POST /api/v1/desktop-login/poll

用途：本机 NiumaNotifier 轮询浏览器登录绑定结果。该接口不需要账号登录态，但必须提供 `poll_token`。

请求：

```json
{
  "request_id": "dlr_...",
  "poll_token": "opaque_desktop_login_poll_token"
}
```

未完成：

```json
{
  "code": 240404,
  "message": "桌面登录会话尚未完成",
  "data": {
    "status": "pending",
    "expires_in": 540
  }
}
```

完成：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "encrypted_result": {
      "alg": "x25519-xsalsa20poly1305-v1",
      "nonce": "base64",
      "ciphertext": "base64"
    }
  }
}
```

本机使用创建绑定会话时生成的一次性私钥解密 `encrypted_result`，得到：

```json
{
  "user": {
    "id": "usr_...",
    "email": "user@example.com",
    "role": "user"
  },
  "device": {
    "id": "dev_...",
    "name": "NiuMa MacBook"
  },
  "device_token": "opaque_device_token"
}
```

成功返回加密结果后，服务端将该桌面登录会话标记为 `consumed`。后续相同 `request_id` + `poll_token` 再次调用返回 `240405`。

### POST /api/v1/devices/register

用途：保留给自动化或未来本机内嵌登录模式。第一版 NiumaNotifier 设置页不直接使用该接口；正式设备绑定走 `/api/v1/desktop-login/start`、浏览器登录和 `/api/v1/desktop-login/poll`。

请求：

```json
{
  "device_name": "NiuMa MacBook",
  "device_fingerprint": "stable-local-fingerprint",
  "capabilities": {
    "agent_protocol_version": 1,
    "rpc_protocol_version": 1,
    "supports_webrtc": true,
    "supports_relay": true,
    "supports_remote_control": true
  }
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "device": {
      "id": "dev_...",
      "name": "NiuMa MacBook"
    },
    "device_token": "opaque_device_token"
  }
}
```

服务端只保存 `device_fingerprint` 和 `device_token` 的哈希。


### GET /api/v1/devices/list

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "id": "dev_...",
        "name": "NiuMa MacBook",
        "online": true,
        "last_seen_at": "2026-06-28T12:00:00Z",
        "capabilities": {
          "agent_protocol_version": 1,
          "rpc_protocol_version": 1,
          "supports_webrtc": true,
          "supports_relay": true,
          "supports_remote_control": true
        }
      }
    ]
  }
}
```

### POST /api/v1/devices/rename

请求：

```json
{
  "device_id": "dev_...",
  "name": "Work Mac"
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "device": {
      "id": "dev_...",
      "name": "Work Mac"
    }
  }
}
```

### POST /api/v1/devices/unbind

请求：

```json
{
  "device_id": "dev_..."
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

解绑后吊销 device token，并主动断开设备 WSS。

### POST /api/v1/devices/revoke-token

请求：

```json
{
  "device_id": "dev_..."
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

该接口只吊销设备 token，不删除设备记录。下次本机登录后可重新注册获取新 token。

### POST /api/v1/connections/create

请求：

```json
{
  "device_id": "dev_...",
  "client_id": "web_...",
  "transport_preference": "webrtc_first"
}
```

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "connection_id": "conn_...",
    "connection_token": "opaque_connection_token",
    "expires_in": 120,
    "signaling_url": "wss://remote.example.com/ws/client",
    "relay_url": "wss://remote.example.com/ws/relay"
  }
}
```

服务端创建连接后，通过设备 WSS 通知对应 Remote Agent 准备信令。

### GET /api/v1/connections/ice-config

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "ice_servers": [
      {
        "urls": ["stun:stun.example.com:3478"]
      },
      {
        "urls": ["turn:turn.example.com:3478"],
        "username": "turn-user",
        "credential": "turn-credential"
      }
    ]
  }
}
```

TURN 凭据可以来自静态配置，也可以后续改成短期凭据。

## WebSocket 协议

WebSocket 不使用普通 HTTP API envelope，但每条消息必须有版本、类型和 ID。

### /ws/device

用途：Remote Agent 常驻连接。

鉴权：

```text
Authorization: Device <device_token>
```

连接成功后服务端：

- 写入 `presence:device:{device_id}`。
- 周期性要求 heartbeat。
- 接收设备能力更新。
- 转发 Web 客户端的连接邀请和信令消息。
- 设备 token 吊销时主动关闭连接。

消息示例：

```json
{
  "version": 1,
  "type": "device.hello",
  "id": "msg_001",
  "data": {
    "device_id": "dev_...",
    "capabilities": {}
  }
}
```

### /ws/client

用途：Web 控制台连接信令通道。

鉴权：

```text
Authorization: Bearer <access_token>
```

Web 客户端必须提供 `connection_id` 和 `connection_token`。服务端校验后，只允许该 socket 操作绑定连接。

### /ws/relay

用途：WebRTC 失败后的 relay fallback。

鉴权：

```text
connection_id + connection_token
```

relay 帧结构：

```json
{
  "version": 1,
  "type": "relay.frame",
  "connection_id": "conn_...",
  "seq": 1,
  "ciphertext": "base64"
}
```

服务端只转发 `ciphertext`，不解析明文。

## Docker 自托管部署

自托管目标：

```bash
docker compose up -d
```

启动服务：

- `remote-server`：Fastify API、WSS、Web 控制台静态资源。
- `postgres`：持久化数据。
- `redis`：在线状态、连接协商、relay 路由、限流。
- `coturn`：可选 TURN 服务。

### 端口策略

宿主机不暴露 PostgreSQL 和 Redis 默认端口。容器内部可以继续使用默认端口，Docker 网络内访问。

建议宿主端口：

```text
remote-server: 27880 -> container 27880
coturn: 13478/udp -> container 3478/udp
coturn: 13478/tcp -> container 3478/tcp
```

不使用这些宿主映射：

```text
80:80
443:443
5432:5432
6379:6379
8080:8080
3000:3000
5173:5173
```

生产环境建议通过用户已有反向代理提供 HTTPS：

```text
https://remote.example.com -> http://remote-server:27880
```

第一版不内置 Caddy 或 Nginx profile，避免默认占用宿主 HTTP/HTTPS 端口。后续可以提供可选反向代理示例，但宿主端口也必须可配置。

### Dockerfile

使用多阶段构建：

```text
deps 阶段:
  npm ci

build 阶段:
  编译 TypeScript server
  构建 React Web 控制台

runner 阶段:
  复制 dist
  复制 web dist
  复制 migrations
  安装 production dependencies
  启动 entrypoint
```

容器启动流程：

```text
node dist/db/migrate.js
node dist/server.js
```

迁移失败时容器直接退出，不继续启动服务。

### docker-compose.yml 示例

```yaml
services:
  remote-server:
    build: .
    ports:
      - "27880:27880"
    env_file:
      - .env
    depends_on:
      - postgres
      - redis

  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: niuma_remote
      POSTGRES_USER: niuma
      POSTGRES_PASSWORD: change-me
    volumes:
      - postgres_data:/var/lib/postgresql/data

  redis:
    image: redis:7-alpine
    command: ["redis-server", "--appendonly", "yes"]
    volumes:
      - redis_data:/data

  coturn:
    image: coturn/coturn:latest
    profiles:
      - turn
    ports:
      - "13478:3478/udp"
      - "13478:3478/tcp"
    command:
      - --listening-port=3478
      - --fingerprint
      - --lt-cred-mech
      - --realm=niuma-remote

volumes:
  postgres_data:
  redis_data:
```

### .env.example

```env
REMOTE_SERVER_PUBLIC_URL=https://remote.example.com
REMOTE_SERVER_BIND=0.0.0.0
REMOTE_SERVER_PORT=27880

DATABASE_URL=postgres://niuma:change-me@postgres:5432/niuma_remote
REDIS_URL=redis://redis:6379

JWT_PRIVATE_KEY=
JWT_PUBLIC_KEY=
TOKEN_PEPPER=

ACCESS_TOKEN_TTL_SECONDS=900
REFRESH_TOKEN_TTL_DAYS=30
CONNECTION_TOKEN_TTL_SECONDS=120

REGISTRATION_MODE=admin_invite
BOOTSTRAP_ADMIN_EMAIL=admin@example.com
BOOTSTRAP_ADMIN_PASSWORD=change-me

TURN_ENABLED=false
TURN_URLS=
TURN_USERNAME=
TURN_CREDENTIAL=
```

`DATABASE_URL` 和 `REDIS_URL` 使用容器网络内地址，不暴露到宿主机。`TOKEN_PEPPER` 用于 refresh token 和 device token 哈希前的额外服务端秘密。

## Bootstrap 管理员

启动时执行：

```text
读取 BOOTSTRAP_ADMIN_EMAIL 和 BOOTSTRAP_ADMIN_PASSWORD
检查 users 表是否为空
如果为空且两个变量都存在，创建 admin 用户
如果 users 表非空，不再使用 bootstrap 密码覆盖任何账号
```

创建成功后写入 `audit_events`：

```text
event_type = admin.bootstrap_created
result = success
```

## 安全边界

- 服务端只保存 refresh token、device token、connection token 的哈希。
- 服务端不保存 E2EE RPC 明文。
- relay 只转发密文。
- 设备 token 不能调用普通用户 API。
- access token 不能直接替代 device token 建立设备常驻连接。
- 解绑设备必须吊销 device token 并关闭在线连接。
- 登录和连接创建必须限流。
- 所有业务接口必须使用统一 envelope，不能把业务失败藏在 `data.error`。

## 测试策略

第一版服务端至少覆盖：

- 邮箱注册成功。
- 注册模式为 `admin_invite` 时普通注册失败。
- 登录成功返回 access token 和 refresh token。
- 密码错误返回 `200402`。
- refresh token 刷新后旧 token 失效。
- logout 吊销当前 refresh token。
- logout-all 吊销用户全部 refresh token。
- desktop-login/start 创建绑定会话，返回 login_url 和 poll_token，且 poll_token 不出现在 login_url 中。
- desktop-login/complete 在浏览器登录态下绑定设备，并只保存 encrypted_result_json。
- desktop-login/poll 未完成时返回 `240404`。
- desktop-login/poll 完成时返回 encrypted_result，随后相同会话再次 poll 返回 `240405`。
- 设备注册返回 device token，数据库只保存哈希。
- device token 可以连接 `/ws/device`。
- 吊销 device token 后 `/ws/device` 被拒绝。
- 设备列表合并 PostgreSQL 设备记录和 Redis 在线状态。
- 设备离线时创建连接返回 `210404`。
- 创建连接生成 connection token 并写入 Redis TTL 状态。
- relay 帧只转发 ciphertext。
- REST 参数校验错误合并进 `message`。
- 路由不存在返回统一 envelope。
- Docker Compose 启动后 `/api/v1/health` 返回成功 envelope。

## 实施拆分建议

1. 初始化 `remote-server` TypeScript/Fastify/Drizzle 项目骨架。
2. 实现统一响应、错误码和 Zod 校验转换。
3. 实现 PostgreSQL schema 和迁移。
4. 实现邮箱密码注册、登录、refresh、logout。
5. 实现 bootstrap 管理员和注册模式。
6. 实现设备注册、设备 token、设备列表和解绑。
7. 实现 Redis presence 和 `/ws/device`。
8. 实现连接创建、connection token 和 ICE 配置。
9. 实现 `/ws/client` 信令转发。
10. 实现 `/ws/relay` 密文转发。
11. 实现 Dockerfile、docker-compose 和 `.env.example`。
12. 实现 Web 控制台登录和设备列表。
13. 与本机 Remote Agent 集成。
