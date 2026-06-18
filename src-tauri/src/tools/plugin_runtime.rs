use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use niuma_core::plugin::{
    current_plugin_registry, PluginManifest, PluginRegistry, PluginRuntimeState, PluginSource,
};
use niuma_core::runtime_event::{RuntimeEvent, RuntimeEventBus, StateChangeReason};
use niuma_core::store::SqliteStateStore;

const FALLBACK_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);

pub trait ToolPlugin {
    fn manifest(&self) -> PluginManifest;
    fn spawn(
        &self,
        store: SqliteStateStore,
        runtime_events: RuntimeEventBus,
    ) -> std::io::Result<thread::JoinHandle<()>>;
}

pub struct CodexBuiltinPlugin;

impl ToolPlugin for CodexBuiltinPlugin {
    fn manifest(&self) -> PluginManifest {
        niuma_core::plugin::builtin_codex_manifest()
    }

    fn spawn(
        &self,
        store: SqliteStateStore,
        runtime_events: RuntimeEventBus,
    ) -> std::io::Result<thread::JoinHandle<()>> {
        crate::tools::codex::session_runtime::spawn_codex_session_runtime(store, runtime_events)
    }
}

pub fn spawn_plugin_runtimes(runtime_events: RuntimeEventBus) {
    let registry = current_plugin_registry();
    spawn_builtin_codex(&registry, runtime_events.clone());
    spawn_external_plugin_manager(runtime_events);
}

fn spawn_builtin_codex(registry: &PluginRegistry, runtime_events: RuntimeEventBus) {
    let plugin = CodexBuiltinPlugin;
    let manifest = plugin.manifest();
    if registry.plugin_by_id(&manifest.id).is_none() {
        return;
    }
    let store = SqliteStateStore::new(SqliteStateStore::default_path());
    match plugin.spawn(store.clone(), runtime_events) {
        Ok(_detached_watcher_thread) => {
            // JoinHandle 在这里丢弃会 detach 后台线程，避免阻塞 Tauri 主循环。
            eprintln!("NiumaNotifier Codex builtin plugin runtime thread started");
            save_runtime_state(&store, &manifest.id, PluginRuntimeState::running());
        }
        Err(error) => {
            // 文件监听只是状态增强能力，启动失败不能影响状态栏应用常驻。
            eprintln!("NiumaNotifier Codex builtin plugin not started: {error}");
            save_runtime_state(
                &store,
                &manifest.id,
                PluginRuntimeState::failed(error.to_string()),
            );
        }
    }
}

fn spawn_external_plugin_manager(runtime_events: RuntimeEventBus) {
    if let Err(error) = thread::Builder::new()
        .name("plugin-runtime-manager".to_string())
        .spawn(move || run_external_plugin_manager(runtime_events))
    {
        eprintln!("NiumaNotifier external plugin manager not started: {error}");
    }
}

struct ManagedExternalPlugin {
    manifest: PluginManifest,
    child: Option<Child>,
    next_start: Instant,
}

fn run_external_plugin_manager(runtime_events: RuntimeEventBus) {
    let store = SqliteStateStore::new(SqliteStateStore::default_path());
    let mut managed = HashMap::<String, ManagedExternalPlugin>::new();
    let mut receiver = runtime_events.subscribe();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("NiumaNotifier plugin manager runtime not started: {error}");
            return;
        }
    };

    loop {
        reconcile_external_plugins(&store, &mut managed);
        wait_for_plugin_reconcile_signal(&runtime, &mut receiver);
    }
}

fn reconcile_external_plugins(
    store: &SqliteStateStore,
    managed: &mut HashMap<String, ManagedExternalPlugin>,
) {
    let registry = current_plugin_registry();
    let manifests = registry
        .manifests()
        .iter()
        .filter(|manifest| manifest.source == PluginSource::External)
        .cloned()
        .collect::<Vec<_>>();
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
            .or_insert_with(|| ManagedExternalPlugin {
                manifest: manifest.clone(),
                child: None,
                next_start: Instant::now(),
            });
        if entry.manifest != manifest {
            stop_child(store, &entry.manifest, &mut entry.child);
            entry.manifest = manifest;
            entry.next_start = Instant::now();
        }
        tick_external_plugin(store, entry);
    }
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

fn tick_external_plugin(store: &SqliteStateStore, entry: &mut ManagedExternalPlugin) {
    let enabled = store
        .listener_config()
        .map(|config| config.is_tool_enabled(&entry.manifest.tool_id))
        .unwrap_or(false);

    if !enabled {
        stop_child(store, &entry.manifest, &mut entry.child);
        entry.next_start = Instant::now();
        save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::stopped());
        return;
    }

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

    if entry.child.is_none() && Instant::now() >= entry.next_start {
        save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::starting());
        match spawn_external_plugin_process(&entry.manifest) {
            Ok(process) => {
                eprintln!(
                    "NiumaNotifier external plugin {} started",
                    entry.manifest.id
                );
                entry.child = Some(process);
                save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::running());
            }
            Err(error) => {
                eprintln!(
                    "NiumaNotifier external plugin {} not started: {error}",
                    entry.manifest.id
                );
                entry.next_start = Instant::now() + Duration::from_secs(10);
                save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::failed(error));
            }
        }
    }
}

fn spawn_external_plugin_process(manifest: &PluginManifest) -> Result<Child, String> {
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
        .env("NIUMA_TOOL_ID", manifest.tool_id.as_str())
        .env(
            "NIUMA_STATE_PATH",
            SqliteStateStore::default_path()
                .to_string_lossy()
                .to_string(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in &manifest.env {
        process.env(key, value);
    }
    if let Some(base_dir) = &manifest.base_dir {
        process.current_dir(base_dir);
    }
    process
        .spawn()
        .map_err(|error| format!("启动插件进程失败：{error}"))
}

fn stop_child(store: &SqliteStateStore, manifest: &PluginManifest, child: &mut Option<Child>) {
    let Some(mut process) = child.take() else {
        return;
    };
    save_runtime_state(store, &manifest.id, PluginRuntimeState::stopping());
    if let Err(error) = process.kill() {
        eprintln!("NiumaNotifier plugin {} stop failed: {error}", manifest.id);
    }
    let _ = process.wait();
}

fn save_runtime_state(store: &SqliteStateStore, plugin_id: &str, state: PluginRuntimeState) {
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
    use niuma_core::plugin::{PluginCapability, PluginSource};
    use std::collections::BTreeMap;

    #[test]
    fn resolves_relative_command_against_manifest_dir() {
        let manifest = PluginManifest {
            id: "demo".to_string(),
            tool_id: ToolKind::Custom("demo".to_string()),
            display_name: "Demo".to_string(),
            version: "0.1.0".to_string(),
            command: Some("./bin/demo".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::EventWatcher],
            icon_url: None,
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
            tool_id: ToolKind::Custom("demo".to_string()),
            display_name: "Demo".to_string(),
            version: "0.1.0".to_string(),
            command: Some("definitely-missing-niuma-command".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::EventWatcher],
            icon_url: None,
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
}
