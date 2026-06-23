fn main() {
    // session provider 是独立 stdio 进程，生命周期由桌面插件管理器控制。
    niuma_codex_plugin_runtime::session_provider::run_stdio_session_provider();
}
