use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use niuma_core::plugin::{
    current_plugin_registry, default_non_tool_plugin_enabled, plugin_uses_listener_config,
    resolve_plugin_config, PluginCapability, PluginManifest, PluginRegistry, PluginRuntimeState,
    BUILTIN_BARK_PLUGIN_ID, BUILTIN_NTFY_PLUGIN_ID,
};
use niuma_core::runtime_event::{RuntimeEvent, RuntimeEventBus, StateChangeReason};
use niuma_core::store::NiumaStore;

const FALLBACK_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
const PARENT_PID_ENV: &str = "NIUMA_PARENT_PID";

pub fn spawn_plugin_runtimes(store: NiumaStore, runtime_events: RuntimeEventBus) {
    spawn_plugin_manager(store, runtime_events);
}

fn spawn_plugin_manager(store: NiumaStore, runtime_events: RuntimeEventBus) {
    if let Err(error) = thread::Builder::new()
        .name("plugin-runtime-manager".to_string())
        .spawn(move || run_plugin_manager(store, runtime_events))
    {
        eprintln!("NiumaNotifier plugin manager not started: {error}");
    }
}

struct ManagedPlugin {
    manifest: PluginManifest,
    child: Option<Child>,
    next_start: Instant,
}

fn run_plugin_manager(store: NiumaStore, runtime_events: RuntimeEventBus) {
    let mut managed = HashMap::<String, ManagedPlugin>::new();
    let mut receiver = runtime_events.subscribe();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("NiumaNotifier plugin manager runtime not started: {error}");
            return;
        }
    };

    loop {
        reconcile_managed_plugins(&store, &mut managed);
        wait_for_plugin_reconcile_signal(&runtime, &mut receiver);
    }
}

fn reconcile_managed_plugins(store: &NiumaStore, managed: &mut HashMap<String, ManagedPlugin>) {
    let registry = current_plugin_registry();
    let manifests = managed_runtime_manifests(&registry);
    let current_ids = manifests
        .iter()
        .map(|manifest| manifest.id.clone())
        .collect::<std::collections::HashSet<_>>();

    for removed_id in managed
        .keys()
        .filter(|plugin_id| !current_ids.contains(*plugin_id))
        .cloned()
        .collect::<Vec<_>>()
    {
        if let Some(mut entry) = managed.remove(&removed_id) {
            stop_child(store, &entry.manifest, &mut entry.child);
            save_runtime_state(store, &removed_id, PluginRuntimeState::stopped());
        }
    }

    for manifest in manifests {
        let entry = managed
            .entry(manifest.id.clone())
            .or_insert_with(|| ManagedPlugin {
                manifest: manifest.clone(),
                child: None,
                next_start: Instant::now(),
            });
        if entry.manifest != manifest {
            stop_child(store, &entry.manifest, &mut entry.child);
            entry.manifest = manifest;
            entry.next_start = Instant::now();
        }
        tick_managed_plugin(store, entry);
    }
}

fn managed_runtime_manifests(registry: &PluginRegistry) -> Vec<PluginManifest> {
    registry
        .manifests()
        .iter()
        .filter(|manifest| is_managed_runtime_manifest(manifest))
        .cloned()
        .collect()
}

fn is_managed_runtime_manifest(manifest: &PluginManifest) -> bool {
    manifest.capabilities.iter().any(|capability| {
        matches!(
            capability,
            PluginCapability::EventWatcher
                | PluginCapability::EventConsumer
                | PluginCapability::StateConsumer
        )
    })
}

fn wait_for_plugin_reconcile_signal(
    runtime: &tokio::runtime::Runtime,
    receiver: &mut tokio::sync::broadcast::Receiver<RuntimeEvent>,
) {
    runtime.block_on(async {
        loop {
            match tokio::time::timeout(FALLBACK_RECONCILE_INTERVAL, receiver.recv()).await {
                Ok(Ok(RuntimeEvent::StateChanged {
                    reason: StateChangeReason::ListenerConfigChanged,
                    ..
                }))
                | Ok(Ok(RuntimeEvent::StateChanged {
                    reason: StateChangeReason::PluginConfigChanged,
                    ..
                }))
                | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_)))
                | Err(_) => return,
                Ok(Ok(_)) => {}
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                    tokio::time::sleep(FALLBACK_RECONCILE_INTERVAL).await;
                    return;
                }
            }
        }
    });
}

