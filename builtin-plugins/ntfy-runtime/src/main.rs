fn main() {
    // 独立 ntfy 插件进程只消费事件流并按自己的配置发送推送。
    niuma_ntfy_plugin_runtime::run_from_env();
}
