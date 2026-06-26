# niuma-codex 可操作等待输入设计

日期：2026-06-26

## 背景

当前 `niuma-codex` 已能通过 relay 观察 Codex app-server 的授权请求，并通过 control socket 把授权决策回写给 Codex。等待输入链路已有部分基础：session watcher 可以从 Codex session JSONL 解析 `request_user_input` 并生成 `input_requested`，relay 也能观察 `item/tool/requestUserInput` 并保存 pending input，但现有设计把 watcher 事件作为第一版唯一事件来源，relay 只暴露 pending request。

本设计覆盖 `docs/superpowers/specs/2026-06-25-niuma-codex-managed-session-design.md` 中的 `Request User Input` 章节：对于通过 `niuma codex` 启动并已绑定的会话，relay 应直接上报可操作的等待输入事件，并支持通过 Niuma Local API 回写用户回答。

## 目标

- `niuma-codex` relay 发现 Codex 等待输入时，主界面、SSE 消费者和手机端都能收到 `input_requested` 事件。
- 事件必须可操作，`interaction.handling = "niuma"`，并携带可渲染的结构化表单。
- 用户在 Niuma 主界面或手机端提交表单后，答案通过 relay control socket 回写 Codex app-server，Codex 继续运行。
- watcher fallback 仍保留，但不能覆盖 relay 已上报的可操作 input。

## 非目标

- 第一版不新增独立 input request store。
- 第一版不新增 `InputResolved` 事件类型，回答成功后复用 `session_activity` 清理等待态。
- 第一版不实现复杂表单 DSL，只支持 Codex 当前 `questions/options` 结构的最小规范化。
- 第一版不支持自定义校验规则、条件表单或跨问题联动。

## 事件模型

扩展 `EventInteractionDetail`，新增可选字段 `schema`。等待输入事件示例：

```json
{
  "kind": "input",
  "handling": "niuma",
  "actionable": true,
  "request_id": "codex-input:niuma_codex_xxx:9",
  "actions": ["submit"],
  "endpoint": "/api/v1/session-control/answer-input",
  "schema": {
    "questions": [
      {
        "id": "app_form",
        "header": "形态",
        "question": "这个程序你更希望主要以什么形态运行？",
        "options": [
          {
            "label": "托盘常驻 (Recommended)",
            "description": "跨平台常驻后台，适合长期监控。"
          }
        ]
      }
    ]
  }
}
```

字段规则：

- `schema` 仅在 `kind = "input"` 时有意义。
- `questions` 来自 Codex 原始 `item/tool/requestUserInput` 参数。
- `id` 为空时由 relay 补为 `question_1`、`question_2`。
- `question` 为空的问题不展示，也不要求提交。
- `options` 非空时前端按单选渲染。
- `options` 为空或不存在时前端按文本输入渲染。
- `label` 是提交值，`description` 只用于展示。

## 答案格式

提交答案统一使用 `Record<string, string[]>`：

```json
{
  "app_form": ["托盘常驻 (Recommended)"]
}
```

第一版 options 全部按单选处理，但值仍使用数组，方便未来兼容多选。文本输入也使用数组：

```json
{
  "notes": ["用户输入的文本"]
}
```

## API 契约

新增接口：

```http
POST /api/v1/session-control/answer-input
Content-Type: application/json
```

请求体：

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "wrapper_session_id": "niuma_codex_xxx",
  "request_id": "codex-input:niuma_codex_xxx:9",
  "answers": {
    "app_form": ["托盘常驻 (Recommended)"]
  }
}
```

成功响应：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "answered": true,
    "wrapper_session_id": "niuma_codex_xxx",
    "request_id": "codex-input:niuma_codex_xxx:9",
    "result": {}
  }
}
```

业务失败遵循项目统一 envelope：返回 `HTTP 200`，`code = 100101`，`message` 描述原因。典型失败：

