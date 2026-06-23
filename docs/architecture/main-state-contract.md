# 主状态契约

## 目标

主状态是 NiumaNotifier 对 UI、SSE、插件和外部集成暴露的统一状态视图。外部调用方不需要理解工具原始事件，只需要消费稳定的 `status`、`session` 和 `detail`。

## 写入入口

- 进程内写入必须通过 `StateMutationService`。
- `StateMutationService` 负责调用 `NiumaStore` 并发布 `RuntimeEventBus` 事件。
- 不允许绕过 `NiumaStore` 状态转移直接修改 `StoredState`。
- Local API 接收到插件事件后，也必须进入同一套状态转移和事件发布流程。

## 状态来源

- `StoredState.runtime_states` 保存运行态 session。
- `StoredState.attention_items` 保存需要用户处理的 blocker。
- `StoredState.latest_activity` 保存最近活动。
- `StoredState.approval_requests` 保存授权请求状态。
- 通知历史持久化到 SQLite；运行态主状态保存在内存中。

## 展示优先级

1. 最早的 `waiting_approval` 或 `waiting_input`。
2. 最早的 `error`。
3. 最近的 `running` 或 `completed`。
4. 无活动时为 `idle`。

## 外部契约

- 普通 JSON API 返回统一 envelope。
- `/api/v1/state/stream` 是 SSE 协议例外，直接发送 `state` event。
- 外部系统应使用 `status` 判断主状态，不应自行根据 `event_type` 推导。
- SSE 的 `id` 和 `version` 只表示展示版本，不是可恢复消费位点。

## 修改主状态时的检查项

- 是否仍通过 `StateMutationService` 写入。
- 是否保持 `NiumaStore` 去重和状态转移规则。
- 是否同步更新 Local API、SSE 文档和测试。
- 是否覆盖等待授权、等待输入、完成、失败、stale 和 idle 场景。
