# 工具适配器架构

## 目标

工具适配器负责把 Codex、Claude Code、Cursor 等外部工具的原始状态转换为统一的 `NiumaEvent`。主程序、Local API 和 UI 只理解归一化事件与主状态，不直接理解各工具的原始文件、日志或 hook payload。

## 边界

- 工具原始事件解析只放在对应 adapter、watcher、hook 或插件 runtime 中。
- API、Tauri 主入口和 CLI 主入口不得散落工具专属解析逻辑。
- 工具事件写入主状态前必须转换为 `NiumaEvent`。
- 进程内状态写入必须通过 `StateMutationService`。
- 插件向宿主提交事件必须复用 Local API，不得直接写 NiumaNotifier 持久化文件。
- hook 到 Local API 的提交必须复用 `niuma_core::local_api_client`。

## 事件流

1. 工具 watcher 或 hook 读取外部工具原始状态。
2. adapter 把原始状态归一化为 `NiumaEvent`。
3. adapter 通过 `StateMutationService::append_events` 或 `/api/v1/plugin-events` 提交事件。
4. `NiumaStore` 执行去重和状态转移。
5. `RuntimeEventBus` 发布变更，驱动 SSE、通知插件和 UI 刷新。

## 新增工具适配步骤

1. 在插件或对应工具模块中实现原始事件解析。
2. 为每类原始事件定义稳定 `dedupe_key`。
3. 补充 `NiumaEvent` 转换测试。
4. 如需读取 session，优先实现 tool session provider RPC。
5. 不为单个工具立即新增 workspace crate，除非已有模块边界无法承载。

## 模块落点

- 工具通用领域类型放在 `crates/niuma-core/src/tools/` 或已有核心模型中。
- Codex hook 相关 CLI 入口放在 `crates/niuma-cli/src/tools/codex/`。
- 插件进程运行管理放在 `src-tauri/src/tools/`。
- 平台差异优先放在 `niuma_core::platform`，不要在每个工具 adapter 中重复判断操作系统。
