# Status Indicator Demo 插件样例

这是一个最小状态指示插件样例，用来验证 `status_indicator` 插件发现、启动和主状态消费链路。

## 安装到本机插件目录

```bash
mkdir -p "$HOME/Library/Application Support/NiumaNotifier/plugins"
cp -R examples/plugins/status-indicator-demo "$HOME/Library/Application Support/NiumaNotifier/plugins/"
```

重启 NiumaNotifier 后，插件管理页应出现 `Status Indicator Demo`。启用后，插件会连接 `/api/v1/state/stream` 并在标准输出中打印状态指示结果。

## 手动运行

```bash
NIUMA_LOCAL_API_URL=http://127.0.0.1:27874 \
NIUMA_PLUGIN_ID=status-indicator-demo \
node examples/plugins/status-indicator-demo/bin/status-indicator-demo.mjs
```

该插件会保持进程常驻，直到收到 `SIGINT` 或 `SIGTERM`。
