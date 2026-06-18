# Demo Tool 插件样例

这是一个最小外部插件样例，用来验证 NiumaNotifier 的插件发现、启动和事件上报链路。

## 安装到本机插件目录

```bash
mkdir -p "$HOME/Library/Application Support/NiumaNotifier/plugins"
cp -R examples/plugins/niuma-plugin-demo "$HOME/Library/Application Support/NiumaNotifier/plugins/"
```

重启 NiumaNotifier 后，界面中的监听列表应出现 `Demo Tool`。启用它后，插件会通过 `/api/v1/plugin-events` 上报一组稳定的测试事件。

## 手动运行

```bash
NIUMA_LOCAL_API_URL=http://127.0.0.1:27874 \
NIUMA_PLUGIN_ID=niuma-plugin-demo \
NIUMA_TOOL_ID=demo_tool \
node examples/plugins/niuma-plugin-demo/bin/niuma-plugin-demo.mjs
```

该插件会保持进程常驻，直到收到 `SIGINT` 或 `SIGTERM`。
