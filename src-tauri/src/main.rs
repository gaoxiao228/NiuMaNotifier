use niuma_api::{local_api_addr, spawn_local_api_with_bus};
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::SqliteStateStore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{RunEvent, WindowEvent};

mod commands;
#[cfg(target_os = "macos")]
mod macos;
mod notification_runtime;
mod tools;
mod tray;

const LOCAL_API_START_DELAY: Duration = Duration::ZERO;
const WATCHER_START_DELAY: Duration = Duration::from_secs(1);

fn spawn_background_services(runtime_events: RuntimeEventBus) {
    let spawn_result = thread::Builder::new()
        .name("niuma-background-services-startup".to_string())
        .spawn(move || {
            if LOCAL_API_START_DELAY > Duration::ZERO {
                thread::sleep(LOCAL_API_START_DELAY);
            }
            let notification_store = SqliteStateStore::new(SqliteStateStore::default_path());
            match notification_runtime::spawn_notification_runtime(
                notification_store,
                runtime_events.clone(),
            ) {
                Ok(_detached_notification_thread) => {
                    // JoinHandle 在这里丢弃会 detach 后台线程，通知运行时通过事件总线常驻消费。
                    eprintln!("NiumaNotifier notification runtime thread started");
                }
                Err(error) => {
                    eprintln!("NiumaNotifier notification runtime not started: {error}");
                }
            }

            let store = SqliteStateStore::new(SqliteStateStore::default_path());
            match spawn_local_api_with_bus(store, runtime_events.clone()) {
                Ok(_) => {
                    eprintln!("NiumaNotifier Local API started at {}", local_api_addr());
                }
                Err(error) => {
                    // 端口可能已被另一个开发实例占用；UI 仍可读取同一份状态文件。
                    eprintln!("NiumaNotifier Local API not started: {error}");
                }
            }

            // Codex session 扫描放到首屏之后，避免文件系统监听和活跃文件轮询抢首屏资源。
            thread::sleep(WATCHER_START_DELAY);
            tools::spawn_tool_runtimes(runtime_events.clone());
        });

    if let Err(error) = spawn_result {
        eprintln!("NiumaNotifier background services startup thread not started: {error}");
    }
}

fn main() {
    let is_quitting = Arc::new(AtomicBool::new(false));
    let runtime_events = RuntimeEventBus::new();
    let mutation_service = StateMutationService::new(
        SqliteStateStore::new(SqliteStateStore::default_path()),
        runtime_events.clone(),
    );

    let app = tauri::Builder::default()
        .manage(commands::AppRuntimeState { mutation_service })
        .enable_macos_default_menu(tray::enable_macos_default_menu())
        .setup({
            let is_quitting = Arc::clone(&is_quitting);
            let runtime_events = runtime_events.clone();
            move |app| {
                if let Err(error) = commands::restore_language_preference_from_store() {
                    eprintln!("NiumaNotifier language preference not restored: {error}");
                }
                let _tray = tray::register_tray(
                    app.handle(),
                    Arc::clone(&is_quitting),
                    runtime_events.clone(),
                )?;
                if tray::install_macos_terminate_guard() {
                    #[cfg(target_os = "macos")]
                    macos::install_terminate_guard();
                }
                spawn_background_services(runtime_events.clone());
                Ok(())
            }
        })
        .on_window_event({
            let is_quitting = Arc::clone(&is_quitting);
            move |window, event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    if !is_quitting.load(Ordering::SeqCst) {
                        api.prevent_close();
                        tray::hide_main_window(window);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_main_state,
            commands::get_sessions,
            commands::get_recent_events,
            commands::get_local_api_url,
            commands::get_active_language,
            commands::save_language_preference,
            commands::get_listener_config,
            commands::save_listener_config,
            commands::get_notification_config,
            commands::save_notification_config,
            commands::get_notification_records,
            commands::send_test_notification,
            commands::dismiss_active_blocker
        ])
        .build(tauri::generate_context!())
        .expect("启动 NiumaNotifier 桌面端失败");

    app.run(move |app, event| {
        match event {
            RunEvent::ExitRequested { api, .. } => {
                match tray::policy_for_exit_request(is_quitting.load(Ordering::SeqCst)) {
                    tray::BackgroundPolicy::HideToStatusItem => {
                        // 关闭窗口或系统普通退出只进入后台；真正退出只走状态栏菜单。
                        api.prevent_exit();
                        tray::hide_main_window_from_app(app);
                    }
                    tray::BackgroundPolicy::QuitApplication => {}
                    tray::BackgroundPolicy::ShowMainWindow => {}
                }
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen {
                has_visible_windows,
                ..
            } => match tray::policy_for_main_visibility(has_visible_windows) {
                tray::BackgroundPolicy::HideToStatusItem => {
                    // Dock 图标仍可见时，点击 Dock 可恢复没有可见窗口的主界面。
                    tray::show_main_window(app);
                }
                tray::BackgroundPolicy::QuitApplication => {}
                tray::BackgroundPolicy::ShowMainWindow => {}
            },
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_keeps_local_api_immediate_and_delays_watcher_only() {
        assert_eq!(LOCAL_API_START_DELAY, Duration::ZERO);
        assert_eq!(WATCHER_START_DELAY, Duration::from_secs(1));
    }
}