fn tick_managed_plugin(store: &NiumaStore, entry: &mut ManagedPlugin) {
    if !plugin_runtime_enabled(store, &entry.manifest) {
        stop_child(store, &entry.manifest, &mut entry.child);
        entry.next_start = Instant::now();
        save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::stopped());
        return;
    }

    let launch_files_ready = match prepare_plugin_launch_files(store, &entry.manifest) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "NiumaNotifier plugin {} launch file update failed: {error}",
                entry.manifest.id
            );
            if entry.child.is_none() {
                entry.next_start = Instant::now() + Duration::from_secs(10);
                save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::failed(error));
            }
            false
        }
    };

    if let Some(process) = entry.child.as_mut() {
        match process.try_wait() {
            Ok(Some(status)) => {
                let message = format!("插件进程退出：{status}");
                eprintln!("NiumaNotifier plugin {} {message}", entry.manifest.id);
                entry.child = None;
                entry.next_start = Instant::now() + Duration::from_secs(5);
                save_runtime_state(
                    store,
                    &entry.manifest.id,
                    PluginRuntimeState::failed(message),
                );
            }
            Ok(None) => {
                save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::running());
                return;
            }
            Err(error) => {
                let message = format!("检查插件进程失败：{error}");
                eprintln!("NiumaNotifier plugin {} {message}", entry.manifest.id);
                entry.child = None;
                entry.next_start = Instant::now() + Duration::from_secs(5);
                save_runtime_state(
                    store,
                    &entry.manifest.id,
                    PluginRuntimeState::failed(message),
                );
            }
        }
    }

    if launch_files_ready && entry.child.is_none() && Instant::now() >= entry.next_start {
        save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::starting());
        match spawn_plugin_process(&entry.manifest) {
            Ok(process) => {
                eprintln!("NiumaNotifier plugin {} started", entry.manifest.id);
                entry.child = Some(process);
                save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::running());
            }
            Err(error) => {
                eprintln!(
                    "NiumaNotifier plugin {} not started: {error}",
                    entry.manifest.id
                );
                entry.next_start = Instant::now() + Duration::from_secs(10);
                save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::failed(error));
            }
        }
    }
}

fn prepare_plugin_launch_files(
    store: &NiumaStore,
    manifest: &PluginManifest,
) -> Result<(), String> {
    if manifest.id == BUILTIN_BARK_PLUGIN_ID || manifest.id == BUILTIN_NTFY_PLUGIN_ID {
        write_notification_plugin_config(store, manifest, &plugin_config_path(&manifest.id))?;
    }
    Ok(())
}

fn write_notification_plugin_config(
    store: &NiumaStore,
    manifest: &PluginManifest,
    path: &PathBuf,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建通知插件配置目录失败：{error}"))?;
    }
    let mut payload = notification_plugin_config_payload(store, manifest)?;
    payload.insert(
        "enabled".to_string(),
        serde_json::json!(plugin_runtime_enabled(store, manifest)),
    );
    fs::write(
        path,
        serde_json::to_string_pretty(&payload)
            .map_err(|error| format!("序列化通知插件配置失败：{error}"))?,
    )
    .map_err(|error| format!("写入通知插件配置失败：{error}"))
}

fn notification_plugin_config_payload(
    store: &NiumaStore,
    manifest: &PluginManifest,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let stored_config = store.plugin_config(&manifest.id)?;
    let config = resolve_plugin_config(manifest, stored_config.clone());
    if stored_config.is_none() {
        // 首次启动写入 manifest 默认配置，后续只以插件 JSON 配置为权威来源。
        store.save_plugin_config(&manifest.id, &config)?;
    }
    Ok(config)
}

