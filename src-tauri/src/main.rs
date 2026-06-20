use niuma_api::{local_api_addr, spawn_local_api_with_bus};
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::state_mutation::StateMutationService;
use niuma_core::store::NiumaStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{Manager, RunEvent, WindowEvent};

mod commands;
#[cfg(target_os = "macos")]
mod macos;
mod tools;
mod tray;

const LOCAL_API_START_DELAY: Duration = Duration::ZERO;
const WATCHER_START_DELAY: Duration = Duration::from_secs(1);
const STALE_SWEEP_INTERVAL: Duration = Duration::from_secs(30);
const CODEX_PLUGIN_BINARY_NAME: &str = "niuma-codex-plugin";
const BARK_PLUGIN_BINARY_NAME: &str = "niuma-plugin-bark";
const NTFY_PLUGIN_BINARY_NAME: &str = "niuma-plugin-ntfy";

fn spawn_background_services(store: NiumaStore, runtime_events: RuntimeEventBus) {
    let spawn_result = thread::Builder::new()
        .name("niuma-background-services-startup".to_string())
        .spawn(move || {
            if LOCAL_API_START_DELAY > Duration::ZERO {
                thread::sleep(LOCAL_API_START_DELAY);
            }
            match spawn_local_api_with_bus(store.clone(), runtime_events.clone()) {
                Ok(_) => {
                    eprintln!("NiumaNotifier Local API started at {}", local_api_addr());
                }
                Err(error) => {
                    // 端口可能已被另一个开发实例占用；UI 仍可读取同一份状态文件。
                    eprintln!("NiumaNotifier Local API not started: {error}");
                }
            }
            spawn_stale_sweep_runtime(store.clone(), runtime_events.clone());

            // Codex session 扫描放到首屏之后，避免文件系统监听和活跃文件轮询抢首屏资源。
            thread::sleep(WATCHER_START_DELAY);
            tools::spawn_tool_runtimes(store.clone(), runtime_events.clone());
        });

    if let Err(error) = spawn_result {
        eprintln!("NiumaNotifier background services startup thread not started: {error}");
    }
}

fn spawn_stale_sweep_runtime(store: NiumaStore, runtime_events: RuntimeEventBus) {
    if let Err(error) = thread::Builder::new()
        .name("stale-sweep-runtime".to_string())
        .spawn(move || {
            let service = StateMutationService::new(store, runtime_events);
            loop {
                thread::sleep(STALE_SWEEP_INTERVAL);
                if let Err(error) = run_stale_sweep_once(&service, chrono::Utc::now()) {
                    eprintln!("NiumaNotifier stale sweep failed: {error}");
                }
            }
        })
    {
        eprintln!("NiumaNotifier stale sweep runtime not started: {error}");
    }
}

fn run_stale_sweep_once(
    service: &StateMutationService,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    service
        .mark_stale_running_sessions(now, chrono::Duration::minutes(10))
        .map(|_| ())
}

fn configure_builtin_codex_plugin_command(app: &tauri::App) {
    configure_builtin_plugin_command(
        app,
        niuma_core::plugin::CODEX_PLUGIN_COMMAND_ENV,
        CODEX_PLUGIN_BINARY_NAME,
    );
}

fn configure_builtin_bark_plugin_command(app: &tauri::App) {
    configure_builtin_plugin_command(
        app,
        niuma_core::plugin::BARK_PLUGIN_COMMAND_ENV,
        BARK_PLUGIN_BINARY_NAME,
    );
}

fn configure_builtin_ntfy_plugin_command(app: &tauri::App) {
    configure_builtin_plugin_command(
        app,
        niuma_core::plugin::NTFY_PLUGIN_COMMAND_ENV,
        NTFY_PLUGIN_BINARY_NAME,
    );
}

fn configure_builtin_plugin_command(app: &tauri::App, env_key: &str, binary_name: &str) {
    if std::env::var_os(env_key).is_some() {
        return;
    }
    let resource_dir = app.path().resource_dir().ok();
    let current_exe = std::env::current_exe().ok();
    if let Some(command) =
        resolve_builtin_plugin_command(binary_name, resource_dir.as_ref(), current_exe.as_ref())
    {
        // 只设置命令路径，不直接启动插件；启动仍由通用插件管理器按 manifest 完成。
        std::env::set_var(env_key, command.to_string_lossy().to_string());
    }
}

