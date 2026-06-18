fn main() {
    // 独立插件进程只负责启动 Codex runtime，生命周期由通用插件管理器控制。
    niuma_codex_plugin_runtime::run_from_env();
}
