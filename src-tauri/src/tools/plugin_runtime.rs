use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use niuma_api::tool_sessions::{ToolSessionDetailProvider, ToolSessionRegistry};
use niuma_core::plugin::{
    current_plugin_registry, default_non_tool_plugin_enabled, plugin_uses_listener_config,
    resolve_plugin_config, PluginCapability, PluginManifest, PluginRegistry, PluginRuntimeState,
    BUILTIN_BARK_PLUGIN_ID, BUILTIN_NTFY_PLUGIN_ID,
};
use niuma_core::runtime_event::{RuntimeEvent, RuntimeEventBus, StateChangeReason};
use niuma_core::store::NiumaStore;
use niuma_core::tool_session::ToolSessionDetail;
use niuma_core::tool_session_rpc::{
    ProviderRpcNotification, ProviderRpcRequest, ProviderRpcResponse, SessionDetailParams,
    SessionDetailResult, SessionSnapshotParams, SessionSnapshotResult,
};

const FALLBACK_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
const SESSION_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
const SESSION_DETAIL_TIMEOUT: Duration = Duration::from_secs(10);
const PARENT_PID_ENV: &str = "NIUMA_PARENT_PID";

pub fn spawn_plugin_runtimes(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    tool_sessions: ToolSessionRegistry,
) {
    spawn_plugin_manager(store, runtime_events, tool_sessions);
}

fn spawn_plugin_manager(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    tool_sessions: ToolSessionRegistry,
) {
    if let Err(error) = thread::Builder::new()
        .name("plugin-runtime-manager".to_string())
        .spawn(move || run_plugin_manager(store, runtime_events, tool_sessions))
    {
        eprintln!("NiumaNotifier plugin manager not started: {error}");
    }
}

struct ManagedPlugin {
    manifest: PluginManifest,
    child: Option<Child>,
    next_start: Instant,
}

fn run_plugin_manager(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    tool_sessions: ToolSessionRegistry,
) {
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
        reconcile_managed_plugins(&store, &tool_sessions, &mut managed);
        wait_for_plugin_reconcile_signal(&runtime, &mut receiver);
    }
}

fn reconcile_managed_plugins(
    store: &NiumaStore,
    tool_sessions: &ToolSessionRegistry,
    managed: &mut HashMap<String, ManagedPlugin>,
) {
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
            stop_child(store, tool_sessions, &entry.manifest, &mut entry.child);
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
            stop_child(store, tool_sessions, &entry.manifest, &mut entry.child);
            entry.manifest = manifest;
            entry.next_start = Instant::now();
        }
        tick_managed_plugin(store, tool_sessions, entry);
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
                | PluginCapability::ToolSessionListProvider
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

