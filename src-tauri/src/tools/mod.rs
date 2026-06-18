pub mod codex;

use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::store::SqliteStateStore;

pub fn spawn_tool_runtimes(runtime_events: RuntimeEventBus) {
    let watcher_store = SqliteStateStore::new(SqliteStateStore::default_path());
    match codex::session_runtime::spawn_codex_session_runtime(watcher_store, runtime_events) {
        Ok(_detached_watcher_thread) => {
            // JoinHandle 在这里丢弃会 detach 后台线程，避免阻塞 Tauri 主循环。
            eprintln!("NiumaNotifier Codex session watcher runtime thread started");
        }
        Err(error) => {
            // 文件监听只是状态增强能力，启动失败不能影响状态栏应用常驻。
            eprintln!("NiumaNotifier Codex session watcher not started: {error}");
        }
    }
}
