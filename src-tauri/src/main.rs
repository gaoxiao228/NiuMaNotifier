use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Manager, RunEvent, WindowEvent};

mod background;
mod builtin_plugins;
mod commands;
#[cfg(target_os = "macos")]
mod macos;
mod remote;
mod tools;
mod tray;

fn main() {
    let is_quitting = Arc::new(AtomicBool::new(false));
    let runtime_events = RuntimeEventBus::new();
    let store = NiumaStore::new(NiumaStore::default_path());
    let tool_sessions = niuma_api::tool_sessions::ToolSessionRegistry::new();
    let mutation_service = StateMutationService::new(store.clone(), runtime_events.clone());
    let remote_agent_status = remote::status::RemoteAgentStatusHandle::default();
    let remote_agent_wake = remote::agent::RemoteAgentWake::default();

    let app = tauri::Builder::default()
        .manage(commands::AppRuntimeState {
            store: store.clone(),
            mutation_service,
            runtime_events: runtime_events.clone(),
            remote_agent_status: remote_agent_status.clone(),
            remote_agent_wake: remote_agent_wake.clone(),
        })
        .enable_macos_default_menu(tray::enable_macos_default_menu())
        .setup({
            let is_quitting = Arc::clone(&is_quitting);
            let runtime_events = runtime_events.clone();
            move |app| {
                if let Err(error) = commands::restore_language_preference_from_store() {
                    eprintln!("NiumaNotifier language preference not restored: {error}");
                }
                builtin_plugins::configure_builtin_codex_plugin_command(app);
                builtin_plugins::configure_builtin_bark_plugin_command(app);
                builtin_plugins::configure_builtin_ntfy_plugin_command(app);
                let _tray = tray::register_tray(
                    app.handle(),
                    Arc::clone(&is_quitting),
                    store.clone(),
                    runtime_events.clone(),
                )?;
                if tray::install_macos_terminate_guard() {
                    #[cfg(target_os = "macos")]
                    macos::install_terminate_guard();
                }
                background::spawn_background_services(
                    store.clone(),
                    runtime_events.clone(),
                    tool_sessions.clone(),
                    remote_agent_status.clone(),
                    remote_agent_wake.clone(),
                );
                Ok(())
            }
        })
        .on_window_event({
            let is_quitting = Arc::clone(&is_quitting);
            move |window, event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    if tray::should_hide_window_on_close(
                        window.label(),
                        is_quitting.load(Ordering::SeqCst),
                    ) {
                        api.prevent_close();
                        tray::hide_main_window(window);
                    } else if window.label() == "event-center" {
                        tray::sync_event_center_menu_visibility(&window.app_handle(), false);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_main_state,
            commands::get_runtime_state_list,
            commands::get_recent_events,
            commands::get_local_api_url,
            commands::get_active_language,
            commands::save_language_preference,
            commands::get_listener_config,
            commands::save_listener_config,
            commands::get_remote_settings,
            commands::save_remote_settings,
            commands::get_remote_agent_status,
            commands::clear_remote_binding,
            commands::start_remote_login,
            commands::poll_remote_login,
            commands::get_plugins,
            commands::remove_plugin,
            commands::set_plugin_enabled,
            commands::run_plugin_action,
            commands::get_plugin_config,
            commands::save_plugin_config,
            commands::select_and_import_plugin_dir,
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
