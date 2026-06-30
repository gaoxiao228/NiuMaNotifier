# 远程后台与外部客户端部署说明

本文档说明 `remote-server`、`remote-admin` 和 `remote-client-web` 的职责边界、管理员初始化方式，以及外部客户端跨源访问配置。

## 组件职责

`remote-server` 是外网服务端，负责账号、设备绑定、设备在线状态、信令、Relay、连接令牌和远程 RPC 通道。

`remote-admin` 是 `remote-server` 内置的后台管理页面，随 `remote-server` 在同一个入口暴露。它只面向管理员账号，用于查看和管理远程服务端侧资源，不作为终端用户远程客户端使用。

`remote-client-web` 是独立外部 Web 客户端，面向普通用户。它通过 `remote-server` 登录账号、查看已绑定设备、连接在线设备，并查看设备上的远程 session 列表。后续 iOS、Android、Windows 桌面端也应按同一套 HTTP / WebSocket / Relay / WebRTC / RPC 协议接入。

## 默认本地入口

Docker Compose 本地部署默认使用以下宿主机端口：

```text
remote-server / remote-admin: http://127.0.0.1:27880
remote-client-web:            http://127.0.0.1:27883
```

如果浏览器使用 `http://localhost:27883` 打开外部客户端，也必须把 `http://localhost:27883` 配置进 CORS Origin，因为浏览器会把 `localhost` 和 `127.0.0.1` 视为不同 Origin。

## CORS 配置

`remote-client-web` 与 `remote-server` 分离部署时，浏览器会跨源访问后端 API。需要在 `remote-server/.env` 中配置：

```env
REMOTE_SERVER_CORS_ORIGINS=http://127.0.0.1:27883,http://localhost:27883
```

多个 Origin 使用英文逗号分隔。线上部署时写完整 Origin，必须包含协议和端口：

```env
REMOTE_SERVER_CORS_ORIGINS=https://client.example.com,https://app.example.com
```

修改后重启 `remote-server`：

```bash
cd remote-server
docker compose up -d --force-recreate remote-server
```

## 外部客户端服务端地址

`remote-client-web` 使用 Vite 构建期变量写入后端地址：

```env
VITE_REMOTE_SERVER_URL=http://127.0.0.1:27880
```

这个值不是运行期环境变量。修改 `remote-server/docker-compose.yml` 中的 `remote-client-web.build.args.VITE_REMOTE_SERVER_URL` 后，需要重新构建镜像：

```bash
cd remote-server
docker compose up -d --build remote-client-web
```

## 管理员账号初始化

后台管理不允许普通用户登录。`remote-admin` 使用管理员专用接口：

```text
POST /api/v1/admin/auth/login
```

普通用户账号即使密码正确，也会返回 `需要管理员权限`，不会进入后台。

自托管部署通过环境变量创建第一个管理员：

```env
BOOTSTRAP_ADMIN_EMAIL=admin@example.com
BOOTSTRAP_ADMIN_PASSWORD=change-me
```

Docker 启动 `remote-server` 时会按顺序执行：

```text
数据库迁移 -> 管理员 bootstrap -> 启动服务
```

bootstrap 规则：

- 如果库里没有管理员，并且配置了 `BOOTSTRAP_ADMIN_EMAIL` / `BOOTSTRAP_ADMIN_PASSWORD`，会创建第一个管理员。
- 如果库里已有管理员，会跳过。
- 如果 `BOOTSTRAP_ADMIN_EMAIL` 已经属于普通用户，不会自动提权，会报错退出。管理员身份必须显式创建。
- 密码长度必须为 8 到 128 个字符。

非 Docker 环境可以手动执行：

```bash
cd remote-server
npm run admin:bootstrap
```

## 普通账号来源

普通用户账号仍通过现有普通认证体系创建和登录。第一版不在 `remote-admin` 提供用户创建页面；管理员创建、邀请和禁用普通用户可以作为后续后台管理功能继续扩展。

## 手动验收清单

1. 打开 `http://localhost:27883`，使用普通账号登录外部客户端，应能进入设备列表。
2. 打开 `http://127.0.0.1:27880`，使用普通账号登录后台，应提示需要管理员权限。
3. 使用 `BOOTSTRAP_ADMIN_EMAIL` / `BOOTSTRAP_ADMIN_PASSWORD` 对应管理员登录后台，应能进入设备管理页面。
4. 如果外部客户端提示网络连接失败，优先检查 `REMOTE_SERVER_CORS_ORIGINS` 是否包含当前浏览器地址对应的完整 Origin。
5. 如果外部客户端请求仍打到错误服务端，检查 `remote-client-web` 镜像是否用正确的 `VITE_REMOTE_SERVER_URL` 重新构建。
