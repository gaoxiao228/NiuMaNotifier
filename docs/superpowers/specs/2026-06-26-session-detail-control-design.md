# 会话详情控制区设计

## 背景

会话列表页面已经能展示 session 列表和选中 session 的详情。最新集成文档规定，`session_detail` 会在 `data.control` 中返回当前 session 的可控能力，前端应根据该字段判断是否支持发送指令和中断，不能按工具名称硬编码。

本次改动只针对会话列表页面中的详情区域，在详情底部增加稳定的控制区。

## 目标

- 会话详情底部始终显示控制区。
- 支持 `send_instruction` 时允许用户输入并发送指令。
- 不支持 `send_instruction` 时输入框和发送按钮禁用。
- 支持 `interrupt` 时显示中断按钮；不支持时完全隐藏中断按钮。
- 中断按钮只有会话列表当前展示状态为 `running` 时可点击。
- 控制接口调用遵循最新文档中的 `control.actions` 和统一响应结构。

## 非目标

- 不新增后端接口。
- 不改变 `session_detail`、`session_detail/stream` 或会话列表的数据结构。
- 不在前端伪造 session 运行状态。
- 不实现历史命令、快捷提示词、草稿持久化或复杂富文本输入。
- 不单独为没有 `send_instruction` 的 session 提供中断入口。

## 数据来源

控制能力来自当前选中 session 的详情数据：

```text
session_detail.data.control
```

前端判断可用性时读取：

- `control.available`
- `control.wrapper_session_id`
- `control.capabilities`
- `control.actions`

发送和中断都必须优先使用 `control.actions` 中 `transport = "local_api"` 的 action endpoint。前端不根据 `tool = "codex"` 推断能力。

中断按钮是否可点击使用会话列表当前展示的状态字段。只有该状态为 `running` 时，中断按钮可点击；其他状态下按钮保持显示但禁用。

## UI 行为

详情底部始终渲染控制区。控制区包含：

- 文本输入框。
- 发送按钮。
- 可选的中断按钮。
- 错误或状态提示区域。

输入框和发送按钮始终存在。只有同时满足以下条件时可用：

- `control.available === true`
- `control.capabilities` 包含 `send_instruction`
- `control.actions` 中存在可用的 `send_instruction` local API action

不满足时输入框和发送按钮禁用。禁用提示使用简短文案，例如“当前会话不支持发送指令”。

中断按钮只在同时满足以下条件时显示：

- `control.available === true`
- `control.capabilities` 包含 `interrupt`
- `control.actions` 中存在可用的 `interrupt` local API action

如果没有 `interrupt` 能力，中断按钮完全隐藏。如果有 `interrupt` 能力但会话列表状态不是 `running`，按钮显示但禁用。

布局上，中断按钮放在输入框右侧靠下，和发送区域形成同一个底部控制组。移动端或窄宽度下可以换行，但仍保持中断按钮靠近输入控制区底部。

## 发送指令流程

用户输入内容后点击发送：

```http
POST <send_instruction action endpoint>
Content-Type: application/json
```

请求体：

```json
{
  "tool": "<当前 session 的 tool>",
  "session_id": "<当前 session_id>",
  "wrapper_session_id": "<control.wrapper_session_id>",
  "content": "<输入内容>"
}
```

交互规则：

- 空内容不能发送。
- 请求中禁用输入框和发送按钮，避免重复提交。
- 成功后清空输入内容。
- 失败后保留输入内容，并在控制区显示响应 `message`。
- 成功后不在前端手动插入消息，依赖现有详情刷新或 SSE 推送更新消息列表。

## 中断流程

用户点击中断：

```http
POST <interrupt action endpoint>
Content-Type: application/json
```

请求体：

```json
{
  "tool": "<当前 session 的 tool>",
  "session_id": "<当前 session_id>",
  "wrapper_session_id": "<control.wrapper_session_id>"
}
```

交互规则：

- 请求中禁用中断按钮，避免重复点击。
- 成功后不在前端伪造 session 状态，等待会话列表和详情刷新。
- 失败时在控制区显示响应 `message`。
- 如果 session 状态刷新为非 `running`，中断按钮保持显示但禁用。

## 错误处理

控制接口使用统一响应结构：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

前端必须读取 `code`。当 `code !== 0` 时，将 `message` 显示到控制区。网络错误或非 JSON 响应也显示为控制区错误，不影响详情内容展示。

发送和中断各自维护 busy 状态。任一请求进行中时，可以禁用相关按钮；是否禁用另一个动作由实现按现有页面交互风格决定，但必须避免同一动作重复提交。

## 国际化

新增文案需要补齐现有语言：

- 简体中文
- 繁体中文
- 英语
- 日文
- 韩文
- 德文

预计文案包括：

- 发送指令输入框占位。
- 发送按钮。
- 中断按钮。
- 不支持发送指令提示。
- 控制请求失败提示。

## 测试

测试重点放在前端渲染和交互状态：

- 控制区在有无 `control` 时都渲染。
- 没有 `send_instruction` 时输入框和发送按钮禁用。
- 有 `send_instruction` action 时输入框和发送按钮可用。
- 没有 `interrupt` 时中断按钮不渲染。
- 有 `interrupt` 且列表状态为 `running` 时中断按钮可点击。
- 有 `interrupt` 但列表状态不是 `running` 时中断按钮渲染但禁用。
- 发送成功清空输入；发送失败保留输入并显示错误。
- 中断失败显示错误。

完成实现后运行：

- `npm test`
- `npm run build`
- 相关 Rust/API 测试按实际改动范围补充运行。

## 风险

- 当前工作区已有大量未提交改动，实施时需要避免混入无关变更。
- 如果会话列表展示状态和详情数据状态不同步，中断按钮可能短时间禁用或可用状态滞后；本次按用户要求以列表展示状态为准。
- 控制区始终显示会占用详情底部空间，需要在移动端保证消息区和输入区不会重叠。
