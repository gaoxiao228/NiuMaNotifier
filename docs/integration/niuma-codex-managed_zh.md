# niuma codex 托管会话

`niuma codex` 是 Codex CLI 的托管启动方式。它会启动真实 Codex，同时在本机建立 relay 和 control socket，让 NiumaNotifier 可以识别该会话并处理授权、等待输入、续写和中断。

## 启动

开发环境中推荐显式指定真实 Codex 路径：

```bash
NIUMA_REAL_CODEX="$(which codex)" ./target/debug/niuma codex
```

`codex` 后面的参数会原样透传给真实 Codex：

```bash
NIUMA_REAL_CODEX="$(which codex)" ./target/debug/niuma codex --model gpt-5.5
NIUMA_REAL_CODEX="$(which codex)" ./target/debug/niuma codex exec --help
```

安装到 PATH 后可以直接使用：

```bash
niuma codex
```

## 查看托管会话

```bash
./target/debug/niuma codex-sessions
```

返回示例：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "active_count": 1,
    "sessions": [
      {
        "wrapper_session_id": "niuma_codex_xxx",
        "state": "bound",
        "pid_alive": true,
        "control_socket_responsive": true,
        "codex_session_id": "codex-session-id",
        "control_socket": "/tmp/niuma-codex/xxx/control.sock"
      }
    ]
  }
}
```

`active_count = 0` 表示当前没有可控制的托管会话。常见原因：

- 不是通过 `niuma codex` 启动，而是直接运行了原生 `codex`。
- `niuma` 二进制没有重新构建，仍是旧版本。
- Codex 会话已退出，registry 里只剩历史记录。
- control socket 不响应。

registry 文件位于：

```text
~/Library/Application Support/NiumaNotifier/managed-sessions/codex.json
```

这个文件用于保存托管会话索引、绑定关系和 control socket 路径，不是用户配置文件。

## 等待输入

当 Codex app-server 发出 `requestUserInput` 时，`niuma codex` relay 会上报 `input_requested` 事件。可由 Niuma 处理的事件会包含：

```json
{
  "interaction": {
    "kind": "input",
    "handling": "niuma",
    "actionable": true,
    "request_id": "codex-input:niuma_codex_xxx:0",
    "endpoint": "/api/v1/session-control/answer-input",
    "schema": {
      "questions": [
        {
          "id": "app_type",
          "question": "请选择应用类型",
          "options": [
            {
              "label": "Web App",
              "description": "适合浏览器访问"
            }
          ]
        }
      ]
    }
  }
}
```

主界面会根据 `interaction.schema.questions` 渲染选项。用户提交后，桌面端会调用：

```http
POST /api/v1/session-control/answer-input
```

请求中的 `answers` 使用 `Record<string, string[]>`：

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "wrapper_session_id": "niuma_codex_xxx",
  "request_id": "codex-input:niuma_codex_xxx:0",
  "answers": {
    "app_type": ["Web App"]
  }
}
```

如果当前主界面只显示“等待输入”但没有选项，先确认主状态里是否有可操作 schema：

```bash
curl -s "http://127.0.0.1:27874/api/v1/main-state" | jq '.data.state.detail.interaction'
```

再确认 control socket 是否抓到了 pending input：

```bash
CONTROL=$(./target/debug/niuma codex-sessions | jq -r '.data.sessions[0].control_socket')
printf '{"type":"requests"}\n' | nc -U "$CONTROL" | jq '.inputs'
```

判断方式：

- `interaction.handling = "niuma"` 且 `actionable = true`：主界面应该显示可提交表单。
- `interaction.handling = "tool"`：这是 watcher 兜底事件，只能回 Codex 内处理。
- control socket 有 inputs，但主状态没有 relay event：检查 Local API 是否正在运行，或 relay 是否能提交 `/api/v1/plugin-events`。

## 授权

授权仍复用现有接口：

```http
POST /api/v1/approval-decisions
```

`niuma codex` relay 发现授权请求后，会通过 Local API 上报可操作授权事件。用户也可能在 Codex TUI 内直接同意或拒绝；relay 会同步“已在 Codex 中处理”的状态，避免主界面残留过期按钮。

## 发送续写

向指定托管会话发送新指令：

```bash
./target/debug/niuma codex-send niuma_codex_xxx "继续"
```

如果当前 Codex thread 是 idle，relay 会发送 `turn/start`。如果 thread 是 active，relay 会发送 `turn/steer`。

对应 Local API：

```http
POST /api/v1/session-control/send-instruction
```

## 中断

中断指定托管会话当前 turn：

```bash
./target/debug/niuma codex-interrupt niuma_codex_xxx
```

对应 Local API：

```http
POST /api/v1/session-control/interrupt
```

只有当前 thread 存在 `inProgress` turn 时才能中断。

## 排查清单

1. 重新构建 CLI：

   ```bash
   cargo build -p niuma-cli
   ```

2. 确认 Local API：

   ```bash
   lsof -nP -iTCP:27874 -sTCP:LISTEN
   ```

3. 确认 Vite 前端端口没有被旧进程占用：

   ```bash
   lsof -nP -iTCP:58415 -sTCP:LISTEN
   ```

4. 确认当前是通过 wrapper 启动：

   ```bash
   ./target/debug/niuma codex-sessions
   ```

5. 如果主界面没有刷新，重启 `npm run tauri dev` 或刷新窗口。
