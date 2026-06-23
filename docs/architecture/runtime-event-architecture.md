# 运行时事件架构

## 目标

`RuntimeEventBus` 用于连接状态写入、SSE 推送、插件运行态和 UI 刷新。它负责通知“状态可能变化”，但不替代 `NiumaStore` 成为状态源。

## 事件发布规则

- 只有实际进入状态机的新 `NiumaEvent` 才发布事件追加通知。
- listener config、reset、stale sweep 等状态变化发布 state changed 事件。
- 手动 dismiss blocker 发布 attention dismissed 事件。
- 重复事件或被状态机忽略的 late terminal event 不应触发无意义刷新。

## 消费者

- Local API SSE 根据事件推送主状态和事件流。
- 通知插件消费事件流并回写通知结果。
- Tauri UI 通过命令和 Local API 读取快照。
- 插件运行态列表保存在内存中，UI 可用轻量轮询同步。

## 设计约束

- `RuntimeEventBus` 不替代 `NiumaStore`，不能作为状态源。
- SSE 客户端重连后必须接受首个快照，不依赖 event id 恢复消费位点。
- 事件消费者收到通知后应重新读取需要的快照，而不是假设事件携带完整状态。
- 新增运行时事件时，应明确发布方、消费者和是否需要触发 SSE。

## 典型流程

1. `StateMutationService` 调用 `NiumaStore` 写入事件或修改状态。
2. 写入成功后发布 `RuntimeEvent`。
3. SSE broadcaster 收到通知后重新计算主状态。
4. UI 或外部客户端消费新的 SSE state。
5. 通知插件消费事件流并通过 Local API 回写发送结果。
