fn main() {
    // 独立 Bark 插件进程只负责消费事件流；真实发送逻辑后续在同一进程内补齐。
    niuma_bark_plugin_runtime::run_from_env();
}
