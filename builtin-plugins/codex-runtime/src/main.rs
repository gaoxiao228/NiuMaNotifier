fn main() {
    // 合并后的 Codex 插件同时提供事件监听和 session provider RPC。
    niuma_codex_plugin_runtime::run_combined_from_env();
}