fn tick_managed_plugin(
    store: &NiumaStore,
    tool_sessions: &ToolSessionRegistry,
    entry: &mut ManagedPlugin,
) {
    if !plugin_runtime_enabled(store, &entry.manifest) {
        stop_child(store, tool_sessions, &entry.manifest, &mut entry.child);
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
                clear_session_provider_runtime(tool_sessions, &entry.manifest);
                entry.next_start = Instant::now() + Duration::from_secs(5);
                save_runtime_state(
                    store,
                    &entry.manifest.id,
                    PluginRuntimeState::failed(message),
                );
            }
            Ok(None) => {
                if !is_session_provider_manifest(&entry.manifest) {
                    save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::running());
                }
                return;
            }
            Err(error) => {
                let message = format!("检查插件进程失败：{error}");
                eprintln!("NiumaNotifier plugin {} {message}", entry.manifest.id);
                entry.child = None;
                clear_session_provider_runtime(tool_sessions, &entry.manifest);
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
        let is_session_provider = is_session_provider_manifest(&entry.manifest);
        match spawn_plugin_process(store, &entry.manifest, tool_sessions) {
            Ok(process) => {
                eprintln!("NiumaNotifier plugin {} started", entry.manifest.id);
                entry.child = Some(process);
                if !is_session_provider {
                    save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::running());
                }
            }
            Err(error) => {
                eprintln!(
                    "NiumaNotifier plugin {} not started: {error}",
                    entry.manifest.id
                );
                clear_session_provider_runtime(tool_sessions, &entry.manifest);
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

fn spawn_plugin_process(
    store: &NiumaStore,
    manifest: &PluginManifest,
    tool_sessions: &ToolSessionRegistry,
) -> Result<Child, String> {
    let mut child = build_plugin_command_for_runtime(manifest)?
        .spawn()
        .map_err(|error| format!("启动插件进程失败：{error}"))?;
    if is_session_provider_manifest(manifest) {
        if let Err(error) = bootstrap_session_provider(store, manifest, &mut child, tool_sessions) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    }
    Ok(child)
}

#[cfg(test)]
fn build_plugin_command(manifest: &PluginManifest) -> Result<Command, String> {
    build_plugin_command_with_stdio(manifest, false)
}

fn build_plugin_command_for_runtime(manifest: &PluginManifest) -> Result<Command, String> {
    build_plugin_command_with_stdio(manifest, is_session_provider_manifest(manifest))
}

fn build_plugin_command_with_stdio(
    manifest: &PluginManifest,
    use_provider_stdio: bool,
) -> Result<Command, String> {
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
        .stdin(if use_provider_stdio {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(if use_provider_stdio {
            Stdio::piped()
        } else {
            Stdio::null()
        })
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

fn is_session_provider_manifest(manifest: &PluginManifest) -> bool {
    manifest
        .capabilities
        .contains(&PluginCapability::ToolSessionListProvider)
}

fn bootstrap_session_provider(
    store: &NiumaStore,
    manifest: &PluginManifest,
    child: &mut Child,
    tool_sessions: &ToolSessionRegistry,
) -> Result<(), String> {
    let tool = manifest
        .tool_id
        .clone()
        .ok_or_else(|| "session provider 缺少 tool_id".to_string())?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "session provider stdin 未启用".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "session provider stdout 未启用".to_string())?;
    let pending = ProviderPendingResponses::default();
    let client = Arc::new(ProviderProcessClient::new(
        manifest.id.clone(),
        stdin,
        pending.clone(),
    ));
    spawn_provider_stdout_reader(
        manifest.id.clone(),
        stdout,
        tool_sessions.clone(),
        pending.clone(),
    );
    if manifest
        .capabilities
        .contains(&PluginCapability::ToolSessionDetailProvider)
    {
        tool_sessions.register_detail_provider(
            tool.clone(),
            Arc::new(SessionProviderDetailClient {
                client: Arc::clone(&client),
            }),
        );
    }
    let snapshot_client = Arc::clone(&client);
    let snapshot_tool = tool.clone();
    spawn_session_snapshot_bootstrap_thread(
        store.clone(),
        manifest.id.clone(),
        tool,
        tool_sessions.clone(),
        move || snapshot_client.session_snapshot(snapshot_tool),
    )?;
    Ok(())
}

fn spawn_session_snapshot_bootstrap_thread<F>(
    store: NiumaStore,
    plugin_id: String,
    tool: niuma_core::models::ToolKind,
    tool_sessions: ToolSessionRegistry,
    fetch_snapshot: F,
) -> Result<thread::JoinHandle<()>, String>
where
    F: FnOnce() -> Result<SessionSnapshotResult, String> + Send + 'static,
{
    thread::Builder::new()
        .name(format!("plugin-provider-bootstrap-{plugin_id}"))
        .spawn(move || match fetch_snapshot() {
            Ok(snapshot) if snapshot.tool == tool => {
                tool_sessions.replace_snapshot(snapshot.tool, snapshot.sessions);
                save_runtime_state(&store, &plugin_id, PluginRuntimeState::running());
            }
            Ok(snapshot) => {
                let message = format!(
                    "provider snapshot tool 不匹配：expected={} actual={}",
                    tool.as_str(),
                    snapshot.tool.as_str()
                );
                clear_session_provider_tool(&tool_sessions, &tool);
                save_runtime_state(&store, &plugin_id, PluginRuntimeState::failed(message));
            }
            Err(error) => {
                clear_session_provider_tool(&tool_sessions, &tool);
                save_runtime_state(&store, &plugin_id, PluginRuntimeState::failed(error));
            }
        })
        .map_err(|error| format!("session provider snapshot bootstrap 未启动：{error}"))
}

fn spawn_provider_stdout_reader(
    plugin_id: String,
    stdout: ChildStdout,
    tool_sessions: ToolSessionRegistry,
    pending: ProviderPendingResponses,
) {
    let thread_plugin_id = plugin_id.clone();
    if let Err(error) = thread::Builder::new()
        .name(format!("plugin-provider-stdout-{plugin_id}"))
        .spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => handle_provider_stdout_line(&tool_sessions, Some(&pending), &line),
                    Err(error) => {
                        eprintln!(
                            "NiumaNotifier provider {thread_plugin_id} stdout read failed: {error}"
                        );
                        break;
                    }
                }
            }
        })
    {
        eprintln!("NiumaNotifier provider {plugin_id} stdout reader not started: {error}");
    }
}