fn resolve_builtin_plugin_command(
    binary_name: &str,
    resource_dir: Option<&PathBuf>,
    current_exe: Option<&PathBuf>,
) -> Option<PathBuf> {
    let executable_name = niuma_core::platform::executable::executable_name(binary_name);
    let mut candidates = Vec::new();
    if let Some(resource_dir) = resource_dir {
        // 打包资源放在 resource_dir/bin，兼容旧版本曾经放在 resource_dir 根目录的情况。
        candidates.push(resource_dir.join("bin").join(&executable_name));
        candidates.push(resource_dir.join(&executable_name));
    }
    if let Some(current_exe) = current_exe {
        if let Some(exe_dir) = current_exe.parent() {
            candidates.push(exe_dir.join(&executable_name));
        }
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn main() {
    let is_quitting = Arc::new(AtomicBool::new(false));
    let runtime_events = RuntimeEventBus::new();
    let store = NiumaStore::new(NiumaStore::default_path());
    let mutation_service = StateMutationService::new(store.clone(), runtime_events.clone());

    let app = tauri::Builder::default()
        .manage(commands::AppRuntimeState {
            store: store.clone(),
            mutation_service,
            runtime_events: runtime_events.clone(),
        })
        .enable_macos_default_menu(tray::enable_macos_default_menu())
        .setup({
            let is_quitting = Arc::clone(&is_quitting);
            let runtime_events = runtime_events.clone();
            move |app| {
                if let Err(error) = commands::restore_language_preference_from_store() {
                    eprintln!("NiumaNotifier language preference not restored: {error}");
                }
                configure_builtin_codex_plugin_command(app);
                configure_builtin_bark_plugin_command(app);
                configure_builtin_ntfy_plugin_command(app);
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
                spawn_background_services(store.clone(), runtime_events.clone());
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
            commands::get_plugins,
            commands::remove_plugin,
            commands::set_plugin_enabled,
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use niuma_core::models::{EventType, NiumaEvent, SessionStatus, ToolKind};

    #[test]
    fn stale_sweep_once_marks_old_running_sessions() {
        let path = std::env::temp_dir().join(format!(
            "niuma-desktop-stale-sweep-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = NiumaStore::new(path.clone());
        store
            .append_event(sample_event("event-running", 1_000))
            .unwrap();
        let service = StateMutationService::new(store.clone(), RuntimeEventBus::new());

        run_stale_sweep_once(
            &service,
            chrono::Utc.timestamp_opt(1_700, 0).single().unwrap(),
        )
        .unwrap();

        assert_eq!(
            store.load().unwrap().sessions[0].status,
            SessionStatus::Stale
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn builtin_plugin_command_prefers_resource_binary() {
        let temp = tempfile::tempdir().unwrap();
        let resource_dir = temp.path().join("resources");
        let resource_bin_dir = resource_dir.join("bin");
        let exe_dir = temp.path().join("bin");
        std::fs::create_dir_all(&resource_bin_dir).unwrap();
        std::fs::create_dir_all(&exe_dir).unwrap();
        let executable_name =
            niuma_core::platform::executable::executable_name(BARK_PLUGIN_BINARY_NAME);
        let resource_binary = resource_bin_dir.join(&executable_name);
        let exe_binary = exe_dir.join(&executable_name);
        std::fs::write(&resource_binary, "").unwrap();
        std::fs::write(&exe_binary, "").unwrap();

        let command = resolve_builtin_plugin_command(
            BARK_PLUGIN_BINARY_NAME,
            Some(&resource_dir),
            Some(&exe_dir.join("NiumaNotifier")),
        );

        assert_eq!(command.as_deref(), Some(resource_binary.as_path()));
    }

    #[test]
    fn builtin_ntfy_plugin_command_prefers_resource_binary() {
        let temp = tempfile::tempdir().unwrap();
        let resource_dir = temp.path().join("resources");
        let resource_bin_dir = resource_dir.join("bin");
        std::fs::create_dir_all(&resource_bin_dir).unwrap();
        let executable_name =
            niuma_core::platform::executable::executable_name(NTFY_PLUGIN_BINARY_NAME);
        let resource_binary = resource_bin_dir.join(&executable_name);
        std::fs::write(&resource_binary, "").unwrap();

        let command =
            resolve_builtin_plugin_command(NTFY_PLUGIN_BINARY_NAME, Some(&resource_dir), None);

        assert_eq!(command.as_deref(), Some(resource_binary.as_path()));
    }

    #[test]
    fn builtin_plugin_command_falls_back_to_current_exe_dir() {
        let temp = tempfile::tempdir().unwrap();
        let exe_dir = temp.path().join("bin");
        std::fs::create_dir_all(&exe_dir).unwrap();
        let executable_name =
            niuma_core::platform::executable::executable_name(BARK_PLUGIN_BINARY_NAME);
        let exe_binary = exe_dir.join(&executable_name);
        std::fs::write(&exe_binary, "").unwrap();

        let command = resolve_builtin_plugin_command(
            BARK_PLUGIN_BINARY_NAME,
            Some(&temp.path().join("missing-resources")),
            Some(&exe_dir.join("NiumaNotifier")),
        );

        assert_eq!(command.as_deref(), Some(exe_binary.as_path()));
    }

    fn sample_event(id: &str, timestamp: i64) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: id.to_string(),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: "session-1".to_string(),
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            event_type: EventType::SessionStarted,
            severity: "info".to_string(),
            summary: "started".to_string(),
            content: None,
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            created_at: chrono::Utc.timestamp_opt(timestamp, 0).single().unwrap(),
        }
    }
    #[test]
    fn startup_keeps_local_api_immediate_and_delays_watcher_only() {
        assert_eq!(LOCAL_API_START_DELAY, Duration::ZERO);
        assert_eq!(WATCHER_START_DELAY, Duration::from_secs(1));
    }
}
