// Task 2 先落解析器，Task 3/4 接入 repository/watcher 后这些模块会被生产路径使用。
#[allow(dead_code)]
mod claude;
#[allow(dead_code)]
mod session_messages;
#[allow(dead_code)]
mod session_provider;

pub fn run_combined_from_env() {
    // Task 1 只注册进程入口；watcher/provider 在后续任务按 TDD 补齐。
    eprintln!("NiumaNotifier Claude Code plugin runtime is not implemented yet");
}

#[cfg(test)]
mod tests;