fn handle_provider_stdout_line(
    tool_sessions: &ToolSessionRegistry,
    pending: Option<&ProviderPendingResponses>,
    line: &str,
) {
    let value = match serde_json::from_str::<serde_json::Value>(line) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("NiumaNotifier provider stdout contains non-JSON line: {error}: {line}");
            return;
        }
    };

    if value.get("id").is_some() && (value.get("result").is_some() || value.get("error").is_some())
    {
        match serde_json::from_value::<ProviderRpcResponse>(value) {
            Ok(response) => {
                if pending.is_some_and(|pending| pending.complete(response)) {
                    return;
                }
                eprintln!("NiumaNotifier provider response has no pending request");
            }
            Err(error) => eprintln!("NiumaNotifier provider response parse failed: {error}"),
        }
        return;
    }

    if value.get("method").is_some() && value.get("id").is_none() {
        match serde_json::from_value::<ProviderRpcNotification>(value) {
            Ok(notification) => handle_provider_notification(tool_sessions, notification),
            Err(error) => eprintln!("NiumaNotifier provider notification parse failed: {error}"),
        }
        return;
    }

    eprintln!("NiumaNotifier provider stdout line is neither response nor notification: {line}");
}

fn handle_provider_notification(
    tool_sessions: &ToolSessionRegistry,
    notification: ProviderRpcNotification,
) {
    match notification.method.as_str() {
        "session_snapshot_updated" => match notification.params_as::<SessionSnapshotResult>() {
            Ok(snapshot) => tool_sessions.replace_snapshot(snapshot.tool, snapshot.sessions),
            Err(error) => {
                eprintln!("NiumaNotifier provider snapshot notification parse failed: {error}")
            }
        },
        method => eprintln!("NiumaNotifier provider notification ignored: {method}"),
    }
}

#[derive(Clone, Default)]
struct ProviderPendingResponses {
    inner: Arc<Mutex<HashMap<String, mpsc::Sender<ProviderRpcResponse>>>>,
}

impl ProviderPendingResponses {
    fn insert(&self, id: String) -> mpsc::Receiver<ProviderRpcResponse> {
        let (sender, receiver) = mpsc::channel();
        self.inner
            .lock()
            .expect("provider pending response lock poisoned")
            .insert(id, sender);
        receiver
    }

    fn complete(&self, response: ProviderRpcResponse) -> bool {
        let sender = self
            .inner
            .lock()
            .expect("provider pending response lock poisoned")
            .remove(&response.id);
        sender.is_some_and(|sender| sender.send(response).is_ok())
    }

    fn remove(&self, id: &str) {
        self.inner
            .lock()
            .expect("provider pending response lock poisoned")
            .remove(id);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("provider pending response lock poisoned")
            .len()
    }
}

struct ProviderProcessClient {
    plugin_id: String,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: ProviderPendingResponses,
    next_id: AtomicU64,
}

impl ProviderProcessClient {
    fn new(plugin_id: String, stdin: ChildStdin, pending: ProviderPendingResponses) -> Self {
        Self {
            plugin_id,
            stdin: Arc::new(Mutex::new(stdin)),
            pending,
            next_id: AtomicU64::new(1),
        }
    }

    fn session_snapshot(
        &self,
        tool: niuma_core::models::ToolKind,
    ) -> Result<SessionSnapshotResult, String> {
        self.call(
            "session_snapshot",
            SessionSnapshotParams { tool },
            SESSION_SNAPSHOT_TIMEOUT,
        )
    }

    fn session_detail(&self, params: SessionDetailParams) -> Result<SessionDetailResult, String> {
        self.call("session_detail", params, SESSION_DETAIL_TIMEOUT)
    }

    fn call<T, R>(&self, method: &str, params: T, timeout: Duration) -> Result<R, String>
    where
        T: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let id = format!(
            "{}-{}",
            self.plugin_id,
            self.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let request = ProviderRpcRequest::new(id.clone(), method, params)?;
        let receiver = self.pending.insert(id.clone());
        let line = serde_json::to_string(&request)
            .map_err(|error| format!("序列化 provider 请求失败：{error}"))?;
        if let Err(error) = self.write_request_line(&line) {
            self.pending.remove(&id);
            return Err(error);
        }
        match receiver.recv_timeout(timeout) {
            Ok(response) => response.result_as::<R>(),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.pending.remove(&id);
                Err(format!("provider 请求超时：{method}"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.pending.remove(&id);
                Err(format!("provider 响应通道已关闭：{method}"))
            }
        }
    }

    fn write_request_line(&self, line: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().expect("provider stdin lock poisoned");
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("写入 provider 请求失败：{error}"))
    }
}