- `tool` 不是 `codex`。
- `session_id` 为空。
- `wrapper_session_id` 不是 `niuma_codex_` 前缀。
- `session_id` 与 `wrapper_session_id` 不匹配。
- managed session 未绑定、已退出或进程不存在。
- 找不到 pending input。
- `answers` 为空或不是对象。

JSON 解析失败属于协议层错误，返回 `HTTP 400` 和 `code = 100004`。

## relay 数据流

1. relay 观察 app-server 到客户端的 JSON-RPC 请求：

```text
item/tool/requestUserInput
```

2. relay 生成 pending input：

```text
request_id = codex-input:<wrapper_session_id>:<jsonrpc_id>
```

3. relay 规范化 `questions`，保存到 `pending_inputs`。
4. relay 调 Local API 上报 `input_requested` 事件，source 使用 `codex-relay`。
5. Local API 写入状态机后发布 runtime event，主界面、SSE 和通知消费者刷新。
6. 用户提交答案后，API 通过 managed registry 找到 control socket，发送：

```json
{
  "type": "answer_input",
  "request_id": "codex-input:niuma_codex_xxx:9",
  "answers": {
    "app_form": ["托盘常驻 (Recommended)"]
  }
}
```

7. relay 使用现有 `AnswerInput` control command 构造 JSON-RPC response frame：

```json
{
  "id": 9,
  "result": {
    "answers": {
      "app_form": ["托盘常驻 (Recommended)"]
    }
  }
}
```

8. 回写成功后 API 追加 `session_activity`，使主状态从 `waiting_input` 恢复。

## watcher fallback 仲裁

watcher 继续可以从 session JSONL 生成 `input_requested`，但该事件默认：

```json
{
  "kind": "input",
  "handling": "tool",
  "actionable": false
}
```

仲裁规则：

- relay 可操作 input 优先级高于 watcher fallback。
- 如果 watcher fallback 已显示，relay 可操作 input 到达后应覆盖当前展示。
- 如果 relay 可操作 input 已显示，后到的 watcher fallback 不应覆盖它。
- 回答成功后的 `session_activity` 清理对应 session 的 `waiting_input`。

第一版使用轻量匹配：`tool + normalized_session_id/session_id + event_type = input_requested + summary/content`。后续如果 Codex 提供更稳定的 input call id，可以再升级为强绑定。

## 前端行为

主界面、手机端或外部面板看到：

```text
interaction.kind = input
interaction.handling = niuma
interaction.actionable = true
```

时渲染结构化表单：

- `options` 非空：单选。
- `options` 为空：文本输入。
- 多个问题按 `questions` 顺序展示。
- 提交前校验每个展示问题都有答案。
- 提交后禁用按钮，等待 API 响应。
- 成功后不只依赖前端乐观清除，最终以 SSE/main-state 刷新为准。

新增 UI 文案必须补齐项目支持的多语言。

## 测试范围

- relay 捕获 `item/tool/requestUserInput` 后生成 pending input 和可操作事件 payload。
- question 规范化：缺失 id 时补 `question_1`，空 question 被过滤。
- control socket `answer_input` 写回 JSON-RPC response frame。
- API `answer-input` 成功调用 control socket。
- API 拒绝非 Codex、空 answers、wrapper/session 不匹配、找不到 pending input。
- relay input 覆盖 watcher fallback。
- watcher fallback 不覆盖 relay input。
- answer 成功后 waiting input 被 `session_activity` 清理。
- 前端结构化表单序列化为 `Record<string, string[]>`。

## 风险与约束

- `answers` 结构需要与 Codex app-server 当前期望保持一致；实现时必须用真实 `request_user_input` 场景手测。
- relay 上报事件后，watcher 仍可能看到同一 input，需要靠仲裁避免 UI 闪回不可操作状态。
- 当前 Local API 默认可信本机调用。手机端如果通过局域网访问，应由外层网络控制保护。
- 不新增 input store 意味着 pending input 不做长期持久化；relay 退出后对应输入不可再从 Niuma 回写，只能回到 Codex 原生界面。

