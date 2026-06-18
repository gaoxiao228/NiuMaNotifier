# 插件父进程存活检测设计

## 背景

当前插件由主 App 通过 `std::process::Command` 启动为子进程。主 App 在内存中持有 `Child` 句柄，正常禁用、移除或 manifest 变化时会调用 `Child::kill()` 停止插件。

问题是主 App 如果突然崩溃或闪退，Rust 标准库不会自动 kill 已启动的子进程，插件可能变成孤儿进程并继续运行。插件后续向 Local API 上报会失败，但进程本身不一定退出。

## 目标

在不引入 pidfile、后台清理任务或跨平台复杂进程组管理的前提下，让插件能感知主 App 已退出，并主动结束自身。

## 非目标

- 不持久化插件 PID。
- 不在 App 启动时扫描并清理历史遗留插件进程。
- 不改变现有正常禁用插件时的 `Child::kill()` 生命周期。
- 不为外部插件提供强制沙箱或签名校验。

## 方案

主 App 启动插件进程时注入新的环境变量：

```text
NIUMA_PARENT_PID=<主 App 进程 pid>
```

插件进程读取该环境变量后启用父进程 watchdog。watchdog 定时检查指定 PID 是否仍存在；如果不存在，插件主动以成功状态退出。

内置 Codex 插件会在 `builtin-plugins/codex-runtime` 中实现 watchdog。外部插件通过开发文档获得同一协议，推荐按相同方式实现。

## 数据流

1. 用户在插件管理页启用插件。
2. 前端保存 listener config。
3. 插件管理器收到 `ListenerConfigChanged`，重新 reconcile。
4. 插件管理器通过 `Command::new(...).spawn()` 启动插件。
5. 启动时注入 `NIUMA_PARENT_PID`、`NIUMA_LOCAL_API_URL`、`NIUMA_PLUGIN_ID`、`NIUMA_TOOL_ID`、`NIUMA_STATE_PATH`。
6. 插件启动后创建 watchdog 线程。
7. 主 App 正常运行时，watchdog 检测父 PID 存活并保持插件运行。
8. 主 App 闪退后，watchdog 检测父 PID 不存在，插件主动退出。

## 检测策略

Unix 平台先使用轻量进程探测：

- `kill(pid, 0)` 成功：进程存在。
- `kill(pid, 0)` 返回 `ESRCH`：进程不存在。
- 返回权限相关错误：保守认为进程存在，避免误退。

检查间隔使用 2 秒，降低实现复杂度和资源开销。

Windows 后续可用平台专用 API 扩展。当前项目主要目标是 macOS 桌面端，本次实现优先覆盖 Unix 路径。

## 错误处理

- `NIUMA_PARENT_PID` 缺失：插件继续按现有逻辑运行，不启用 watchdog。
- `NIUMA_PARENT_PID` 格式错误：插件继续按现有逻辑运行，不启用 watchdog。
- 父进程存在性检测返回未知错误：保守认为父进程仍存在，并记录调试日志或标准错误。
- 父进程不存在：插件主动退出，退出码为 0。

## 测试计划

先写失败测试，再实现：

- 插件启动命令会注入 `NIUMA_PARENT_PID`。
- watchdog 对缺失或非法 PID 不触发退出。
- watchdog 能识别一个不存在的 PID。
- Codex runtime 在父进程不存在时会触发退出判定。

实现后运行相关 Rust 测试，至少覆盖：

- `src-tauri/src/tools/plugin_runtime.rs` 单元测试。
- `builtin-plugins/codex-runtime` 单元测试。

## 兼容性

该方案对旧外部插件向后兼容。旧插件忽略未知环境变量即可继续运行。内置 Codex 插件会立即获得父进程退出自清理能力。外部插件是否自清理由插件作者按文档实现。