struct SessionProviderDetailClient {
    client: Arc<ProviderProcessClient>,
}

impl ToolSessionDetailProvider for SessionProviderDetailClient {
    fn detail(
        &self,
        tool: &niuma_core::models::ToolKind,
        session_id: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ToolSessionDetail, String> {
        let result = self.client.session_detail(SessionDetailParams {
            tool: tool.clone(),
            session_id: session_id.to_string(),
            limit,
            cursor,
        })?;
        Ok(result.detail)
    }
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

fn clear_session_provider_runtime(tool_sessions: &ToolSessionRegistry, manifest: &PluginManifest) {
    if !is_session_provider_manifest(manifest) {
        return;
    }
    if let Some(tool) = &manifest.tool_id {
        clear_session_provider_tool(tool_sessions, tool);
    }
}

fn clear_session_provider_tool(
    tool_sessions: &ToolSessionRegistry,
    tool: &niuma_core::models::ToolKind,
) {
    // 同一 tool 当前只允许一个 provider；按 tool 清理不会误伤其他 provider。
    tool_sessions.unregister_detail_provider(tool);
    tool_sessions.clear_snapshot(tool);
}

fn stop_child(
    store: &NiumaStore,
    tool_sessions: &ToolSessionRegistry,
    manifest: &PluginManifest,
    child: &mut Option<Child>,
) {
    clear_session_provider_runtime(tool_sessions, manifest);
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

        assert_eq!(manifests.len(), 4);
        assert!(manifests
            .iter()
            .any(|manifest| manifest.id == "builtin-codex"
                && manifest.source == PluginSource::Builtin));
        assert!(manifests.iter().any(|manifest| manifest.id
            == niuma_core::plugin::BUILTIN_CODEX_SESSION_PROVIDER_PLUGIN_ID
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
    fn managed_runtime_manifests_include_tool_session_list_providers() {
        let mut registry = PluginRegistry::new();
        registry.register(session_provider_manifest("codex-session-provider"));

        let manifests = managed_runtime_manifests(&registry);

        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].id, "codex-session-provider");
    }

    #[test]
    fn provider_notification_updates_session_snapshot() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let line = serde_json::json!({
            "method": "session_snapshot_updated",
            "params": {
                "tool": "codex",
                "sessions": [provider_test_session("s1")]
            }
        })
        .to_string();

        handle_provider_stdout_line(&registry, None, &line);

        let sessions = registry
            .list(niuma_api::tool_sessions::ToolSessionListQuery {
                tool: Some("codex".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s1");
    }

    #[test]
    fn invalid_provider_stdout_line_does_not_block_later_notification() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();

        // provider stdout 可能混入日志或半行 JSON；解析失败不能终止后续合法通知处理。
        handle_provider_stdout_line(&registry, None, "{not-json");
        handle_provider_stdout_line(
            &registry,
            None,
            &serde_json::json!({
                "method": "session_snapshot_updated",
                "params": {
                    "tool": "codex",
                    "sessions": [provider_test_session("s1")]
                }
            })
            .to_string(),
        );

        let sessions = registry
            .list(niuma_api::tool_sessions::ToolSessionListQuery {
                tool: Some("codex".to_string()),
                include_subagents: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s1");
    }

    #[test]
    fn provider_response_line_matches_pending_request_id() {
        let pending = ProviderPendingResponses::default();
        let receiver = pending.insert("req-1".to_string());
        let line = serde_json::json!({
            "id": "req-1",
            "result": {
                "tool": "codex",
                "sessions": []
            }
        })
        .to_string();

        handle_provider_stdout_line(
            &niuma_api::tool_sessions::ToolSessionRegistry::new(),
            Some(&pending),
            &line,
        );

        let response = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(response.id, "req-1");
        assert!(response.error.is_none());
    }

    #[test]
    fn pending_response_timeout_removes_pending_entry() {
        let pending = ProviderPendingResponses::default();
        let _receiver = pending.insert("req-timeout".to_string());

        // 超时路径会调用 remove；这里直接校验 map 可观测状态，避免真实等待慢测试。
        pending.remove("req-timeout");

        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn session_snapshot_bootstrap_returns_before_snapshot_fetch_finishes() {
        let store = NiumaStore::new(test_sqlite_path("session_snapshot_bootstrap_async"));
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let started_at = Instant::now();

        let handle = spawn_session_snapshot_bootstrap_thread(
            store.clone(),
            "provider-async".to_string(),
            ToolKind::Codex,
            registry.clone(),
            move || {
                std::thread::sleep(Duration::from_millis(200));
                Ok(SessionSnapshotResult {
                    tool: ToolKind::Codex,
                    sessions: vec![provider_test_session_item("s1")],
                })
            },
        )
        .unwrap();

        assert!(started_at.elapsed() < Duration::from_millis(100));
        handle.join().unwrap();
        let state = store
            .plugin_runtime_states()
            .unwrap()
            .remove("provider-async")
            .unwrap();
        assert_eq!(
            state.status,
            niuma_core::plugin::PluginRuntimeStatus::Running
        );
        assert_eq!(
            registry
                .find_session(&ToolKind::Codex, "s1")
                .unwrap()
                .session_id,
            "s1"
        );
    }

    #[test]
    fn failed_session_snapshot_bootstrap_clears_registry_for_provider_tool() {
        let store = NiumaStore::new(test_sqlite_path("session_snapshot_bootstrap_failed"));
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        registry.replace_snapshot(
            ToolKind::Codex,
            vec![provider_test_session_item("stale-session")],
        );
        registry.register_detail_provider(ToolKind::Codex, Arc::new(FakeToolSessionDetailProvider));

        let handle = spawn_session_snapshot_bootstrap_thread(
            store.clone(),
            "provider-failed".to_string(),
            ToolKind::Codex,
            registry.clone(),
            || Err("provider 请求超时：session_snapshot".to_string()),
        )
        .unwrap();
        handle.join().unwrap();

        let state = store
            .plugin_runtime_states()
            .unwrap()
            .remove("provider-failed")
            .unwrap();
        assert_eq!(
            state.status,
            niuma_core::plugin::PluginRuntimeStatus::Failed
        );
        assert!(registry
            .find_session(&ToolKind::Codex, "stale-session")
            .is_none());
        assert_eq!(
            registry
                .detail(&ToolKind::Codex, "stale-session", None, None)
                .unwrap_err(),
            "session detail provider 尚未就绪"
        );
    }

    #[test]
    fn spawn_plugin_runtimes_requires_shared_store_from_main_app() {
        let _spawn: fn(NiumaStore, RuntimeEventBus, ToolSessionRegistry) = spawn_plugin_runtimes;
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

    fn session_provider_manifest(id: &str) -> PluginManifest {
        PluginManifest {
            id: id.to_string(),
            kind: PluginKind::Tool,
            tool_id: Some(ToolKind::Codex),
            display_name: "Codex Session Provider".to_string(),
            version: "0.1.0".to_string(),
            command: Some("definitely-missing-niuma-command".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            platforms: Vec::new(),
            capabilities: vec![PluginCapability::ToolSessionListProvider],
            icon_url: None,
            config_schema: Vec::new(),
            source: PluginSource::External,
            base_dir: None,
        }
    }

    fn provider_test_session(session_id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": format!("codex:{session_id}"),
            "tool": "codex",
            "session_id": session_id,
            "project_path": "/tmp/demo",
            "project_name": "demo",
            "file_path": format!("/tmp/demo/{session_id}.jsonl"),
            "modified_at": "1970-01-01T00:00:20Z",
            "discovered_at": "1970-01-01T00:00:01Z",
            "last_seen_at": "1970-01-01T00:00:30Z",
            "is_active": true,
            "is_subagent": false,
            "status": "active"
        })
    }

    fn provider_test_session_item(
        session_id: &str,
    ) -> niuma_core::tool_session::ToolSessionListItem {
        serde_json::from_value(provider_test_session(session_id)).unwrap()
    }

    struct FakeToolSessionDetailProvider;

    impl ToolSessionDetailProvider for FakeToolSessionDetailProvider {
        fn detail(
            &self,
            _tool: &ToolKind,
            session_id: &str,
            _limit: Option<usize>,
            _cursor: Option<String>,
        ) -> Result<ToolSessionDetail, String> {
            Ok(ToolSessionDetail {
                tool: ToolKind::Codex,
                session_id: session_id.to_string(),
                project_path: "/tmp/demo".to_string(),
                project_name: "demo".to_string(),
                is_subagent: false,
                parent_session_id: None,
                messages: Vec::new(),
                next_cursor: None,
            })
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
