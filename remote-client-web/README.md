# remote-client-web

`remote-client-web` 是 NiuMaNotifier 的独立外部 Web 客户端，面向终端用户使用。它负责登录远程服务端账号、选择在线设备、建立远程连接，并通过统一远程协议查看本机 NiuMaNotifier 的实时 session 列表。

`remote-server` 内置 Web 页面定位为 `remote-admin` 后台管理，不是终端用户外部客户端。除 Web 形态外，iOS、Android、Windows 桌面端等客户端也可以按同一套 HTTP / WebSocket / Relay / WebRTC / RPC 协议实现。

## 本地开发

```bash
npm install
npm run dev
```

开发服务默认监听：

```text
http://127.0.0.1:27882/
```

## 构建

```bash
npm run build
```

构建产物输出到 `dist/`。

## Docker

从 `remote-server` 目录运行：

```bash
docker compose build remote-client-web
```

Docker Compose 默认把容器内 Nginx 映射到宿主机：

```text
http://127.0.0.1:27883/
```

## 远程服务端地址

`remote-client-web` 使用 Vite 环境变量指定远程服务端 API 地址：

```text
VITE_REMOTE_SERVER_URL=http://127.0.0.1:27880
```

该变量是构建期内联变量，不是运行期环境变量。修改 `VITE_REMOTE_SERVER_URL` 后，需要重新构建静态资源或 Docker 镜像。

Docker Compose 默认在构建 `remote-client-web` 镜像时写入 `http://127.0.0.1:27880`，对应本地 `remote-admin` 和远程服务端 API 的默认入口。

如果需要指向线上远程服务端，可以在构建镜像时显式传入 build arg：

```bash
docker build \
  -f remote-client-web/Dockerfile \
  --build-arg VITE_REMOTE_SERVER_URL=https://remote.example.com \
  -t niuma-remote-client-web .
```

使用 Docker Compose 时，在 `remote-server/docker-compose.yml` 的 `remote-client-web.build.args.VITE_REMOTE_SERVER_URL` 中覆盖该值，然后重新执行：

```bash
docker compose build remote-client-web
```

## 第一版范围

第一版只实现外部客户端闭环：

- 登录远程服务端账号。
- 查看设备列表。
- 连接在线设备。
- 查看实时 session 列表。
- 在顶部紧凑状态栏显示 Relay、WebRTC 和当前通道状态。
