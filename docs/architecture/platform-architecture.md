# 平台架构

## 目标

平台差异优先收敛到 `niuma_core::platform`，避免 macOS、Windows、Linux 判断散落在工具适配器和业务模块中。平台模块提供可复用能力，桌面壳模块只处理 Tauri 和系统 UI 生命周期。

## 边界

- 跨平台路径、可执行文件名、locale 检测放在 `crates/niuma-core/src/platform/`。
- Tauri 窗口、托盘和 macOS 退出策略属于 `src-tauri`。
- 工具 adapter 不直接判断操作系统，除非该判断只影响该工具的原始文件格式。
- Local API、状态聚合和插件协议不依赖具体桌面平台。

## 当前模块

- `platform::paths`：应用数据路径。
- `platform::executable`：平台可执行文件名。
- `platform::locale`：系统语言偏好。

## 新增平台能力步骤

1. 先判断能力是否可复用。
2. 可复用能力放入 `niuma_core::platform`。
3. 桌面 UI 生命周期能力放入 `src-tauri`。
4. 添加平台无关单元测试；平台专属行为用条件编译测试覆盖。

## 设计约束

- `niuma-core` 不应依赖 Tauri。
- `niuma-api` 不应包含桌面窗口、托盘或系统菜单逻辑。
- `src-tauri` 可以组合 core/api 能力，但不应承载工具原始协议解析。
