use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use niuma_core::plugin::{default_user_plugin_dir, PluginManifest, PluginRegistry, PluginSource};
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::store::SqliteStateStore;

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
    let registry = PluginRegistry::with_builtin_plugins()
        .discover_external_plugins(&default_user_plugin_dir());
    spawn_builtin_codex(&registry, runtime_events.clone());
    spawn_external_plugins(&registry);
}

fn spawn_builtin_codex(registry: &PluginRegistry, runtime_events: RuntimeEventBus) {
    let plugin = CodexBuiltinPlugin;
    let manifest = plugin.manifest();
    if registry.plugin_by_id(&manifest.id).is_none() {
        return;
    }
    let store = SqliteStateStore::new(SqliteStateStore::default_path());
    match plugin.spawn(store, runtime_events) {
        Ok(_detached_watcher_thread) => {
            // JoinHandle 在这里丢弃会 detach 后台线程，避免阻塞 Tauri 主循环。
            eprintln!("NiumaNotifier Codex builtin plugin runtime thread started");
        }
        Err(error) => {
            // 文件监听只是状态增强能力，启动失败不能影响状态栏应用常驻。
            eprintln!("NiumaNotifier Codex builtin plugin not started: {error}");
        }
    }
}

fn spawn_external_plugins(registry: &PluginRegistry) {
    for manifest in registry
        .manifests()
        .iter()
        .filter(|manifest| manifest.source == PluginSource::External)
        .cloned()
    {
        if let Err(error) = thread::Builder::new()
            .name(format!("plugin-runtime-{}", manifest.id))
            .spawn(move || run_external_plugin_supervisor(manifest))
        {
            eprintln!("NiumaNotifier external plugin supervisor not started: {error}");
        }
    }
}

fn run_external_plugin_supervisor(manifest: PluginManifest) {
    let store = SqliteStateStore::new(SqliteStateStore::default_path());
    let mut child = None;
    let mut next_start = Instant::now();
    loop {
        let enabled = store
            .listener_config()
            .map(|config| config.is_tool_enabled(&manifest.tool_id))
            .unwrap_or(false);

        if !enabled {
            stop_child(&manifest, &mut child);
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        if let Some(process) = child.as_mut() {
            match process.try_wait() {
                Ok(Some(status)) => {
                    eprintln!(
                        "NiumaNotifier plugin {} exited with status {status}",
                        manifest.id
                    );
                    child = None;
                    next_start = Instant::now() + Duration::from_secs(5);
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "NiumaNotifier plugin {} status failed: {error}",
                        manifest.id
                    );
                    child = None;
                    next_start = Instant::now() + Duration::from_secs(5);
                }
            }
        }

        if child.is_none() && Instant::now() >= next_start {
            match spawn_external_plugin_process(&manifest) {
                Ok(process) => {
                    eprintln!("NiumaNotifier external plugin {} started", manifest.id);
                    child = Some(process);
                }
                Err(error) => {
                    eprintln!(
                        "NiumaNotifier external plugin {} not started: {error}",
                        manifest.id
                    );
                    next_start = Instant::now() + Duration::from_secs(10);
                }
            }
        }

        thread::sleep(Duration::from_secs(1));
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

fn stop_child(manifest: &PluginManifest, child: &mut Option<Child>) {
    let Some(mut process) = child.take() else {
        return;
    };
    if let Err(error) = process.kill() {
        eprintln!("NiumaNotifier plugin {} stop failed: {error}", manifest.id);
    }
    let _ = process.wait();
}

fn resolve_command_path(manifest: &PluginManifest, command: &str) -> PathBuf {
    let path = PathBuf::from(command);
    if path.is_absolute() {
        return path;
    }
    manifest
        .base_dir
        .as_ref()
        .map(|base_dir| base_dir.join(path))
        .unwrap_or_else(|| PathBuf::from(command))
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
}