fn plugin_runtime_enabled(store: &NiumaStore, manifest: &PluginManifest) -> bool {
    if plugin_uses_listener_config(manifest) {
        let Some(tool) = &manifest.tool_id else {
            return false;
        };
        return store
            .listener_config()
            .map(|config| config.is_tool_enabled(tool))
            .unwrap_or(false);
    }
    store
        .plugin_enabled_map()
        .map(|map| {
            map.get(&manifest.id)
                .copied()
                .unwrap_or_else(|| default_non_tool_plugin_enabled(manifest))
        })
        .unwrap_or(false)
}

fn spawn_plugin_process(manifest: &PluginManifest) -> Result<Child, String> {
    build_plugin_command(manifest)?
        .spawn()
        .map_err(|error| format!("启动插件进程失败：{error}"))
}

fn build_plugin_command(manifest: &PluginManifest) -> Result<Command, String> {
    let command = manifest
        .command
        .as_ref()
        .ok_or_else(|| "外部插件缺少 command".to_string())?;
    let command_path = resolve_command_path(manifest, command);
    let mut process = Command::new(command_path);
    process
        .args(&manifest.args)
        .env(
            "NIUMA_LOCAL_API_URL",
            format!("http://{}", niuma_api::local_api_addr()),
        )
        .env("NIUMA_PLUGIN_ID", &manifest.id)
        .env(
            "NIUMA_PLUGIN_CONFIG_PATH",
            plugin_config_path(&manifest.id)
                .to_string_lossy()
                .to_string(),
        )
        .env(
            "NIUMA_PLUGIN_DATA_DIR",
            plugin_data_dir(&manifest.id).to_string_lossy().to_string(),
        )
        .env(PARENT_PID_ENV, std::process::id().to_string())
        .env(
            "NIUMA_DB_PATH",
            NiumaStore::default_path().to_string_lossy().to_string(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(tool_id) = &manifest.tool_id {
        process.env("NIUMA_TOOL_ID", tool_id.as_str());
    }
    for (key, value) in &manifest.env {
        process.env(key, value);
    }
    if let Some(base_dir) = &manifest.base_dir {
        process.current_dir(base_dir);
    }
    Ok(process)
}

fn plugin_config_path(plugin_id: &str) -> PathBuf {
    niuma_core::platform::paths::app_data_dir()
        .join("plugin-configs")
        .join(plugin_id)
        .join("config.json")
}

fn plugin_data_dir(plugin_id: &str) -> PathBuf {
    niuma_core::platform::paths::app_data_dir()
        .join("plugin-data")
        .join(plugin_id)
}

fn stop_child(store: &NiumaStore, manifest: &PluginManifest, child: &mut Option<Child>) {
    let Some(mut process) = child.take() else {
        return;
    };
    save_runtime_state(store, &manifest.id, PluginRuntimeState::stopping());
    if let Err(error) = process.kill() {
        eprintln!("NiumaNotifier plugin {} stop failed: {error}", manifest.id);
    }
    let _ = process.wait();
}

fn save_runtime_state(store: &NiumaStore, plugin_id: &str, state: PluginRuntimeState) {
    if let Err(error) = store.save_plugin_runtime_state(plugin_id, state) {
        eprintln!("NiumaNotifier plugin runtime state save failed for {plugin_id}: {error}");
    }
}

fn resolve_command_path(manifest: &PluginManifest, command: &str) -> PathBuf {
    let path = PathBuf::from(command);
    if path.is_absolute() {
        return path;
    }
    if !command_has_path_separator(command) {
        return resolve_bare_command_path(command);
    }
    // 带路径的相对命令按插件目录解析；裸命令如 "node" 交给系统 PATH 查找。
    manifest
        .base_dir
        .as_ref()
        .map(|base_dir| base_dir.join(path))
        .unwrap_or_else(|| PathBuf::from(command))
}

fn resolve_bare_command_path(command: &str) -> PathBuf {
    let executable_name = niuma_core::platform::executable::executable_name(command);
    let path_dirs = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    let fallback_dirs = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ];
    for dir in path_dirs
        .into_iter()
        .chain(fallback_dirs.iter().map(PathBuf::from))
    {
        let candidate = dir.join(&executable_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(command)
}

fn command_has_path_separator(command: &str) -> bool {
    command.contains('/') || command.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::models::ToolKind;
    use niuma_core::plugin::{PluginCapability, PluginKind, PluginSource};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn resolves_relative_command_against_manifest_dir() {
        let manifest = PluginManifest {
            id: "demo".to_string(),
            kind: PluginKind::Tool,
            tool_id: Some(ToolKind::Custom("demo".to_string())),
            display_name: "Demo".to_string(),
            version: "0.1.0".to_string(),
            command: Some("./bin/demo".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::EventWatcher],
            icon_url: None,
            config_schema: Vec::new(),
            source: PluginSource::External,
            base_dir: Some(PathBuf::from("/tmp/plugin-demo")),
        };

        assert_eq!(
            resolve_command_path(&manifest, "./bin/demo"),
            PathBuf::from("/tmp/plugin-demo/./bin/demo")
        );
    }

    #[test]
    fn leaves_bare_command_for_path_lookup() {
        let manifest = PluginManifest {
            id: "demo".to_string(),
            kind: PluginKind::Tool,
            tool_id: Some(ToolKind::Custom("demo".to_string())),
            display_name: "Demo".to_string(),
            version: "0.1.0".to_string(),
            command: Some("definitely-missing-niuma-command".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::EventWatcher],
            icon_url: None,
            config_schema: Vec::new(),
            source: PluginSource::External,
            base_dir: Some(PathBuf::from("/tmp/plugin-demo")),
        };

        assert_eq!(
            resolve_command_path(&manifest, "definitely-missing-niuma-command"),
            PathBuf::from("definitely-missing-niuma-command")
        );
    }

    #[test]
    fn listener_config_changed_wakes_plugin_manager_wait() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let bus = RuntimeEventBus::new();
        let mut receiver = bus.subscribe();

        bus.publish_state_changed(StateChangeReason::ListenerConfigChanged);
        let started_at = Instant::now();
        wait_for_plugin_reconcile_signal(&runtime, &mut receiver);

        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn plugin_config_changed_wakes_plugin_manager_wait() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let bus = RuntimeEventBus::new();
        let mut receiver = bus.subscribe();

        bus.publish_state_changed(StateChangeReason::PluginConfigChanged);
        let started_at = Instant::now();
        wait_for_plugin_reconcile_signal(&runtime, &mut receiver);

        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn managed_runtime_manifests_include_builtin_codex() {
        let registry = PluginRegistry::with_builtin_plugins();
        let manifests = managed_runtime_manifests(&registry);

        assert_eq!(manifests.len(), 3);
        assert!(manifests
            .iter()
            .any(|manifest| manifest.id == "builtin-codex"
                && manifest.source == PluginSource::Builtin));
        assert!(manifests
            .iter()
            .any(|manifest| manifest.id == "builtin-bark"
                && manifest.source == PluginSource::Builtin));
        assert!(manifests
            .iter()
            .any(|manifest| manifest.id == "builtin-ntfy"
                && manifest.source == PluginSource::Builtin));
    }

    #[test]
    fn managed_runtime_manifests_include_event_consumers() {
        let mut registry = PluginRegistry::new();
        registry.register(notification_consumer_manifest("builtin-bark"));
        registry.register(state_consumer_manifest("status-indicator-demo"));

        let manifests = managed_runtime_manifests(&registry);

        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].id, "builtin-bark");
        assert_eq!(manifests[1].id, "status-indicator-demo");
    }

    #[test]
    fn spawn_plugin_runtimes_requires_shared_store_from_main_app() {
        let _spawn: fn(NiumaStore, RuntimeEventBus) = spawn_plugin_runtimes;
    }

    #[test]
    fn event_consumer_runtime_enabled_reads_plugin_enabled_map() {
        let store = NiumaStore::new(test_sqlite_path("event_consumer_enabled"));
        let mut enabled = BTreeMap::new();
        enabled.insert("builtin-bark".to_string(), true);
        enabled.insert("status-indicator-demo".to_string(), true);
        store.save_plugin_enabled_map(&enabled).unwrap();

        assert!(plugin_runtime_enabled(
            &store,
            &notification_consumer_manifest("builtin-bark")
        ));
        assert!(plugin_runtime_enabled(
            &store,
            &state_consumer_manifest("status-indicator-demo")
        ));
    }

    #[test]
    fn event_consumer_runtime_enabled_by_default_until_explicitly_disabled() {
        let store = NiumaStore::new(test_sqlite_path("event_consumer_disabled"));
        let manifest = notification_consumer_manifest("builtin-bark");

        assert!(plugin_runtime_enabled(&store, &manifest));

        let mut enabled = BTreeMap::new();
        enabled.insert("builtin-bark".to_string(), false);
        store.save_plugin_enabled_map(&enabled).unwrap();

        assert!(!plugin_runtime_enabled(&store, &manifest));
    }

    #[test]
    fn build_plugin_command_injects_parent_pid() {
        let manifest = PluginManifest {
            id: "demo".to_string(),
            kind: PluginKind::Tool,
            tool_id: Some(ToolKind::Custom("demo".to_string())),
            display_name: "Demo".to_string(),
            version: "0.1.0".to_string(),
            command: Some("definitely-missing-niuma-command".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::EventWatcher],
            icon_url: None,
            config_schema: Vec::new(),
            source: PluginSource::External,
            base_dir: Some(PathBuf::from("/tmp/plugin-demo")),
        };

        let command = build_plugin_command(&manifest).unwrap();

        assert_eq!(
            command_env_value(&command, "NIUMA_PARENT_PID"),
            Some(std::process::id().to_string())
        );
        assert!(
            command_env_value(&command, "NIUMA_PLUGIN_CONFIG_PATH").is_some_and(|value| value
                .contains("plugin-configs")
                && value.ends_with("config.json"))
        );
        assert!(command_env_value(&command, "NIUMA_PLUGIN_DATA_DIR")
            .is_some_and(|value| value.contains("plugin-data") && value.contains("demo")));
    }

    #[test]
    fn bark_plugin_config_payload_uses_plugin_config_store() {
        let store = NiumaStore::new(test_sqlite_path("bark_plugin_config_store"));
        let manifest = niuma_core::plugin::builtin_bark_manifest();
        let mut config = serde_json::Map::new();
        config.insert("device_key".to_string(), json!("device-1"));
        store.save_plugin_config(&manifest.id, &config).unwrap();

        let payload = notification_plugin_config_payload(&store, &manifest).unwrap();

        assert_eq!(payload.get("device_key"), Some(&json!("device-1")));
        assert!(payload.get("server").is_none());
        assert!(payload.get("group").is_none());
        assert!(payload.get("icon_url").is_none());
    }

    fn command_env_value(command: &Command, key: &str) -> Option<String> {
        command.get_envs().find_map(|(name, value)| {
            if name == std::ffi::OsStr::new(key) {
                value.map(|value| value.to_string_lossy().to_string())
            } else {
                None
            }
        })
    }

    fn notification_consumer_manifest(id: &str) -> PluginManifest {
        PluginManifest {
            id: id.to_string(),
            kind: PluginKind::Notification,
            tool_id: None,
            display_name: "Bark".to_string(),
            version: "0.1.0".to_string(),
            command: Some("definitely-missing-niuma-command".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::EventConsumer],
            icon_url: None,
            config_schema: Vec::new(),
            source: PluginSource::Builtin,
            base_dir: None,
        }
    }

    fn state_consumer_manifest(id: &str) -> PluginManifest {
        PluginManifest {
            id: id.to_string(),
            kind: PluginKind::StatusIndicator,
            tool_id: None,
            display_name: "Status Indicator".to_string(),
            version: "0.1.0".to_string(),
            command: Some("definitely-missing-niuma-command".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::StateConsumer],
            icon_url: None,
            config_schema: Vec::new(),
            source: PluginSource::External,
            base_dir: None,
        }
    }

    fn test_sqlite_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "niuma-plugin-runtime-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("niuma.sqlite")
    }
}
